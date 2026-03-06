use anyhow::Result;
use tracing::warn;

use super::BitbucketSource;
use super::model::{
    BitbucketCommentItem, BitbucketIssueItem, BitbucketPullRequestItem, map_issue_comment,
    map_pr_comment, matches_issue_filters, matches_pr_filters,
};
use super::query::{BitbucketFilters, parse_bitbucket_query, parse_repo};
use crate::error::AppError;
use crate::model::Conversation;
use crate::source::{ContentKind, FetchRequest, FetchTarget};

impl BitbucketSource {
    pub(super) fn fetch_cloud_stream(
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

    fn search_stream(
        &self,
        req: &FetchRequest,
        raw_query: &str,
        emit: &mut dyn FnMut(Conversation) -> Result<()>,
    ) -> Result<usize> {
        let filters = parse_bitbucket_query(raw_query);
        let (workspace, repo_slug) = parse_repo(filters.repo.as_deref())?;
        let repo = format!("{workspace}/{repo_slug}");

        match filters.kind {
            ContentKind::Issue => self.search_issues_stream(req, &repo, &filters, emit),
            ContentKind::Pr => self.search_prs_stream(req, &repo, &filters, emit),
        }
    }

    fn search_issues_stream(
        &self,
        req: &FetchRequest,
        repo: &str,
        filters: &BitbucketFilters,
        emit: &mut dyn FnMut(Conversation) -> Result<()>,
    ) -> Result<usize> {
        let url = format!("{}/repositories/{repo}/issues", self.base_url);
        let params = vec![("sort".to_string(), "-updated_on".to_string())];
        let mut emitted = 0usize;
        self.get_pages_stream(
            &url,
            &params,
            req.token.as_deref(),
            req.account_email.as_deref(),
            req.per_page,
            &mut |item: BitbucketIssueItem| {
                if !matches_issue_filters(&item, filters) {
                    return Ok(());
                }
                let conversation = self.fetch_issue_conversation(repo, item, req)?;
                emit(conversation)?;
                emitted += 1;
                Ok(())
            },
        )?;
        Ok(emitted)
    }

    fn search_prs_stream(
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
        let mut emitted = 0usize;
        self.get_pages_stream(
            &url,
            &params,
            req.token.as_deref(),
            req.account_email.as_deref(),
            req.per_page,
            &mut |item: BitbucketPullRequestItem| {
                if !matches_pr_filters(&item, filters) {
                    return Ok(());
                }
                let conversation = self.fetch_pr_conversation(repo, item, req)?;
                emit(conversation)?;
                emitted += 1;
                Ok(())
            },
        )?;
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
        let (workspace, repo_slug) = parse_repo(Some(repo))?;
        let repo = format!("{workspace}/{repo_slug}");
        let id = id.parse::<u64>().map_err(|_| {
            AppError::usage(format!(
                "Bitbucket expects a numeric issue/PR id, got '{id}'."
            ))
        })?;
        match kind {
            ContentKind::Issue => {
                if let Some(issue) = self.fetch_issue_by_id(&repo, id, req)? {
                    emit(self.fetch_issue_conversation(&repo, issue, req)?)?;
                    return Ok(1);
                }
                if allow_fallback_to_pr && let Some(pr) = self.fetch_pr_by_id(&repo, id, req)? {
                    warn!(
                        "Warning: --id defaulted to issue, but found PR #{id}; use --type pr for clarity."
                    );
                    emit(self.fetch_pr_conversation(&repo, pr, req)?)?;
                    return Ok(1);
                }
                Err(AppError::not_found(format!("Issue #{id} not found in repo {repo}.")).into())
            }
            ContentKind::Pr => {
                if let Some(pr) = self.fetch_pr_by_id(&repo, id, req)? {
                    emit(self.fetch_pr_conversation(&repo, pr, req)?)?;
                    return Ok(1);
                }
                if self.fetch_issue_by_id(&repo, id, req)?.is_some() {
                    return Err(AppError::usage(format!(
                        "ID {id} in repo {repo} is an issue, not a pull request."
                    ))
                    .into());
                }
                Err(
                    AppError::not_found(format!("Pull request #{id} not found in repo {repo}."))
                        .into(),
                )
            }
        }
    }

    fn fetch_issue_by_id(
        &self,
        repo: &str,
        id: u64,
        req: &FetchRequest,
    ) -> Result<Option<BitbucketIssueItem>> {
        let url = format!("{}/repositories/{repo}/issues/{id}", self.base_url);
        self.get_one(
            &url,
            req.token.as_deref(),
            req.account_email.as_deref(),
            "issue fetch",
        )
    }

    fn fetch_pr_by_id(
        &self,
        repo: &str,
        id: u64,
        req: &FetchRequest,
    ) -> Result<Option<BitbucketPullRequestItem>> {
        let url = format!("{}/repositories/{repo}/pullrequests/{id}", self.base_url);
        self.get_one(
            &url,
            req.token.as_deref(),
            req.account_email.as_deref(),
            "pull request fetch",
        )
    }

    fn fetch_issue_conversation(
        &self,
        repo: &str,
        item: BitbucketIssueItem,
        req: &FetchRequest,
    ) -> Result<Conversation> {
        let comments = if req.include_comments {
            self.fetch_issue_comments(repo, item.id, req)?
        } else {
            Vec::new()
        };
        Ok(Conversation {
            id: item.id.to_string(),
            title: item.title,
            state: item.state,
            body: item.content.and_then(|c| c.raw).filter(|b| !b.is_empty()),
            comments,
        })
    }

    fn fetch_pr_conversation(
        &self,
        repo: &str,
        item: BitbucketPullRequestItem,
        req: &FetchRequest,
    ) -> Result<Conversation> {
        let comments = if req.include_comments {
            self.fetch_pr_comments(repo, item.id, req)?
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
        })
    }

    fn fetch_issue_comments(
        &self,
        repo: &str,
        id: u64,
        req: &FetchRequest,
    ) -> Result<Vec<crate::model::Comment>> {
        let url = format!("{}/repositories/{repo}/issues/{id}/comments", self.base_url);
        let mut comments = Vec::new();
        self.get_pages_stream(
            &url,
            &[],
            req.token.as_deref(),
            req.account_email.as_deref(),
            req.per_page,
            &mut |item: BitbucketCommentItem| {
                if item.deleted.unwrap_or(false) {
                    return Ok(());
                }
                comments.push(map_issue_comment(item));
                Ok(())
            },
        )?;
        Ok(comments)
    }

    fn fetch_pr_comments(
        &self,
        repo: &str,
        id: u64,
        req: &FetchRequest,
    ) -> Result<Vec<crate::model::Comment>> {
        let url = format!(
            "{}/repositories/{repo}/pullrequests/{id}/comments",
            self.base_url
        );
        let mut comments = Vec::new();
        self.get_pages_stream(
            &url,
            &[],
            req.token.as_deref(),
            req.account_email.as_deref(),
            req.per_page,
            &mut |item: BitbucketCommentItem| {
                if item.deleted.unwrap_or(false) {
                    return Ok(());
                }
                if let Some(mapped) = map_pr_comment(item, req.include_review_comments) {
                    comments.push(mapped);
                }
                Ok(())
            },
        )?;
        comments.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        Ok(comments)
    }
}
