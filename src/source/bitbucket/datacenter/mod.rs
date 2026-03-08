use anyhow::Result;
use tracing::warn;

use self::model::{
    BitbucketDcActivityItem, BitbucketDcPullRequestItem, collect_comments_from_activity,
    map_linked_jira_issues, map_url_links, matches_pr_filters,
};
use super::BitbucketSource;
use super::query::{parse_bitbucket_query, parse_project_repo};
use crate::error::AppError;
use crate::model::{Comment, Conversation, ConversationMetadata};
use crate::source::{ContentKind, FetchRequest, FetchTarget};

mod api;
mod model;

impl BitbucketSource {
    pub(super) async fn fetch_datacenter_stream(
        &self,
        req: &FetchRequest,
        emit: &mut dyn FnMut(Conversation) -> Result<()>,
    ) -> Result<usize> {
        match &req.target {
            FetchTarget::Search { raw_query } => {
                self.search_datacenter_stream(req, raw_query, emit).await
            }
            FetchTarget::Id {
                repo,
                id,
                kind,
                allow_fallback_to_pr: _,
            } => {
                self.fetch_datacenter_by_id_stream(req, repo, id, *kind, emit)
                    .await
            }
        }
    }

    async fn search_datacenter_stream(
        &self,
        req: &FetchRequest,
        raw_query: &str,
        emit: &mut dyn FnMut(Conversation) -> Result<()>,
    ) -> Result<usize> {
        let mut filters = parse_bitbucket_query(raw_query);
        if !filters.kind_explicit {
            filters.kind = ContentKind::Pr;
        }
        if matches!(filters.kind, ContentKind::Issue) {
            return Err(AppError::usage(
                "Bitbucket Data Center supports pull requests only. Use --type pr or omit --type.",
            )
            .into());
        }

        let (project, repo_slug) = parse_project_repo(filters.repo.as_deref())?;
        let repo = format!("{project}/{repo_slug}");
        self.search_datacenter_prs_stream(req, &repo, &filters, emit)
            .await
    }

    async fn search_datacenter_prs_stream(
        &self,
        req: &FetchRequest,
        repo: &str,
        filters: &super::query::BitbucketFilters,
        emit: &mut dyn FnMut(Conversation) -> Result<()>,
    ) -> Result<usize> {
        let (project, repo_slug) = parse_project_repo(Some(repo))?;
        let url = format!(
            "{}/rest/api/latest/projects/{project}/repos/{repo_slug}/pull-requests",
            self.base_url
        );
        let params = vec![("state".to_string(), "ALL".to_string())];

        let items: Vec<BitbucketDcPullRequestItem> = self
            .datacenter_get_pages(&url, &params, req.token.as_deref(), req.per_page)
            .await?;

        let mut emitted = 0usize;
        for item in items {
            if !matches_pr_filters(&item, filters) {
                continue;
            }
            emit(
                self.fetch_datacenter_pr_conversation(repo, item, req)
                    .await?,
            )?;
            emitted += 1;
        }

        Ok(emitted)
    }

    async fn fetch_datacenter_by_id_stream(
        &self,
        req: &FetchRequest,
        repo: &str,
        id: &str,
        kind: ContentKind,
        emit: &mut dyn FnMut(Conversation) -> Result<()>,
    ) -> Result<usize> {
        let id = id.parse::<u64>().map_err(|_| {
            AppError::usage(format!(
                "Bitbucket Data Center expects a numeric pull request id, got '{id}'."
            ))
        })?;

        if matches!(kind, ContentKind::Issue) {
            return Err(AppError::usage(
                "Bitbucket Data Center supports pull requests only. Use --type pr or omit --type.",
            )
            .into());
        }

        if let Some(pr) = self.fetch_datacenter_pr_by_id(repo, id, req).await? {
            emit(self.fetch_datacenter_pr_conversation(repo, pr, req).await?)?;
            return Ok(1);
        }

        Err(AppError::not_found(format!("Pull request #{id} not found in repo {repo}.")).into())
    }

    async fn fetch_datacenter_pr_by_id(
        &self,
        repo: &str,
        id: u64,
        req: &FetchRequest,
    ) -> Result<Option<BitbucketDcPullRequestItem>> {
        let (project, repo_slug) = parse_project_repo(Some(repo))?;
        let url = format!(
            "{}/rest/api/latest/projects/{project}/repos/{repo_slug}/pull-requests/{id}",
            self.base_url
        );
        self.datacenter_get_one(&url, req.token.as_deref(), "pull request fetch")
            .await
    }

    async fn fetch_datacenter_pr_conversation(
        &self,
        repo: &str,
        item: BitbucketDcPullRequestItem,
        req: &FetchRequest,
    ) -> Result<Conversation> {
        let comments = if req.include_comments {
            self.fetch_datacenter_pr_comments(repo, item.id, req)
                .await?
        } else {
            Vec::new()
        };

        Ok(Conversation {
            id: item.id.to_string(),
            title: item.title,
            state: item.state,
            body: item.description.filter(|body| !body.is_empty()),
            comments,
            metadata: if req.include_links {
                match self
                    .fetch_datacenter_links(repo, item.id, item.links.as_ref(), req)
                    .await
                {
                    Ok(metadata) => metadata,
                    Err(err) => {
                        warn!(
                            repo,
                            id = item.id,
                            error = %err,
                            "Bitbucket Data Center links fetch failed; continuing without links"
                        );
                        ConversationMetadata::empty()
                    }
                }
            } else {
                ConversationMetadata::empty()
            },
        })
    }

    async fn fetch_datacenter_links(
        &self,
        repo: &str,
        id: u64,
        pr_links: Option<&serde_json::Value>,
        req: &FetchRequest,
    ) -> Result<ConversationMetadata> {
        let span = tracing::debug_span!(
            "bitbucket.hydrate.links",
            bitbucket.links.count = tracing::field::Empty
        );
        let _span_guard = span.enter();
        let mut links = map_url_links(pr_links);
        links.extend(
            self.fetch_datacenter_linked_jira_issues(repo, id, req)
                .await?,
        );
        span.record(
            "bitbucket.links.count",
            i64::try_from(links.len()).unwrap_or(i64::MAX),
        );
        Ok(ConversationMetadata::with_links(links))
    }

    async fn fetch_datacenter_linked_jira_issues(
        &self,
        repo: &str,
        id: u64,
        req: &FetchRequest,
    ) -> Result<Vec<crate::model::ConversationLink>> {
        let (project, repo_slug) = parse_project_repo(Some(repo))?;
        let url = format!(
            "{}/rest/jira/latest/projects/{project}/repos/{repo_slug}/pull-requests/{id}/issues",
            self.base_url
        );
        if let Some(payload) = self
            .datacenter_get_one::<serde_json::Value>(
                &url,
                req.token.as_deref(),
                "jira issue links fetch",
            )
            .await?
        {
            return Ok(map_linked_jira_issues(&payload));
        }
        Ok(Vec::new())
    }

    async fn fetch_datacenter_pr_comments(
        &self,
        repo: &str,
        id: u64,
        req: &FetchRequest,
    ) -> Result<Vec<Comment>> {
        let span = tracing::debug_span!(
            "bitbucket.hydrate.issue_comments",
            bitbucket.comments.count = tracing::field::Empty
        );
        let _span_guard = span.enter();
        let (project, repo_slug) = parse_project_repo(Some(repo))?;
        let url = format!(
            "{}/rest/api/latest/projects/{project}/repos/{repo_slug}/pull-requests/{id}/activities",
            self.base_url
        );

        let items: Vec<BitbucketDcActivityItem> = self
            .datacenter_get_pages(&url, &[], req.token.as_deref(), req.per_page)
            .await?;

        let mut comments = Vec::new();
        for item in items {
            collect_comments_from_activity(item, req.include_review_comments, &mut comments);
        }

        comments.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        span.record(
            "bitbucket.comments.count",
            i64::try_from(comments.len()).unwrap_or(i64::MAX),
        );
        Ok(comments)
    }
}
