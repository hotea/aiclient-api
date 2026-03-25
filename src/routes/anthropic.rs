use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::Value;

use crate::convert::anthropic_types::AnthropicMessagesRequest;
use crate::convert::{from_anthropic, to_anthropic};
use crate::providers::router::resolve_provider;
use crate::providers::{OutputFormat, ProviderResponse};
use crate::server::state::AppState;
use crate::util::error::AppError;
use crate::util::stream::into_sse_response;

pub async fn messages(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Response, AppError> {
    let model = body
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("claude-3-5-sonnet")
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
    if provider.supports_passthrough(OutputFormat::Anthropic) {
        let response = provider
            .passthrough(&resolved_model, body, OutputFormat::Anthropic, stream)
            .await?;
        match response {
            ProviderResponse::Stream(s) => {
                let sse = into_sse_response(s, OutputFormat::Anthropic, resolved_model.clone());
                return Ok(sse.into_response());
            }
            ProviderResponse::Complete(val) => {
                return Ok(Json(val).into_response());
            }
        }
    }

    // Parse request for conversion path
    let req: AnthropicMessagesRequest = serde_json::from_value(body)
        .map_err(|e| AppError::BadRequest(format!("Invalid request: {}", e)))?;

    let provider_req = from_anthropic(req)?;

    let response = provider.chat(provider_req).await?;

    match response {
        ProviderResponse::Stream(s) => {
            let sse = into_sse_response(s, OutputFormat::Anthropic, resolved_model.clone());
            Ok(sse.into_response())
        }
        ProviderResponse::Complete(val) => {
            let anthropic_resp = to_anthropic(&val, &resolved_model);
            Ok(Json(anthropic_resp).into_response())
        }
    }
}
