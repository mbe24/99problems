use std::collections::{HashMap, HashSet};

use anyhow::Result;
use reqwest::StatusCode;
use reqwest::header::{CONTENT_TYPE, HeaderMap};
use reqwest_middleware::RequestBuilder;
use serde::Deserialize;
use tracing::{Instrument, debug, debug_span, trace, warn};

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
use crate::source::{ContentKind, FetchRequest};

pub(super) struct GitLabHttpPayload {
    status: StatusCode,
    content_type: String,
    body: String,
    headers: HeaderMap,
}

#[derive(Clone, Copy)]
pub(super) struct GitLabPageContext {
    page_kind: &'static str,
    operation: &'static str,
}

impl GitLabPageContext {
    pub(super) const fn new(page_kind: &'static str, operation: &'static str) -> Self {
        Self {
            page_kind,
            operation,
        }
    }
}

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

    #[cfg(feature = "telemetry-otel")]
    fn apply_otel_span_name(req: RequestBuilder, span_name: &'static str) -> RequestBuilder {
        req.with_extension(reqwest_tracing::OtelName(span_name.into()))
    }

    #[cfg(not(feature = "telemetry-otel"))]
    fn apply_otel_span_name(req: RequestBuilder, _span_name: &'static str) -> RequestBuilder {
        req
    }

    fn map_request_error(
        operation: &str,
        err: reqwest_middleware::Error,
    ) -> (AppError, &'static str, String) {
        match err {
            reqwest_middleware::Error::Reqwest(err) => {
                let message = err.to_string();
                (
                    app_error_from_reqwest("GitLab", operation, &err),
                    "request_send_error",
                    message,
                )
            }
            other @ reqwest_middleware::Error::Middleware(_) => {
                let message = other.to_string();
                (
                    AppError::provider(format!("GitLab API {operation} middleware error: {other}"))
                        .with_provider("gitlab"),
                    "request_middleware_error",
                    message,
                )
            }
        }
    }

    pub(super) async fn execute_request(
        req: RequestBuilder,
        operation: &str,
        span_name: &'static str,
    ) -> Result<GitLabHttpPayload> {
        let exchange_span = debug_span!(
            "gitlab.http.exchange",
            operation = operation,
            status_code = tracing::field::Empty,
            body_bytes = tracing::field::Empty,
            error.type = tracing::field::Empty,
            error.message = tracing::field::Empty
        );
        let response = Self::apply_otel_span_name(req, span_name)
            .send()
            .instrument(debug_span!("gitlab.http.request", operation = operation))
            .instrument(exchange_span.clone())
            .await;
        let response = match response {
            Ok(response) => response,
            Err(err) => {
                let (mapped, error_type, error_message) = Self::map_request_error(operation, err);
                exchange_span.record("error.type", error_type);
                exchange_span.record("error.message", error_message.as_str());
                return Err(mapped.into());
            }
        };

        let status = response.status();
        let headers = response.headers().clone();
        let content_type = headers
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_string();
        let body = {
            let read_span = debug_span!(
                "gitlab.http.response.read",
                operation = operation,
                status_code = tracing::field::Empty,
                error.type = tracing::field::Empty,
                error.message = tracing::field::Empty
            );
            read_span.record("status_code", i64::from(status.as_u16()));
            match response
                .text()
                .instrument(read_span.clone())
                .instrument(exchange_span.clone())
                .await
            {
                Ok(body) => body,
                Err(err) => {
                    let error_message = err.to_string();
                    read_span.record("error.type", "response_read_error");
                    read_span.record("error.message", error_message.as_str());
                    exchange_span.record("error.type", "response_read_error");
                    exchange_span.record("error.message", error_message.as_str());
                    return Err(app_error_from_reqwest("GitLab", operation, &err).into());
                }
            }
        };

        exchange_span.record("status_code", i64::from(status.as_u16()));
        exchange_span.record("body_bytes", usize_to_i64(body.len()));
        Ok(GitLabHttpPayload {
            status,
            content_type,
            body,
            headers,
        })
    }

    pub(super) fn decode_gitlab_json<T: for<'de> Deserialize<'de>>(
        payload: &GitLabHttpPayload,
        token: Option<&str>,
        operation: &str,
    ) -> Result<T> {
        let decode_span = debug_span!(
            "gitlab.http.decode",
            operation = operation,
            status_code = i64::from(payload.status.as_u16()),
            content_type = payload.content_type.as_str(),
            error.type = tracing::field::Empty,
            error.message = tracing::field::Empty
        );
        let _decode_guard = decode_span.enter();

        if !payload.status.is_success() {
            let error_message = format!("HTTP {}", payload.status.as_u16());
            decode_span.record("error.type", "http_status");
            decode_span.record("error.message", error_message.as_str());

            if payload.status == StatusCode::UNAUTHORIZED || payload.status == StatusCode::FORBIDDEN
            {
                let hint = if token.is_some() {
                    "GitLab token seems invalid or lacks required scope (use read_api)."
                } else {
                    "No GitLab token detected. Set --token, TOKEN_GITLAB (or GITLAB_TOKEN), or [instances.<alias>].token."
                };
                return Err(AppError::auth(format!(
                    "GitLab API auth error {}: {hint} {}",
                    payload.status,
                    body_snippet(&payload.body)
                ))
                .with_provider("gitlab")
                .with_http_status(payload.status)
                .into());
            }

            return Err(
                AppError::from_http("GitLab", operation, payload.status, &payload.body)
                    .with_provider("gitlab")
                    .into(),
            );
        }

        if !payload.content_type.contains("application/json") {
            let error_message = format!("unexpected content-type '{}'", payload.content_type);
            decode_span.record("error.type", "unexpected_content_type");
            decode_span.record("error.message", error_message.as_str());
            return Err(AppError::provider(format!(
                "GitLab API {} returned non-JSON content-type '{}' (body starts with: {}).",
                operation,
                payload.content_type,
                body_snippet(&payload.body)
            ))
            .with_provider("gitlab")
            .with_http_status(payload.status)
            .into());
        }

        match serde_json::from_str(&payload.body) {
            Ok(decoded) => Ok(decoded),
            Err(err) => {
                let error_message = format!("decode failed: {err}");
                decode_span.record("error.type", "decode_error");
                decode_span.record("error.message", error_message.as_str());
                Err(app_error_from_decode(
                    "GitLab",
                    operation,
                    format!("{err} (body starts with: {})", body_snippet(&payload.body)),
                )
                .into())
            }
        }
    }

    pub(super) async fn get_pages<T: for<'de> Deserialize<'de>>(
        &self,
        url: &str,
        params: &[(String, String)],
        token: Option<&str>,
        per_page: u32,
        allow_unauthenticated_empty: bool,
        context: GitLabPageContext,
    ) -> Result<Vec<T>> {
        let mut results = vec![];
        let mut page = 1u32;
        let per_page = Self::bounded_per_page(per_page);
        let page_kind = context.page_kind;
        let operation = context.operation;

        loop {
            let page_span = debug_span!(
                "gitlab.page.fetch",
                operation = operation,
                page_kind = page_kind,
                page,
                per_page
            );
            let mut query = params.to_vec();
            query.push(("per_page".into(), per_page.to_string()));
            query.push(("page".into(), page.to_string()));
            page_span.in_scope(|| debug!(url = %url, page, per_page, "fetching GitLab page"));

            let req = Self::apply_auth(self.client.get(url).query(&query), token);
            let payload = Self::execute_request(req, operation, "reqwest.http.get")
                .instrument(page_span.clone())
                .await?;

            if !payload.status.is_success() {
                if allow_unauthenticated_empty
                    && token.is_none()
                    && (payload.status == StatusCode::UNAUTHORIZED
                        || payload.status == StatusCode::FORBIDDEN)
                {
                    return Ok(vec![]);
                }
                if payload.status == StatusCode::UNAUTHORIZED
                    || payload.status == StatusCode::FORBIDDEN
                {
                    let hint = if token.is_some() {
                        "GitLab token seems invalid or lacks required scope (use read_api)."
                    } else {
                        "No GitLab token detected. Set --token, TOKEN_GITLAB (or GITLAB_TOKEN), or [instances.<alias>].token."
                    };
                    return Err(AppError::auth(format!(
                        "GitLab API auth error {}: {hint} {}",
                        payload.status,
                        body_snippet(&payload.body)
                    ))
                    .with_provider("gitlab")
                    .with_http_status(payload.status)
                    .into());
                }
                return Err(AppError::from_http(
                    "GitLab",
                    operation,
                    payload.status,
                    &payload.body,
                )
                .with_provider("gitlab")
                .into());
            }

            let next_page = payload
                .headers
                .get("x-next-page")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .trim()
                .to_string();

            let items: Vec<T> = page_span.in_scope(|| {
                debug_span!(
                    "gitlab.page.decode",
                    operation = operation,
                    page_kind = page_kind
                )
                .in_scope(|| Self::decode_gitlab_json(&payload, token, operation))
            })?;
            page_span.in_scope(|| trace!(count = items.len(), page, "decoded GitLab page"));
            results.extend(items);

            if next_page.is_empty() {
                break;
            }

            page = next_page.parse::<u32>().unwrap_or(page + 1);
        }

        Ok(results)
    }

    pub(super) async fn get_one<T: for<'de> Deserialize<'de>>(
        &self,
        url: &str,
        token: Option<&str>,
        operation: &'static str,
    ) -> Result<Option<T>> {
        let single_span = debug_span!("gitlab.single.fetch", operation = operation);
        async {
            let req = Self::apply_auth(self.client.get(url), token);
            let payload = Self::execute_request(req, operation, "reqwest.http.get")
                .instrument(single_span.clone())
                .await?;

            if payload.status == StatusCode::NOT_FOUND {
                return Ok(None);
            }

            let item = debug_span!("gitlab.single.decode", operation = operation)
                .in_scope(|| Self::decode_gitlab_json(&payload, token, operation))?;
            Ok(Some(item))
        }
        .instrument(single_span.clone())
        .await
    }

    pub(super) async fn fetch_notes(
        &self,
        repo: &str,
        iid: u64,
        is_pr: bool,
        req: &FetchRequest,
    ) -> Result<Vec<Comment>> {
        let span = debug_span!(
            "gitlab.hydrate.issue_comments",
            is_pr,
            gitlab.comments.count = tracing::field::Empty
        );
        async {
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

            let notes: Vec<GitLabNote> = self
                .get_pages(
                    &url,
                    &[],
                    req.token.as_deref(),
                    req.per_page,
                    true,
                    GitLabPageContext::new("comments", "comments fetch"),
                )
                .await?;
            let comments: Vec<Comment> = notes
                .into_iter()
                .filter(|n| !n.system)
                .map(map_note_comment)
                .collect();
            span.record("gitlab.comments.count", usize_to_i64(comments.len()));
            Ok(comments)
        }
        .instrument(span.clone())
        .await
    }

    pub(super) async fn fetch_review_comments(
        &self,
        repo: &str,
        iid: u64,
        req: &FetchRequest,
    ) -> Result<Vec<Comment>> {
        let span = debug_span!(
            "gitlab.hydrate.review_comments",
            gitlab.review_comments.count = tracing::field::Empty
        );
        async {
            let project = encode_project_path(repo);
            let url = format!(
                "{}/api/v4/projects/{project}/merge_requests/{iid}/discussions",
                self.base_url
            );

            let discussions: Vec<GitLabDiscussion> = self
                .get_pages(
                    &url,
                    &[],
                    req.token.as_deref(),
                    req.per_page,
                    true,
                    GitLabPageContext::new("review_comments", "review comments fetch"),
                )
                .await?;
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

            span.record("gitlab.review_comments.count", usize_to_i64(comments.len()));
            Ok(comments)
        }
        .instrument(span.clone())
        .await
    }

    pub(super) async fn fetch_links(
        &self,
        repo: &str,
        iid: u64,
        is_pr: bool,
        conversation_url: Option<&str>,
        req: &FetchRequest,
    ) -> ConversationMetadata {
        let span = debug_span!(
            "gitlab.hydrate.links",
            is_pr,
            gitlab.links.count = tracing::field::Empty
        );
        async {
            if !req.include_links {
                return ConversationMetadata::empty();
            }

            let project = encode_project_path(repo);
            let mut links: Vec<ConversationLink> = Vec::new();
            if let Some(url) = conversation_url {
                links.push(map_url_reference(url));
            }
            if is_pr {
                self.collect_pr_links(&project, repo, iid, req, &mut links)
                    .await;
            } else {
                self.collect_issue_links(&project, repo, iid, req, &mut links)
                    .await;
            }

            prune_redundant_relates(&mut links);
            span.record("gitlab.links.count", usize_to_i64(links.len()));
            ConversationMetadata::with_links(links)
        }
        .instrument(span.clone())
        .await
    }

    pub(super) async fn fetch_conversation_by_id(
        &self,
        repo: &str,
        iid: u64,
        kind: ContentKind,
        req: &FetchRequest,
    ) -> Result<Conversation> {
        enum SeedBody {
            Issue(Option<GitLabIssueItem>),
            Pr(Option<GitLabMergeRequestItem>),
        }

        let is_pr = matches!(kind, ContentKind::Pr);
        let span = debug_span!(
            "gitlab.hydrate.issue",
            include_comments = req.include_comments,
            include_review_comments = req.include_review_comments,
            include_links = req.include_links,
            is_pr
        );
        async {
            let body_task = async {
                match kind {
                    ContentKind::Issue => Ok::<SeedBody, anyhow::Error>(SeedBody::Issue(
                        self.fetch_issue_by_iid(repo, iid, req.token.as_deref())
                            .await?,
                    )),
                    ContentKind::Pr => Ok::<SeedBody, anyhow::Error>(SeedBody::Pr(
                        self.fetch_mr_by_iid(repo, iid, req.token.as_deref())
                            .await?,
                    )),
                }
            }
            .instrument(debug_span!("gitlab.hydrate.issue_body"));
            let notes_task = async {
                if !req.include_comments {
                    return Ok::<Vec<Comment>, anyhow::Error>(Vec::new());
                }
                self.fetch_notes(repo, iid, is_pr, req).await
            };
            let review_comments_task = async {
                if !req.include_comments || !is_pr || !req.include_review_comments {
                    return Ok::<Vec<Comment>, anyhow::Error>(Vec::new());
                }
                self.fetch_review_comments(repo, iid, req).await
            };
            let metadata_task = async {
                if req.include_links {
                    self.fetch_links(repo, iid, is_pr, None, req).await
                } else {
                    ConversationMetadata::empty()
                }
            };

            let (body_result, notes_result, review_comments_result, metadata) =
                tokio::join!(body_task, notes_task, review_comments_task, metadata_task);

            let mut comments = notes_result?;
            comments.extend(review_comments_result?);
            comments.sort_by(|a, b| a.created_at.cmp(&b.created_at));

            match body_result? {
                SeedBody::Issue(Some(issue)) => {
                    let mut links = metadata.links;
                    if let Some(url) = issue.web_url.as_deref() {
                        links.push(map_url_reference(url));
                    }
                    Ok(Conversation {
                        id: issue.iid.to_string(),
                        title: issue.title,
                        state: issue.state,
                        body: issue.description,
                        comments,
                        metadata: ConversationMetadata::with_links(links),
                    })
                }
                SeedBody::Issue(None) => Err(AppError::not_found(format!(
                    "Issue #{iid} not found in repo {repo}."
                ))
                .into()),
                SeedBody::Pr(Some(mr)) => {
                    let mut links = metadata.links;
                    if let Some(url) = mr.web_url.as_deref() {
                        links.push(map_url_reference(url));
                    }
                    Ok(Conversation {
                        id: mr.iid.to_string(),
                        title: mr.title,
                        state: mr.state,
                        body: mr.description,
                        comments,
                        metadata: ConversationMetadata::with_links(links),
                    })
                }
                SeedBody::Pr(None) => {
                    if self
                        .fetch_issue_by_iid(repo, iid, req.token.as_deref())
                        .await?
                        .is_some()
                    {
                        return Err(AppError::usage(format!(
                            "ID {iid} in repo {repo} is an issue, not a merge request."
                        ))
                        .into());
                    }
                    Err(AppError::not_found(format!(
                        "Merge request !{iid} not found in repo {repo}."
                    ))
                    .into())
                }
            }
        }
        .instrument(span)
        .await
    }

    async fn collect_pr_links(
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
        let related_issues_url = format!(
            "{}/api/v4/projects/{project}/merge_requests/{iid}/related_issues",
            self.base_url
        );
        let (closed_issues_result, related_issues_result) = tokio::join!(
            self.get_pages::<GitLabRelatedIssueRef>(
                &closes_url,
                &[],
                req.token.as_deref(),
                req.per_page,
                true,
                GitLabPageContext::new("links_closes_issues", "closes issues fetch"),
            ),
            self.get_pages::<GitLabRelatedIssueRef>(
                &related_issues_url,
                &[],
                req.token.as_deref(),
                req.per_page,
                true,
                GitLabPageContext::new("links_related_issues", "related issues fetch"),
            )
        );

        match closed_issues_result {
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
        match related_issues_result {
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

    async fn collect_issue_links(
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
        let closed_by_url = format!(
            "{}/api/v4/projects/{project}/issues/{iid}/closed_by",
            self.base_url
        );
        let related_mr_url = format!(
            "{}/api/v4/projects/{project}/issues/{iid}/related_merge_requests",
            self.base_url
        );
        let (issue_links_result, closed_by_result, related_mrs_result) = tokio::join!(
            self.get_pages::<GitLabIssueLinkItem>(
                &links_url,
                &[],
                req.token.as_deref(),
                req.per_page,
                true,
                GitLabPageContext::new("links_issue_links", "issue links fetch"),
            ),
            self.get_pages::<GitLabMergeRequestRef>(
                &closed_by_url,
                &[],
                req.token.as_deref(),
                req.per_page,
                true,
                GitLabPageContext::new("links_closed_by", "closed_by fetch"),
            ),
            self.get_pages::<GitLabMergeRequestRef>(
                &related_mr_url,
                &[],
                req.token.as_deref(),
                req.per_page,
                true,
                GitLabPageContext::new(
                    "links_related_merge_requests",
                    "related merge requests fetch",
                ),
            )
        );

        match issue_links_result {
            Ok(issue_links) => {
                links.extend(
                    issue_links
                        .iter()
                        .filter_map(|link| map_issue_link(link, iid)),
                );
            }
            Err(err) => warn!(repo, iid, error = %err, "GitLab issue links fetch failed"),
        }
        match closed_by_result {
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
        match related_mrs_result {
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

    pub(super) async fn fetch_conversation(
        &self,
        repo: &str,
        seed: ConversationSeed,
        req: &FetchRequest,
    ) -> Result<Conversation> {
        let span = debug_span!(
            "gitlab.hydrate.issue",
            include_comments = req.include_comments,
            include_review_comments = req.include_review_comments,
            include_links = req.include_links,
            is_pr = seed.is_pr
        );
        async {
            let notes_task = async {
                if !req.include_comments {
                    return Ok::<Vec<Comment>, anyhow::Error>(Vec::new());
                }
                self.fetch_notes(repo, seed.id, seed.is_pr, req).await
            };
            let review_comments_task = async {
                if !req.include_comments || !seed.is_pr || !req.include_review_comments {
                    return Ok::<Vec<Comment>, anyhow::Error>(Vec::new());
                }
                self.fetch_review_comments(repo, seed.id, req).await
            };
            let metadata_task = async {
                if req.include_links {
                    self.fetch_links(repo, seed.id, seed.is_pr, seed.web_url.as_deref(), req)
                        .await
                } else {
                    ConversationMetadata::empty()
                }
            };

            let (notes_result, review_comments_result, metadata) =
                tokio::join!(notes_task, review_comments_task, metadata_task);
            let mut comments = notes_result?;
            comments.extend(review_comments_result?);
            comments.sort_by(|a, b| a.created_at.cmp(&b.created_at));

            Ok(Conversation {
                id: seed.id.to_string(),
                title: seed.title,
                state: seed.state,
                body: seed.body,
                comments,
                metadata,
            })
        }
        .instrument(span.clone())
        .await
    }

    pub(super) async fn fetch_issue_by_iid(
        &self,
        repo: &str,
        iid: u64,
        token: Option<&str>,
    ) -> Result<Option<GitLabIssueItem>> {
        let project = encode_project_path(repo);
        let url = format!("{}/api/v4/projects/{project}/issues/{iid}", self.base_url);
        self.get_one(&url, token, "issue fetch").await
    }

    pub(super) async fn fetch_mr_by_iid(
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
        self.get_one(&url, token, "merge request fetch").await
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

fn body_snippet(body: &str) -> String {
    body.chars()
        .take(200)
        .collect::<String>()
        .replace('\n', " ")
}

fn usize_to_i64(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}
