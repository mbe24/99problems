use reqwest::StatusCode;
use serde_json::json;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCategory {
    Usage,
    Auth,
    NotFound,
    RateLimited,
    Network,
    Provider,
    Internal,
}

impl ErrorCategory {
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::Usage => "usage",
            Self::Auth => "auth",
            Self::NotFound => "not_found",
            Self::RateLimited => "rate_limited",
            Self::Network => "network",
            Self::Provider => "provider",
            Self::Internal => "internal",
        }
    }

    #[must_use]
    pub fn exit_code(self) -> i32 {
        match self {
            Self::Usage => 2,
            Self::Auth => 3,
            Self::NotFound => 4,
            Self::RateLimited => 5,
            Self::Network => 6,
            Self::Provider => 7,
            Self::Internal => 1,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AppError {
    category: ErrorCategory,
    message: String,
    hint: Option<String>,
    provider: Option<String>,
    http_status: Option<u16>,
}

impl AppError {
    #[must_use]
    pub fn usage(message: impl Into<String>) -> Self {
        Self::new(ErrorCategory::Usage, message)
    }

    #[must_use]
    pub fn auth(message: impl Into<String>) -> Self {
        Self::new(ErrorCategory::Auth, message)
    }

    #[must_use]
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorCategory::NotFound, message)
    }

    #[must_use]
    pub fn rate_limited(message: impl Into<String>) -> Self {
        Self::new(ErrorCategory::RateLimited, message)
    }

    #[must_use]
    pub fn network(message: impl Into<String>) -> Self {
        Self::new(ErrorCategory::Network, message)
    }

    #[must_use]
    pub fn provider(message: impl Into<String>) -> Self {
        Self::new(ErrorCategory::Provider, message)
    }

    #[must_use]
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorCategory::Internal, message)
    }

    #[must_use]
    pub fn from_http(provider: &str, operation: &str, status: StatusCode, body: &str) -> Self {
        let mut err = match status.as_u16() {
            401 | 403 => Self::auth(format!(
                "{provider} API {operation} error {status}: {}",
                body_snippet(body)
            )),
            404 => Self::not_found(format!(
                "{provider} API {operation} error {status}: {}",
                body_snippet(body)
            )),
            429 => Self::rate_limited(format!(
                "{provider} API {operation} error {status}: {}",
                body_snippet(body)
            )),
            _ => Self::provider(format!(
                "{provider} API {operation} error {status}: {}",
                body_snippet(body)
            )),
        };
        err.provider = Some(provider.to_string());
        err.http_status = Some(status.as_u16());
        err
    }

    #[must_use]
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    #[must_use]
    pub fn with_provider(mut self, provider: impl Into<String>) -> Self {
        self.provider = Some(provider.into());
        self
    }

    #[must_use]
    pub fn with_http_status(mut self, status: StatusCode) -> Self {
        self.http_status = Some(status.as_u16());
        self
    }

    #[must_use]
    pub fn exit_code(&self) -> i32 {
        self.category.exit_code()
    }

    #[must_use]
    pub fn render_text(&self) -> String {
        match &self.hint {
            Some(hint) => format!("{}\nHint: {hint}", self.message),
            None => self.message.clone(),
        }
    }

    #[must_use]
    pub fn render_json(&self) -> String {
        json!({
            "code": self.category.code(),
            "exit_code": self.exit_code(),
            "message": self.message,
            "hint": self.hint,
            "provider": self.provider,
            "http_status": self.http_status,
        })
        .to_string()
    }

    #[must_use]
    pub fn category(&self) -> ErrorCategory {
        self.category
    }

    fn new(category: ErrorCategory, message: impl Into<String>) -> Self {
        Self {
            category,
            message: message.into(),
            hint: None,
            provider: None,
            http_status: None,
        }
    }
}

impl Display for AppError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for AppError {}

#[must_use]
pub fn classify_anyhow_error(err: &anyhow::Error) -> AppError {
    if let Some(app_err) = err.downcast_ref::<AppError>() {
        return app_err.clone();
    }

    if let Some(req_err) = err.downcast_ref::<reqwest::Error>() {
        if req_err.is_timeout() || req_err.is_connect() {
            return AppError::network(format!("Network request failed: {req_err}"));
        }
        return AppError::provider(format!("Remote request failed: {req_err}"));
    }

    if let Some(io_err) = err.downcast_ref::<std::io::Error>() {
        return AppError::internal(format!("I/O error: {io_err}"));
    }

    AppError::internal(err.to_string())
}

#[must_use]
pub fn app_error_from_reqwest(provider: &str, operation: &str, err: &reqwest::Error) -> AppError {
    if err.is_timeout() || err.is_connect() {
        return AppError::network(format!("{provider} {operation} request failed: {err}"))
            .with_provider(provider);
    }
    AppError::provider(format!("{provider} {operation} request failed: {err}"))
        .with_provider(provider)
}

#[must_use]
pub fn app_error_from_decode(provider: &str, operation: &str, err: impl Display) -> AppError {
    AppError::provider(format!(
        "{provider} {operation} response decode failed: {err}"
    ))
    .with_provider(provider)
}

fn body_snippet(body: &str) -> String {
    body.chars()
        .take(200)
        .collect::<String>()
        .replace('\n', " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_code_mapping_is_stable() {
        assert_eq!(ErrorCategory::Usage.exit_code(), 2);
        assert_eq!(ErrorCategory::Auth.exit_code(), 3);
        assert_eq!(ErrorCategory::NotFound.exit_code(), 4);
        assert_eq!(ErrorCategory::RateLimited.exit_code(), 5);
        assert_eq!(ErrorCategory::Network.exit_code(), 6);
        assert_eq!(ErrorCategory::Provider.exit_code(), 7);
        assert_eq!(ErrorCategory::Internal.exit_code(), 1);
    }

    #[test]
    fn json_renderer_includes_required_fields() {
        let rendered = AppError::auth("invalid token")
            .with_provider("github")
            .with_http_status(StatusCode::UNAUTHORIZED)
            .render_json();
        let value: serde_json::Value = serde_json::from_str(&rendered).unwrap();
        assert_eq!(value["code"], "auth");
        assert_eq!(value["exit_code"], 3);
        assert_eq!(value["provider"], "github");
        assert_eq!(value["http_status"], 401);
    }
}
