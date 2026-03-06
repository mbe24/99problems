use anyhow::Result;
use reqwest::StatusCode;
use reqwest::blocking::{RequestBuilder, Response};
use serde::de::DeserializeOwned;

use crate::error::{AppError, app_error_from_decode, app_error_from_reqwest};

pub(in crate::source::bitbucket) fn send(req: RequestBuilder, operation: &str) -> Result<Response> {
    req.send()
        .map_err(|err| app_error_from_reqwest("Bitbucket", operation, &err).into())
}

pub(in crate::source::bitbucket) fn parse_bitbucket_json<T: DeserializeOwned>(
    resp: Response,
    token: Option<&str>,
    operation: &str,
) -> Result<T> {
    let status = resp.status();
    let body = resp.text()?;
    if !status.is_success() {
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Err(AppError::auth(format!(
                "Bitbucket API {operation} error {status}: {}. {}",
                body_snippet(&body),
                auth_hint(token)
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

fn auth_hint(token: Option<&str>) -> &'static str {
    if token.is_some() {
        "Check Bitbucket token credentials and scopes."
    } else {
        "No Bitbucket token detected. Set --token, BITBUCKET_TOKEN, or [instances.<alias>].token."
    }
}

fn body_snippet(body: &str) -> String {
    body.chars()
        .take(200)
        .collect::<String>()
        .replace('\n', " ")
}
