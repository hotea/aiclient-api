use aiclient_api::convert::stream::{chunk_to_anthropic, chunk_to_openai};

fn extract_data_json(output: &[u8]) -> Vec<serde_json::Value> {
    let text = std::str::from_utf8(output).unwrap();
    text.lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter(|data| *data != "[DONE]")
        .filter_map(|data| serde_json::from_str(data).ok())
        .collect()
}

// ---------------------------------------------------------------------------
// chunk_to_openai tests
// ---------------------------------------------------------------------------

#[test]
fn test_chunk_to_openai_passthrough_openai_format() {
    let input = b"data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n";
    let output = chunk_to_openai(input, "gpt-4");

    // Should pass through unchanged (same bytes back out)
    assert_eq!(output, input.to_vec());
}

#[test]
fn test_chunk_to_openai_converts_anthropic_content_delta() {
    let input =
        b"data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\n";
    let output = chunk_to_openai(input, "gpt-4");

    let events = extract_data_json(&output);
    assert_eq!(events.len(), 1, "expected exactly one data event");

    let event = &events[0];
    assert!(
        event.get("choices").is_some(),
        "converted event should have 'choices'"
    );
    let content = &event["choices"][0]["delta"]["content"];
    assert_eq!(content, "Hello", "delta.content should be 'Hello'");
}

#[test]
fn test_chunk_to_openai_converts_anthropic_message_delta() {
    let input =
        b"data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n";
    let output = chunk_to_openai(input, "gpt-4");

    let events = extract_data_json(&output);
    assert_eq!(events.len(), 1, "expected exactly one data event");

    let event = &events[0];
    assert!(
        event.get("choices").is_some(),
        "converted event should have 'choices'"
    );
    let finish_reason = &event["choices"][0]["finish_reason"];
    assert_eq!(finish_reason, "stop", "finish_reason should be 'stop'");
}

#[test]
fn test_chunk_to_openai_done_passthrough() {
    let input = b"data: [DONE]\n\n";
    let output = chunk_to_openai(input, "gpt-4");
    assert_eq!(output, b"data: [DONE]\n\n");
}

#[test]
fn test_chunk_to_openai_invalid_utf8() {
    // Bytes that are not valid UTF-8
    let input: &[u8] = &[0xFF, 0xFE, 0x00];
    let output = chunk_to_openai(input, "gpt-4");
    assert_eq!(output, input.to_vec(), "invalid UTF-8 should be returned as-is");
}

// ---------------------------------------------------------------------------
// chunk_to_anthropic tests
// ---------------------------------------------------------------------------

#[test]
fn test_chunk_to_anthropic_passthrough_anthropic_format() {
    let input =
        b"data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hi\"}}\n\n";
    let output = chunk_to_anthropic(input, "claude-3");

    // Should pass through — parse the JSON back and verify the type field
    let events = extract_data_json(&output);
    assert_eq!(events.len(), 1, "expected exactly one data event");
    assert_eq!(
        events[0]["type"], "content_block_delta",
        "type should be content_block_delta"
    );
    assert_eq!(
        events[0]["delta"]["text"], "hi",
        "text should be 'hi'"
    );
}

#[test]
fn test_chunk_to_anthropic_converts_openai_content_delta() {
    let input =
        b"data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"},\"finish_reason\":null}]}\n\n";
    let output = chunk_to_anthropic(input, "claude-3");

    let events = extract_data_json(&output);
    assert_eq!(events.len(), 1, "expected exactly one data event");

    let event = &events[0];
    assert_eq!(
        event["type"], "content_block_delta",
        "type should be content_block_delta"
    );
    assert_eq!(
        event["delta"]["type"], "text_delta",
        "delta.type should be text_delta"
    );
    assert_eq!(
        event["delta"]["text"], "Hello",
        "delta.text should be 'Hello'"
    );
}

#[test]
fn test_chunk_to_anthropic_converts_openai_finish() {
    let input =
        b"data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n";
    let output = chunk_to_anthropic(input, "claude-3");

    let events = extract_data_json(&output);
    assert_eq!(events.len(), 1, "expected exactly one data event");

    let event = &events[0];
    assert_eq!(
        event["type"], "message_delta",
        "type should be message_delta"
    );
    assert_eq!(
        event["delta"]["stop_reason"], "end_turn",
        "stop_reason should be 'end_turn'"
    );
}

#[test]
fn test_chunk_to_anthropic_done_becomes_message_stop() {
    let input = b"data: [DONE]\n\n";
    let output = chunk_to_anthropic(input, "claude-3");

    let text = std::str::from_utf8(&output).unwrap();
    assert!(
        text.contains("\"type\":\"message_stop\""),
        "output should contain message_stop type; got: {text}"
    );

    // There should be a data line whose JSON has type = message_stop
    let events = extract_data_json(&output);
    assert_eq!(events.len(), 1, "expected exactly one data event");
    assert_eq!(events[0]["type"], "message_stop");
}

#[test]
fn test_chunk_to_anthropic_invalid_utf8() {
    // Bytes that are not valid UTF-8
    let input: &[u8] = &[0xFF, 0xFE, 0x00];
    let output = chunk_to_anthropic(input, "claude-3");
    assert_eq!(output, input.to_vec(), "invalid UTF-8 should be returned as-is");
}
