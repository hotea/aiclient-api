use chrono::Utc;
use uuid::Uuid;

/// Convert a provider response to OpenAI format.
/// If the response already looks like OpenAI format (has "choices"), return as-is.
/// Otherwise, wrap in OpenAI response structure.
pub fn to_openai(resp: &serde_json::Value, model: &str) -> serde_json::Value {
    // If resp already looks like OpenAI format (has "choices"), return as-is
    if resp.get("choices").is_some() {
        return resp.clone();
    }

    // Try to extract content from Anthropic-style response
    let (content_text, tool_calls, finish_reason, usage) = if let Some(content_arr) =
        resp.get("content").and_then(|c| c.as_array())
    {
        let text = content_arr
            .iter()
            .filter_map(|block| {
                if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                    block
                        .get("text")
                        .and_then(|t| t.as_str())
                        .map(|s| s.to_string())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("");
        let tool_calls = content_arr
            .iter()
            .filter(|block| block.get("type").and_then(|t| t.as_str()) == Some("tool_use"))
            .filter_map(|block| {
                let id = block.get("id").and_then(|v| v.as_str())?.to_string();
                let name = block.get("name").and_then(|v| v.as_str())?.to_string();
                let input = block.get("input").cloned().unwrap_or_else(|| serde_json::json!({}));

                Some(serde_json::json!({
                    "id": id,
                    "type": "function",
                    "function": {
                        "name": name,
                        "arguments": serde_json::to_string(&input).unwrap_or_else(|_| "{}".to_string()),
                    }
                }))
            })
            .collect::<Vec<_>>();
        let finish = resp
            .get("stop_reason")
            .and_then(|s| s.as_str())
            .map(|s| match s {
                "end_turn" => "stop",
                "max_tokens" => "length",
                "tool_use" => "tool_calls",
                other => other,
            })
            .unwrap_or("stop")
            .to_string();
        let usage_val = resp.get("usage").cloned();
        (text, tool_calls, finish, usage_val)
    } else {
        // Generic extraction
        let text = resp
            .get("content")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();
        ("".to_string() + &text, Vec::new(), "stop".to_string(), None)
    };

    let id = format!(
        "chatcmpl-{}",
        &Uuid::new_v4().to_string().replace('-', "")[..24]
    );
    let created = Utc::now().timestamp();

    let mut openai_usage = serde_json::Value::Null;
    if let Some(u) = usage {
        let input = u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let output = u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        openai_usage = serde_json::json!({
            "prompt_tokens": input,
            "completion_tokens": output,
            "total_tokens": input + output,
        });
    }

    let mut result = serde_json::json!({
        "id": id,
        "object": "chat.completion",
        "created": created,
        "model": model,
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": content_text,
                "tool_calls": if tool_calls.is_empty() { serde_json::Value::Null } else { serde_json::Value::Array(tool_calls) },
            },
            "finish_reason": finish_reason,
        }],
    });

    if let Some(conversation_id) = resp.get("conversation_id").cloned() {
        result["conversation_id"] = conversation_id;
    }
    if let Some(utterance_id) = resp.get("utterance_id").cloned() {
        result["utterance_id"] = utterance_id;
    }

    if !openai_usage.is_null() {
        result["usage"] = openai_usage;
    }

    result
}
