use std::collections::{HashMap, HashSet};

use anyhow::Result;
use reqwest::StatusCode;
use reqwest::blocking::{RequestBuilder, Response};
use serde::Deserialize;
use tracing::{debug, debug_span, trace, warn};

use super::model::{
    ConversationSeed, GitLabDiscussion, GitLabIssueItem, GitLabIssueLinkItem,
    GitLabMergeRequestItem, GitLabMergeRequestRef, GitLabNote, GitLabRelatedIssueRef,
    map_closed_by_link, map_closes_related_issue_link, map_issue_link, map_note_comment,
    map_related_issue_link, map_related_mr_link, map_review_comment, map_url_reference,
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
        let _span = debug_span!("gitlab.http.send", operation = operation).entered();
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
            let _page_span = debug_span!(
                "gitlab.page.fetch",
                operation = "page fetch",
                page,
                per_page
            )
            .entered();
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

            let items: Vec<T> = {
                let _decode_span =
                    debug_span!("gitlab.page.decode", operation = "page fetch").entered();
                resp.json()
                    .map_err(|err| app_error_from_decode("GitLab", "page fetch", err))?
            };
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
        let _span = debug_span!("gitlab.single.fetch", operation = "single fetch").entered();
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

        let item = {
            let _decode_span =
                debug_span!("gitlab.single.decode", operation = "single fetch").entered();
            resp.json()
                .map_err(|err| app_error_from_decode("GitLab", "single fetch", err))?
        };
        Ok(Some(item))
    }

    pub(super) fn fetch_notes(
        &self,
        repo: &str,
        iid: u64,
        is_pr: bool,
        req: &FetchRequest,
    ) -> Result<Vec<Comment>> {
        let _span = debug_span!("gitlab.hydrate.issue_comments", is_pr).entered();
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
        let _span = debug_span!("gitlab.hydrate.review_comments").entered();
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
        conversation_url: Option<&str>,
        req: &FetchRequest,
    ) -> ConversationMetadata {
        let _span = debug_span!("gitlab.hydrate.links", is_pr).entered();
        if !req.include_links {
            return ConversationMetadata::empty();
        }

        let project = encode_project_path(repo);
        let mut links: Vec<ConversationLink> = Vec::new();
        if let Some(url) = conversation_url {
            links.push(map_url_reference(url));
        }
        if is_pr {
            self.collect_pr_links(&project, repo, iid, req, &mut links);
        } else {
            self.collect_issue_links(&project, repo, iid, req, &mut links);
        }

        prune_redundant_relates(&mut links);
        ConversationMetadata::with_links(links)
    }

    fn collect_pr_links(
        &self,
        project: &str,
        repo: &str,
        iid: u64,
        req: &FetchRequest,
        links: &mut Vec<ConversationLink>,
    ) {
        let closes_url = format!(
            "{}/api/v4/projects/{project}/merge_requests/{iid}/closes_issues",
            self.base_url
        );
        match self.get_pages::<GitLabRelatedIssueRef>(
            &closes_url,
            &[],
            req.token.as_deref(),
            req.per_page,
            true,
        ) {
            Ok(closed_issues) => {
                for issue in closed_issues {
                    if let Some(url) = issue.web_url.as_deref() {
                        links.push(map_url_reference(url));
                    }
                    if let Some(link) = map_closes_related_issue_link(&issue) {
                        links.push(link);
                    }
                }
            }
            Err(err) => warn!(repo, iid, error = %err, "GitLab closes_issues fetch failed"),
        }

        let related_issues_url = format!(
            "{}/api/v4/projects/{project}/merge_requests/{iid}/related_issues",
            self.base_url
        );
        match self.get_pages::<GitLabRelatedIssueRef>(
            &related_issues_url,
            &[],
            req.token.as_deref(),
            req.per_page,
            true,
        ) {
            Ok(related_issues) => {
                for issue in related_issues {
                    if let Some(url) = issue.web_url.as_deref() {
                        links.push(map_url_reference(url));
                    }
                    if let Some(link) = map_related_issue_link(&issue) {
                        links.push(link);
                    }
                }
            }
            Err(err) => warn!(repo, iid, error = %err, "GitLab related_issues fetch failed"),
        }
    }

    fn collect_issue_links(
        &self,
        project: &str,
        repo: &str,
        iid: u64,
        req: &FetchRequest,
        links: &mut Vec<ConversationLink>,
    ) {
        let links_url = format!(
            "{}/api/v4/projects/{project}/issues/{iid}/links",
            self.base_url
        );
        match self.get_pages::<GitLabIssueLinkItem>(
            &links_url,
            &[],
            req.token.as_deref(),
            req.per_page,
            true,
        ) {
            Ok(issue_links) => {
                links.extend(
                    issue_links
                        .iter()
                        .filter_map(|link| map_issue_link(link, iid)),
                );
            }
            Err(err) => warn!(repo, iid, error = %err, "GitLab issue links fetch failed"),
        }

        let closed_by_url = format!(
            "{}/api/v4/projects/{project}/issues/{iid}/closed_by",
            self.base_url
        );
        match self.get_pages::<GitLabMergeRequestRef>(
            &closed_by_url,
            &[],
            req.token.as_deref(),
            req.per_page,
            true,
        ) {
            Ok(closed_by) => {
                for mr in closed_by {
                    if let Some(url) = mr.web_url.as_deref() {
                        links.push(map_url_reference(url));
                    }
                    links.push(map_closed_by_link(&mr));
                }
            }
            Err(err) => warn!(repo, iid, error = %err, "GitLab closed_by fetch failed"),
        }

        let related_mr_url = format!(
            "{}/api/v4/projects/{project}/issues/{iid}/related_merge_requests",
            self.base_url
        );
        match self.get_pages::<GitLabMergeRequestRef>(
            &related_mr_url,
            &[],
            req.token.as_deref(),
            req.per_page,
            true,
        ) {
            Ok(related_mrs) => {
                for mr in related_mrs {
                    if let Some(url) = mr.web_url.as_deref() {
                        links.push(map_url_reference(url));
                    }
                    links.push(map_related_mr_link(&mr));
                }
            }
            Err(err) => warn!(
                repo,
                iid,
                error = %err,
                "GitLab related_merge_requests fetch failed"
            ),
        }
    }

    pub(super) fn fetch_conversation(
        &self,
        repo: &str,
        seed: ConversationSeed,
        req: &FetchRequest,
    ) -> Result<Conversation> {
        let _span = debug_span!(
            "gitlab.hydrate.conversation",
            include_comments = req.include_comments,
            include_review_comments = req.include_review_comments,
            include_links = req.include_links,
            is_pr = seed.is_pr
        )
        .entered();
        let mut comments = Vec::new();
        if req.include_comments {
            let _notes_span = debug_span!("gitlab.hydrate.notes.stage").entered();
            comments = self.fetch_notes(repo, seed.id, seed.is_pr, req)?;
            if seed.is_pr && req.include_review_comments {
                let _reviews_span = debug_span!("gitlab.hydrate.review_comments.stage").entered();
                comments.extend(self.fetch_review_comments(repo, seed.id, req)?);
                comments.sort_by(|a, b| a.created_at.cmp(&b.created_at));
            }
        }
        let metadata = if req.include_links {
            let _links_span = debug_span!("gitlab.hydrate.links.stage").entered();
            self.fetch_links(repo, seed.id, seed.is_pr, seed.web_url.as_deref(), req)
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

fn prune_redundant_relates(links: &mut Vec<ConversationLink>) {
    let mut strongest: HashMap<(String, Option<String>), u8> = HashMap::new();
    for link in links.iter() {
        let key = (link.id.clone(), link.kind.clone());
        let rank = relation_rank(link.relation.as_str());
        let current = strongest.entry(key).or_insert(rank);
        if rank > *current {
            *current = rank;
        }
    }
    links.retain(|link| {
        let key = (link.id.clone(), link.kind.clone());
        let best = strongest.get(&key).copied().unwrap_or(0);
        !(link.relation == "relates" && best > relation_rank("relates"))
    });
}

fn relation_rank(relation: &str) -> u8 {
    match relation {
        "closes" | "closed_by" | "blocks" | "blocked_by" | "parent" | "child" => 2,
        "relates" => 1,
        _ => 0,
    }
}
