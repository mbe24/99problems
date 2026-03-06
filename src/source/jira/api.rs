use anyhow::Result;
use reqwest::StatusCode;
use reqwest::blocking::{RequestBuilder, Response};
use reqwest::header::CONTENT_TYPE;
use serde::Deserialize;
use tracing::{debug, trace};

use super::model::{JiraCommentsPage, JiraIssueItem, extract_adf_text};
use super::{JiraSource, PAGE_SIZE};
use crate::error::{AppError, app_error_from_decode, app_error_from_reqwest};
use crate::model::{Comment, Conversation};
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

    pub(super) fn send(req: RequestBuilder, operation: &str) -> Result<Response> {
        req.send()
            .map_err(|err| app_error_from_reqwest("Jira", operation, &err).into())
    }

    pub(super) fn fetch_issue(&self, id_or_key: &str, req: &FetchRequest) -> Result<Conversation> {
        let fields = "summary,description,status";
        let url = format!("{}/rest/api/3/issue/{}", self.base_url, id_or_key);
        let http = Self::apply_auth(
            self.client.get(&url),
            req.token.as_deref(),
            req.account_email.as_deref(),
        )
        .query(&[("fields", fields)]);
        let resp = Self::send(http, "issue fetch")?;
        if resp.status() == StatusCode::NOT_FOUND {
            let body = resp.text().unwrap_or_default();
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
        let issue: JiraIssueItem = Self::parse_jira_json(
            resp,
            req.token.as_deref(),
            req.account_email.as_deref(),
            "issue fetch",
        )?;
        let comments = if req.include_comments {
            self.fetch_comments(&issue.key, req)?
        } else {
            vec![]
        };
        Ok(Conversation {
            id: issue.key,
            title: issue.fields.summary,
            state: issue.fields.status.name,
            body: issue
                .fields
                .description
                .as_ref()
                .map(extract_adf_text)
                .filter(|s| !s.is_empty()),
            comments,
        })
    }

    pub(super) fn fetch_comments(
        &self,
        issue_key: &str,
        req: &FetchRequest,
    ) -> Result<Vec<Comment>> {
        let mut start_at = 0u32;
        let per_page = Self::bounded_per_page(req.per_page);
        let mut out = vec![];

        loop {
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
            let resp = Self::send(http, "comment fetch")?;
            let page: JiraCommentsPage = Self::parse_jira_json(
                resp,
                req.token.as_deref(),
                req.account_email.as_deref(),
                "comment fetch",
            )?;
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

    pub(super) fn parse_jira_json<T: for<'de> Deserialize<'de>>(
        resp: Response,
        token: Option<&str>,
        account_email: Option<&str>,
        operation: &str,
    ) -> Result<T> {
        let status = resp.status();
        let content_type = resp
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let body = resp.text()?;

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
