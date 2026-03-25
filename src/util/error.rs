use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

use crate::providers::OutputFormat;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Provider error: {0}")]
    Provider(#[from] anyhow::Error),

    #[error("Authentication required: {0}")]
    Unauthorized(String),

    #[error("Provider unavailable: {0}")]
    Unavailable(String),

    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Rate limited")]
    RateLimited,

    #[error("Upstream error: {status} {body}")]
    Upstream { status: u16, body: String },
}

impl AppError {
    /// Extract the HTTP status code and message from this error.
    pub fn status_and_message(&self) -> (StatusCode, String) {
        match self {
            AppError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg.clone()),
            AppError::Unavailable(msg) => (StatusCode::SERVICE_UNAVAILABLE, msg.clone()),
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            AppError::RateLimited => (StatusCode::TOO_MANY_REQUESTS, "Rate limit exceeded".into()),
            AppError::Upstream { status, body } => {
                let code = StatusCode::from_u16(*status).unwrap_or(StatusCode::BAD_GATEWAY);
                (code, body.clone())
            }
            AppError::Provider(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        }
    }

    /// Build an error response in OpenAI format.
    pub fn openai_error(status: StatusCode, message: &str) -> Response {
        let error_type = match status {
            StatusCode::UNAUTHORIZED => "authentication_error",
            StatusCode::BAD_REQUEST => "invalid_request_error",
            StatusCode::TOO_MANY_REQUESTS => "rate_limit_error",
            StatusCode::NOT_FOUND => "not_found_error",
            _ => "server_error",
        };
        let body = json!({
            "error": {
                "message": message,
                "type": error_type,
                "code": serde_json::Value::Null,
            }
        });
        (status, axum::Json(body)).into_response()
    }

    /// Build an error response in Anthropic format.
    pub fn anthropic_error(status: StatusCode, message: &str) -> Response {
        let error_type = match status {
            StatusCode::UNAUTHORIZED => "authentication_error",
            StatusCode::BAD_REQUEST => "invalid_request_error",
            StatusCode::TOO_MANY_REQUESTS => "rate_limit_error",
            StatusCode::NOT_FOUND => "not_found_error",
            StatusCode::SERVICE_UNAVAILABLE => "api_error",
            StatusCode::FORBIDDEN => "permission_error",
            _ => "api_error",
        };
        let body = json!({
            "type": "error",
            "error": {
                "type": error_type,
                "message": message,
            }
        });
        (status, axum::Json(body)).into_response()
    }

    /// Build an error response matching the given output format.
    pub fn format_error(status: StatusCode, message: &str, format: OutputFormat) -> Response {
        match format {
            OutputFormat::OpenAI => Self::openai_error(status, message),
            OutputFormat::Anthropic => Self::anthropic_error(status, message),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AppError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg.clone()),
            AppError::Unavailable(msg) => (StatusCode::SERVICE_UNAVAILABLE, msg.clone()),
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            AppError::RateLimited => (StatusCode::TOO_MANY_REQUESTS, "Rate limit exceeded".into()),
            AppError::Upstream { status, body } => {
                let code = StatusCode::from_u16(*status).unwrap_or(StatusCode::BAD_GATEWAY);
                return (code, body.clone()).into_response();
            }
            AppError::Provider(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        };
        // Default to OpenAI format when format is unknown (e.g. middleware errors)
        Self::openai_error(status, &message)
    }
}
