use aiclient_api::convert::openai_types::*;
use aiclient_api::convert::anthropic_types::*;

#[test]
fn test_openai_request_deserialize() {
    let json =
        r#"{"model":"gpt-4","messages":[{"role":"user","content":"hello"}],"stream":false}"#;
    let req: OpenAIChatRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.model, "gpt-4");
    assert_eq!(req.messages.len(), 1);
    assert_eq!(req.messages[0].role, "user");
}

#[test]
fn test_anthropic_request_deserialize() {
    let json = r#"{"model":"claude-3","messages":[{"role":"user","content":"hello"}],"max_tokens":1024}"#;
    let req: AnthropicMessagesRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.model, "claude-3");
    assert_eq!(req.max_tokens, 1024);
}

#[test]
fn test_openai_response_serialize() {
    let resp = OpenAIChatResponse {
        id: "chatcmpl-123".into(),
        object: "chat.completion".into(),
        created: 1234567890,
        model: "gpt-4".into(),
        choices: vec![OpenAIChoice {
            index: 0,
            message: Some(OpenAIMessage {
                role: "assistant".into(),
                content: Some(serde_json::Value::String("Hello!".into())),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            }),
            delta: None,
            finish_reason: Some("stop".into()),
        }],
        usage: Some(OpenAIUsage {
            prompt_tokens: 5,
            completion_tokens: 1,
            total_tokens: 6,
        }),
    };
    let json = serde_json::to_value(&resp).unwrap();
    assert_eq!(json["model"], "gpt-4");
    assert_eq!(json["choices"][0]["message"]["content"], "Hello!");
}

#[test]
fn test_from_openai_extracts_system() {
    use aiclient_api::convert::from_openai;

    let req = OpenAIChatRequest {
        model: "gpt-4".into(),
        messages: vec![
            OpenAIMessage {
                role: "system".into(),
                content: Some(serde_json::Value::String("You are a helpful assistant.".into())),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
            OpenAIMessage {
                role: "user".into(),
                content: Some(serde_json::Value::String("Hello!".into())),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
        ],
        stream: Some(false),
        temperature: None,
        max_tokens: Some(100),
        tools: None,
        tool_choice: None,
        extra: None,
    };
    let pr = from_openai(req).unwrap();
    assert!(pr.system.is_some());
    assert_eq!(pr.system.as_deref(), Some("You are a helpful assistant."));
    // User messages should not include the system message
    assert_eq!(pr.messages.len(), 1);
    assert_eq!(pr.messages[0]["role"], "user");
}

#[test]
fn test_from_anthropic_uses_top_level_system() {
    use aiclient_api::convert::from_anthropic;

    let req = AnthropicMessagesRequest {
        model: "claude-3".into(),
        messages: vec![AnthropicMessage {
            role: "user".into(),
            content: serde_json::Value::String("Hello!".into()),
        }],
        system: Some(serde_json::Value::String("You are helpful".into())),
        max_tokens: 1024,
        stream: None,
        temperature: None,
        tools: None,
        tool_choice: None,
        thinking: None,
        extra: None,
    };
    let pr = from_anthropic(req).unwrap();
    assert_eq!(pr.system.as_deref(), Some("You are helpful"));
}
