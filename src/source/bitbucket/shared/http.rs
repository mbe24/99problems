use anyhow::Result;
use reqwest::StatusCode;
use reqwest::header::CONTENT_TYPE;
use reqwest_middleware::RequestBuilder;
use serde::de::DeserializeOwned;
use tracing::{Instrument, debug_span};

use crate::error::{AppError, app_error_from_decode, app_error_from_reqwest};

pub(in crate::source::bitbucket) struct BitbucketHttpPayload {
    pub(in crate::source::bitbucket) status: StatusCode,
    pub(in crate::source::bitbucket) content_type: String,
    pub(in crate::source::bitbucket) body: String,
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
                app_error_from_reqwest("Bitbucket", operation, &err),
                "request_send_error",
                message,
            )
        }
        other @ reqwest_middleware::Error::Middleware(_) => {
            let message = other.to_string();
            (
                AppError::provider(format!(
                    "Bitbucket API {operation} middleware error: {other}"
                ))
                .with_provider("bitbucket"),
                "request_middleware_error",
                message,
            )
        }
    }
}

pub(in crate::source::bitbucket) async fn execute_request(
    req: RequestBuilder,
    operation: &str,
) -> Result<BitbucketHttpPayload> {
    let exchange_span = debug_span!(
        "bitbucket.http.exchange",
        operation = operation,
        status_code = tracing::field::Empty,
        body_bytes = tracing::field::Empty,
        error.type = tracing::field::Empty,
        error.message = tracing::field::Empty
    );
    let response = apply_otel_span_name(req, "reqwest.http.get")
        .send()
        .instrument(debug_span!("bitbucket.http.request", operation = operation))
        .instrument(exchange_span.clone())
        .await;
    let response = match response {
        Ok(response) => response,
        Err(err) => {
            let (mapped, error_type, error_message) = map_request_error(operation, err);
            exchange_span.record("error.type", error_type);
            exchange_span.record("error.message", error_message.as_str());
            return Err(mapped.into());
        }
    };

    let status = response.status();
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    let body = {
        let read_span = debug_span!(
            "bitbucket.http.response.read",
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
                return Err(app_error_from_reqwest("Bitbucket", operation, &err).into());
            }
        }
    };

    exchange_span.record("status_code", i64::from(status.as_u16()));
    exchange_span.record("body_bytes", usize_to_i64(body.len()));
    Ok(BitbucketHttpPayload {
        status,
        content_type,
        body,
    })
}

pub(in crate::source::bitbucket) fn decode_bitbucket_json<T: DeserializeOwned>(
    payload: &BitbucketHttpPayload,
    token: Option<&str>,
    operation: &str,
) -> Result<T> {
    let decode_span = debug_span!(
        "bitbucket.http.decode",
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
        if payload.status == StatusCode::UNAUTHORIZED || payload.status == StatusCode::FORBIDDEN {
            return Err(AppError::auth(format!(
                "Bitbucket API {operation} error {}: {}. {}",
                payload.status,
                body_snippet(&payload.body),
                auth_hint(token)
            ))
            .with_provider("bitbucket")
            .with_http_status(payload.status)
            .into());
        }
        return Err(
            AppError::from_http("Bitbucket", operation, payload.status, &payload.body)
                .with_provider("bitbucket")
                .into(),
        );
    }

    if !payload.content_type.contains("application/json") {
        let error_message = format!("unexpected content-type '{}'", payload.content_type);
        decode_span.record("error.type", "unexpected_content_type");
        decode_span.record("error.message", error_message.as_str());
        return Err(AppError::provider(format!(
            "Bitbucket API {} returned non-JSON content-type '{}' (body starts with: {}).",
            operation,
            payload.content_type,
            body_snippet(&payload.body)
        ))
        .with_provider("bitbucket")
        .with_http_status(payload.status)
        .into());
    }

    serde_json::from_str(&payload.body).map_err(|err| {
        let error_message = format!("decode failed: {err}");
        decode_span.record("error.type", "decode_error");
        decode_span.record("error.message", error_message.as_str());
        app_error_from_decode(
            "Bitbucket",
            operation,
            format!("{err} (body starts with: {})", body_snippet(&payload.body)),
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

fn usize_to_i64(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}
