use anyhow::Result;
use tracing::Instrument;

use self::model::{
    BitbucketCommentItem, BitbucketPullRequestItem, map_pr_comment, map_url_links,
    matches_pr_filters,
};
use super::BitbucketSource;
use super::query::{BitbucketFilters, parse_bitbucket_query, parse_workspace_repo};
use crate::error::AppError;
use crate::model::{Conversation, ConversationMetadata};
use crate::source::{ContentKind, FetchRequest, FetchTarget};

mod api;
mod model;

impl BitbucketSource {
    pub(super) async fn fetch_cloud_stream(
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

    async fn search_stream(
        &self,
        req: &FetchRequest,
        raw_query: &str,
        emit: &mut dyn FnMut(Conversation) -> Result<()>,
    ) -> Result<usize> {
        let filters = parse_bitbucket_query(raw_query);
        if matches!(filters.kind, ContentKind::Issue) {
            return Err(AppError::usage(
                "Bitbucket Cloud supports pull requests only. Use --type pr or omit --type.",
            )
            .into());
        }
        let (workspace, repo_slug) = parse_workspace_repo(filters.repo.as_deref())?;
        let repo = format!("{workspace}/{repo_slug}");

        self.search_prs_stream(req, &repo, &filters, emit).await
    }

    async fn search_prs_stream(
        &self,
        req: &FetchRequest,
        repo: &str,
        filters: &BitbucketFilters,
        emit: &mut dyn FnMut(Conversation) -> Result<()>,
    ) -> Result<usize> {
        let url = format!("{}/repositories/{repo}/pullrequests", self.base_url);
        let params = vec![
            ("sort".to_string(), "-updated_on".to_string()),
            ("state".to_string(), "ALL".to_string()),
        ];
        let items: Vec<BitbucketPullRequestItem> = self
            .cloud_get_pages(&url, &params, req.token.as_deref(), req.per_page)
            .await?;

        let mut emitted = 0usize;
        for item in items {
            if !matches_pr_filters(&item, filters) {
                continue;
            }
            let conversation = self.fetch_pr_conversation(repo, item, req).await?;
            emit(conversation)?;
            emitted += 1;
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
        let (workspace, repo_slug) = parse_workspace_repo(Some(repo))?;
        let repo = format!("{workspace}/{repo_slug}");
        let id = id.parse::<u64>().map_err(|_| {
            AppError::usage(format!(
                "Bitbucket Cloud expects a numeric pull request id, got '{id}'."
            ))
        })?;
        match kind {
            ContentKind::Issue => Err(AppError::usage(
                "Bitbucket Cloud supports pull requests only. Use --type pr or omit --type.",
            )
            .into()),
            ContentKind::Pr => {
                emit(self.fetch_pr_conversation_by_id(&repo, id, req).await?)?;
                Ok(1)
            }
        }
    }

    async fn fetch_pr_by_id(
        &self,
        repo: &str,
        id: u64,
        req: &FetchRequest,
    ) -> Result<Option<BitbucketPullRequestItem>> {
        let url = format!("{}/repositories/{repo}/pullrequests/{id}", self.base_url);
        self.cloud_get_one(&url, req.token.as_deref(), "pull request fetch")
            .await
    }

    async fn fetch_pr_conversation(
        &self,
        repo: &str,
        item: BitbucketPullRequestItem,
        req: &FetchRequest,
    ) -> Result<Conversation> {
        let span = tracing::debug_span!(
            "bitbucket.hydrate.issue",
            include_comments = req.include_comments,
            include_review_comments = req.include_review_comments,
            include_links = req.include_links,
            is_pr = true
        );
        async {
            let comments = if req.include_comments {
                self.fetch_pr_comments(repo, item.id, req).await?
            } else {
                Vec::new()
            };
            let body = item
                .description
                .or_else(|| item.summary.and_then(|s| s.raw))
                .filter(|b| !b.is_empty());
            Ok(Conversation {
                id: item.id.to_string(),
                title: item.title,
                state: item.state,
                body,
                comments,
                metadata: if req.include_links {
                    ConversationMetadata::with_links(map_url_links(item.links.as_ref()))
                } else {
                    ConversationMetadata::empty()
                },
            })
        }
        .instrument(span.clone())
        .await
    }

    async fn fetch_pr_conversation_by_id(
        &self,
        repo: &str,
        id: u64,
        req: &FetchRequest,
    ) -> Result<Conversation> {
        let span = tracing::debug_span!(
            "bitbucket.hydrate.issue",
            include_comments = req.include_comments,
            include_review_comments = req.include_review_comments,
            include_links = req.include_links,
            is_pr = true
        );
        async {
            let pr_task = self.fetch_pr_by_id(repo, id, req);
            let comments_task = async {
                if req.include_comments {
                    self.fetch_pr_comments(repo, id, req).await
                } else {
                    Ok(Vec::new())
                }
            };
            let (pr_result, comments_result) = tokio::join!(pr_task, comments_task);
            let pr = match pr_result? {
                Some(pr) => pr,
                None => {
                    return Err(AppError::not_found(format!(
                        "Pull request #{id} not found in repo {repo}."
                    ))
                    .into());
                }
            };

            let comments = comments_result?;
            let body = pr
                .description
                .or_else(|| pr.summary.and_then(|s| s.raw))
                .filter(|b| !b.is_empty());
            Ok(Conversation {
                id: pr.id.to_string(),
                title: pr.title,
                state: pr.state,
                body,
                comments,
                metadata: if req.include_links {
                    ConversationMetadata::with_links(map_url_links(pr.links.as_ref()))
                } else {
                    ConversationMetadata::empty()
                },
            })
        }
        .instrument(span.clone())
        .await
    }

    async fn fetch_pr_comments(
        &self,
        repo: &str,
        id: u64,
        req: &FetchRequest,
    ) -> Result<Vec<crate::model::Comment>> {
        let span = tracing::debug_span!(
            "bitbucket.hydrate.issue_comments",
            bitbucket.comments.count = tracing::field::Empty
        );
        async {
            let url = format!(
                "{}/repositories/{repo}/pullrequests/{id}/comments",
                self.base_url
            );
            let items: Vec<BitbucketCommentItem> = self
                .cloud_get_pages(&url, &[], req.token.as_deref(), req.per_page)
                .await?;

            let mut comments = Vec::new();
            for item in items {
                if item.deleted.unwrap_or(false) {
                    continue;
                }
                if let Some(mapped) = map_pr_comment(item, req.include_review_comments) {
                    comments.push(mapped);
                }
            }
            comments.sort_by(|a, b| a.created_at.cmp(&b.created_at));
            span.record(
                "bitbucket.comments.count",
                i64::try_from(comments.len()).unwrap_or(i64::MAX),
            );
            Ok(comments)
        }
        .instrument(span.clone())
        .await
    }
}
