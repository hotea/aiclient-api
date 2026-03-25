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
    let (content_text, finish_reason, usage) = if let Some(content_arr) = resp.get("content").and_then(|c| c.as_array()) {
        let text = content_arr
            .iter()
            .filter_map(|block| {
                if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                    block.get("text").and_then(|t| t.as_str()).map(|s| s.to_string())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("");
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
        (text, finish, usage_val)
    } else {
        // Generic extraction
        let text = resp
            .get("content")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();
        ("".to_string() + &text, "stop".to_string(), None)
    };

    let id = format!("chatcmpl-{}", &Uuid::new_v4().to_string().replace('-', "")[..24]);
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
            },
            "finish_reason": finish_reason,
        }],
    });

    if !openai_usage.is_null() {
        result["usage"] = openai_usage;
    }

    result
}
