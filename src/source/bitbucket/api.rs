use anyhow::Result;
use reqwest::StatusCode;
use reqwest::blocking::{RequestBuilder, Response};
use serde::de::DeserializeOwned;
use tracing::{debug, trace};

use super::model::BitbucketPage;
use super::{BitbucketSource, PAGE_SIZE};
use crate::error::{AppError, app_error_from_decode, app_error_from_reqwest};

impl BitbucketSource {
    pub(super) fn bounded_per_page(per_page: u32) -> u32 {
        per_page.clamp(1, PAGE_SIZE)
    }

    pub(super) fn send(req: RequestBuilder, operation: &str) -> Result<Response> {
        req.send()
            .map_err(|err| app_error_from_reqwest("Bitbucket", operation, &err).into())
    }

    pub(super) fn get_one<T: DeserializeOwned>(
        &self,
        url: &str,
        token: Option<&str>,
        account_email: Option<&str>,
        operation: &str,
    ) -> Result<Option<T>> {
        let request = apply_auth(self.client.get(url), token, account_email)
            .header("Accept", "application/json");
        let response = Self::send(request, operation)?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let item = parse_bitbucket_json(response, token, account_email, operation)?;
        Ok(Some(item))
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn get_pages_stream<T: DeserializeOwned>(
        &self,
        url: &str,
        params: &[(String, String)],
        token: Option<&str>,
        account_email: Option<&str>,
        per_page: u32,
        emit: &mut dyn FnMut(T) -> Result<()>,
    ) -> Result<usize> {
        let per_page = Self::bounded_per_page(per_page);
        let mut emitted = 0usize;
        let mut next_url = Some(url.to_string());
        let mut first = true;

        while let Some(current_url) = next_url {
            debug!(url = %current_url, per_page, "fetching Bitbucket page");
            let mut request = apply_auth(self.client.get(&current_url), token, account_email)
                .header("Accept", "application/json");
            if first {
                let mut merged_params = params.to_vec();
                merged_params.push(("pagelen".to_string(), per_page.to_string()));
                request = request.query(&merged_params);
                first = false;
            }

            let response = Self::send(request, "page fetch")?;
            let page: BitbucketPage<T> =
                parse_bitbucket_json(response, token, account_email, "page fetch")?;
            trace!(count = page.values.len(), "decoded Bitbucket page");
            for item in page.values {
                emit(item)?;
                emitted += 1;
            }
            next_url = page.next;
        }

        Ok(emitted)
    }
}

fn apply_auth(
    req: RequestBuilder,
    token: Option<&str>,
    account_email: Option<&str>,
) -> RequestBuilder {
    match resolve_auth_mode(token, account_email) {
        AuthMode::None => req,
        AuthMode::Bearer(token) => req.bearer_auth(token),
        AuthMode::Basic { user, secret } => req.basic_auth(user, Some(secret)),
    }
}

fn auth_hint(token: Option<&str>, account_email: Option<&str>) -> &'static str {
    if token.is_some() {
        if account_email.is_some() {
            "Check Bitbucket token credentials and scopes."
        } else {
            "If this is an Atlassian API token, also set account email (--account-email, BITBUCKET_ACCOUNT_EMAIL, or [instances.<alias>].account_email), or pass --token as email:token."
        }
    } else {
        "No Bitbucket token detected. Set --token, BITBUCKET_TOKEN, or [instances.<alias>].token."
    }
}

fn parse_bitbucket_json<T: DeserializeOwned>(
    resp: Response,
    token: Option<&str>,
    account_email: Option<&str>,
    operation: &str,
) -> Result<T> {
    let status = resp.status();
    let body = resp.text()?;
    if !status.is_success() {
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Err(AppError::auth(format!(
                "Bitbucket API {operation} error {status}: {}. {}",
                body_snippet(&body),
                auth_hint(token, account_email)
            ))
            .with_provider("bitbucket")
            .with_http_status(status)
            .into());
        }
        return Err(AppError::from_http("Bitbucket", operation, status, &body)
            .with_provider("bitbucket")
            .into());
    }

    serde_json::from_str(&body).map_err(|err| {
        app_error_from_decode(
            "Bitbucket",
            operation,
            format!("{err} (body starts with: {})", body_snippet(&body)),
        )
        .into()
    })
}

fn body_snippet(body: &str) -> String {
    body.chars()
        .take(200)
        .collect::<String>()
        .replace('\n', " ")
}

#[derive(Debug, PartialEq, Eq)]
enum AuthMode<'a> {
    None,
    Bearer(&'a str),
    Basic { user: &'a str, secret: &'a str },
}

fn resolve_auth_mode<'a>(token: Option<&'a str>, account_email: Option<&'a str>) -> AuthMode<'a> {
    match token {
        Some(t) if t.contains(':') => {
            let (user, secret) = t.split_once(':').unwrap_or_default();
            AuthMode::Basic { user, secret }
        }
        Some(t) if looks_like_atlassian_bearer(t) => AuthMode::Bearer(t),
        Some(t) => match account_email {
            Some(email) => AuthMode::Basic {
                user: email,
                secret: t,
            },
            None => AuthMode::Bearer(t),
        },
        None => AuthMode::None,
    }
}

fn looks_like_atlassian_bearer(token: &str) -> bool {
    token.starts_with("AT")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_auth_mode_prefers_explicit_basic() {
        assert_eq!(
            resolve_auth_mode(Some("user:pass"), Some("mail@example.com")),
            AuthMode::Basic {
                user: "user",
                secret: "pass"
            }
        );
    }

    #[test]
    fn resolve_auth_mode_uses_bearer_for_atlassian_tokens() {
        assert_eq!(
            resolve_auth_mode(Some("ATxxxx"), Some("mail@example.com")),
            AuthMode::Bearer("ATxxxx")
        );
    }

    #[test]
    fn resolve_auth_mode_uses_email_basic_for_non_atlassian_tokens() {
        assert_eq!(
            resolve_auth_mode(Some("plain-token"), Some("mail@example.com")),
            AuthMode::Basic {
                user: "mail@example.com",
                secret: "plain-token"
            }
        );
    }
}
