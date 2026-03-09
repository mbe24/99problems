use anyhow::Result;
use async_trait::async_trait;
use reqwest_middleware::{ClientBuilder as MiddlewareClientBuilder, ClientWithMiddleware};
use tracing::{Instrument, debug, debug_span, trace};

use super::{ContentKind, FetchRequest, FetchTarget, Source};
use crate::error::AppError;
use crate::model::Conversation;

mod api;
mod model;
mod query;

use crate::model::ConversationMetadata;
use model::{JiraIssueItem, JiraSearchResponse, extract_adf_text};
use query::{build_jql, parse_jira_query};

pub(super) const JIRA_DEFAULT_BASE_URL: &str = "https://jira.atlassian.com";
pub(super) const PAGE_SIZE: u32 = 100;

pub struct JiraSource {
    client: ClientWithMiddleware,
    base_url: String,
}

impl JiraSource {
    /// Create a Jira source client.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be constructed.
    pub fn new(base_url: Option<String>, telemetry_active: bool) -> Result<Self> {
        let http_client = reqwest::Client::builder()
            .user_agent(concat!("99problems-cli/", env!("CARGO_PKG_VERSION")))
            .build()?;
        let client = Self::build_client(http_client, telemetry_active);
        trace!(
            telemetry_active,
            "initialized Jira HTTP client (reqwest HTTP/2 enabled; protocol is negotiated at runtime)"
        );
        let base_url = base_url
            .unwrap_or_else(|| JIRA_DEFAULT_BASE_URL.to_string())
            .trim_end_matches('/')
            .to_string();
        Ok(Self { client, base_url })
    }

    #[cfg(feature = "telemetry-otel")]
    fn build_client(http_client: reqwest::Client, telemetry_active: bool) -> ClientWithMiddleware {
        let builder = MiddlewareClientBuilder::new(http_client);
        if telemetry_active {
            builder
                .with(reqwest_tracing::TracingMiddleware::default())
                .build()
        } else {
            builder.build()
        }
    }

    #[cfg(not(feature = "telemetry-otel"))]
    fn build_client(http_client: reqwest::Client, _telemetry_active: bool) -> ClientWithMiddleware {
        MiddlewareClientBuilder::new(http_client).build()
    }

    async fn search_stream(
        &self,
        req: &FetchRequest,
        raw_query: &str,
        emit: &mut dyn FnMut(Conversation) -> Result<()>,
    ) -> Result<usize> {
        let filters = parse_jira_query(raw_query);
        if matches!(filters.kind, ContentKind::Pr) {
            return Err(AppError::usage(
                "Platform 'jira' does not support pull requests. Use --type issue.",
            )
            .into());
        }
        let jql = build_jql(&filters)?;
        let per_page = Self::bounded_per_page(req.per_page);
        let mut start_at = 0u32;
        let mut next_page_token: Option<String> = None;
        let mut emitted = 0usize;

        loop {
            let page = self
                .fetch_search_page(req, &jql, per_page, start_at, next_page_token.as_deref())
                .await?;
            trace!(count = page.issues.len(), "decoded Jira search page");
            for issue in page.issues {
                emit(self.build_search_conversation(req, issue).await?)?;
                emitted += 1;
            }

            if let Some(token) = page.next_page_token {
                next_page_token = Some(token);
                continue;
            }
            if let (Some(s), Some(m), Some(t)) = (page.start_at, page.max_results, page.total) {
                let next = s + m;
                if next < t {
                    start_at = next;
                    next_page_token = None;
                    continue;
                }
                break;
            }
            if page.is_last == Some(false) {
                return Err(AppError::provider(
                    "Jira API search response indicated more pages but provided no pagination cursor.",
                )
                .with_provider("jira")
                .into());
            }
            break;
        }

        Ok(emitted)
    }

    async fn fetch_search_page(
        &self,
        req: &FetchRequest,
        jql: &str,
        per_page: u32,
        start_at: u32,
        next_page_token: Option<&str>,
    ) -> Result<JiraSearchResponse> {
        let page_span = debug_span!(
            "jira.search.page",
            start_at,
            per_page,
            has_next_page_token = next_page_token.is_some()
        );
        let payload = async {
            let url = format!("{}/rest/api/3/search/jql", self.base_url);
            debug!(
                start_at,
                per_page,
                has_next_page_token = next_page_token.is_some(),
                "fetching Jira search page"
            );

            let query_params = search_query_params(jql, per_page, start_at, next_page_token);
            let http = Self::apply_auth(
                self.client.get(&url),
                req.token.as_deref(),
                req.account_email.as_deref(),
            )
            .query(&query_params);
            Self::execute_request(http, "search").await
        }
        .instrument(page_span)
        .await?;

        let decode_span = debug_span!("jira.search.decode", operation = "search");
        decode_span.in_scope(|| {
            Self::decode_jira_json(
                &payload,
                req.token.as_deref(),
                req.account_email.as_deref(),
                "search",
            )
        })
    }

    async fn build_search_conversation(
        &self,
        req: &FetchRequest,
        issue: JiraIssueItem,
    ) -> Result<Conversation> {
        let issue_key = issue.key;
        let fields = issue.fields;
        let comments_task = async {
            if req.include_comments {
                self.fetch_comments(&issue_key, req).await
            } else {
                Ok(Vec::new())
            }
        }
        .instrument(debug_span!("jira.search.hydrate.comments"));
        let metadata_task = async {
            if req.include_links {
                self.fetch_metadata(&issue_key, fields.clone(), req).await
            } else {
                ConversationMetadata::empty()
            }
        }
        .instrument(debug_span!("jira.search.hydrate.links"));
        let (comments_result, metadata) = tokio::join!(comments_task, metadata_task);
        let comments = comments_result?;

        Ok(Conversation {
            id: issue_key,
            title: fields.summary,
            state: fields.status.name,
            body: fields
                .description
                .as_ref()
                .map(extract_adf_text)
                .filter(|s| !s.is_empty()),
            comments,
            metadata,
        })
    }
}

fn search_query_params(
    jql: &str,
    per_page: u32,
    start_at: u32,
    next_page_token: Option<&str>,
) -> Vec<(String, String)> {
    let mut query_params = vec![
        ("jql".to_string(), jql.to_string()),
        ("maxResults".to_string(), per_page.to_string()),
        (
            "fields".to_string(),
            "summary,description,status,parent,subtasks,issuelinks,attachment".to_string(),
        ),
    ];
    if let Some(token) = next_page_token {
        query_params.push(("nextPageToken".to_string(), token.to_string()));
    } else {
        query_params.push(("startAt".to_string(), start_at.to_string()));
    }
    query_params
}

#[async_trait(?Send)]
impl Source for JiraSource {
    async fn fetch_stream(
        &self,
        req: &FetchRequest,
        emit: &mut dyn FnMut(Conversation) -> Result<()>,
    ) -> Result<usize> {
        match &req.target {
            FetchTarget::Search { raw_query } => self.search_stream(req, raw_query, emit).await,
            FetchTarget::Id { id, kind, .. } => {
                if matches!(kind, ContentKind::Pr) {
                    return Err(AppError::usage(
                        "Platform 'jira' does not support pull requests. Use --type issue.",
                    )
                    .into());
                }
                emit(self.fetch_issue(id, req).await?)?;
                Ok(1)
            }
        }
    }
}
