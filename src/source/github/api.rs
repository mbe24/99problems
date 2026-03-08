use anyhow::Result;
use reqwest::StatusCode;
use reqwest::header::{CONTENT_TYPE, HeaderMap};
use reqwest_middleware::RequestBuilder;
use serde::Deserialize;
use tracing::{debug, debug_span, trace, warn};

use super::model::{
    ConversationSeed, IssueCommentItem, ReviewCommentItem, map_graphql_link_nodes,
    map_issue_collection_links, map_issue_comment, map_issue_url_links, map_review_comment,
    map_timeline_links,
};
use super::{GITHUB_API_BASE, GITHUB_API_VERSION, GitHubSource, PAGE_SIZE};
use crate::error::{AppError, app_error_from_decode, app_error_from_reqwest};
use crate::model::{Comment, Conversation, ConversationLink, ConversationMetadata};
use crate::source::FetchRequest;

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
        let _exchange_guard = exchange_span.enter();
        let response = {
            let _request_span = debug_span!("github.http.request", operation = operation).entered();
            Self::apply_otel_span_name(req, span_name).send().await
        };
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
                "github.http.response.read",
                operation = operation,
                status_code = tracing::field::Empty,
                error.type = tracing::field::Empty,
                error.message = tracing::field::Empty
            );
            let _read_guard = read_span.enter();
            read_span.record("status_code", i64::from(status.as_u16()));
            match response.text().await {
                Ok(body) => body,
                Err(err) => {
                    let error_message = err.to_string();
                    read_span.record("error.type", "response_read_error");
                    read_span.record("error.message", error_message.as_str());
                    exchange_span.record("error.type", "response_read_error");
                    exchange_span.record("error.message", error_message.as_str());
                    return Err(app_error_from_reqwest("GitHub", operation, &err).into());
                }
            }
        };

        exchange_span.record("status_code", i64::from(status.as_u16()));
        exchange_span.record("body_bytes", usize_to_i64(body.len()));
        Ok(GitHubHttpPayload {
            status,
            content_type,
            body,
            headers,
        })
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
            let _page_span = debug_span!(
                "github.page.fetch",
                operation = "page fetch",
                page,
                per_page
            )
            .entered();
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

            let items: Vec<T> = {
                let _decode_span =
                    debug_span!("github.page.decode", operation = "page fetch").entered();
                Self::decode_github_json(&payload, token, "page fetch")?
            };
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
        let _span_guard = span.enter();
        let comments_url = format!("{GITHUB_API_BASE}/repos/{repo}/issues/{id}/comments");
        let raw_comments: Vec<IssueCommentItem> = self
            .get_pages(&comments_url, req.token.as_deref(), req.per_page)
            .await?;
        let comments: Vec<Comment> = raw_comments.into_iter().map(map_issue_comment).collect();
        span.record("github.comments.count", usize_to_i64(comments.len()));
        Ok(comments)
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
        let _span_guard = span.enter();
        let comments_url = format!("{GITHUB_API_BASE}/repos/{repo}/pulls/{id}/comments");
        let raw_comments: Vec<ReviewCommentItem> = self
            .get_pages(&comments_url, req.token.as_deref(), req.per_page)
            .await?;
        let comments: Vec<Comment> = raw_comments.into_iter().map(map_review_comment).collect();
        span.record("github.review_comments.count", usize_to_i64(comments.len()));
        Ok(comments)
    }

    pub(super) async fn fetch_links(
        &self,
        repo: &str,
        id: u64,
        is_pr: bool,
        req: &FetchRequest,
    ) -> ConversationMetadata {
        let span = debug_span!(
            "github.hydrate.links",
            is_pr,
            github.links.count = tracing::field::Empty
        );
        let _span_guard = span.enter();
        if !req.include_links {
            return ConversationMetadata::empty();
        }

        let mut links: Vec<ConversationLink> = Vec::new();
        let timeline_url = format!("{GITHUB_API_BASE}/repos/{repo}/issues/{id}/timeline");
        let timeline_result = {
            let _timeline_span = debug_span!("github.links.timeline").entered();
            self.get_pages::<serde_json::Value>(&timeline_url, req.token.as_deref(), req.per_page)
                .await
        };
        match timeline_result {
            Ok(events) => {
                for event in events {
                    links.extend(map_timeline_links(&event));
                }
            }
            Err(err) => warn!(repo, id, error = %err, "GitHub timeline link fetch failed"),
        }

        let issue_url = format!("{GITHUB_API_BASE}/repos/{repo}/issues/{id}");
        let issue_result = {
            let _issue_span = debug_span!("github.links.issue_detail").entered();
            self.get_one::<serde_json::Value>(&issue_url, req.token.as_deref())
                .await
        };
        match issue_result {
            Ok(Some(issue)) => links.extend(map_issue_url_links(&issue)),
            Ok(None) => {}
            Err(err) => warn!(repo, id, error = %err, "GitHub issue detail link fetch failed"),
        }

        let blocked_by_url =
            format!("{GITHUB_API_BASE}/repos/{repo}/issues/{id}/dependencies/blocked_by");
        let blocked_by_result = {
            let _blocked_by_span = debug_span!("github.links.blocked_by").entered();
            self.get_pages::<serde_json::Value>(&blocked_by_url, req.token.as_deref(), req.per_page)
                .await
        };
        match blocked_by_result {
            Ok(blocked_by) => links.extend(map_issue_collection_links(&blocked_by, "blocked_by")),
            Err(err) => warn!(repo, id, error = %err, "GitHub blocked_by dependency fetch failed"),
        }

        let blocking_url =
            format!("{GITHUB_API_BASE}/repos/{repo}/issues/{id}/dependencies/blocking");
        let blocking_result = {
            let _blocking_span = debug_span!("github.links.blocking").entered();
            self.get_pages::<serde_json::Value>(&blocking_url, req.token.as_deref(), req.per_page)
                .await
        };
        match blocking_result {
            Ok(blocking) => links.extend(map_issue_collection_links(&blocking, "blocks")),
            Err(err) => warn!(repo, id, error = %err, "GitHub blocking dependency fetch failed"),
        }

        let sub_issues_url = format!("{GITHUB_API_BASE}/repos/{repo}/issues/{id}/sub_issues");
        let sub_issues_result = {
            let _sub_issues_span = debug_span!("github.links.sub_issues").entered();
            self.get_pages::<serde_json::Value>(&sub_issues_url, req.token.as_deref(), req.per_page)
                .await
        };
        match sub_issues_result {
            Ok(sub_issues) => links.extend(map_issue_collection_links(&sub_issues, "child")),
            Err(err) => warn!(repo, id, error = %err, "GitHub sub-issues fetch failed"),
        }

        let parent_url = format!("{GITHUB_API_BASE}/repos/{repo}/issues/{id}/parent");
        let parent_result = {
            let _parent_span = debug_span!("github.links.parent").entered();
            self.get_one::<serde_json::Value>(&parent_url, req.token.as_deref())
                .await
        };
        match parent_result {
            Ok(Some(parent)) => {
                links.extend(map_issue_collection_links(
                    std::slice::from_ref(&parent),
                    "parent",
                ));
            }
            Ok(None) => {}
            Err(err) => warn!(repo, id, error = %err, "GitHub parent issue fetch failed"),
        }

        let graphql_result = {
            let _graphql_span = debug_span!("github.links.graphql").entered();
            self.fetch_graphql_links(repo, id, is_pr, req.token.as_deref())
                .await
        };
        match graphql_result {
            Ok(graph_links) => links.extend(graph_links),
            Err(err) => warn!(repo, id, error = %err, "GitHub GraphQL links fetch failed"),
        }

        span.record("github.links.count", usize_to_i64(links.len()));
        ConversationMetadata::with_links(links)
    }

    pub(super) async fn get_one<T: for<'de> Deserialize<'de>>(
        &self,
        url: &str,
        token: Option<&str>,
    ) -> Result<Option<T>> {
        let _span = debug_span!("github.single.fetch", operation = "single fetch").entered();
        let req = Self::apply_auth(self.client.get(url), token);
        let payload = Self::execute_request(req, "single fetch", "reqwest.http.get").await?;
        if payload.status == StatusCode::NOT_FOUND {
            return Ok(None);
        }

        let item = {
            let _decode_span =
                debug_span!("github.single.decode", operation = "single fetch").entered();
            Self::decode_github_json(&payload, token, "single fetch")?
        };
        Ok(Some(item))
    }

    async fn fetch_graphql_links(
        &self,
        repo: &str,
        id: u64,
        is_pr: bool,
        token: Option<&str>,
    ) -> Result<Vec<ConversationLink>> {
        let Some((owner, name)) = repo.split_once('/') else {
            return Ok(Vec::new());
        };
        let (query, relation, kind, path) = if is_pr {
            (
                "query($owner:String!,$name:String!,$n:Int!){ repository(owner:$owner,name:$name){ pullRequest(number:$n){ closingIssuesReferences(first:100){nodes{number url}} } } }",
                "closes",
                "issue",
                "/data/repository/pullRequest/closingIssuesReferences/nodes",
            )
        } else {
            (
                "query($owner:String!,$name:String!,$n:Int!){ repository(owner:$owner,name:$name){ issue(number:$n){ closedByPullRequestsReferences(first:100){nodes{number url}} } } }",
                "closed_by",
                "pr",
                "/data/repository/issue/closedByPullRequestsReferences/nodes",
            )
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
        let payload = {
            let _send_span =
                debug_span!("github.graphql.send", operation = "graphql fetch").entered();
            Self::execute_request(request, "graphql fetch", "reqwest.http.post").await?
        };
        let payload: serde_json::Value = {
            let _decode_span =
                debug_span!("github.graphql.decode", operation = "graphql fetch").entered();
            Self::decode_github_json(&payload, token, "graphql fetch")?
        };
        if let Some(errors) = payload.get("errors") {
            return Err(
                AppError::provider(format!("GitHub GraphQL returned errors: {errors}")).into(),
            );
        }
        let nodes = payload
            .pointer(path)
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(map_graphql_link_nodes(&nodes, relation, kind))
    }

    pub(super) async fn fetch_conversation(
        &self,
        repo: &str,
        item: ConversationSeed,
        req: &FetchRequest,
    ) -> Result<Conversation> {
        let _span = debug_span!(
            "github.hydrate.conversation",
            include_comments = req.include_comments,
            include_review_comments = req.include_review_comments,
            include_links = req.include_links,
            is_pr = item.is_pr
        )
        .entered();
        let mut comments = Vec::new();
        if req.include_comments {
            let _issue_comments_span = debug_span!("github.hydrate.issue_comments.stage").entered();
            comments = self.fetch_issue_comments(repo, item.id, req).await?;
            if item.is_pr && req.include_review_comments {
                let _review_comments_span =
                    debug_span!("github.hydrate.review_comments.stage").entered();
                comments.extend(self.fetch_review_comments(repo, item.id, req).await?);
                comments.sort_by(|a, b| a.created_at.cmp(&b.created_at));
            }
        }
        let metadata = if req.include_links {
            let _links_span = debug_span!("github.hydrate.links.stage").entered();
            self.fetch_links(repo, item.id, item.is_pr, req).await
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

fn body_snippet(body: &str) -> String {
    body.chars()
        .take(200)
        .collect::<String>()
        .replace('\n', " ")
}

fn usize_to_i64(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}
