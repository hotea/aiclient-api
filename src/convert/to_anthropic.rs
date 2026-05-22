use chrono::Utc;
use uuid::Uuid;

/// Convert a provider response to Anthropic format.
/// If the response already looks like Anthropic format (has "content" array with "type"), return as-is.
/// Otherwise, wrap in Anthropic response structure.
pub fn to_anthropic(resp: &serde_json::Value, model: &str) -> serde_json::Value {
    // If resp already looks like Anthropic format (has "content" array with blocks that have "type"), return as-is
    if let Some(content) = resp.get("content") {
        if let Some(arr) = content.as_array() {
            if arr.iter().any(|block| block.get("type").is_some()) {
                return resp.clone();
            }
        }
    }

    // Try to extract content from OpenAI-style response
    let (content_blocks, stop_reason, usage) =
        if let Some(choices) = resp.get("choices").and_then(|c| c.as_array()) {
            let message = choices.first().and_then(|c| c.get("message"));
            let mut blocks = Vec::new();

            if let Some(text) = message
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_str())
                .filter(|text| !text.is_empty())
            {
                blocks.push(serde_json::json!({
                    "type": "text",
                    "text": text,
                }));
            }

            if let Some(tool_calls) = message
                .and_then(|m| m.get("tool_calls"))
                .and_then(|t| t.as_array())
            {
                for tool_call in tool_calls {
                    let Some(id) = tool_call.get("id").and_then(|v| v.as_str()) else {
                        continue;
                    };
                    let function = tool_call.get("function").unwrap_or(tool_call);
                    let Some(name) = function.get("name").and_then(|v| v.as_str()) else {
                        continue;
                    };
                    let input = function
                        .get("arguments")
                        .map(parse_json_or_string)
                        .unwrap_or_else(|| serde_json::json!({}));

                    blocks.push(serde_json::json!({
                        "type": "tool_use",
                        "id": id,
                        "name": name,
                        "input": input,
                    }));
                }
            }

            let finish = choices
                .first()
                .and_then(|c| c.get("finish_reason"))
                .and_then(|f| f.as_str())
                .map(|s| match s {
                    "stop" => "end_turn",
                    "length" => "max_tokens",
                    "tool_calls" => "tool_use",
                    other => other,
                })
                .unwrap_or("end_turn")
                .to_string();
            let usage_val = resp.get("usage").cloned();
            (blocks, finish, usage_val)
        } else {
            (Vec::new(), "end_turn".to_string(), None)
        };

    let id = format!("msg_{}", &Uuid::new_v4().to_string().replace('-', "")[..24]);
    let _created = Utc::now().timestamp();

    let mut anthropic_usage = serde_json::Value::Null;
    if let Some(u) = usage {
        let input = u.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let output = u
            .get("completion_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        anthropic_usage = serde_json::json!({
            "input_tokens": input,
            "output_tokens": output,
        });
    }

    let mut result = serde_json::json!({
        "id": id,
        "type": "message",
        "role": "assistant",
        "content": if content_blocks.is_empty() {
            vec![serde_json::json!({
                "type": "text",
                "text": "",
            })]
        } else {
            content_blocks
        },
        "model": model,
        "stop_reason": stop_reason,
    });

    if let Some(conversation_id) = resp.get("conversation_id").cloned() {
        result["conversation_id"] = conversation_id;
    }
    if let Some(utterance_id) = resp.get("utterance_id").cloned() {
        result["utterance_id"] = utterance_id;
    }

    if !anthropic_usage.is_null() {
        result["usage"] = anthropic_usage;
    }

    result
}

fn parse_json_or_string(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::String(text) => {
            serde_json::from_str(text).unwrap_or_else(|_| serde_json::Value::String(text.clone()))
        }
        other => other.clone(),
    }
}
