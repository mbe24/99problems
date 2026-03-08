use anyhow::Result;
use reqwest::blocking::Client;
use tracing::{debug, debug_span, trace};

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
            let page =
                self.fetch_search_page(req, &jql, per_page, start_at, next_page_token.as_deref())?;
            trace!(count = page.issues.len(), "decoded Jira search page");
            for issue in page.issues {
                emit(self.build_search_conversation(req, issue)?)?;
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

    fn fetch_search_page(
        &self,
        req: &FetchRequest,
        jql: &str,
        per_page: u32,
        start_at: u32,
        next_page_token: Option<&str>,
    ) -> Result<JiraSearchResponse> {
        let _page_span = debug_span!(
            "jira.search.page",
            start_at,
            per_page,
            has_next_page_token = next_page_token.is_some()
        )
        .entered();
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
        let resp = Self::send(http, "search")?;

        let _decode_span = debug_span!("jira.search.decode", operation = "search").entered();
        Self::parse_jira_json(
            resp,
            req.token.as_deref(),
            req.account_email.as_deref(),
            "search",
        )
    }

    fn build_search_conversation(
        &self,
        req: &FetchRequest,
        issue: JiraIssueItem,
    ) -> Result<Conversation> {
        let fields = issue.fields;
        let comments = if req.include_comments {
            let _comments_span = debug_span!("jira.search.hydrate.comments").entered();
            self.fetch_comments(&issue.key, req)?
        } else {
            vec![]
        };
        let metadata = if req.include_links {
            let _links_span = debug_span!("jira.search.hydrate.links").entered();
            self.fetch_metadata(&issue.key, fields.clone(), req)
        } else {
            ConversationMetadata::empty()
        };

        Ok(Conversation {
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
