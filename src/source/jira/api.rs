use anyhow::Result;
use reqwest::StatusCode;
use reqwest::header::CONTENT_TYPE;
use reqwest_middleware::RequestBuilder;
use serde::de::DeserializeOwned;
use tracing::{debug, debug_span, trace, warn};

use super::model::{
    JiraCommentsPage, JiraIssueFields, JiraIssueItem, JiraKeySearchResponse, JiraRemoteLinkItem,
    extract_adf_text, map_attachment_links, map_issue_links, map_parent_child_links,
    map_remote_links,
};
use super::{JiraSource, PAGE_SIZE};
use crate::error::{AppError, app_error_from_decode, app_error_from_reqwest};
use crate::model::{Comment, Conversation, ConversationMetadata};
use crate::source::FetchRequest;

pub(super) struct JiraHttpPayload {
    status: StatusCode,
    content_type: String,
    body: String,
}

impl JiraSource {
    pub(super) fn apply_auth(
        req: RequestBuilder,
        token: Option<&str>,
        account_email: Option<&str>,
    ) -> RequestBuilder {
        let req = req.header("Accept", "application/json");
        match token {
            Some(t) if t.contains(':') => {
                let (user, api_token) = t.split_once(':').unwrap_or_default();
                req.basic_auth(user, Some(api_token))
            }
            Some(t) => match account_email {
                Some(email) => req.basic_auth(email, Some(t)),
                None => req.bearer_auth(t),
            },
            None => req,
        }
    }

    pub(super) fn bounded_per_page(per_page: u32) -> u32 {
        per_page.clamp(1, PAGE_SIZE)
    }

    #[cfg(feature = "telemetry-otel")]
    fn apply_otel_span_name(req: RequestBuilder) -> RequestBuilder {
        req.with_extension(reqwest_tracing::OtelName("reqwest.http.get".into()))
    }

    #[cfg(not(feature = "telemetry-otel"))]
    fn apply_otel_span_name(req: RequestBuilder) -> RequestBuilder {
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
                    app_error_from_reqwest("Jira", operation, &err),
                    "request_send_error",
                    message,
                )
            }
            other @ reqwest_middleware::Error::Middleware(_) => {
                let message = other.to_string();
                (
                    AppError::provider(format!("Jira API {operation} middleware error: {other}"))
                        .with_provider("jira"),
                    "request_middleware_error",
                    message,
                )
            }
        }
    }

    pub(super) async fn execute_request(
        req: RequestBuilder,
        operation: &str,
    ) -> Result<JiraHttpPayload> {
        let exchange_span = debug_span!(
            "jira.http.exchange",
            operation = operation,
            status_code = tracing::field::Empty,
            body_bytes = tracing::field::Empty,
            error.type = tracing::field::Empty,
            error.message = tracing::field::Empty
        );
        let _exchange_guard = exchange_span.enter();
        let response = {
            let _request_span = debug_span!("jira.http.request", operation = operation).entered();
            Self::apply_otel_span_name(req).send().await
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
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let body = {
            let read_span = debug_span!(
                "jira.http.response.read",
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
                    return Err(app_error_from_reqwest("Jira", operation, &err).into());
                }
            }
        };
        exchange_span.record("status_code", i64::from(status.as_u16()));
        exchange_span.record("body_bytes", usize_to_i64(body.len()));
        Ok(JiraHttpPayload {
            status,
            content_type,
            body,
        })
    }

    pub(super) async fn fetch_issue(
        &self,
        id_or_key: &str,
        req: &FetchRequest,
    ) -> Result<Conversation> {
        let _span = debug_span!("jira.hydrate.issue").entered();
        let fields = "summary,description,status,parent,subtasks,issuelinks,attachment";
        let url = format!("{}/rest/api/3/issue/{}", self.base_url, id_or_key);
        let http = Self::apply_auth(
            self.client.get(&url),
            req.token.as_deref(),
            req.account_email.as_deref(),
        )
        .query(&[("fields", fields)]);
        let payload = Self::execute_request(http, "issue fetch").await?;
        if payload.status == StatusCode::NOT_FOUND {
            let auth_hint = if req.token.is_some() {
                if req.account_email.is_none() {
                    " Check Jira permissions or configure account_email for API-token auth."
                } else {
                    " Check Jira permissions for this issue."
                }
            } else {
                " Jira often returns 404 for unauthorized issues. Set --token, JIRA_TOKEN, or [instances.<alias>].token."
            };
            return Err(AppError::not_found(format!(
                "Jira issue '{}' was not found or is not accessible.{} Response: {}",
                id_or_key,
                auth_hint,
                body_snippet(&payload.body)
            ))
            .with_provider("jira")
            .with_http_status(StatusCode::NOT_FOUND)
            .into());
        }
        let issue: JiraIssueItem = {
            let _decode_span =
                debug_span!("jira.issue.decode", operation = "issue fetch").entered();
            Self::decode_jira_json(
                &payload,
                req.token.as_deref(),
                req.account_email.as_deref(),
                "issue fetch",
            )?
        };
        let fields = issue.fields;
        let comments = if req.include_comments {
            self.fetch_comments(&issue.key, req).await?
        } else {
            vec![]
        };
        let metadata = if req.include_links {
            self.fetch_metadata(&issue.key, fields.clone(), req).await
        } else {
            ConversationMetadata::empty()
        };
        Ok(Conversation {
            id: issue.key,
            title: fields.summary,
            state: fields.status.name,
            body: fields
                .description
                .as_ref()
                .map(extract_adf_text)
                .filter(|s| !s.is_empty()),
            comments,
            metadata,
        })
    }

    pub(super) async fn fetch_metadata(
        &self,
        issue_key: &str,
        fields: JiraIssueFields,
        req: &FetchRequest,
    ) -> ConversationMetadata {
        let _span = debug_span!("jira.hydrate.links").entered();
        let mut links = map_parent_child_links(&fields);
        links.extend(map_issue_links(fields.issuelinks));
        links.extend(map_attachment_links(fields.attachment));
        match self.fetch_remote_links(issue_key, req).await {
            Ok(remote_links) => links.extend(remote_links),
            Err(err) => {
                warn!(
                    issue_key,
                    error = %err,
                    "Jira remote link fetch failed; continuing without remote links"
                );
            }
        }
        match self.fetch_child_issue_links(issue_key, req).await {
            Ok(child_links) => links.extend(child_links),
            Err(err) => {
                warn!(
                    issue_key,
                    error = %err,
                    "Jira child issue lookup failed; continuing without child issue links"
                );
            }
        }
        ConversationMetadata::with_links(links)
    }

    async fn fetch_remote_links(
        &self,
        issue_key: &str,
        req: &FetchRequest,
    ) -> Result<Vec<crate::model::ConversationLink>> {
        let span = debug_span!(
            "jira.links.remote",
            jira.links.remote.count = tracing::field::Empty
        );
        let _span_guard = span.enter();
        let url = format!("{}/rest/api/3/issue/{issue_key}/remotelink", self.base_url);
        let http = Self::apply_auth(
            self.client.get(&url),
            req.token.as_deref(),
            req.account_email.as_deref(),
        );
        let payload = Self::execute_request(http, "remote link fetch").await?;
        let items: Vec<JiraRemoteLinkItem> = {
            let _decode_span =
                debug_span!("jira.links.remote.decode", operation = "remote link fetch").entered();
            Self::decode_jira_json(
                &payload,
                req.token.as_deref(),
                req.account_email.as_deref(),
                "remote link fetch",
            )?
        };
        let links = map_remote_links(items);
        span.record("jira.links.remote.count", usize_to_i64(links.len()));
        Ok(links)
    }

    async fn fetch_child_issue_links(
        &self,
        issue_key: &str,
        req: &FetchRequest,
    ) -> Result<Vec<crate::model::ConversationLink>> {
        let span = debug_span!(
            "jira.links.child_search",
            jira.links.child.count = tracing::field::Empty
        );
        let _span_guard = span.enter();
        let per_page = Self::bounded_per_page(req.per_page);
        let mut start_at = 0u32;
        let mut next_page_token: Option<String> = None;
        let mut links = Vec::new();
        let jql = format!("parent = {issue_key}");

        loop {
            let _page_span =
                debug_span!("jira.links.child_search.page", start_at, per_page).entered();
            let url = format!("{}/rest/api/3/search/jql", self.base_url);
            let mut query_params: Vec<(String, String)> = vec![
                ("jql".into(), jql.clone()),
                ("maxResults".into(), per_page.to_string()),
                ("fields".into(), "key".into()),
            ];
            if let Some(token) = &next_page_token {
                query_params.push(("nextPageToken".into(), token.clone()));
            } else {
                query_params.push(("startAt".into(), start_at.to_string()));
            }

            let http = Self::apply_auth(
                self.client.get(&url),
                req.token.as_deref(),
                req.account_email.as_deref(),
            )
            .query(&query_params);
            let payload = Self::execute_request(http, "child issue search").await?;
            let page: JiraKeySearchResponse = {
                let _decode_span = debug_span!(
                    "jira.links.child_search.decode",
                    operation = "child issue search"
                )
                .entered();
                Self::decode_jira_json(
                    &payload,
                    req.token.as_deref(),
                    req.account_email.as_deref(),
                    "child issue search",
                )?
            };

            for issue in page.issues {
                links.push(crate::model::ConversationLink {
                    id: issue.key,
                    relation: "child".to_string(),
                    kind: Some("issue".to_string()),
                });
            }

            if let Some(token) = page.next_page_token {
                next_page_token = Some(token);
                continue;
            }
            if let (Some(s), Some(m), Some(t)) = (page.start_at, page.max_results, page.total) {
                let next = s + m;
                if next < t {
                    start_at = next;
                    next_page_token = None;
                    continue;
                }
                break;
            }
            if page.is_last == Some(false) {
                return Err(AppError::provider(
                    "Jira child issue search response indicated more pages but provided no pagination cursor.",
                )
                .with_provider("jira")
                .into());
            }
            break;
        }

        span.record("jira.links.child.count", usize_to_i64(links.len()));
        Ok(links)
    }

    pub(super) async fn fetch_comments(
        &self,
        issue_key: &str,
        req: &FetchRequest,
    ) -> Result<Vec<Comment>> {
        let span = debug_span!(
            "jira.hydrate.issue_comments",
            jira.comments.count = tracing::field::Empty
        );
        let _span_guard = span.enter();
        let mut start_at = 0u32;
        let per_page = Self::bounded_per_page(req.per_page);
        let mut out = vec![];

        loop {
            let _page_span = debug_span!("jira.comments.page", start_at, per_page).entered();
            let url = format!("{}/rest/api/3/issue/{issue_key}/comment", self.base_url);
            debug!(issue_key, start_at, per_page, "fetching Jira comment page");
            let http = Self::apply_auth(
                self.client.get(&url),
                req.token.as_deref(),
                req.account_email.as_deref(),
            )
            .query(&[
                ("startAt", start_at.to_string()),
                ("maxResults", per_page.to_string()),
            ]);
            let payload = Self::execute_request(http, "comment fetch").await?;
            let page: JiraCommentsPage = {
                let _decode_span =
                    debug_span!("jira.comments.decode", operation = "comment fetch").entered();
                Self::decode_jira_json(
                    &payload,
                    req.token.as_deref(),
                    req.account_email.as_deref(),
                    "comment fetch",
                )?
            };
            trace!(
                count = page.comments.len(),
                start_at = page.start_at,
                "decoded Jira comments page"
            );

            for c in page.comments {
                let body = extract_adf_text(&c.body);
                out.push(Comment {
                    author: c.author.map(|a| a.display_name),
                    created_at: c.created,
                    body: if body.is_empty() { None } else { Some(body) },
                    kind: Some("issue_comment".into()),
                    review_path: None,
                    review_line: None,
                    review_side: None,
                });
            }

            let next = page.start_at + page.max_results;
            if next >= page.total {
                break;
            }
            start_at = next;
        }

        span.record("jira.comments.count", usize_to_i64(out.len()));
        Ok(out)
    }

    pub(super) fn decode_jira_json<T: DeserializeOwned>(
        payload: &JiraHttpPayload,
        token: Option<&str>,
        account_email: Option<&str>,
        operation: &str,
    ) -> Result<T> {
        let decode_span = debug_span!(
            "jira.http.decode",
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
                    if account_email.is_some() {
                        " Jira auth failed. Check token format/scope (email:api_token for Atlassian Cloud)."
                    } else {
                        " Jira auth failed. If this is an Atlassian API token, also set account email (--account-email, JIRA_ACCOUNT_EMAIL, or [instances.<alias>].account_email), or pass --token as email:api_token."
                    }
                } else {
                    " No Jira token detected. Set --token, JIRA_TOKEN, or [instances.<alias>].token."
                }
            } else {
                ""
            };
            let mut err = AppError::from_http("Jira", operation, payload.status, &payload.body)
                .with_provider("jira");
            if !auth_hint.is_empty() {
                err = err.with_hint(auth_hint.trim());
            }
            return Err(err.into());
        }

        if !payload.content_type.contains("application/json") {
            let error_message = format!("unexpected content-type '{}'", payload.content_type);
            decode_span.record("error.type", "unexpected_content_type");
            decode_span.record("error.message", error_message.as_str());
            return Err(AppError::provider(format!(
                "Jira API {} returned non-JSON content-type '{}' (body starts with: {}). This often means an auth/login page.",
                operation,
                payload.content_type,
                body_snippet(&payload.body)
            ))
            .with_provider("jira")
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
                    "Jira",
                    operation,
                    format!("{err} (body starts with: {})", body_snippet(&payload.body)),
                )
                .into())
            }
        }
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
