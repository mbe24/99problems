use anyhow::Result;
use reqwest::blocking::Client;
use tracing::warn;

use super::{ContentKind, FetchRequest, FetchTarget, Source};
use crate::error::AppError;
use crate::model::Conversation;

mod api;
mod model;
mod query;

use model::{ConversationSeed, GitLabIssueItem, GitLabMergeRequestItem};
use query::{build_search_params, encode_project_path, parse_gitlab_query};

pub(super) const GITLAB_DEFAULT_BASE_URL: &str = "https://gitlab.com";
pub(super) const PAGE_SIZE: u32 = 100;

pub struct GitLabSource {
    client: Client,
    base_url: String,
}

impl GitLabSource {
    /// Create a GitLab source client.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be constructed.
    pub fn new(base_url: Option<String>) -> Result<Self> {
        let client = Client::builder()
            .user_agent(concat!("99problems-cli/", env!("CARGO_PKG_VERSION")))
            .build()?;

        let base_url = base_url
            .unwrap_or_else(|| GITLAB_DEFAULT_BASE_URL.to_string())
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
                self.get_pages_stream(
                    &url,
                    &params,
                    req.token.as_deref(),
                    req.per_page,
                    false,
                    &mut |i: GitLabIssueItem| {
                        let conversation = self.fetch_conversation(
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
                        )?;
                        emit(conversation)?;
                        emitted += 1;
                        Ok(())
                    },
                )?;
            }
            ContentKind::Pr => {
                let url = format!("{}/api/v4/projects/{project}/merge_requests", self.base_url);
                self.get_pages_stream(
                    &url,
                    &params,
                    req.token.as_deref(),
                    req.per_page,
                    false,
                    &mut |mr: GitLabMergeRequestItem| {
                        let conversation = self.fetch_conversation(
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
                        )?;
                        emit(conversation)?;
                        emitted += 1;
                        Ok(())
                    },
                )?;
            }
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
        let iid = id.parse::<u64>().map_err(|_| {
            AppError::usage(format!("GitLab expects a numeric issue/MR id, got '{id}'."))
        })?;
        match kind {
            ContentKind::Issue => {
                if let Some(issue) = self.fetch_issue_by_iid(repo, iid, req.token.as_deref())? {
                    let conversation = self.fetch_conversation(
                        repo,
                        ConversationSeed {
                            id: issue.iid,
                            title: issue.title,
                            state: issue.state,
                            body: issue.description,
                            is_pr: false,
                            web_url: issue.web_url,
                        },
                        req,
                    )?;
                    emit(conversation)?;
                    return Ok(1);
                }

                if allow_fallback_to_pr
                    && let Some(mr) = self.fetch_mr_by_iid(repo, iid, req.token.as_deref())?
                {
                    warn!(
                        "Warning: --id defaulted to issue, but found MR !{iid}; use --type pr for clarity."
                    );
                    let conversation = self.fetch_conversation(
                        repo,
                        ConversationSeed {
                            id: mr.iid,
                            title: mr.title,
                            state: mr.state,
                            body: mr.description,
                            is_pr: true,
                            web_url: mr.web_url,
                        },
                        req,
                    )?;
                    emit(conversation)?;
                    return Ok(1);
                }

                Err(AppError::not_found(format!("Issue #{iid} not found in repo {repo}.")).into())
            }
            ContentKind::Pr => {
                if let Some(mr) = self.fetch_mr_by_iid(repo, iid, req.token.as_deref())? {
                    let conversation = self.fetch_conversation(
                        repo,
                        ConversationSeed {
                            id: mr.iid,
                            title: mr.title,
                            state: mr.state,
                            body: mr.description,
                            is_pr: true,
                            web_url: mr.web_url,
                        },
                        req,
                    )?;
                    emit(conversation)?;
                    return Ok(1);
                }

                if self
                    .fetch_issue_by_iid(repo, iid, req.token.as_deref())?
                    .is_some()
                {
                    return Err(AppError::usage(format!(
                        "ID {iid} in repo {repo} is an issue, not a merge request."
                    ))
                    .into());
                }

                Err(
                    AppError::not_found(format!("Merge request !{iid} not found in repo {repo}."))
                        .into(),
                )
            }
        }
    }
}

impl Source for GitLabSource {
    fn fetch_stream(
        &self,
        req: &FetchRequest,
        emit: &mut dyn FnMut(Conversation) -> Result<()>,
    ) -> Result<usize> {
        match &req.target {
            FetchTarget::Search { raw_query } => self.search_stream(req, raw_query, emit),
            FetchTarget::Id {
                repo,
                id,
                kind,
                allow_fallback_to_pr,
            } => self.fetch_by_id_stream(req, repo, id, *kind, *allow_fallback_to_pr, emit),
        }
    }
}
