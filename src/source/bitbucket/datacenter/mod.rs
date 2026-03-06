use anyhow::Result;

use self::model::{
    BitbucketDcActivityItem, BitbucketDcPullRequestItem, collect_comments_from_activity,
    matches_pr_filters,
};
use super::BitbucketSource;
use super::query::{parse_bitbucket_query, parse_project_repo};
use crate::error::AppError;
use crate::model::{Comment, Conversation, ConversationMetadata};
use crate::source::{ContentKind, FetchRequest, FetchTarget};

mod api;
mod model;

impl BitbucketSource {
    pub(super) fn fetch_datacenter_stream(
        &self,
        req: &FetchRequest,
        emit: &mut dyn FnMut(Conversation) -> Result<()>,
    ) -> Result<usize> {
        match &req.target {
            FetchTarget::Search { raw_query } => {
                self.search_datacenter_stream(req, raw_query, emit)
            }
            FetchTarget::Id {
                repo,
                id,
                kind,
                allow_fallback_to_pr: _,
            } => self.fetch_datacenter_by_id_stream(req, repo, id, *kind, emit),
        }
    }

    fn search_datacenter_stream(
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
    }

    fn search_datacenter_prs_stream(
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

        let mut emitted = 0usize;
        self.datacenter_get_pages_stream(
            &url,
            &params,
            req.token.as_deref(),
            req.per_page,
            &mut |item: BitbucketDcPullRequestItem| {
                if !matches_pr_filters(&item, filters) {
                    return Ok(());
                }
                emit(self.fetch_datacenter_pr_conversation(repo, item, req)?)?;
                emitted += 1;
                Ok(())
            },
        )?;

        Ok(emitted)
    }

    fn fetch_datacenter_by_id_stream(
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

        if let Some(pr) = self.fetch_datacenter_pr_by_id(repo, id, req)? {
            emit(self.fetch_datacenter_pr_conversation(repo, pr, req)?)?;
            return Ok(1);
        }

        Err(AppError::not_found(format!("Pull request #{id} not found in repo {repo}.")).into())
    }

    fn fetch_datacenter_pr_by_id(
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
    }

    fn fetch_datacenter_pr_conversation(
        &self,
        repo: &str,
        item: BitbucketDcPullRequestItem,
        req: &FetchRequest,
    ) -> Result<Conversation> {
        let comments = if req.include_comments {
            self.fetch_datacenter_pr_comments(repo, item.id, req)?
        } else {
            Vec::new()
        };

        Ok(Conversation {
            id: item.id.to_string(),
            title: item.title,
            state: item.state,
            body: item.description.filter(|body| !body.is_empty()),
            comments,
            metadata: ConversationMetadata::empty(),
        })
    }

    fn fetch_datacenter_pr_comments(
        &self,
        repo: &str,
        id: u64,
        req: &FetchRequest,
    ) -> Result<Vec<Comment>> {
        let (project, repo_slug) = parse_project_repo(Some(repo))?;
        let url = format!(
            "{}/rest/api/latest/projects/{project}/repos/{repo_slug}/pull-requests/{id}/activities",
            self.base_url
        );

        let mut comments = Vec::new();
        self.datacenter_get_pages_stream(
            &url,
            &[],
            req.token.as_deref(),
            req.per_page,
            &mut |item: BitbucketDcActivityItem| {
                collect_comments_from_activity(item, req.include_review_comments, &mut comments);
                Ok(())
            },
        )?;

        comments.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        Ok(comments)
    }
}
