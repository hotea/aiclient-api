use aiclient_api::providers::OutputFormat;
use aiclient_api::util::error::AppError;
use axum::http::StatusCode;

// ---------------------------------------------------------------------------
// status_and_message
// ---------------------------------------------------------------------------

#[test]
fn test_status_and_message_unauthorized() {
    let err = AppError::Unauthorized("invalid token".into());
    let (status, message) = err.status_and_message();
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(message, "invalid token");
}

#[test]
fn test_status_and_message_unavailable() {
    let err = AppError::Unavailable("copilot is down".into());
    let (status, message) = err.status_and_message();
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(message, "copilot is down");
}

#[test]
fn test_status_and_message_bad_request() {
    let err = AppError::BadRequest("missing model field".into());
    let (status, message) = err.status_and_message();
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(message, "missing model field");
}

#[test]
fn test_status_and_message_rate_limited() {
    let err = AppError::RateLimited;
    let (status, _message) = err.status_and_message();
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
}

#[test]
fn test_status_and_message_upstream() {
    let err = AppError::Upstream {
        status: 502,
        body: "bad gateway from upstream".into(),
    };
    let (status, message) = err.status_and_message();
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(message, "bad gateway from upstream");
}

#[test]
fn test_status_and_message_provider() {
    let err = AppError::Provider(anyhow::anyhow!("something went wrong internally"));
    let (status, _message) = err.status_and_message();
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
}

// ---------------------------------------------------------------------------
// openai_error
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_openai_error_format() {
    let response = AppError::openai_error(StatusCode::UNAUTHORIZED, "bad key");
    let (parts, body) = response.into_parts();

    assert_eq!(parts.status, StatusCode::UNAUTHORIZED);

    let bytes = axum::body::to_bytes(body, 1024 * 1024).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    assert_eq!(json["error"]["message"], "bad key");
    assert_eq!(json["error"]["type"], "authentication_error");
    assert_eq!(json["error"]["code"], serde_json::Value::Null);
}

// ---------------------------------------------------------------------------
// anthropic_error
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_anthropic_error_format() {
    let response = AppError::anthropic_error(StatusCode::UNAUTHORIZED, "bad key");
    let (parts, body) = response.into_parts();

    assert_eq!(parts.status, StatusCode::UNAUTHORIZED);

    let bytes = axum::body::to_bytes(body, 1024 * 1024).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    assert_eq!(json["type"], "error");
    assert_eq!(json["error"]["type"], "authentication_error");
    assert_eq!(json["error"]["message"], "bad key");
}

// ---------------------------------------------------------------------------
// format_error
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_format_error_delegates_to_openai() {
    let response =
        AppError::format_error(StatusCode::BAD_REQUEST, "bad request", OutputFormat::OpenAI);
    let (parts, body) = response.into_parts();

    assert_eq!(parts.status, StatusCode::BAD_REQUEST);

    let bytes = axum::body::to_bytes(body, 1024 * 1024).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    // OpenAI format has a top-level "error" object
    assert!(json.get("error").is_some(), "expected top-level 'error' key");
    assert_eq!(json["error"]["type"], "invalid_request_error");
    assert_eq!(json["error"]["message"], "bad request");
    assert_eq!(json["error"]["code"], serde_json::Value::Null);
    // Anthropic's top-level "type" key must not be present
    assert!(json.get("type").is_none(), "unexpected 'type' key (Anthropic format)");
}

#[tokio::test]
async fn test_format_error_delegates_to_anthropic() {
    let response = AppError::format_error(
        StatusCode::SERVICE_UNAVAILABLE,
        "provider down",
        OutputFormat::Anthropic,
    );
    let (parts, body) = response.into_parts();

    assert_eq!(parts.status, StatusCode::SERVICE_UNAVAILABLE);

    let bytes = axum::body::to_bytes(body, 1024 * 1024).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    // Anthropic format has a top-level "type": "error"
    assert_eq!(json["type"], "error");
    assert_eq!(json["error"]["type"], "api_error");
    assert_eq!(json["error"]["message"], "provider down");
    // OpenAI's nested "code" key must not be present
    assert!(
        json["error"].get("code").is_none(),
        "unexpected 'code' key (OpenAI format)"
    );
}
