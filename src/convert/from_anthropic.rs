use anyhow::Result;

use crate::convert::anthropic_types::AnthropicMessagesRequest;
use crate::providers::ProviderRequest;

pub fn from_anthropic(req: AnthropicMessagesRequest) -> Result<ProviderRequest> {
    let model = req.model.clone();
    let stream = req.stream.unwrap_or(false);
    let temperature = req.temperature;
    let max_tokens = Some(req.max_tokens);
    let tools = req.tools;
    let tool_choice = req.tool_choice;

    // Extract system from the top-level system field
    let system: Option<String> = req.system.as_ref().map(|s| match s {
        serde_json::Value::String(text) => text.clone(),
        other => other.to_string(),
    });

    // Convert messages to Vec<serde_json::Value>
    let messages: Vec<serde_json::Value> = req
        .messages
        .into_iter()
        .map(|msg| {
            serde_json::json!({
                "role": msg.role,
                "content": msg.content,
            })
        })
        .collect();

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
