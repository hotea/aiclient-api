use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::Value;

use crate::convert::openai_types::OpenAIChatRequest;
use crate::convert::{from_openai, to_openai};
use crate::providers::router::resolve_provider;
use crate::providers::{OutputFormat, ProviderResponse};
use crate::server::state::AppState;
use crate::util::error::AppError;
use crate::util::stream::into_sse_response;

pub async fn chat_completions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    match chat_completions_inner(state, headers, body).await {
        Ok(resp) => resp,
        Err(e) => {
            let (status, message) = e.status_and_message();
            AppError::openai_error(status, &message)
        }
    }
}

async fn chat_completions_inner(
    state: AppState,
    headers: HeaderMap,
    body: Value,
) -> Result<Response, AppError> {
    let model = body
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("gpt-4")
        .to_string();

    let stream = body
        .get("stream")
        .and_then(|s| s.as_bool())
        .unwrap_or(false);

    let header_provider = headers
        .get("x-provider")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let (provider, resolved_model) =
        resolve_provider(&state, &model, header_provider.as_deref()).await?;

    // Check passthrough support
    if provider.supports_passthrough(OutputFormat::OpenAI) {
        let response = provider
            .passthrough(&resolved_model, body, OutputFormat::OpenAI, stream)
            .await?;
        match response {
            ProviderResponse::Stream(s) => {
                let sse = into_sse_response(s, OutputFormat::OpenAI, resolved_model.clone());
                return Ok(sse.into_response());
            }
            ProviderResponse::Complete(val) => {
                return Ok(Json(val).into_response());
            }
        }
    }

    // Parse request for conversion path
    let req: OpenAIChatRequest = serde_json::from_value(body)
        .map_err(|e| AppError::BadRequest(format!("Invalid request: {}", e)))?;

    let provider_req = from_openai(req)?;

    let response = provider.chat(provider_req).await?;

    match response {
        ProviderResponse::Stream(s) => {
            let sse = into_sse_response(s, OutputFormat::OpenAI, resolved_model.clone());
            Ok(sse.into_response())
        }
        ProviderResponse::Complete(val) => {
            let openai_resp = to_openai(&val, &resolved_model);
            Ok(Json(openai_resp).into_response())
        }
    }
}

pub async fn list_models(State(state): State<AppState>) -> Response {
    match list_models_inner(state).await {
        Ok(resp) => Json(resp).into_response(),
        Err(e) => {
            let (status, message) = e.status_and_message();
            AppError::openai_error(status, &message)
        }
    }
}

async fn list_models_inner(state: AppState) -> Result<Value, AppError> {
    let providers = state.providers.read().await;
    let mut models = Vec::new();
    for provider in providers.values() {
        if let Ok(provider_models) = provider.list_models().await {
            for m in provider_models {
                models.push(serde_json::json!({
                    "id": m.id,
                    "object": "model",
                    "created": 0,
                    "owned_by": m.provider,
                }));
            }
        }
    }
    Ok(serde_json::json!({
        "object": "list",
        "data": models,
    }))
}
