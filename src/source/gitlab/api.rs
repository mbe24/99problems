use std::collections::HashSet;

use anyhow::Result;
use reqwest::StatusCode;
use reqwest::blocking::{RequestBuilder, Response};
use serde::Deserialize;
use tracing::{debug, trace, warn};

use super::model::{
    ConversationSeed, GitLabDiscussion, GitLabIssueItem, GitLabIssueLinkItem, GitLabLinkIssueRef,
    GitLabMergeRequestItem, GitLabMergeRequestRef, GitLabNote, map_closed_by_link,
    map_closes_issue_link, map_issue_link, map_note_comment, map_review_comment,
};
use super::query::encode_project_path;
use super::{GitLabSource, PAGE_SIZE};
use crate::error::{AppError, app_error_from_decode, app_error_from_reqwest};
use crate::model::{Comment, Conversation, ConversationLink, ConversationMetadata};
use crate::source::FetchRequest;

impl GitLabSource {
    pub(super) fn apply_auth(req: RequestBuilder, token: Option<&str>) -> RequestBuilder {
        match token {
            Some(t) => req.header("PRIVATE-TOKEN", t),
            None => req,
        }
    }

    pub(super) fn bounded_per_page(per_page: u32) -> u32 {
        per_page.clamp(1, PAGE_SIZE)
    }

    pub(super) fn send(req: RequestBuilder, operation: &str) -> Result<Response> {
        req.send()
            .map_err(|err| app_error_from_reqwest("GitLab", operation, &err).into())
    }

    pub(super) fn get_pages<T: for<'de> Deserialize<'de>>(
        &self,
        url: &str,
        params: &[(String, String)],
        token: Option<&str>,
        per_page: u32,
        allow_unauthenticated_empty: bool,
    ) -> Result<Vec<T>> {
        let mut results = vec![];
        self.get_pages_stream(
            url,
            params,
            token,
            per_page,
            allow_unauthenticated_empty,
            &mut |item| {
                results.push(item);
                Ok(())
            },
        )?;
        Ok(results)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn get_pages_stream<T: for<'de> Deserialize<'de>>(
        &self,
        url: &str,
        params: &[(String, String)],
        token: Option<&str>,
        per_page: u32,
        allow_unauthenticated_empty: bool,
        emit: &mut dyn FnMut(T) -> Result<()>,
    ) -> Result<usize> {
        let mut emitted = 0usize;
        let mut page = 1u32;
        let per_page = Self::bounded_per_page(per_page);

        loop {
            let mut query = params.to_vec();
            query.push(("per_page".into(), per_page.to_string()));
            query.push(("page".into(), page.to_string()));
            debug!(url = %url, page, per_page, "fetching GitLab page");

            let req = Self::apply_auth(self.client.get(url).query(&query), token);
            let resp = Self::send(req, "page fetch")?;

            if !resp.status().is_success() {
                let status = resp.status();
                if allow_unauthenticated_empty
                    && token.is_none()
                    && (status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN)
                {
                    return Ok(0);
                }
                if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
                    let body = resp.text()?;
                    let hint = if token.is_some() {
                        "GitLab token seems invalid or lacks required scope (use read_api)."
                    } else {
                        "No GitLab token detected. Set --token, GITLAB_TOKEN, or [instances.<alias>].token."
                    };
                    return Err(AppError::auth(format!(
                        "GitLab API auth error {status}: {hint} {body}"
                    ))
                    .with_provider("gitlab")
                    .with_http_status(status)
                    .into());
                }
                let body = resp
                    .text()
                    .map_err(|err| app_error_from_reqwest("GitLab", "error body read", &err))?;
                return Err(AppError::from_http("GitLab", "page fetch", status, &body).into());
            }

            let next_page = resp
                .headers()
                .get("x-next-page")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .trim()
                .to_string();

            let items: Vec<T> = resp
                .json()
                .map_err(|err| app_error_from_decode("GitLab", "page fetch", err))?;
            trace!(count = items.len(), page, "decoded GitLab page");
            for item in items {
                emit(item)?;
                emitted += 1;
            }

            if next_page.is_empty() {
                break;
            }

            page = next_page.parse::<u32>().unwrap_or(page + 1);
        }

        Ok(emitted)
    }

    pub(super) fn get_one<T: for<'de> Deserialize<'de>>(
        &self,
        url: &str,
        token: Option<&str>,
    ) -> Result<Option<T>> {
        let req = Self::apply_auth(self.client.get(url), token);
        let resp = Self::send(req, "single fetch")?;

        if resp.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if resp.status() == StatusCode::UNAUTHORIZED || resp.status() == StatusCode::FORBIDDEN {
            let status = resp.status();
            let hint = if token.is_some() {
                "GitLab token seems invalid or lacks required scope (use read_api)."
            } else {
                "No GitLab token detected. Set --token, GITLAB_TOKEN, or [instances.<alias>].token."
            };
            let body = resp.text()?;
            return Err(
                AppError::auth(format!("GitLab API auth error {status}: {hint} {body}"))
                    .with_provider("gitlab")
                    .with_http_status(status)
                    .into(),
            );
        }
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp
                .text()
                .map_err(|err| app_error_from_reqwest("GitLab", "error body read", &err))?;
            return Err(AppError::from_http("GitLab", "single fetch", status, &body).into());
        }

        Ok(Some(resp.json().map_err(|err| {
            app_error_from_decode("GitLab", "single fetch", err)
        })?))
    }

    pub(super) fn fetch_notes(
        &self,
        repo: &str,
        iid: u64,
        is_pr: bool,
        req: &FetchRequest,
    ) -> Result<Vec<Comment>> {
        let project = encode_project_path(repo);
        let url = if is_pr {
            format!(
                "{}/api/v4/projects/{project}/merge_requests/{iid}/notes",
                self.base_url
            )
        } else {
            format!(
                "{}/api/v4/projects/{project}/issues/{iid}/notes",
                self.base_url
            )
        };

        let notes: Vec<GitLabNote> =
            self.get_pages(&url, &[], req.token.as_deref(), req.per_page, true)?;
        Ok(notes
            .into_iter()
            .filter(|n| !n.system)
            .map(map_note_comment)
            .collect())
    }

    pub(super) fn fetch_review_comments(
        &self,
        repo: &str,
        iid: u64,
        req: &FetchRequest,
    ) -> Result<Vec<Comment>> {
        let project = encode_project_path(repo);
        let url = format!(
            "{}/api/v4/projects/{project}/merge_requests/{iid}/discussions",
            self.base_url
        );

        let discussions: Vec<GitLabDiscussion> =
            self.get_pages(&url, &[], req.token.as_deref(), req.per_page, true)?;
        let mut seen = HashSet::new();
        let mut comments = vec![];

        for discussion in discussions {
            for note in discussion.notes {
                if note.system || note.position.is_none() || !seen.insert(note.id) {
                    continue;
                }
                comments.push(map_review_comment(note));
            }
        }

        Ok(comments)
    }

    pub(super) fn fetch_links(
        &self,
        repo: &str,
        iid: u64,
        is_pr: bool,
        req: &FetchRequest,
    ) -> Result<ConversationMetadata> {
        if !req.include_links {
            return Ok(ConversationMetadata::empty());
        }

        let project = encode_project_path(repo);
        let mut links: Vec<ConversationLink> = Vec::new();
        if is_pr {
            let closes_url = format!(
                "{}/api/v4/projects/{project}/merge_requests/{iid}/closes_issues",
                self.base_url
            );
            let closed_issues: Vec<GitLabLinkIssueRef> =
                self.get_pages(&closes_url, &[], req.token.as_deref(), req.per_page, true)?;
            links.extend(closed_issues.into_iter().map(map_closes_issue_link));
        } else {
            let links_url = format!(
                "{}/api/v4/projects/{project}/issues/{iid}/links",
                self.base_url
            );
            let issue_links: Vec<GitLabIssueLinkItem> =
                self.get_pages(&links_url, &[], req.token.as_deref(), req.per_page, true)?;
            links.extend(
                issue_links
                    .iter()
                    .filter_map(|link| map_issue_link(link, iid)),
            );

            let closed_by_url = format!(
                "{}/api/v4/projects/{project}/issues/{iid}/closed_by",
                self.base_url
            );
            let closed_by: Vec<GitLabMergeRequestRef> = self.get_pages(
                &closed_by_url,
                &[],
                req.token.as_deref(),
                req.per_page,
                true,
            )?;
            links.extend(closed_by.into_iter().map(map_closed_by_link));
        }

        Ok(ConversationMetadata::with_links(links))
    }

    pub(super) fn fetch_conversation(
        &self,
        repo: &str,
        seed: ConversationSeed,
        req: &FetchRequest,
    ) -> Result<Conversation> {
        let mut comments = Vec::new();
        if req.include_comments {
            comments = self.fetch_notes(repo, seed.id, seed.is_pr, req)?;
            if seed.is_pr && req.include_review_comments {
                comments.extend(self.fetch_review_comments(repo, seed.id, req)?);
                comments.sort_by(|a, b| a.created_at.cmp(&b.created_at));
            }
        }
        let metadata = if req.include_links {
            match self.fetch_links(repo, seed.id, seed.is_pr, req) {
                Ok(metadata) => metadata,
                Err(err) => {
                    warn!(
                        id = seed.id,
                        repo,
                        error = %err,
                        "GitLab links fetch failed; continuing without links"
                    );
                    ConversationMetadata::empty()
                }
            }
        } else {
            ConversationMetadata::empty()
        };

        Ok(Conversation {
            id: seed.id.to_string(),
            title: seed.title,
            state: seed.state,
            body: seed.body,
            comments,
            metadata,
        })
    }

    pub(super) fn fetch_issue_by_iid(
        &self,
        repo: &str,
        iid: u64,
        token: Option<&str>,
    ) -> Result<Option<GitLabIssueItem>> {
        let project = encode_project_path(repo);
        let url = format!("{}/api/v4/projects/{project}/issues/{iid}", self.base_url);
        self.get_one(&url, token)
    }

    pub(super) fn fetch_mr_by_iid(
        &self,
        repo: &str,
        iid: u64,
        token: Option<&str>,
    ) -> Result<Option<GitLabMergeRequestItem>> {
        let project = encode_project_path(repo);
        let url = format!(
            "{}/api/v4/projects/{project}/merge_requests/{iid}",
            self.base_url
        );
        self.get_one(&url, token)
    }
}
