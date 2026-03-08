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

    pub(super) async fn send(req: RequestBuilder, operation: &str) -> Result<reqwest::Response> {
        let _span = debug_span!("jira.http.request", operation = operation).entered();
        req.send().await.map_err(|err| match err {
            reqwest_middleware::Error::Reqwest(err) => {
                app_error_from_reqwest("Jira", operation, &err).into()
            }
            other @ reqwest_middleware::Error::Middleware(_) => {
                AppError::provider(format!("Jira API {operation} middleware error: {other}"))
                    .with_provider("jira")
                    .into()
            }
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
        let resp = Self::send(http, "issue fetch").await?;
        if resp.status() == StatusCode::NOT_FOUND {
            let body = resp.text().await.unwrap_or_default();
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
                body_snippet(&body)
            ))
            .with_provider("jira")
            .with_http_status(StatusCode::NOT_FOUND)
            .into());
        }
        let issue: JiraIssueItem = {
            let _decode_span =
                debug_span!("jira.issue.decode", operation = "issue fetch").entered();
            Self::parse_jira_json(
                resp,
                req.token.as_deref(),
                req.account_email.as_deref(),
                "issue fetch",
            )
            .await?
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
        let _span = debug_span!("jira.links.remote").entered();
        let url = format!("{}/rest/api/3/issue/{issue_key}/remotelink", self.base_url);
        let http = Self::apply_auth(
            self.client.get(&url),
            req.token.as_deref(),
            req.account_email.as_deref(),
        );
        let resp = Self::send(http, "remote link fetch").await?;
        let items: Vec<JiraRemoteLinkItem> = {
            let _decode_span =
                debug_span!("jira.links.remote.decode", operation = "remote link fetch").entered();
            Self::parse_jira_json(
                resp,
                req.token.as_deref(),
                req.account_email.as_deref(),
                "remote link fetch",
            )
            .await?
        };
        Ok(map_remote_links(items))
    }

    async fn fetch_child_issue_links(
        &self,
        issue_key: &str,
        req: &FetchRequest,
    ) -> Result<Vec<crate::model::ConversationLink>> {
        let _span = debug_span!("jira.links.child_search").entered();
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
            let resp = Self::send(http, "child issue search").await?;
            let page: JiraKeySearchResponse = {
                let _decode_span = debug_span!(
                    "jira.links.child_search.decode",
                    operation = "child issue search"
                )
                .entered();
                Self::parse_jira_json(
                    resp,
                    req.token.as_deref(),
                    req.account_email.as_deref(),
                    "child issue search",
                )
                .await?
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

        Ok(links)
    }

    pub(super) async fn fetch_comments(
        &self,
        issue_key: &str,
        req: &FetchRequest,
    ) -> Result<Vec<Comment>> {
        let _span = debug_span!("jira.hydrate.issue_comments").entered();
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
            let resp = Self::send(http, "comment fetch").await?;
            let page: JiraCommentsPage = {
                let _decode_span =
                    debug_span!("jira.comments.decode", operation = "comment fetch").entered();
                Self::parse_jira_json(
                    resp,
                    req.token.as_deref(),
                    req.account_email.as_deref(),
                    "comment fetch",
                )
                .await?
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

        Ok(out)
    }

    pub(super) async fn parse_jira_json<T: DeserializeOwned>(
        resp: reqwest::Response,
        token: Option<&str>,
        account_email: Option<&str>,
        operation: &str,
    ) -> Result<T> {
        let _span = debug_span!("jira.http.decode", operation = operation).entered();
        let status = resp.status();
        let content_type = resp
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let body = resp.text().await?;

        if !status.is_success() {
            let auth_hint = if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN
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
            let mut err =
                AppError::from_http("Jira", operation, status, &body).with_provider("jira");
            if !auth_hint.is_empty() {
                err = err.with_hint(auth_hint.trim());
            }
            return Err(err.into());
        }

        if !content_type.contains("application/json") {
            return Err(AppError::provider(format!(
                "Jira API {} returned non-JSON content-type '{}' (body starts with: {}). This often means an auth/login page.",
                operation,
                content_type,
                body_snippet(&body)
            ))
            .with_provider("jira")
            .with_http_status(status)
            .into());
        }

        serde_json::from_str(&body).map_err(|e| {
            app_error_from_decode(
                "Jira",
                operation,
                format!("{e} (body starts with: {})", body_snippet(&body)),
            )
            .into()
        })
    }
}

fn body_snippet(body: &str) -> String {
    body.chars()
        .take(200)
        .collect::<String>()
        .replace('\n', " ")
}
