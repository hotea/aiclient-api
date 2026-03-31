use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::Value;

use crate::convert::anthropic_types::AnthropicMessagesRequest;
use crate::convert::{from_anthropic, to_anthropic, to_openai};
use crate::providers::router::resolve_provider;
use crate::providers::{OutputFormat, ProviderResponse};
use crate::server::state::AppState;
use crate::util::error::AppError;
use crate::util::stream::into_sse_response;

pub async fn messages(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    match messages_inner(state, headers, body).await {
        Ok(resp) => resp,
        Err(e) => {
            let (status, message) = e.status_and_message();
            AppError::anthropic_error(status, &message)
        }
    }
}

async fn messages_inner(
    state: AppState,
    headers: HeaderMap,
    body: Value,
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

    // Determine output format: header > config default > endpoint default (Anthropic)
    let output_format = if let Some(format_header) = headers.get("x-output-format") {
        match format_header.to_str().ok() {
            Some("openai") => OutputFormat::OpenAI,
            Some("anthropic") => OutputFormat::Anthropic,
            _ => OutputFormat::Anthropic,
        }
    } else {
        // Use config default
        let config = state.config.load();
        match config.default_format {
            crate::config::types::Format::OpenAI => OutputFormat::OpenAI,
            crate::config::types::Format::Anthropic => OutputFormat::Anthropic,
        }
    };

    let (provider, resolved_model) =
        resolve_provider(&state, &model, header_provider.as_deref()).await?;

    // Check passthrough support
    if provider.supports_passthrough(output_format) {
        let response = provider
            .passthrough(&resolved_model, body, output_format, stream)
            .await?;
        match response {
            ProviderResponse::Stream(s) => {
                let sse = into_sse_response(s, output_format, resolved_model.clone());
                return Ok(sse.into_response());
            }
            ProviderResponse::Complete(val) => {
                // Record usage statistics from passthrough response
                if let Some(usage) = val.get("usage") {
                    let input_tokens = usage.get("input_tokens")
                        .or_else(|| usage.get("prompt_tokens"))
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    let output_tokens = usage.get("output_tokens")
                        .or_else(|| usage.get("completion_tokens"))
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    
                    state.usage_tracker.record(
                        provider.name(),
                        &resolved_model,
                        input_tokens,
                        output_tokens,
                    ).await;
                }
                
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
            let sse = into_sse_response(s, output_format, resolved_model.clone());
            Ok(sse.into_response())
        }
        ProviderResponse::Complete(val) => {
            // Convert to the requested output format
            let final_response = match output_format {
                OutputFormat::Anthropic => to_anthropic(&val, &resolved_model),
                OutputFormat::OpenAI => to_openai(&val, &resolved_model),
            };
            
            // Record usage statistics
            if let Some(usage) = val.get("usage") {
                let input_tokens = usage.get("input_tokens")
                    .or_else(|| usage.get("prompt_tokens"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let output_tokens = usage.get("output_tokens")
                    .or_else(|| usage.get("completion_tokens"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                
                state.usage_tracker.record(
                    provider.name(),
                    &resolved_model,
                    input_tokens,
                    output_tokens,
                ).await;
            }
            
            Ok(Json(final_response).into_response())
        }
    }
}
