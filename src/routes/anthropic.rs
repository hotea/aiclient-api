use axum::body::Body;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures::stream;
use futures::{Stream, StreamExt};
use serde_json::Value;
use std::collections::HashMap;
use tracing::{debug, info};

use crate::config::types::ProviderRoutingMode;
use crate::convert::anthropic_types::AnthropicMessagesRequest;
use crate::convert::stream::chunk_to_anthropic;
use crate::convert::{from_anthropic, to_anthropic, to_openai};
use crate::providers::kiro::anthropic_stream_from_kiro;
use crate::providers::router::resolve_provider;
use crate::providers::{OutputFormat, ProviderResponse};
use crate::server::state::AppState;
use crate::util::error::AppError;
use crate::util::stream::into_sse_response;

fn ensure_conversation_id(extra: &mut Value, messages: &[Value]) -> String {
    let from_extra = extra
        .get("conversation_id")
        .and_then(|v| v.as_str())
        .or_else(|| extra.get("conversationId").and_then(|v| v.as_str()));

    let from_messages = messages.iter().rev().find_map(|message| {
        message
            .get("conversation_id")
            .and_then(|v| v.as_str())
            .or_else(|| message.get("conversationId").and_then(|v| v.as_str()))
    });

    let conversation_id = from_extra
        .or(from_messages)
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    if !extra.is_object() {
        *extra = serde_json::json!({});
    }
    extra["conversation_id"] = Value::String(conversation_id.clone());
    conversation_id
}

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
        .map(str::to_string)
        .unwrap_or_else(|| default_request_model(&state, "claude-3-5-sonnet"));

    let stream = body
        .get("stream")
        .and_then(|s| s.as_bool())
        .unwrap_or(false);

    if let Some(thinking) = body.get("thinking") {
        info!(thinking = %thinking, stream, "anthropic thinking requested");
    }

    let header_provider = headers
        .get("x-provider")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    // Determine output format: explicit header overrides the Anthropic endpoint default.
    let output_format = if let Some(format_header) = headers.get("x-output-format") {
        match format_header.to_str().ok() {
            Some("openai") => OutputFormat::OpenAI,
            Some("anthropic") => OutputFormat::Anthropic,
            _ => OutputFormat::Anthropic,
        }
    } else {
        OutputFormat::Anthropic
    };

    let (provider, resolved_model) =
        resolve_provider(&state, &model, header_provider.as_deref()).await?;

    // Check passthrough support
    if provider.supports_passthrough(output_format) {
        let mut body = body;
        body["model"] = serde_json::Value::String(resolved_model.clone());

        let response = provider
            .passthrough(&resolved_model, body, output_format, stream)
            .await?;
        match response {
            ProviderResponse::Stream(s) => {
                if output_format == OutputFormat::Anthropic {
                    let body_stream = anthropic_passthrough_stream(s, resolved_model.clone());

                    return Ok((
                        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                        Body::from_stream(body_stream),
                    )
                        .into_response());
                }

                let sse = into_sse_response(s, output_format, resolved_model.clone());
                return Ok(sse.into_response());
            }
            ProviderResponse::Complete(val) => {
                // Record usage statistics from passthrough response
                if let Some(usage) = val.get("usage") {
                    let input_tokens = usage
                        .get("input_tokens")
                        .or_else(|| usage.get("prompt_tokens"))
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    let output_tokens = usage
                        .get("output_tokens")
                        .or_else(|| usage.get("completion_tokens"))
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);

                    state
                        .usage_tracker
                        .record(
                            provider.name(),
                            &resolved_model,
                            input_tokens,
                            output_tokens,
                        )
                        .await;
                }

                return Ok(Json(val).into_response());
            }
        }
    }

    // Parse request for conversion path
    let req: AnthropicMessagesRequest = serde_json::from_value(body)
        .map_err(|e| AppError::BadRequest(format!("Invalid request: {}", e)))?;

    let mut provider_req = from_anthropic(req)?;
    provider_req.model = resolved_model.clone();
    let response_conversation_id = Some(ensure_conversation_id(
        &mut provider_req.extra,
        &provider_req.messages,
    ));

    let response = provider.chat(provider_req).await?;

    match response {
        ProviderResponse::Stream(s) => {
            if output_format == OutputFormat::Anthropic
                && provider.prefers_native_anthropic_streaming()
            {
                let body_stream =
                    anthropic_stream_from_kiro(s, resolved_model.clone(), response_conversation_id);
                return Ok((
                    [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                    Body::from_stream(body_stream),
                )
                    .into_response());
            }
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
                let input_tokens = usage
                    .get("input_tokens")
                    .or_else(|| usage.get("prompt_tokens"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let output_tokens = usage
                    .get("output_tokens")
                    .or_else(|| usage.get("completion_tokens"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);

                state
                    .usage_tracker
                    .record(
                        provider.name(),
                        &resolved_model,
                        input_tokens,
                        output_tokens,
                    )
                    .await;
            }

            Ok(Json(final_response).into_response())
        }
    }
}

fn find_sse_event_end(buffer: &[u8]) -> Option<usize> {
    for i in 0..buffer.len().saturating_sub(1) {
        if buffer[i] == b'\n' && buffer[i + 1] == b'\n' {
            return Some(i + 2);
        }
    }

    for i in 0..buffer.len().saturating_sub(3) {
        if buffer[i] == b'\r'
            && buffer[i + 1] == b'\n'
            && buffer[i + 2] == b'\r'
            && buffer[i + 3] == b'\n'
        {
            return Some(i + 4);
        }
    }

    None
}

fn default_request_model(state: &AppState, fallback: &str) -> String {
    let config = state.config.load();
    if config.routing.mode == ProviderRoutingMode::Auto {
        config
            .routing
            .models
            .first()
            .cloned()
            .unwrap_or_else(|| "auto".to_string())
    } else {
        fallback.to_string()
    }
}

fn anthropic_passthrough_stream(
    stream: std::pin::Pin<Box<dyn Stream<Item = anyhow::Result<bytes::Bytes>> + Send>>,
    model: String,
) -> impl Stream<Item = anyhow::Result<axum::body::Bytes>> {
    stream::unfold(
        (stream, Vec::<u8>::new(), model, ThinkingLogState::default()),
        |(mut stream, mut buffer, model, mut thinking_state)| async move {
            loop {
                while let Some(end) = find_sse_event_end(&buffer) {
                    let chunk = buffer.drain(..end).collect::<Vec<_>>();
                    if chunk.windows(7).any(|w| w == b"event: ") {
                        log_anthropic_thinking_chunk(&chunk, &mut thinking_state);
                        return Some((
                            Ok(axum::body::Bytes::from(chunk)),
                            (stream, buffer, model, thinking_state),
                        ));
                    }

                    let converted = chunk_to_anthropic(&chunk, &model);
                    if !converted.is_empty() {
                        log_anthropic_thinking_chunk(&converted, &mut thinking_state);
                        return Some((
                            Ok(axum::body::Bytes::from(converted)),
                            (stream, buffer, model, thinking_state),
                        ));
                    }
                }

                match stream.as_mut().next().await {
                    Some(Ok(bytes)) => buffer.extend_from_slice(&bytes),
                    Some(Err(e)) => return Some((Err(e), (stream, buffer, model, thinking_state))),
                    None => {
                        if buffer.is_empty() {
                            return None;
                        }

                        let converted = chunk_to_anthropic(&buffer, &model);
                        if converted.is_empty() {
                            return None;
                        }

                        log_anthropic_thinking_chunk(&converted, &mut thinking_state);
                        return Some((
                            Ok(axum::body::Bytes::from(converted)),
                            (stream, Vec::new(), model, thinking_state),
                        ));
                    }
                }
            }
        },
    )
}

#[derive(Default)]
struct ThinkingLogState {
    last_thinking_by_index: HashMap<u32, String>,
}

fn log_anthropic_thinking_chunk(chunk: &[u8], state: &mut ThinkingLogState) {
    let text = match std::str::from_utf8(chunk) {
        Ok(text) => text,
        Err(_) => return,
    };

    let mut current_event: Option<&str> = None;
    let mut current_data: Vec<&str> = Vec::new();

    for line in text.lines() {
        if line.is_empty() {
            log_anthropic_thinking_event(current_event, &current_data, state);
            current_event = None;
            current_data.clear();
            continue;
        }

        if let Some(event_name) = line.strip_prefix("event: ") {
            current_event = Some(event_name);
            continue;
        }

        if let Some(data) = line.strip_prefix("data: ") {
            current_data.push(data);
        }
    }

    if !current_data.is_empty() {
        log_anthropic_thinking_event(current_event, &current_data, state);
    }
}

fn log_anthropic_thinking_event(
    event_name: Option<&str>,
    data_lines: &[&str],
    state: &mut ThinkingLogState,
) {
    if data_lines.is_empty() {
        return;
    }

    let data = data_lines.join("\n");
    let value: serde_json::Value = match serde_json::from_str(&data) {
        Ok(value) => value,
        Err(_) => return,
    };

    let event_type = value
        .get("type")
        .and_then(|v| v.as_str())
        .or(event_name)
        .unwrap_or("unknown");

    match event_type {
        "content_block_start" => {
            let Some(content_block) = value.get("content_block") else {
                return;
            };
            let Some(block_type) = content_block.get("type").and_then(|v| v.as_str()) else {
                return;
            };
            if block_type == "thinking" {
                let index = value.get("index").and_then(|v| v.as_u64()).unwrap_or(0);
                info!(index, "anthropic thinking block started");
            }
        }
        "content_block_delta" => {
            let Some(delta) = value.get("delta") else {
                return;
            };
            let Some(delta_type) = delta.get("type").and_then(|v| v.as_str()) else {
                return;
            };

            let index = value.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as u32;

            match delta_type {
                "thinking_delta" => {
                    let text = delta.get("thinking").and_then(|v| v.as_str()).unwrap_or("");
                    let previous = state
                        .last_thinking_by_index
                        .get(&index)
                        .map(String::as_str)
                        .unwrap_or("");
                    let behavior = if !previous.is_empty() && text == previous {
                        "exact_duplicate"
                    } else if !previous.is_empty() && text.starts_with(previous) {
                        "cumulative_prefix"
                    } else {
                        "fresh_delta"
                    };

                    debug!(
                        index,
                        delta_len = text.len(),
                        previous_len = previous.len(),
                        behavior,
                        text = text,
                        "anthropic thinking delta"
                    );

                    if behavior != "fresh_delta" {
                        info!(
                            index,
                            delta_len = text.len(),
                            previous_len = previous.len(),
                            behavior,
                            "anthropic thinking appears repeated"
                        );
                    }

                    state.last_thinking_by_index.insert(index, text.to_string());
                }
                "signature_delta" => {
                    let signature_len = delta
                        .get("signature")
                        .and_then(|v| v.as_str())
                        .map(str::len)
                        .unwrap_or(0);
                    debug!(index, signature_len, "anthropic thinking signature delta");
                }
                _ => {}
            }
        }
        "content_block_stop" => {
            let index = value.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            if let Some(final_text) = state.last_thinking_by_index.remove(&index) {
                info!(
                    index,
                    final_len = final_text.len(),
                    "anthropic thinking block finished"
                );
            }
        }
        _ => {}
    }
}
