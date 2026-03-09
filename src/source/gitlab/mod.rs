use anyhow::Result;
use async_trait::async_trait;
use reqwest_middleware::{ClientBuilder as MiddlewareClientBuilder, ClientWithMiddleware};
use tracing::{Instrument, debug_span};

use super::{ContentKind, FetchRequest, FetchTarget, Source};
use crate::error::AppError;
use crate::model::Conversation;

mod api;
mod model;
mod query;

use api::GitLabPageContext;
use model::{ConversationSeed, GitLabIssueItem, GitLabMergeRequestItem};
use query::{build_search_params, encode_project_path, parse_gitlab_query};

pub(super) const GITLAB_DEFAULT_BASE_URL: &str = "https://gitlab.com";
pub(super) const PAGE_SIZE: u32 = 100;

pub struct GitLabSource {
    client: ClientWithMiddleware,
    base_url: String,
}

impl GitLabSource {
    /// Create a GitLab source client.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be constructed.
    pub fn new(base_url: Option<String>, telemetry_active: bool) -> Result<Self> {
        let http_client = reqwest::Client::builder()
            .user_agent(concat!("99problems-cli/", env!("CARGO_PKG_VERSION")))
            .build()?;

        let client = Self::build_client(http_client, telemetry_active);
        let base_url = base_url
            .unwrap_or_else(|| GITLAB_DEFAULT_BASE_URL.to_string())
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
        let span = debug_span!("gitlab.search");
        async {
            let filters = parse_gitlab_query(raw_query);
            let repo = filters
                .repo
                .as_deref()
                .ok_or_else(|| {
                    AppError::usage(
                        "No repo: found in query. Use --repo or include 'repo:group/project' in -q",
                    )
                })?
                .to_string();
            let project = encode_project_path(&repo);
            let params = build_search_params(&filters);
            let mut emitted = 0usize;

            match filters.kind {
                ContentKind::Issue => {
                    let url = format!("{}/api/v4/projects/{project}/issues", self.base_url);
                    let issues: Vec<GitLabIssueItem> = self
                        .get_pages(
                            &url,
                            &params,
                            req.token.as_deref(),
                            req.per_page,
                            false,
                            GitLabPageContext::new("search", "issue search fetch"),
                        )
                        .await?;
                    for i in issues {
                        let conversation = self
                            .fetch_conversation(
                                &repo,
                                ConversationSeed {
                                    id: i.iid,
                                    title: i.title,
                                    state: i.state,
                                    body: i.description,
                                    is_pr: false,
                                    web_url: i.web_url,
                                },
                                req,
                            )
                            .await?;
                        emit(conversation)?;
                        emitted += 1;
                    }
                }
                ContentKind::Pr => {
                    let url = format!("{}/api/v4/projects/{project}/merge_requests", self.base_url);
                    let mrs: Vec<GitLabMergeRequestItem> = self
                        .get_pages(
                            &url,
                            &params,
                            req.token.as_deref(),
                            req.per_page,
                            false,
                            GitLabPageContext::new("search", "merge request search fetch"),
                        )
                        .await?;
                    for mr in mrs {
                        let conversation = self
                            .fetch_conversation(
                                &repo,
                                ConversationSeed {
                                    id: mr.iid,
                                    title: mr.title,
                                    state: mr.state,
                                    body: mr.description,
                                    is_pr: true,
                                    web_url: mr.web_url,
                                },
                                req,
                            )
                            .await?;
                        emit(conversation)?;
                        emitted += 1;
                    }
                }
            }
            Ok(emitted)
        }
        .instrument(span)
        .await
    }

    async fn fetch_by_id_stream(
        &self,
        req: &FetchRequest,
        repo: &str,
        id: &str,
        kind: ContentKind,
        emit: &mut dyn FnMut(Conversation) -> Result<()>,
    ) -> Result<usize> {
        let span = debug_span!("gitlab.id.fetch");
        async {
            let iid = id.parse::<u64>().map_err(|_| {
                AppError::usage(format!("GitLab expects a numeric issue/MR id, got '{id}'."))
            })?;
            let conversation = self.fetch_conversation_by_id(repo, iid, kind, req).await?;
            emit(conversation)?;
            Ok(1)
        }
        .instrument(span)
        .await
    }
}

#[async_trait(?Send)]
impl Source for GitLabSource {
    async fn fetch_stream(
        &self,
        req: &FetchRequest,
        emit: &mut dyn FnMut(Conversation) -> Result<()>,
    ) -> Result<usize> {
        match &req.target {
            FetchTarget::Search { raw_query } => self.search_stream(req, raw_query, emit).await,
            FetchTarget::Id { repo, id, kind } => {
                self.fetch_by_id_stream(req, repo, id, *kind, emit).await
            }
        }
    }
}
