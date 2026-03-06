use anyhow::Result;
use reqwest::blocking::Client;
use tracing::{debug, trace};

use super::{ContentKind, FetchRequest, FetchTarget, Source};
use crate::error::AppError;
use crate::model::Conversation;

mod api;
mod model;
mod query;

use crate::model::ConversationMetadata;
use model::{JiraSearchResponse, extract_adf_text, map_issue_links};
use query::{build_jql, parse_jira_query};

pub(super) const JIRA_DEFAULT_BASE_URL: &str = "https://jira.atlassian.com";
pub(super) const PAGE_SIZE: u32 = 100;

pub struct JiraSource {
    client: Client,
    base_url: String,
}

impl JiraSource {
    /// Create a Jira source client.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be constructed.
    pub fn new(base_url: Option<String>) -> Result<Self> {
        let client = Client::builder()
            .user_agent(concat!("99problems-cli/", env!("CARGO_PKG_VERSION")))
            .build()?;
        let base_url = base_url
            .unwrap_or_else(|| JIRA_DEFAULT_BASE_URL.to_string())
            .trim_end_matches('/')
            .to_string();
        Ok(Self { client, base_url })
    }

    fn search_stream(
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
            let url = format!("{}/rest/api/3/search/jql", self.base_url);
            debug!(
                start_at,
                per_page,
                has_next_page_token = next_page_token.is_some(),
                "fetching Jira search page"
            );
            let mut query_params: Vec<(String, String)> = vec![
                ("jql".into(), jql.clone()),
                ("maxResults".into(), per_page.to_string()),
                (
                    "fields".into(),
                    "summary,description,status,issuelinks".into(),
                ),
            ];
            if let Some(token) = &next_page_token {
                query_params.push(("nextPageToken".into(), token.clone()));
            } else {
                query_params.push(("startAt".into(), start_at.to_string()));
            }

            let http = Self::apply_auth(
                self.client.get(&url),
                req.token.as_deref(),
                req.account_email.as_deref(),
            )
            .query(&query_params);
            let resp = Self::send(http, "search")?;
            let page: JiraSearchResponse = Self::parse_jira_json(
                resp,
                req.token.as_deref(),
                req.account_email.as_deref(),
                "search",
            )?;
            trace!(count = page.issues.len(), "decoded Jira search page");
            for issue in page.issues {
                let fields = issue.fields;
                let comments = if req.include_comments {
                    self.fetch_comments(&issue.key, req)?
                } else {
                    vec![]
                };
                let metadata = if req.include_links {
                    ConversationMetadata::with_links(map_issue_links(fields.issuelinks))
                } else {
                    ConversationMetadata::empty()
                };
                emit(Conversation {
                    id: issue.key,
                    title: fields.summary,
                    state: fields.status.name,
                    body: fields
                        .description
                        .as_ref()
                        .map(extract_adf_text)
                        .filter(|s| !s.is_empty()),
                    comments,
                    metadata,
                })?;
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
}

impl Source for JiraSource {
    fn fetch_stream(
        &self,
        req: &FetchRequest,
        emit: &mut dyn FnMut(Conversation) -> Result<()>,
    ) -> Result<usize> {
        match &req.target {
            FetchTarget::Search { raw_query } => self.search_stream(req, raw_query, emit),
            FetchTarget::Id { id, kind, .. } => {
                if matches!(kind, ContentKind::Pr) {
                    return Err(AppError::usage(
                        "Platform 'jira' does not support pull requests. Use --type issue.",
                    )
                    .into());
                }
                emit(self.fetch_issue(id, req)?)?;
                Ok(1)
            }
        }
    }
}
