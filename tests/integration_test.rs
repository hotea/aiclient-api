use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;

use aiclient_api::config::types::Config;
use aiclient_api::providers::{Model, Provider, ProviderRequest, ProviderResponse};
use aiclient_api::server::state::AppState;

// ---------------------------------------------------------------------------
// Mock Provider
// ---------------------------------------------------------------------------

struct MockProvider {
    provider_name: String,
    called: Arc<AtomicBool>,
}

impl MockProvider {
    fn new(name: &str) -> Self {
        Self {
            provider_name: name.to_string(),
            called: Arc::new(AtomicBool::new(false)),
        }
    }

    fn was_called(&self) -> bool {
        self.called.load(Ordering::Relaxed)
    }
}

#[async_trait]
impl Provider for MockProvider {
    fn name(&self) -> &str {
        &self.provider_name
    }

    fn is_healthy(&self) -> bool {
        true
    }

    async fn list_models(&self) -> Result<Vec<Model>> {
        Ok(vec![Model {
            id: format!("{}/test-model", self.provider_name),
            provider: self.provider_name.clone(),
            vendor: "mock".to_string(),
            display_name: "Test Model".to_string(),
            max_input_tokens: Some(128_000),
            max_output_tokens: Some(4_096),
            supports_streaming: true,
            supports_tools: true,
            supports_vision: false,
            supports_thinking: false,
        }])
    }

    async fn chat(&self, request: ProviderRequest) -> Result<ProviderResponse> {
        self.called.store(true, Ordering::Relaxed);
        Ok(ProviderResponse::Complete(json!({
            "id": "mock-response",
            "content": [{"type": "text", "text": "Hello from mock"}],
            "model": request.model,
            "role": "assistant",
            "stop_reason": "end_turn",
        })))
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn test_config(default_provider: &str, api_key: &str) -> Config {
    Config {
        default_provider: default_provider.to_string(),
        api_key: api_key.to_string(),
        ..Config::default()
    }
}

async fn build_test_server_with_provider(
    provider_name: &str,
    api_key: &str,
) -> (axum_test::TestServer, Arc<MockProvider>) {
    let config = test_config(provider_name, api_key);
    let state = AppState::new(config);
    let mock = Arc::new(MockProvider::new(provider_name));
    {
        let mut providers = state.providers.write().await;
        providers.insert(provider_name.to_string(), mock.clone() as Arc<dyn Provider>);
    }
    let app = aiclient_api::server::build_router(state);
    let server = axum_test::TestServer::new(app);
    (server, mock)
}

async fn build_test_server_with_two_providers(
    api_key: &str,
) -> (axum_test::TestServer, Arc<MockProvider>, Arc<MockProvider>) {
    let config = test_config("provider_a", api_key);
    let state = AppState::new(config);
    let mock_a = Arc::new(MockProvider::new("provider_a"));
    let mock_b = Arc::new(MockProvider::new("provider_b"));
    {
        let mut providers = state.providers.write().await;
        providers.insert("provider_a".to_string(), mock_a.clone() as Arc<dyn Provider>);
        providers.insert("provider_b".to_string(), mock_b.clone() as Arc<dyn Provider>);
    }
    let app = aiclient_api::server::build_router(state);
    let server = axum_test::TestServer::new(app);
    (server, mock_a, mock_b)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Send POST /v1/chat/completions with an OpenAI-format body and verify the
/// response is valid OpenAI JSON (contains "choices", "object", "model").
#[tokio::test]
async fn test_openai_endpoint_with_mock_provider() {
    let (server, _mock) = build_test_server_with_provider("mock", "").await;

    let body = json!({
        "model": "test-model",
        "messages": [
            {"role": "user", "content": "Hello"}
        ]
    });

    let response = server
        .post("/v1/chat/completions")
        .json(&body)
        .await;

    response.assert_status_ok();

    let json: serde_json::Value = response.json();
    assert_eq!(json["object"], "chat.completion");
    assert!(json["choices"].is_array(), "response should contain choices array");
    let choices = json["choices"].as_array().unwrap();
    assert!(!choices.is_empty(), "choices should not be empty");
    assert!(json["model"].is_string(), "response should contain model");
    // The converted response should have message.content with the mock text
    let message = &choices[0]["message"];
    assert_eq!(message["role"], "assistant");
    assert!(
        message["content"].as_str().unwrap().contains("Hello from mock"),
        "content should contain mock response text"
    );
}

/// Send POST /v1/messages with an Anthropic-format body and verify the
/// response is valid Anthropic JSON (contains "type": "message", "content" blocks).
#[tokio::test]
async fn test_anthropic_endpoint_with_mock_provider() {
    let (server, _mock) = build_test_server_with_provider("mock", "").await;

    let body = json!({
        "model": "test-model",
        "max_tokens": 1024,
        "messages": [
            {"role": "user", "content": "Hello"}
        ]
    });

    let response = server
        .post("/v1/messages")
        .json(&body)
        .await;

    response.assert_status_ok();

    let json: serde_json::Value = response.json();
    // The mock returns Anthropic-style content blocks, and to_anthropic
    // should pass them through or re-wrap them.
    assert_eq!(json["role"], "assistant");
    assert!(json["content"].is_array(), "response should contain content array");
    let content = json["content"].as_array().unwrap();
    assert!(!content.is_empty(), "content should not be empty");
    assert_eq!(content[0]["type"], "text");
    assert!(
        content[0]["text"].as_str().unwrap().contains("Hello from mock"),
        "text block should contain mock response text"
    );
}

/// Register two providers ("provider_a" and "provider_b"). Send a request
/// with model "provider_a/test-model" and verify provider_a was called while
/// provider_b was not.
#[tokio::test]
async fn test_model_routing_with_prefix() {
    let (server, mock_a, mock_b) = build_test_server_with_two_providers("").await;

    let body = json!({
        "model": "provider_a/test-model",
        "messages": [
            {"role": "user", "content": "Hello"}
        ]
    });

    let response = server
        .post("/v1/chat/completions")
        .json(&body)
        .await;

    response.assert_status_ok();
    assert!(mock_a.was_called(), "provider_a should have been called");
    assert!(!mock_b.was_called(), "provider_b should NOT have been called");
}

/// Set api_key in config. Send a request WITHOUT an Authorization header.
/// Assert 401 Unauthorized.
#[tokio::test]
async fn test_auth_middleware_rejects_without_key() {
    let (server, _mock) = build_test_server_with_provider("mock", "test123").await;

    let body = json!({
        "model": "test-model",
        "messages": [
            {"role": "user", "content": "Hello"}
        ]
    });

    let response = server
        .post("/v1/chat/completions")
        .json(&body)
        .await;

    response.assert_status(axum::http::StatusCode::UNAUTHORIZED);
}

/// Set api_key in config. Send a request WITH the correct Bearer token.
/// Assert 200 OK.
#[tokio::test]
async fn test_auth_middleware_accepts_correct_key() {
    let (server, _mock) = build_test_server_with_provider("mock", "test123").await;

    let body = json!({
        "model": "test-model",
        "messages": [
            {"role": "user", "content": "Hello"}
        ]
    });

    let response = server
        .post("/v1/chat/completions")
        .add_header(
            axum::http::header::AUTHORIZATION,
            axum::http::HeaderValue::from_static("Bearer test123"),
        )
        .json(&body)
        .await;

    response.assert_status_ok();
}

/// Register a mock provider with known models. GET /v1/models and assert the
/// response contains the expected model list.
#[tokio::test]
async fn test_models_endpoint() {
    let (server, _mock) = build_test_server_with_provider("mock", "").await;

    let response = server.get("/v1/models").await;

    response.assert_status_ok();

    let json: serde_json::Value = response.json();
    assert_eq!(json["object"], "list");
    let data = json["data"].as_array().expect("data should be an array");
    assert!(!data.is_empty(), "models list should not be empty");

    // The mock provider returns one model: "mock/test-model"
    let model_ids: Vec<&str> = data
        .iter()
        .filter_map(|m| m["id"].as_str())
        .collect();
    assert!(
        model_ids.contains(&"mock/test-model"),
        "models list should contain mock/test-model, got: {:?}",
        model_ids
    );
    assert_eq!(data[0]["owned_by"], "mock");
}

// ---------------------------------------------------------------------------
// Router tests
// ---------------------------------------------------------------------------

/// Register provider_a and provider_b. Send a request with model "test-model"
/// (no prefix) and header `X-Provider: provider_b`. Assert provider_b was
/// called, provider_a was not.
#[tokio::test]
async fn test_model_routing_with_x_provider_header() {
    let (server, mock_a, mock_b) = build_test_server_with_two_providers("").await;

    let body = json!({
        "model": "test-model",
        "messages": [
            {"role": "user", "content": "Hello"}
        ]
    });

    let response = server
        .post("/v1/chat/completions")
        .add_header(
            axum::http::HeaderName::from_static("x-provider"),
            axum::http::HeaderValue::from_static("provider_b"),
        )
        .json(&body)
        .await;

    response.assert_status_ok();
    assert!(!mock_a.was_called(), "provider_a should NOT have been called");
    assert!(mock_b.was_called(), "provider_b should have been called");
}

/// Register provider_a as the default provider. Send a request with model
/// "test-model" (no prefix, no X-Provider header). Assert provider_a was
/// called.
#[tokio::test]
async fn test_model_routing_falls_back_to_default() {
    let (server, mock_a) = build_test_server_with_provider("provider_a", "").await;

    let body = json!({
        "model": "test-model",
        "messages": [
            {"role": "user", "content": "Hello"}
        ]
    });

    let response = server
        .post("/v1/chat/completions")
        .json(&body)
        .await;

    response.assert_status_ok();
    assert!(mock_a.was_called(), "provider_a (default) should have been called");
}

/// Send a request with model "nonexistent/test-model". The router should fail
/// to find a provider named "nonexistent" and return a non-200 response.
#[tokio::test]
async fn test_model_routing_unknown_prefix_returns_error() {
    let (server, _mock) = build_test_server_with_provider("mock", "").await;

    let body = json!({
        "model": "nonexistent/test-model",
        "messages": [
            {"role": "user", "content": "Hello"}
        ]
    });

    let response = server
        .post("/v1/chat/completions")
        .json(&body)
        .await;

    assert!(
        response.status_code() != axum::http::StatusCode::OK,
        "unknown provider prefix should result in a non-200 response, got {}",
        response.status_code()
    );
}

// ---------------------------------------------------------------------------
// Auth middleware tests
// ---------------------------------------------------------------------------

/// Set api_key to "secret". Send a request with "Bearer wrong". Assert 401.
#[tokio::test]
async fn test_auth_middleware_wrong_key_returns_401() {
    let (server, _mock) = build_test_server_with_provider("mock", "secret").await;

    let body = json!({
        "model": "test-model",
        "messages": [
            {"role": "user", "content": "Hello"}
        ]
    });

    let response = server
        .post("/v1/chat/completions")
        .add_header(
            axum::http::header::AUTHORIZATION,
            axum::http::HeaderValue::from_static("Bearer wrong"),
        )
        .json(&body)
        .await;

    response.assert_status(axum::http::StatusCode::UNAUTHORIZED);
}

/// Set api_key to "" (empty). Send a request without any Authorization header.
/// Assert 200 (empty api_key disables auth enforcement).
#[tokio::test]
async fn test_auth_middleware_no_key_config_allows_all() {
    let (server, _mock) = build_test_server_with_provider("mock", "").await;

    let body = json!({
        "model": "test-model",
        "messages": [
            {"role": "user", "content": "Hello"}
        ]
    });

    let response = server
        .post("/v1/chat/completions")
        .json(&body)
        .await;

    response.assert_status_ok();
}

/// Set api_key to "secret". Send with "Basic secret" instead of "Bearer secret".
/// Assert 401 (wrong auth scheme).
#[tokio::test]
async fn test_auth_middleware_invalid_format_returns_401() {
    let (server, _mock) = build_test_server_with_provider("mock", "secret").await;

    let body = json!({
        "model": "test-model",
        "messages": [
            {"role": "user", "content": "Hello"}
        ]
    });

    let response = server
        .post("/v1/chat/completions")
        .add_header(
            axum::http::header::AUTHORIZATION,
            axum::http::HeaderValue::from_static("Basic secret"),
        )
        .json(&body)
        .await;

    response.assert_status(axum::http::StatusCode::UNAUTHORIZED);
}

// ---------------------------------------------------------------------------
// Error format tests
// ---------------------------------------------------------------------------

/// Send a body with "messages" as a non-array value to /v1/chat/completions.
/// The response should be in OpenAI error format: `error.message` and
/// `error.type` fields.
#[tokio::test]
async fn test_openai_endpoint_error_format() {
    let (server, _mock) = build_test_server_with_provider("mock", "").await;

    // "messages" must be an array; passing a string forces a deserialization error.
    let body = json!({
        "model": "test-model",
        "messages": "not-an-array"
    });

    let response = server
        .post("/v1/chat/completions")
        .json(&body)
        .await;

    assert!(
        response.status_code() != axum::http::StatusCode::OK,
        "invalid request should not return 200"
    );

    let json: serde_json::Value = response.json();
    assert!(
        json["error"]["message"].is_string(),
        "OpenAI error response should have error.message, got: {json}"
    );
    assert!(
        json["error"]["type"].is_string(),
        "OpenAI error response should have error.type, got: {json}"
    );
}

/// Send a body missing the required `max_tokens` field to /v1/messages.
/// The response should be in Anthropic error format: `type: "error"`,
/// `error.type`, and `error.message` fields.
#[tokio::test]
async fn test_anthropic_endpoint_error_format() {
    let (server, _mock) = build_test_server_with_provider("mock", "").await;

    // AnthropicMessagesRequest.max_tokens is required; omitting it triggers
    // a deserialization error on the conversion path.
    let body = json!({
        "model": "test-model",
        "messages": [
            {"role": "user", "content": "Hello"}
        ]
        // max_tokens intentionally omitted
    });

    let response = server
        .post("/v1/messages")
        .json(&body)
        .await;

    assert!(
        response.status_code() != axum::http::StatusCode::OK,
        "missing max_tokens should not return 200"
    );

    let json: serde_json::Value = response.json();
    assert_eq!(
        json["type"], "error",
        "Anthropic error response should have type: \"error\", got: {json}"
    );
    assert!(
        json["error"]["type"].is_string(),
        "Anthropic error response should have error.type, got: {json}"
    );
    assert!(
        json["error"]["message"].is_string(),
        "Anthropic error response should have error.message, got: {json}"
    );
}

/// Set api_key to "test123". Send to /v1/messages without an Authorization
/// header. The auth middleware error should be in Anthropic format (has
/// `type: "error"`).
#[tokio::test]
async fn test_anthropic_auth_error_format() {
    let (server, _mock) = build_test_server_with_provider("mock", "test123").await;

    let body = json!({
        "model": "test-model",
        "max_tokens": 1024,
        "messages": [
            {"role": "user", "content": "Hello"}
        ]
    });

    let response = server
        .post("/v1/messages")
        .json(&body)
        .await;

    response.assert_status(axum::http::StatusCode::UNAUTHORIZED);

    let json: serde_json::Value = response.json();
    assert_eq!(
        json["type"], "error",
        "Anthropic auth error should have type: \"error\", got: {json}"
    );
    assert!(
        json["error"]["type"].is_string(),
        "Anthropic auth error should have error.type, got: {json}"
    );
}

/// Set api_key to "secret". GET /healthz without any Authorization header.
/// Assert 200 (health endpoint is outside the auth middleware).
#[tokio::test]
async fn test_healthz_no_auth_required() {
    let (server, _mock) = build_test_server_with_provider("mock", "secret").await;

    let response = server.get("/healthz").await;

    response.assert_status_ok();
}
