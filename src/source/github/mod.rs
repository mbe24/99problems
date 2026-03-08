use anyhow::Result;
use async_trait::async_trait;
use reqwest::blocking::Client;
use tokio::task::block_in_place;
use tracing::{debug, debug_span, trace, warn};

use super::{ContentKind, FetchRequest, FetchTarget, Source};
use crate::error::{AppError, app_error_from_decode, app_error_from_reqwest};
use crate::model::Conversation;

mod api;
mod model;
mod query;

use model::{ConversationSeed, IssueItem, SearchResponse};
use query::{extract_repo, repo_from_repository_url};

pub(super) const GITHUB_API_BASE: &str = "https://api.github.com";
pub(super) const GITHUB_API_VERSION: &str = "2022-11-28";
pub(super) const PAGE_SIZE: u32 = 100;

pub struct GitHubSource {
    client: Client,
}

impl GitHubSource {
    /// Create a GitHub source client.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be constructed.
    pub fn new() -> Result<Self> {
        let client = Client::builder()
            .user_agent(concat!("99problems-cli/", env!("CARGO_PKG_VERSION")))
            .build()?;
        Ok(Self { client })
    }

    fn search_stream(
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
            let _page_span = debug_span!("github.search.page", page, per_page).entered();
            debug!(page, per_page, "fetching GitHub search page");
            let req_http = self.client.get(&search_url).query(&[
                ("q", raw_query),
                ("per_page", &per_page.to_string()),
                ("page", &page.to_string()),
            ]);
            let req_http = Self::apply_auth(req_http, req.token.as_deref());
            let resp = Self::send(req_http, "search")?;

            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp
                    .text()
                    .map_err(|err| app_error_from_reqwest("GitHub", "error body read", &err))?;
                return Err(AppError::from_http("GitHub", "search", status, &body).into());
            }

            let search: SearchResponse = {
                let _decode_span =
                    debug_span!("github.search.decode", operation = "search").entered();
                resp.json()
                    .map_err(|err| app_error_from_decode("GitHub", "search", err))?
            };
            trace!(
                count = search.items.len(),
                page, "decoded GitHub search page"
            );
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

                let conversation = self.fetch_conversation(
                    &repo,
                    ConversationSeed {
                        id: item.number,
                        title: item.title,
                        state: item.state,
                        body: item.body,
                        is_pr: item.pull_request.is_some(),
                    },
                    req,
                )?;
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

    fn fetch_by_id_stream(
        &self,
        req: &FetchRequest,
        repo: &str,
        id: &str,
        kind: ContentKind,
        allow_fallback_to_pr: bool,
        emit: &mut dyn FnMut(Conversation) -> Result<()>,
    ) -> Result<usize> {
        let _span = debug_span!("github.id.fetch").entered();
        let issue_id = id.parse::<u64>().map_err(|_| {
            AppError::usage(format!("GitHub expects a numeric issue/PR id, got '{id}'."))
        })?;
        let issue_url = format!("{GITHUB_API_BASE}/repos/{repo}/issues/{issue_id}");
        let request = Self::apply_auth(self.client.get(&issue_url), req.token.as_deref());
        let resp = Self::send(request, "issue fetch")?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp
                .text()
                .map_err(|err| app_error_from_reqwest("GitHub", "error body read", &err))?;
            return Err(AppError::from_http("GitHub", "issue fetch", status, &body).into());
        }
        let issue: IssueItem = resp
            .json()
            .map_err(|err| app_error_from_decode("GitHub", "issue fetch", err))?;
        let is_pr = issue.pull_request.is_some();

        match kind {
            ContentKind::Issue if is_pr && !allow_fallback_to_pr => {
                return Err(AppError::usage(format!(
                    "ID {issue_id} in repo {repo} is a pull request. Use --type pr or omit --type."
                ))
                .into());
            }
            ContentKind::Issue if is_pr && allow_fallback_to_pr => {
                warn!(
                    "Warning: --id defaulted to issue, but found PR #{issue_id}; use --type pr for clarity."
                );
            }
            ContentKind::Pr if !is_pr => {
                return Err(AppError::usage(format!(
                    "ID {issue_id} in repo {repo} is an issue, not a pull request."
                ))
                .into());
            }
            _ => {}
        }

        let conversation = self.fetch_conversation(
            repo,
            ConversationSeed {
                id: issue.number,
                title: issue.title,
                state: issue.state,
                body: issue.body,
                is_pr,
            },
            req,
        )?;
        emit(conversation)?;
        Ok(1)
    }
}

#[async_trait(?Send)]
impl Source for GitHubSource {
    async fn fetch_stream(
        &self,
        req: &FetchRequest,
        emit: &mut dyn FnMut(Conversation) -> Result<()>,
    ) -> Result<usize> {
        block_in_place(|| match &req.target {
            FetchTarget::Search { raw_query } => self.search_stream(req, raw_query, emit),
            FetchTarget::Id {
                repo,
                id,
                kind,
                allow_fallback_to_pr,
            } => self.fetch_by_id_stream(req, repo, id, *kind, *allow_fallback_to_pr, emit),
        })
    }
}
