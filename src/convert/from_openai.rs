use anyhow::Result;

use crate::convert::openai_types::OpenAIChatRequest;
use crate::providers::ProviderRequest;

pub fn from_openai(req: OpenAIChatRequest) -> Result<ProviderRequest> {
    let model = req.model.clone();
    let stream = req.stream.unwrap_or(false);
    let temperature = req.temperature;
    let max_tokens = req.max_tokens;
    let tools = req.tools;
    let tool_choice = req.tool_choice;

    // Extract system message from messages list
    let mut system: Option<String> = None;
    let mut messages: Vec<serde_json::Value> = Vec::new();

    for msg in req.messages {
        if msg.role == "system" {
            // Extract system message content
            if let Some(content) = &msg.content {
                let text = match content {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                // Append to system (in case of multiple system messages)
                if let Some(existing) = system {
                    system = Some(format!("{}\n{}", existing, text));
                } else {
                    system = Some(text);
                }
            }
        } else {
            let mut msg_obj = serde_json::json!({
                "role": msg.role,
            });
            if let Some(content) = msg.content {
                msg_obj["content"] = content;
            }
            if let Some(name) = msg.name {
                msg_obj["name"] = serde_json::Value::String(name);
            }
            if let Some(tool_calls) = msg.tool_calls {
                msg_obj["tool_calls"] = serde_json::to_value(tool_calls)?;
            }
            if let Some(tool_call_id) = msg.tool_call_id {
                msg_obj["tool_call_id"] = serde_json::Value::String(tool_call_id);
            }
            messages.push(msg_obj);
        }
    }

    let extra = req
        .extra
        .map(|m| serde_json::Value::Object(m))
        .unwrap_or(serde_json::Value::Null);

    Ok(ProviderRequest {
        model,
        messages,
        system,
        temperature,
        max_tokens,
        stream,
        tools,
        tool_choice,
        extra,
    })
}
