use aiclient_api::convert::openai_types::*;
use aiclient_api::convert::anthropic_types::*;
use aiclient_api::convert::{to_openai, to_anthropic, from_openai, from_anthropic};

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

#[test]
fn test_openai_request_round_trip() {
    let json = r#"{"model":"gpt-4","messages":[{"role":"user","content":"hello"}],"stream":false,"max_tokens":100}"#;
    let req: OpenAIChatRequest = serde_json::from_str(json).unwrap();
    let reserialized = serde_json::to_string(&req).unwrap();
    let round_tripped: OpenAIChatRequest = serde_json::from_str(&reserialized).unwrap();
    assert_eq!(req.model, round_tripped.model);
    assert_eq!(req.messages.len(), round_tripped.messages.len());
    assert_eq!(req.max_tokens, round_tripped.max_tokens);
}

#[test]
fn test_anthropic_request_round_trip() {
    let json = r#"{"model":"claude-3","messages":[{"role":"user","content":"hello"}],"max_tokens":1024,"stream":true}"#;
    let req: AnthropicMessagesRequest = serde_json::from_str(json).unwrap();
    let reserialized = serde_json::to_string(&req).unwrap();
    let round_tripped: AnthropicMessagesRequest = serde_json::from_str(&reserialized).unwrap();
    assert_eq!(req.model, round_tripped.model);
    assert_eq!(req.max_tokens, round_tripped.max_tokens);
    assert_eq!(req.stream, round_tripped.stream);
}

// --- to_openai tests ---

#[test]
fn test_to_openai_passthrough_if_already_openai() {
    let resp = serde_json::json!({
        "id": "chatcmpl-abc",
        "choices": [{"index": 0, "message": {"role": "assistant", "content": "hi"}, "finish_reason": "stop"}],
        "model": "gpt-4"
    });
    let result = to_openai(&resp, "gpt-4");
    // Should be returned as-is: same "choices" key present
    assert!(result.get("choices").is_some());
    assert_eq!(result["id"], "chatcmpl-abc");
}

#[test]
fn test_to_openai_converts_anthropic_response() {
    let resp = serde_json::json!({
        "content": [{"type": "text", "text": "Hello"}],
        "stop_reason": "end_turn",
        "usage": {"input_tokens": 10, "output_tokens": 5}
    });
    let result = to_openai(&resp, "claude-3");
    assert!(result.get("choices").is_some());
    let choices = result["choices"].as_array().unwrap();
    assert!(!choices.is_empty());
    assert_eq!(choices[0]["message"]["content"], "Hello");
    assert_eq!(choices[0]["finish_reason"], "stop");
    assert_eq!(result["usage"]["prompt_tokens"], 10);
    assert_eq!(result["usage"]["completion_tokens"], 5);
}

#[test]
fn test_to_openai_stop_reason_mapping() {
    let max_tokens_resp = serde_json::json!({
        "content": [{"type": "text", "text": "truncated"}],
        "stop_reason": "max_tokens"
    });
    let result = to_openai(&max_tokens_resp, "claude-3");
    assert_eq!(result["choices"][0]["finish_reason"], "length");

    let tool_use_resp = serde_json::json!({
        "content": [{"type": "text", "text": "tool call"}],
        "stop_reason": "tool_use"
    });
    let result = to_openai(&tool_use_resp, "claude-3");
    assert_eq!(result["choices"][0]["finish_reason"], "tool_calls");
}

#[test]
fn test_to_openai_generates_id_and_model() {
    let resp = serde_json::json!({
        "content": [{"type": "text", "text": "hi"}],
        "stop_reason": "end_turn"
    });
    let result = to_openai(&resp, "my-model");
    let id = result["id"].as_str().unwrap();
    assert!(id.starts_with("chatcmpl-"), "id should start with 'chatcmpl-', got: {id}");
    assert_eq!(result["model"], "my-model");
}

// --- to_anthropic tests ---

#[test]
fn test_to_anthropic_passthrough_if_already_anthropic() {
    let resp = serde_json::json!({
        "id": "msg_abc",
        "content": [{"type": "text", "text": "Already Anthropic"}],
        "stop_reason": "end_turn"
    });
    let result = to_anthropic(&resp, "claude-3");
    // Should be returned as-is
    assert_eq!(result["id"], "msg_abc");
    assert_eq!(result["content"][0]["text"], "Already Anthropic");
}

#[test]
fn test_to_anthropic_converts_openai_response() {
    let resp = serde_json::json!({
        "choices": [{"message": {"role": "assistant", "content": "Hi"}, "finish_reason": "stop"}],
        "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
    });
    let result = to_anthropic(&resp, "claude-3");
    let content = result["content"].as_array().unwrap();
    assert!(!content.is_empty());
    assert_eq!(content[0]["type"], "text");
    assert_eq!(content[0]["text"], "Hi");
    assert_eq!(result["stop_reason"], "end_turn");
    assert_eq!(result["usage"]["input_tokens"], 10);
}

#[test]
fn test_to_anthropic_finish_reason_mapping() {
    let length_resp = serde_json::json!({
        "choices": [{"message": {"role": "assistant", "content": "cut"}, "finish_reason": "length"}]
    });
    let result = to_anthropic(&length_resp, "claude-3");
    assert_eq!(result["stop_reason"], "max_tokens");

    let tool_resp = serde_json::json!({
        "choices": [{"message": {"role": "assistant", "content": ""}, "finish_reason": "tool_calls"}]
    });
    let result = to_anthropic(&tool_resp, "claude-3");
    assert_eq!(result["stop_reason"], "tool_use");
}

#[test]
fn test_to_anthropic_generates_id() {
    let resp = serde_json::json!({
        "choices": [{"message": {"role": "assistant", "content": "hello"}, "finish_reason": "stop"}]
    });
    let result = to_anthropic(&resp, "claude-3");
    let id = result["id"].as_str().unwrap();
    assert!(id.starts_with("msg_"), "id should start with 'msg_', got: {id}");
}

// --- from_openai additional tests ---

#[test]
fn test_from_openai_multiple_system_messages() {
    let req = OpenAIChatRequest {
        model: "gpt-4".into(),
        messages: vec![
            OpenAIMessage {
                role: "system".into(),
                content: Some(serde_json::Value::String("First system.".into())),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
            OpenAIMessage {
                role: "system".into(),
                content: Some(serde_json::Value::String("Second system.".into())),
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
        stream: None,
        temperature: None,
        max_tokens: None,
        tools: None,
        tool_choice: None,
        extra: None,
    };
    let pr = from_openai(req).unwrap();
    let system = pr.system.unwrap();
    assert_eq!(system, "First system.\nSecond system.");
    assert_eq!(pr.messages.len(), 1);
}

#[test]
fn test_from_openai_preserves_tool_calls() {
    let tool_call = OpenAIToolCall {
        id: "call_abc".into(),
        call_type: "function".into(),
        function: OpenAIFunction {
            name: "get_weather".into(),
            arguments: r#"{"location":"Tokyo"}"#.into(),
        },
    };
    let req = OpenAIChatRequest {
        model: "gpt-4".into(),
        messages: vec![OpenAIMessage {
            role: "assistant".into(),
            content: None,
            name: None,
            tool_calls: Some(vec![tool_call]),
            tool_call_id: None,
        }],
        stream: None,
        temperature: None,
        max_tokens: None,
        tools: None,
        tool_choice: None,
        extra: None,
    };
    let pr = from_openai(req).unwrap();
    assert_eq!(pr.messages.len(), 1);
    let msg = &pr.messages[0];
    assert!(msg.get("tool_calls").is_some(), "tool_calls field should be preserved");
    assert_eq!(msg["tool_calls"][0]["id"], "call_abc");
}

// --- from_anthropic additional tests ---

#[test]
fn test_from_anthropic_no_system() {
    let req = AnthropicMessagesRequest {
        model: "claude-3".into(),
        messages: vec![AnthropicMessage {
            role: "user".into(),
            content: serde_json::Value::String("Hello".into()),
        }],
        system: None,
        max_tokens: 512,
        stream: None,
        temperature: None,
        tools: None,
        tool_choice: None,
        thinking: None,
        extra: None,
    };
    let pr = from_anthropic(req).unwrap();
    assert!(pr.system.is_none(), "system should be None when not provided");
}

#[test]
fn test_from_anthropic_stream_flag() {
    let req = AnthropicMessagesRequest {
        model: "claude-3".into(),
        messages: vec![AnthropicMessage {
            role: "user".into(),
            content: serde_json::Value::String("Hi".into()),
        }],
        system: None,
        max_tokens: 128,
        stream: Some(true),
        temperature: None,
        tools: None,
        tool_choice: None,
        thinking: None,
        extra: None,
    };
    let pr = from_anthropic(req).unwrap();
    assert!(pr.stream, "stream=true should be preserved in ProviderRequest");
}
