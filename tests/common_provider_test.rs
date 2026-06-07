use axum::extract::State;
use axum::http::HeaderMap;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::Mutex;

use aiclient_api::config::types::Config;
use aiclient_api::providers::common::{CommonProvider, CommonProviderConfig};
use aiclient_api::providers::Provider;
use aiclient_api::server::state::AppState;

#[derive(Clone, Default)]
struct UpstreamState {
    requests: Arc<Mutex<Vec<Value>>>,
    auth_headers: Arc<Mutex<Vec<String>>>,
}

async fn mock_chat(
    State(state): State<UpstreamState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Json<Value> {
    let auth = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();

    state.requests.lock().await.push(body.clone());
    state.auth_headers.lock().await.push(auth);

    Json(json!({
        "id": "chatcmpl-common-test",
        "object": "chat.completion",
        "created": 0,
        "model": body["model"],
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": format!("echo: {}", body["messages"][0]["content"].as_str().unwrap_or(""))
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 3,
            "completion_tokens": 4,
            "total_tokens": 7
        }
    }))
}

async fn mock_models() -> Json<Value> {
    Json(json!({
        "object": "list",
        "data": [
            { "id": "remote-model", "object": "model" }
        ]
    }))
}

async fn start_mock_upstream() -> (String, UpstreamState) {
    let state = UpstreamState::default();
    let app = Router::new()
        .route("/v1/chat/completions", post(mock_chat))
        .route("/v1/models", get(mock_models))
        .with_state(state.clone());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    (format!("http://{}", addr), state)
}

fn common_config(name: &str, base_url: String, models: Vec<String>) -> CommonProviderConfig {
    CommonProviderConfig {
        name: name.to_string(),
        base_url,
        api_key: "test-key".to_string(),
        api_keys: Vec::new(),
        api_key_env: String::new(),
        api_key_envs: Vec::new(),
        auth_scheme: "Bearer".to_string(),
        chat_completions_path: "/chat/completions".to_string(),
        models_path: "/models".to_string(),
        models,
        vendor: "test-vendor".to_string(),
        supports_streaming: true,
        supports_tools: true,
        supports_vision: false,
        supports_thinking: false,
        headers: HashMap::new(),
    }
}

async fn build_gateway(
    provider_name: &str,
    base_url: String,
    models: Vec<String>,
) -> (axum_test::TestServer, UpstreamState) {
    let (upstream_base, upstream_state) = start_mock_upstream().await;
    let provider_base = if base_url.is_empty() {
        format!("{}/v1", upstream_base)
    } else {
        base_url
    };

    let config = Config {
        default_provider: provider_name.to_string(),
        ..Config::default()
    };
    let state = AppState::new(config);
    let provider =
        CommonProvider::new(common_config(provider_name, provider_base, models)).unwrap();
    {
        let mut providers = state.providers.write().await;
        providers.insert(provider_name.to_string(), provider as Arc<dyn Provider>);
    }

    let app = aiclient_api::server::build_router(state);
    (axum_test::TestServer::new(app), upstream_state)
}

#[tokio::test]
async fn test_common_provider_forwards_openai_chat_completion() {
    let (server, upstream) = build_gateway(
        "opencode",
        String::new(),
        vec!["nemotron-3-ultra-free".to_string()],
    )
    .await;

    let response = server
        .post("/v1/chat/completions")
        .json(&json!({
            "model": "opencode/nemotron-3-ultra-free",
            "messages": [{"role": "user", "content": "如何深度理解DIKW模型"}],
            "temperature": 0.7,
            "max_tokens": 20480
        }))
        .await;

    response.assert_status_ok();
    let body: Value = response.json();
    assert_eq!(body["model"], "nemotron-3-ultra-free");
    assert_eq!(
        body["choices"][0]["message"]["content"],
        "echo: 如何深度理解DIKW模型"
    );

    let upstream_requests = upstream.requests.lock().await;
    assert_eq!(upstream_requests[0]["model"], "nemotron-3-ultra-free");
    assert_eq!(upstream_requests[0]["temperature"], 0.7);
    assert_eq!(upstream_requests[0]["max_tokens"], 20480);

    let auth_headers = upstream.auth_headers.lock().await;
    assert_eq!(auth_headers[0], "Bearer test-key");
}

#[tokio::test]
async fn test_common_provider_lists_configured_models_with_provider_prefix() {
    let (server, _upstream) = build_gateway(
        "nvidia",
        String::new(),
        vec!["nemotron-3-ultra-free".to_string()],
    )
    .await;

    let response = server.get("/v1/models").await;
    response.assert_status_ok();
    let body: Value = response.json();
    assert_eq!(body["data"][0]["id"], "nvidia/nemotron-3-ultra-free");
    assert_eq!(body["data"][0]["owned_by"], "nvidia");
}

#[tokio::test]
async fn test_common_provider_accepts_full_chat_completions_url() {
    let (upstream_base, upstream_state) = start_mock_upstream().await;
    let full_chat_url = format!("{}/v1/chat/completions", upstream_base);

    let config = Config {
        default_provider: "fullurl".to_string(),
        ..Config::default()
    };
    let state = AppState::new(config);
    let provider = CommonProvider::new(common_config(
        "fullurl",
        full_chat_url,
        vec!["remote-model".to_string()],
    ))
    .unwrap();
    {
        let mut providers = state.providers.write().await;
        providers.insert("fullurl".to_string(), provider as Arc<dyn Provider>);
    }
    let app = aiclient_api::server::build_router(state);
    let server = axum_test::TestServer::new(app);

    let response = server
        .post("/v1/chat/completions")
        .json(&json!({
            "model": "fullurl/remote-model",
            "messages": [{"role": "user", "content": "hello"}]
        }))
        .await;

    response.assert_status_ok();
    assert_eq!(
        upstream_state.requests.lock().await[0]["model"],
        "remote-model"
    );
}

#[tokio::test]
async fn test_common_provider_rotates_multiple_api_keys() {
    let (upstream_base, upstream_state) = start_mock_upstream().await;
    let config = Config {
        default_provider: "rotate".to_string(),
        ..Config::default()
    };
    let state = AppState::new(config);
    let mut provider_config = common_config(
        "rotate",
        format!("{}/v1", upstream_base),
        vec!["rotation-model".to_string()],
    );
    provider_config.api_key = String::new();
    provider_config.api_keys = vec!["key-one".to_string(), "key-two".to_string()];
    let provider = CommonProvider::new(provider_config).unwrap();
    {
        let mut providers = state.providers.write().await;
        providers.insert("rotate".to_string(), provider as Arc<dyn Provider>);
    }
    let app = aiclient_api::server::build_router(state);
    let server = axum_test::TestServer::new(app);

    for _ in 0..3 {
        let response = server
            .post("/v1/chat/completions")
            .json(&json!({
                "model": "rotate/rotation-model",
                "messages": [{"role": "user", "content": "hello"}]
            }))
            .await;
        response.assert_status_ok();
    }

    let auth_headers = upstream_state.auth_headers.lock().await;
    assert_eq!(
        auth_headers.as_slice(),
        ["Bearer key-one", "Bearer key-two", "Bearer key-one",]
    );
}
