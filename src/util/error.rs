use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

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
        let body = json!({ "error": { "message": message, "type": "error" } });
        (status, axum::Json(body)).into_response()
    }
}
