use anyhow::Result;
use reqwest::StatusCode;
use reqwest::header::{CONTENT_TYPE, HeaderMap};
use reqwest_middleware::RequestBuilder;
use serde::Deserialize;
use tracing::{Instrument, debug, debug_span, trace, warn};

use super::model::{
    ConversationSeed, IssueCommentItem, IssueItem, PullRequestMarker, ReviewCommentItem,
    map_graphql_link_nodes, map_issue_collection_links, map_issue_comment, map_pull_request_links,
    map_review_comment, map_timeline_links,
};
use super::{GITHUB_API_BASE, GITHUB_API_VERSION, GitHubSource, PAGE_SIZE};
use crate::error::{AppError, app_error_from_decode, app_error_from_reqwest};
use crate::model::{Comment, Conversation, ConversationLink, ConversationMetadata};
use crate::source::{ContentKind, FetchRequest};

pub(super) struct GitHubHttpPayload {
    status: StatusCode,
    content_type: String,
    body: String,
    headers: HeaderMap,
}

impl GitHubSource {
    pub(super) fn apply_auth(req: RequestBuilder, token: Option<&str>) -> RequestBuilder {
        let req = req
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", GITHUB_API_VERSION);
        if let Some(t) = token {
            req.header("Authorization", format!("Bearer {t}"))
        } else {
            req
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
                    app_error_from_reqwest("GitHub", operation, &err),
                    "request_send_error",
                    message,
                )
            }
            other @ reqwest_middleware::Error::Middleware(_) => {
                let message = other.to_string();
                (
                    AppError::provider(format!("GitHub API {operation} middleware error: {other}"))
                        .with_provider("github"),
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
    ) -> Result<GitHubHttpPayload> {
        let exchange_span = debug_span!(
            "github.http.exchange",
            operation = operation,
            status_code = tracing::field::Empty,
            body_bytes = tracing::field::Empty,
            error.type = tracing::field::Empty,
            error.message = tracing::field::Empty
        );
        let span_for_record = exchange_span.clone();
        async move {
            let request_span = debug_span!("github.http.request", operation = operation);
            let response = Self::apply_otel_span_name(req, span_name)
                .send()
                .instrument(request_span)
                .await;
            let response = match response {
                Ok(response) => response,
                Err(err) => {
                    let (mapped, error_type, error_message) =
                        Self::map_request_error(operation, err);
                    span_for_record.record("error.type", error_type);
                    span_for_record.record("error.message", error_message.as_str());
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
            let read_span = debug_span!(
                "github.http.response.read",
                operation = operation,
                status_code = tracing::field::Empty,
                error.type = tracing::field::Empty,
                error.message = tracing::field::Empty
            );
            read_span.record("status_code", i64::from(status.as_u16()));
            let body = match response.text().instrument(read_span.clone()).await {
                Ok(body) => body,
                Err(err) => {
                    let error_message = err.to_string();
                    read_span.record("error.type", "response_read_error");
                    read_span.record("error.message", error_message.as_str());
                    span_for_record.record("error.type", "response_read_error");
                    span_for_record.record("error.message", error_message.as_str());
                    return Err(app_error_from_reqwest("GitHub", operation, &err).into());
                }
            };

            span_for_record.record("status_code", i64::from(status.as_u16()));
            span_for_record.record("body_bytes", usize_to_i64(body.len()));
            Ok(GitHubHttpPayload {
                status,
                content_type,
                body,
                headers,
            })
        }
        .instrument(exchange_span)
        .await
    }

    pub(super) fn decode_github_json<T: for<'de> Deserialize<'de>>(
        payload: &GitHubHttpPayload,
        token: Option<&str>,
        operation: &str,
    ) -> Result<T> {
        let decode_span = debug_span!(
            "github.http.decode",
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

            let auth_hint = if payload.status == StatusCode::UNAUTHORIZED
                || payload.status == StatusCode::FORBIDDEN
            {
                if token.is_some() {
                    "GitHub token seems invalid or lacks required scope."
                } else {
                    "No GitHub token detected. Set --token, GITHUB_TOKEN, or [instances.<alias>].token."
                }
            } else {
                ""
            };

            let mut err = AppError::from_http("GitHub", operation, payload.status, &payload.body)
                .with_provider("github");
            if !auth_hint.is_empty() {
                err = err.with_hint(auth_hint);
            }
            return Err(err.into());
        }

        if !payload.content_type.contains("application/json") {
            let error_message = format!("unexpected content-type '{}'", payload.content_type);
            decode_span.record("error.type", "unexpected_content_type");
            decode_span.record("error.message", error_message.as_str());
            return Err(AppError::provider(format!(
                "GitHub API {} returned non-JSON content-type '{}' (body starts with: {}).",
                operation,
                payload.content_type,
                body_snippet(&payload.body)
            ))
            .with_provider("github")
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
                    "GitHub",
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
            let payload = Self::execute_request(req, "page fetch", "reqwest.http.get").await?;

            let has_next = payload
                .headers
                .get("link")
                .and_then(|v| v.to_str().ok())
                .is_some_and(|l| l.contains(r#"rel=\"next\""#));

            let items: Vec<T> = Self::decode_github_json(&payload, token, "page fetch")?;
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

    pub(super) async fn fetch_issue_comments(
        &self,
        repo: &str,
        id: u64,
        req: &FetchRequest,
    ) -> Result<Vec<Comment>> {
        let span = debug_span!(
            "github.hydrate.issue_comments",
            github.comments.count = tracing::field::Empty
        );
        let span_for_record = span.clone();
        async {
            let comments_url = format!("{GITHUB_API_BASE}/repos/{repo}/issues/{id}/comments");
            let raw_comments: Vec<IssueCommentItem> = self
                .get_pages(&comments_url, req.token.as_deref(), req.per_page)
                .await?;
            let comments: Vec<Comment> = raw_comments.into_iter().map(map_issue_comment).collect();
            span_for_record.record("github.comments.count", usize_to_i64(comments.len()));
            Ok(comments)
        }
        .instrument(span)
        .await
    }

    pub(super) async fn fetch_review_comments(
        &self,
        repo: &str,
        id: u64,
        req: &FetchRequest,
    ) -> Result<Vec<Comment>> {
        let span = debug_span!(
            "github.hydrate.review_comments",
            github.review_comments.count = tracing::field::Empty
        );
        let span_for_record = span.clone();
        async {
            let comments_url = format!("{GITHUB_API_BASE}/repos/{repo}/pulls/{id}/comments");
            let raw_comments: Vec<ReviewCommentItem> = self
                .get_pages(&comments_url, req.token.as_deref(), req.per_page)
                .await?;
            let comments: Vec<Comment> = raw_comments.into_iter().map(map_review_comment).collect();
            span_for_record.record("github.review_comments.count", usize_to_i64(comments.len()));
            Ok(comments)
        }
        .instrument(span)
        .await
    }

    pub(super) async fn fetch_links(
        &self,
        repo: &str,
        id: u64,
        is_pr: Option<bool>,
        pull_request: Option<&PullRequestMarker>,
        req: &FetchRequest,
    ) -> ConversationMetadata {
        let span = debug_span!(
            "github.hydrate.links",
            is_pr = is_pr.unwrap_or(false),
            is_pr_known = is_pr.is_some(),
            github.links.count = tracing::field::Empty
        );
        let span_for_record = span.clone();
        async {
            if !req.include_links {
                return ConversationMetadata::empty();
            }

            let mut links: Vec<ConversationLink> = map_pull_request_links(pull_request);
            let token = req.token.as_deref();
            let per_page = req.per_page;
            let timeline_url = format!("{GITHUB_API_BASE}/repos/{repo}/issues/{id}/timeline");
            let blocked_by_url =
                format!("{GITHUB_API_BASE}/repos/{repo}/issues/{id}/dependencies/blocked_by");
            let blocking_url =
                format!("{GITHUB_API_BASE}/repos/{repo}/issues/{id}/dependencies/blocking");
            let sub_issues_url = format!("{GITHUB_API_BASE}/repos/{repo}/issues/{id}/sub_issues");
            let parent_url = format!("{GITHUB_API_BASE}/repos/{repo}/issues/{id}/parent");

            let timeline_task = self
                .get_pages::<serde_json::Value>(&timeline_url, token, per_page)
                .instrument(debug_span!("github.links.timeline"));
            let blocked_by_task = self
                .get_pages::<serde_json::Value>(&blocked_by_url, token, per_page)
                .instrument(debug_span!("github.links.blocked_by"));
            let blocking_task = self
                .get_pages::<serde_json::Value>(&blocking_url, token, per_page)
                .instrument(debug_span!("github.links.blocking"));
            let sub_issues_task = self
                .get_pages::<serde_json::Value>(&sub_issues_url, token, per_page)
                .instrument(debug_span!("github.links.sub_issues"));
            let parent_task = self
                .get_one::<serde_json::Value>(&parent_url, token, "parent fetch")
                .instrument(debug_span!("github.links.parent"));
            let graphql_task = self
                .fetch_graphql_links(repo, id, is_pr, token)
                .instrument(debug_span!("github.links.graphql"));

            let (
                timeline_result,
                blocked_by_result,
                blocking_result,
                sub_issues_result,
                parent_result,
                graphql_result,
            ) = tokio::join!(
                timeline_task,
                blocked_by_task,
                blocking_task,
                sub_issues_task,
                parent_task,
                graphql_task
            );

            merge_timeline_links(&mut links, timeline_result, repo, id);
            for (result, relation, warning) in [
                (
                    blocked_by_result,
                    "blocked_by",
                    "GitHub blocked_by dependency fetch failed",
                ),
                (
                    blocking_result,
                    "blocks",
                    "GitHub blocking dependency fetch failed",
                ),
                (sub_issues_result, "child", "GitHub sub-issues fetch failed"),
            ] {
                merge_issue_collection_links(&mut links, result, relation, repo, id, warning);
            }
            merge_parent_links(&mut links, parent_result, repo, id);
            merge_graphql_links(&mut links, graphql_result, repo, id);

            span_for_record.record("github.links.count", usize_to_i64(links.len()));
            ConversationMetadata::with_links(links)
        }
        .instrument(span)
        .await
    }

    pub(super) async fn get_one<T: for<'de> Deserialize<'de>>(
        &self,
        url: &str,
        token: Option<&str>,
        operation: &str,
    ) -> Result<Option<T>> {
        let req = Self::apply_auth(self.client.get(url), token);
        let payload = Self::execute_request(req, operation, "reqwest.http.get").await?;
        if payload.status == StatusCode::NOT_FOUND {
            trace!(
                operation,
                url = %url,
                "GitHub endpoint returned 404; treating as empty result"
            );
            return Ok(None);
        }

        let item = Self::decode_github_json(&payload, token, operation)?;
        Ok(Some(item))
    }

    async fn fetch_graphql_links(
        &self,
        repo: &str,
        id: u64,
        is_pr: Option<bool>,
        token: Option<&str>,
    ) -> Result<Vec<ConversationLink>> {
        let Some((owner, name)) = repo.split_once('/') else {
            return Ok(Vec::new());
        };
        let query = match is_pr {
            Some(true) => {
                "query($owner:String!,$name:String!,$n:Int!){ repository(owner:$owner,name:$name){ pullRequest(number:$n){ closingIssuesReferences(first:100){nodes{number url}} } } }"
            }
            Some(false) => {
                "query($owner:String!,$name:String!,$n:Int!){ repository(owner:$owner,name:$name){ issue(number:$n){ closedByPullRequestsReferences(first:100){nodes{number url}} } } }"
            }
            None => {
                "query($owner:String!,$name:String!,$n:Int!){ repository(owner:$owner,name:$name){ issue(number:$n){ closedByPullRequestsReferences(first:100){nodes{number url}} } pullRequest(number:$n){ closingIssuesReferences(first:100){nodes{number url}} } } }"
            }
        };

        let body = serde_json::json!({
            "query": query,
            "variables": {
                "owner": owner,
                "name": name,
                "n": id
            }
        });

        let request = Self::apply_auth(self.client.post("https://api.github.com/graphql"), token)
            .header("Content-Type", "application/json")
            .json(&body);
        let payload = Self::execute_request(request, "graphql fetch", "reqwest.http.post").await?;
        let payload: serde_json::Value =
            Self::decode_github_json(&payload, token, "graphql fetch")?;
        if let Some(errors) = payload.get("errors") {
            return Err(
                AppError::provider(format!("GitHub GraphQL returned errors: {errors}")).into(),
            );
        }

        let mut links = Vec::new();
        if is_pr != Some(true) {
            let issue_nodes = payload
                .pointer("/data/repository/issue/closedByPullRequestsReferences/nodes")
                .and_then(serde_json::Value::as_array)
                .cloned()
                .unwrap_or_default();
            links.extend(map_graphql_link_nodes(&issue_nodes, "closed_by", "pr"));
        }
        if is_pr != Some(false) {
            let pr_nodes = payload
                .pointer("/data/repository/pullRequest/closingIssuesReferences/nodes")
                .and_then(serde_json::Value::as_array)
                .cloned()
                .unwrap_or_default();
            links.extend(map_graphql_link_nodes(&pr_nodes, "closes", "issue"));
        }

        Ok(links)
    }

    pub(super) async fn fetch_conversation(
        &self,
        repo: &str,
        item: ConversationSeed,
        req: &FetchRequest,
    ) -> Result<Conversation> {
        let ConversationSeed {
            id,
            title,
            state,
            body,
            is_pr,
            pull_request,
        } = item;
        Box::pin(
            async {
                let comments_task = async {
                    if !req.include_comments {
                        return Ok::<Vec<Comment>, anyhow::Error>(Vec::new());
                    }
                    let mut comments = self.fetch_issue_comments(repo, id, req).await?;
                    if is_pr && req.include_review_comments {
                        comments.extend(self.fetch_review_comments(repo, id, req).await?);
                        comments.sort_by(|a, b| a.created_at.cmp(&b.created_at));
                    }
                    Ok::<Vec<Comment>, anyhow::Error>(comments)
                };
                let metadata_task = async {
                    if req.include_links {
                        self.fetch_links(repo, id, Some(is_pr), pull_request.as_ref(), req)
                            .await
                    } else {
                        ConversationMetadata::empty()
                    }
                };
                let (comments_result, metadata) = tokio::join!(comments_task, metadata_task);
                let comments = comments_result?;

                Ok(Conversation {
                    id: id.to_string(),
                    title,
                    state,
                    body,
                    comments,
                    metadata,
                })
            }
            .instrument(debug_span!(
                "github.hydrate.issue",
                include_comments = req.include_comments,
                include_review_comments = req.include_review_comments,
                include_links = req.include_links,
                is_pr
            )),
        )
        .await
    }

    pub(super) async fn fetch_conversation_by_id(
        &self,
        repo: &str,
        issue_id: u64,
        kind: ContentKind,
        req: &FetchRequest,
    ) -> Result<Conversation> {
        Box::pin(
            async {
                let issue_task = self
                    .fetch_issue_seed(repo, issue_id, req)
                    .instrument(debug_span!("github.hydrate.issue_body"));
                let issue_comments_task = async {
                    if req.include_comments {
                        self.fetch_issue_comments(repo, issue_id, req).await
                    } else {
                        Ok(Vec::new())
                    }
                };
                let links_task = async {
                    if req.include_links {
                        self.fetch_links(repo, issue_id, None, None, req).await
                    } else {
                        ConversationMetadata::empty()
                    }
                };
                let (issue_result, issue_comments_result, metadata) =
                    tokio::join!(issue_task, issue_comments_task, links_task);

                let seed = issue_result?;
                Self::validate_issue_kind(repo, issue_id, kind, seed.is_pr)?;
                let mut comments = issue_comments_result?;
                let mut links = metadata.links;

                if seed.is_pr && req.include_comments && req.include_review_comments {
                    comments.extend(self.fetch_review_comments(repo, seed.id, req).await?);
                    comments.sort_by(|a, b| a.created_at.cmp(&b.created_at));
                }

                if req.include_links {
                    links.extend(map_pull_request_links(seed.pull_request.as_ref()));
                }

                Ok(Conversation {
                    id: seed.id.to_string(),
                    title: seed.title,
                    state: seed.state,
                    body: seed.body,
                    comments,
                    metadata: ConversationMetadata::with_links(links),
                })
            }
            .instrument(debug_span!(
                "github.hydrate.issue",
                include_comments = req.include_comments,
                include_review_comments = req.include_review_comments,
                include_links = req.include_links
            )),
        )
        .await
    }

    async fn fetch_issue_seed(
        &self,
        repo: &str,
        issue_id: u64,
        req: &FetchRequest,
    ) -> Result<ConversationSeed> {
        let issue_url = format!("{GITHUB_API_BASE}/repos/{repo}/issues/{issue_id}");
        let request = Self::apply_auth(self.client.get(&issue_url), req.token.as_deref());
        let payload = Self::execute_request(request, "issue fetch", "reqwest.http.get").await?;
        let issue: IssueItem =
            Self::decode_github_json(&payload, req.token.as_deref(), "issue fetch")?;

        Ok(ConversationSeed {
            id: issue.number,
            title: issue.title,
            state: issue.state,
            body: issue.body,
            is_pr: issue.pull_request.is_some(),
            pull_request: issue.pull_request,
        })
    }

    fn validate_issue_kind(
        repo: &str,
        issue_id: u64,
        kind: ContentKind,
        is_pr: bool,
    ) -> Result<()> {
        match kind {
            ContentKind::Issue if is_pr => Err(AppError::usage(format!(
                "ID {issue_id} in repo {repo} is a pull request. Use --type pr."
            ))
            .into()),
            ContentKind::Pr if !is_pr => Err(AppError::usage(format!(
                "ID {issue_id} in repo {repo} is an issue, not a pull request."
            ))
            .into()),
            _ => Ok(()),
        }
    }
}

fn merge_timeline_links(
    links: &mut Vec<ConversationLink>,
    result: Result<Vec<serde_json::Value>>,
    repo: &str,
    id: u64,
) {
    match result {
        Ok(events) => {
            for event in events {
                links.extend(map_timeline_links(&event));
            }
        }
        Err(err) => warn!(repo, id, error = %err, "GitHub timeline link fetch failed"),
    }
}

fn merge_issue_collection_links(
    links: &mut Vec<ConversationLink>,
    result: Result<Vec<serde_json::Value>>,
    relation: &str,
    repo: &str,
    id: u64,
    warning: &str,
) {
    match result {
        Ok(items) => links.extend(map_issue_collection_links(&items, relation)),
        Err(err) => warn!(repo, id, error = %err, "{warning}"),
    }
}

fn merge_parent_links(
    links: &mut Vec<ConversationLink>,
    result: Result<Option<serde_json::Value>>,
    repo: &str,
    id: u64,
) {
    match result {
        Ok(Some(parent)) => {
            links.extend(map_issue_collection_links(
                std::slice::from_ref(&parent),
                "parent",
            ));
        }
        Ok(None) => {}
        Err(err) => warn!(repo, id, error = %err, "GitHub parent issue fetch failed"),
    }
}

fn merge_graphql_links(
    links: &mut Vec<ConversationLink>,
    result: Result<Vec<ConversationLink>>,
    repo: &str,
    id: u64,
) {
    match result {
        Ok(graph_links) => links.extend(graph_links),
        Err(err) => warn!(repo, id, error = %err, "GitHub GraphQL links fetch failed"),
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
