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

use model::{ConversationSeed, SearchResponse};
use query::{extract_repo, repo_from_repository_url};

pub(super) const GITHUB_API_BASE: &str = "https://api.github.com";
pub(super) const GITHUB_API_VERSION: &str = "2022-11-28";
pub(super) const PAGE_SIZE: u32 = 100;

pub struct GitHubSource {
    client: ClientWithMiddleware,
}

impl GitHubSource {
    /// Create a GitHub source client.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be constructed.
    pub fn new(telemetry_active: bool) -> Result<Self> {
        let http_client = reqwest::Client::builder()
            .user_agent(concat!("99problems-cli/", env!("CARGO_PKG_VERSION")))
            .build()?;
        let client = Self::build_client(http_client, telemetry_active);
        Ok(Self { client })
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
        let search_url = format!("{GITHUB_API_BASE}/search/issues");
        let mut page = 1u32;
        let mut emitted = 0usize;
        let per_page = Self::bounded_per_page(req.per_page);
        let repo_from_query = extract_repo(raw_query);

        loop {
            let search: SearchResponse = async {
                debug!(page, per_page, "fetching GitHub search page");
                let req_http = self.client.get(&search_url).query(&[
                    ("q", raw_query),
                    ("per_page", &per_page.to_string()),
                    ("page", &page.to_string()),
                ]);
                let req_http = Self::apply_auth(req_http, req.token.as_deref());
                let payload = Self::execute_request(req_http, "search", "reqwest.http.get").await?;
                let decode_span = debug_span!("github.search.decode", operation = "search");
                let search: SearchResponse = decode_span.in_scope(|| {
                    Self::decode_github_json(&payload, req.token.as_deref(), "search")
                })?;
                trace!(
                    count = search.items.len(),
                    page, "decoded GitHub search page"
                );
                Ok::<SearchResponse, anyhow::Error>(search)
            }
            .instrument(debug_span!("github.search.page", page, per_page))
            .await?;
            let done = search.items.len() < per_page as usize;
            for item in search.items {
                let repo = item
                    .repository_url
                    .as_deref()
                    .and_then(repo_from_repository_url)
                    .or_else(|| repo_from_query.clone())
                    .ok_or_else(|| {
                        AppError::usage(format!(
                            "Could not determine repo for item #{}. Include repo:owner/name in query.",
                            item.number
                        ))
                    })?;

                let conversation = Box::pin(self.fetch_conversation(
                    &repo,
                    ConversationSeed {
                        id: item.number,
                        title: item.title,
                        state: item.state,
                        body: item.body,
                        is_pr: item.pull_request.is_some(),
                        pull_request: item.pull_request,
                    },
                    req,
                ))
                .await?;
                emit(conversation)?;
                emitted += 1;
            }
            if done {
                break;
            }
            page += 1;
        }

        Ok(emitted)
    }

    async fn fetch_by_id_stream(
        &self,
        req: &FetchRequest,
        repo: &str,
        id: &str,
        kind: ContentKind,
        emit: &mut dyn FnMut(Conversation) -> Result<()>,
    ) -> Result<usize> {
        async {
            let issue_id = id.parse::<u64>().map_err(|_| {
                AppError::usage(format!("GitHub expects a numeric issue/PR id, got '{id}'."))
            })?;
            let conversation =
                Box::pin(self.fetch_conversation_by_id(repo, issue_id, kind, req)).await?;
            emit(conversation)?;
            Ok(1)
        }
        .instrument(debug_span!("github.id.fetch"))
        .await
    }
}

#[async_trait(?Send)]
impl Source for GitHubSource {
    async fn fetch_stream(
        &self,
        req: &FetchRequest,
        emit: &mut dyn FnMut(Conversation) -> Result<()>,
    ) -> Result<usize> {
        match &req.target {
            FetchTarget::Search { raw_query } => {
                Box::pin(self.search_stream(req, raw_query, emit)).await
            }
            FetchTarget::Id { repo, id, kind } => {
                Box::pin(self.fetch_by_id_stream(req, repo, id, *kind, emit)).await
            }
        }
    }
}
