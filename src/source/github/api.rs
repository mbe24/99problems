use anyhow::Result;
use reqwest::blocking::{RequestBuilder, Response};
use serde::Deserialize;
use tracing::{debug, trace, warn};

use super::model::{
    ConversationSeed, IssueCommentItem, ReviewCommentItem, map_issue_comment, map_review_comment,
    map_timeline_links,
};
use super::{GITHUB_API_BASE, GITHUB_API_VERSION, GitHubSource, PAGE_SIZE};
use crate::error::{AppError, app_error_from_decode, app_error_from_reqwest};
use crate::model::{Comment, Conversation, ConversationLink, ConversationMetadata};
use crate::source::FetchRequest;

impl GitHubSource {
    pub(super) fn apply_auth(req: RequestBuilder, token: Option<&str>) -> RequestBuilder {
        match token {
            Some(t) => req
                .header("Authorization", format!("Bearer {t}"))
                .header("X-GitHub-Api-Version", GITHUB_API_VERSION),
            None => req,
        }
    }

    pub(super) fn bounded_per_page(per_page: u32) -> u32 {
        per_page.clamp(1, PAGE_SIZE)
    }

    pub(super) fn send(req: RequestBuilder, operation: &str) -> Result<Response> {
        req.send()
            .map_err(|err| app_error_from_reqwest("GitHub", operation, &err).into())
    }

    pub(super) fn get_pages<T: for<'de> Deserialize<'de>>(
        &self,
        url: &str,
        token: Option<&str>,
        per_page: u32,
    ) -> Result<Vec<T>> {
        let mut results = vec![];
        let mut page = 1u32;
        let per_page = Self::bounded_per_page(per_page);

        loop {
            debug!(url = %url, page, per_page, "fetching GitHub page");
            let req = self.client.get(url).query(&[
                ("per_page", per_page.to_string()),
                ("page", page.to_string()),
            ]);
            let req = Self::apply_auth(req, token);
            let resp = Self::send(req, "page fetch")?;

            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp
                    .text()
                    .map_err(|err| app_error_from_reqwest("GitHub", "error body read", &err))?;
                return Err(AppError::from_http("GitHub", "page fetch", status, &body).into());
            }

            let has_next = resp
                .headers()
                .get("link")
                .and_then(|v| v.to_str().ok())
                .is_some_and(|l| l.contains(r#"rel="next""#));

            let items: Vec<T> = resp
                .json()
                .map_err(|err| app_error_from_decode("GitHub", "page fetch", err))?;
            trace!(count = items.len(), page, "decoded GitHub page");
            let done = items.is_empty() || !has_next;
            results.extend(items);
            if done {
                break;
            }
            page += 1;
        }

        Ok(results)
    }

    pub(super) fn fetch_issue_comments(
        &self,
        repo: &str,
        id: u64,
        req: &FetchRequest,
    ) -> Result<Vec<Comment>> {
        let comments_url = format!("{GITHUB_API_BASE}/repos/{repo}/issues/{id}/comments");
        let raw_comments: Vec<IssueCommentItem> =
            self.get_pages(&comments_url, req.token.as_deref(), req.per_page)?;
        Ok(raw_comments.into_iter().map(map_issue_comment).collect())
    }

    pub(super) fn fetch_review_comments(
        &self,
        repo: &str,
        id: u64,
        req: &FetchRequest,
    ) -> Result<Vec<Comment>> {
        let comments_url = format!("{GITHUB_API_BASE}/repos/{repo}/pulls/{id}/comments");
        let raw_comments: Vec<ReviewCommentItem> =
            self.get_pages(&comments_url, req.token.as_deref(), req.per_page)?;
        Ok(raw_comments.into_iter().map(map_review_comment).collect())
    }

    pub(super) fn fetch_links(
        &self,
        repo: &str,
        id: u64,
        req: &FetchRequest,
    ) -> Result<ConversationMetadata> {
        if !req.include_links {
            return Ok(ConversationMetadata::empty());
        }

        let timeline_url = format!("{GITHUB_API_BASE}/repos/{repo}/issues/{id}/timeline");
        let events: Vec<serde_json::Value> =
            self.get_pages(&timeline_url, req.token.as_deref(), req.per_page)?;
        let mut links: Vec<ConversationLink> = Vec::new();
        for event in events {
            links.extend(map_timeline_links(&event));
        }
        Ok(ConversationMetadata::with_links(links))
    }

    pub(super) fn fetch_conversation(
        &self,
        repo: &str,
        item: ConversationSeed,
        req: &FetchRequest,
    ) -> Result<Conversation> {
        let mut comments = Vec::new();
        if req.include_comments {
            comments = self.fetch_issue_comments(repo, item.id, req)?;
            if item.is_pr && req.include_review_comments {
                comments.extend(self.fetch_review_comments(repo, item.id, req)?);
                comments.sort_by(|a, b| a.created_at.cmp(&b.created_at));
            }
        }
        let metadata = if req.include_links {
            match self.fetch_links(repo, item.id, req) {
                Ok(metadata) => metadata,
                Err(err) => {
                    warn!(
                        id = item.id,
                        repo,
                        error = %err,
                        "GitHub links fetch failed; continuing without links"
                    );
                    ConversationMetadata::empty()
                }
            }
        } else {
            ConversationMetadata::empty()
        };

        Ok(Conversation {
            id: item.id.to_string(),
            title: item.title,
            state: item.state,
            body: item.body,
            comments,
            metadata,
        })
    }
}
