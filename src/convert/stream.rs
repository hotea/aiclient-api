//! SSE stream chunk conversion helpers.
//! Functions to convert SSE chunks between OpenAI and Anthropic formats.

/// Convert a raw SSE chunk to OpenAI format.
/// If the chunk already looks like OpenAI format (has "choices"), return as-is.
/// Handles `data: [DONE]` terminator.
pub fn chunk_to_openai(chunk: &[u8], _model: &str) -> Vec<u8> {
    let text = match std::str::from_utf8(chunk) {
        Ok(s) => s,
        Err(_) => return chunk.to_vec(),
    };

    let mut result = Vec::new();
    for line in text.lines() {
        if let Some(data) = line.strip_prefix("data: ") {
            if data.trim() == "[DONE]" {
                result.extend_from_slice(b"data: [DONE]\n\n");
                continue;
            }
            // Try to parse as JSON
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(data) {
                // If already OpenAI format (has "choices"), pass through
                if val.get("choices").is_some() {
                    result.extend_from_slice(line.as_bytes());
                    result.extend_from_slice(b"\n\n");
                } else {
                    // Try to convert from Anthropic streaming format
                    let converted = convert_anthropic_chunk_to_openai(&val, _model);
                    if let Some(c) = converted {
                        let serialized = serde_json::to_string(&c).unwrap_or_default();
                        result.extend_from_slice(b"data: ");
                        result.extend_from_slice(serialized.as_bytes());
                        result.extend_from_slice(b"\n\n");
                    } else {
                        // Pass through unknown format
                        result.extend_from_slice(line.as_bytes());
                        result.extend_from_slice(b"\n\n");
                    }
                }
            } else {
                // Pass through non-JSON data lines
                result.extend_from_slice(line.as_bytes());
                result.extend_from_slice(b"\n\n");
            }
        } else if !line.is_empty() {
            result.extend_from_slice(line.as_bytes());
            result.extend_from_slice(b"\n");
        }
    }
    result
}

/// Convert a raw SSE chunk to Anthropic format.
/// If the chunk already looks like Anthropic format (has event type fields), return as-is.
/// Handles `data: [DONE]` terminator.
pub fn chunk_to_anthropic(chunk: &[u8], _model: &str) -> Vec<u8> {
    let text = match std::str::from_utf8(chunk) {
        Ok(s) => s,
        Err(_) => return chunk.to_vec(),
    };

    let mut result = Vec::new();
    for line in text.lines() {
        if let Some(data) = line.strip_prefix("data: ") {
            if data.trim() == "[DONE]" {
                // OpenAI uses [DONE], Anthropic uses message_stop event
                // We'll emit the Anthropic message_stop event
                let stop_event = serde_json::json!({"type": "message_stop"});
                let serialized = serde_json::to_string(&stop_event).unwrap_or_default();
                result.extend_from_slice(b"event: message_stop\n");
                result.extend_from_slice(b"data: ");
                result.extend_from_slice(serialized.as_bytes());
                result.extend_from_slice(b"\n\n");
                continue;
            }
            // Try to parse as JSON
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(data) {
                // If already Anthropic format (has "type" field that's an event type), pass through
                if let Some(event_type) = val.get("type").and_then(|t| t.as_str()) {
                    if matches!(
                        event_type,
                        "message_start"
                            | "content_block_start"
                            | "content_block_delta"
                            | "content_block_stop"
                            | "message_delta"
                            | "message_stop"
                    ) {
                        result.extend_from_slice(b"data: ");
                        result.extend_from_slice(data.as_bytes());
                        result.extend_from_slice(b"\n\n");
                        continue;
                    }
                }
                // Try to convert from OpenAI streaming format
                let converted = convert_openai_chunk_to_anthropic(&val, _model);
                if let Some(c) = converted {
                    for event in c {
                        let serialized = serde_json::to_string(&event).unwrap_or_default();
                        result.extend_from_slice(b"data: ");
                        result.extend_from_slice(serialized.as_bytes());
                        result.extend_from_slice(b"\n\n");
                    }
                } else {
                    result.extend_from_slice(line.as_bytes());
                    result.extend_from_slice(b"\n\n");
                }
            } else {
                result.extend_from_slice(line.as_bytes());
                result.extend_from_slice(b"\n\n");
            }
        } else if !line.is_empty() {
            result.extend_from_slice(line.as_bytes());
            result.extend_from_slice(b"\n");
        }
    }
    result
}

fn convert_anthropic_chunk_to_openai(
    val: &serde_json::Value,
    model: &str,
) -> Option<serde_json::Value> {
    let event_type = val.get("type").and_then(|t| t.as_str())?;

    match event_type {
        "content_block_delta" => {
            let delta = val.get("delta")?;
            let delta_type = delta.get("type").and_then(|t| t.as_str())?;
            if delta_type == "text_delta" {
                let text = delta.get("text").and_then(|t| t.as_str()).unwrap_or("");
                let chunk = serde_json::json!({
                    "id": "chatcmpl-stream",
                    "object": "chat.completion.chunk",
                    "created": 0,
                    "model": model,
                    "choices": [{
                        "index": 0,
                        "delta": {
                            "content": text,
                        },
                        "finish_reason": null,
                    }]
                });
                Some(chunk)
            } else {
                None
            }
        }
        "message_delta" => {
            let delta = val.get("delta")?;
            let stop_reason = delta.get("stop_reason").and_then(|r| r.as_str());
            let finish_reason = stop_reason.map(|s| match s {
                "end_turn" => "stop",
                "max_tokens" => "length",
                "tool_use" => "tool_calls",
                other => other,
            });
            let chunk = serde_json::json!({
                "id": "chatcmpl-stream",
                "object": "chat.completion.chunk",
                "created": 0,
                "model": model,
                "choices": [{
                    "index": 0,
                    "delta": {},
                    "finish_reason": finish_reason,
                }]
            });
            Some(chunk)
        }
        _ => None,
    }
}

fn convert_openai_chunk_to_anthropic(
    val: &serde_json::Value,
    _model: &str,
) -> Option<Vec<serde_json::Value>> {
    let choices = val.get("choices").and_then(|c| c.as_array())?;
    let first_choice = choices.first()?;
    let delta = first_choice.get("delta")?;

    let mut events = Vec::new();

    if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
        if !content.is_empty() {
            let event = serde_json::json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {
                    "type": "text_delta",
                    "text": content,
                }
            });
            events.push(event);
        }
    }

    if let Some(finish_reason) = first_choice.get("finish_reason").and_then(|f| f.as_str()) {
        let stop_reason = match finish_reason {
            "stop" => "end_turn",
            "length" => "max_tokens",
            "tool_calls" => "tool_use",
            other => other,
        };
        let event = serde_json::json!({
            "type": "message_delta",
            "delta": {
                "type": "message_delta",
                "stop_reason": stop_reason,
            }
        });
        events.push(event);
    }

    if events.is_empty() {
        None
    } else {
        Some(events)
    }
}
