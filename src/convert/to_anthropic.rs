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
    let (content_text, stop_reason, usage) = if let Some(choices) = resp.get("choices").and_then(|c| c.as_array()) {
        let text = choices
            .first()
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();
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
        (text, finish, usage_val)
    } else {
        ("".to_string(), "end_turn".to_string(), None)
    };

    let id = format!("msg_{}", Uuid::new_v4().to_string().replace('-', "")[..24].to_string());
    let _created = Utc::now().timestamp();

    let mut anthropic_usage = serde_json::Value::Null;
    if let Some(u) = usage {
        let input = u.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let output = u.get("completion_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        anthropic_usage = serde_json::json!({
            "input_tokens": input,
            "output_tokens": output,
        });
    }

    let mut result = serde_json::json!({
        "id": id,
        "type": "message",
        "role": "assistant",
        "content": [{
            "type": "text",
            "text": content_text,
        }],
        "model": model,
        "stop_reason": stop_reason,
    });

    if !anthropic_usage.is_null() {
        result["usage"] = anthropic_usage;
    }

    result
}
