use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufReader, Cursor, Read};

const MAX_FRAME_SIZE: u32 = 4 * 1024 * 1024;

const EVENT_ASSISTANT_RESPONSE: &str = "assistantResponseEvent";
const EVENT_REASONING_CONTENT: &str = "reasoningContentEvent";
const EVENT_TOOL_USE: &str = "toolUseEvent";
const EVENT_TOOL_RESULT: &str = "toolResultEvent";
const EVENT_CONTEXT_USAGE: &str = "contextUsageEvent";
const EVENT_METERING: &str = "meteringEvent";
const EVENT_MESSAGE_METADATA: &str = "messageMetadataEvent";
const EVENT_INVALID_STATE: &str = "invalidStateEvent";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentEvent {
    pub content: Option<String>,
    #[serde(rename = "modelId")]
    pub model_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningEvent {
    pub text: Option<String>,
    pub signature: Option<String>,
    #[serde(rename = "redactedContent")]
    pub redacted_content: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolUseEvent {
    pub name: Option<String>,
    #[serde(rename = "toolUseId")]
    pub tool_use_id: Option<String>,
    pub input: Option<String>,
    pub stop: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultContentBlock {
    pub text: Option<String>,
    pub json: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultData {
    #[serde(rename = "toolUseId")]
    pub tool_use_id: Option<String>,
    pub content: Option<Vec<ToolResultContentBlock>>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultEvent {
    #[serde(rename = "toolResult")]
    pub tool_result: Option<ToolResultData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextUsageEvent {
    #[serde(rename = "contextUsagePercentage")]
    pub context_usage_percentage: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeteringEvent {
    pub usage: Option<f64>,
    #[serde(rename = "inputTokens")]
    pub input_tokens: Option<u32>,
    #[serde(rename = "outputTokens")]
    pub output_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageMetadataEvent {
    #[serde(rename = "conversationId")]
    pub conversation_id: Option<String>,
    #[serde(rename = "utteranceId")]
    pub utterance_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvalidStateEvent {
    pub reason: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ToolUseCompleteEvent {
    pub name: String,
    pub tool_use_id: String,
    pub input: String,
}

#[derive(Debug, Clone)]
pub enum KiroEvent {
    Content(ContentEvent),
    Reasoning(ReasoningEvent),
    ToolUse(ToolUseEvent),
    ToolUseComplete(ToolUseCompleteEvent),
    ToolResult(ToolResultEvent),
    ContextUsage(ContextUsageEvent),
    Metering(MeteringEvent),
    MessageMetadata(MessageMetadataEvent),
    InvalidState(InvalidStateEvent),
    Unknown,
}

#[derive(Debug, Default)]
struct ToolUseAccumulator {
    by_id: HashMap<String, PartialToolUse>,
}

#[derive(Debug, Default)]
struct PartialToolUse {
    name: Option<String>,
    input: String,
}

impl ToolUseAccumulator {
    fn update(&mut self, event: &ToolUseEvent) -> Option<ToolUseCompleteEvent> {
        let tool_use_id = event.tool_use_id.clone()?;
        let partial = self.by_id.entry(tool_use_id.clone()).or_default();

        if let Some(name) = &event.name {
            partial.name = Some(name.clone());
        }
        if let Some(input) = &event.input {
            partial.input.push_str(input);
        }

        if event.stop.unwrap_or(false) {
            let partial = self.by_id.remove(&tool_use_id).unwrap_or_default();
            return Some(ToolUseCompleteEvent {
                name: partial
                    .name
                    .or_else(|| event.name.clone())
                    .unwrap_or_default(),
                tool_use_id,
                input: partial.input,
            });
        }

        None
    }
}

pub fn parse_event_stream(data: &[u8]) -> Result<Vec<KiroEvent>> {
    let mut reader = BufReader::new(Cursor::new(data));
    let mut events = Vec::new();
    let mut accumulator = ToolUseAccumulator::default();

    loop {
        match read_frame(&mut reader) {
            Ok(Some((headers, payload))) => {
                let (message_type, event_type) = extract_frame_headers(&headers)?;

                if message_type.as_deref() == Some("exception") {
                    let err_text = String::from_utf8_lossy(&payload).to_string();
                    tracing::warn!(event_type, err_text, "kiro upstream exception frame");
                    continue;
                }

                match event_type.as_deref() {
                    Some(EVENT_ASSISTANT_RESPONSE) => {
                        let event: ContentEvent = serde_json::from_slice(&payload)?;
                        events.push(KiroEvent::Content(event));
                    }
                    Some(EVENT_REASONING_CONTENT) => {
                        let event: ReasoningEvent = serde_json::from_slice(&payload)?;
                        events.push(KiroEvent::Reasoning(event));
                    }
                    Some(EVENT_TOOL_USE) => {
                        let event: ToolUseEvent = serde_json::from_slice(&payload)?;
                        if let Some(complete) = accumulator.update(&event) {
                            events.push(KiroEvent::ToolUseComplete(complete));
                        }
                        events.push(KiroEvent::ToolUse(event));
                    }
                    Some(EVENT_TOOL_RESULT) => {
                        let event: ToolResultEvent = serde_json::from_slice(&payload)?;
                        events.push(KiroEvent::ToolResult(event));
                    }
                    Some(EVENT_CONTEXT_USAGE) => {
                        let event: ContextUsageEvent = serde_json::from_slice(&payload)?;
                        events.push(KiroEvent::ContextUsage(event));
                    }
                    Some(EVENT_METERING) => {
                        let event: MeteringEvent = serde_json::from_slice(&payload)?;
                        events.push(KiroEvent::Metering(event));
                    }
                    Some(EVENT_MESSAGE_METADATA) => {
                        let event: MessageMetadataEvent = serde_json::from_slice(&payload)?;
                        events.push(KiroEvent::MessageMetadata(event));
                    }
                    Some(EVENT_INVALID_STATE) => {
                        let event: InvalidStateEvent = serde_json::from_slice(&payload)?;
                        events.push(KiroEvent::InvalidState(event));
                    }
                    Some(other) => {
                        tracing::debug!(
                            event_type = other,
                            payload_len = payload.len(),
                            "ignored kiro frame"
                        );
                    }
                    None => {
                        tracing::debug!(
                            payload_len = payload.len(),
                            "kiro frame missing event type"
                        );
                    }
                }
            }
            Ok(None) => break,
            Err(err) => return Err(err),
        }
    }

    Ok(events)
}

fn read_frame<R: Read>(reader: &mut R) -> Result<Option<(Vec<u8>, Vec<u8>)>> {
    let mut prelude = [0u8; 12];
    let mut read = 0usize;

    while read < prelude.len() {
        match reader.read(&mut prelude[read..]) {
            Ok(0) if read == 0 => return Ok(None),
            Ok(0) => return Err(anyhow!("truncated event stream prelude")),
            Ok(n) => read += n,
            Err(err) => return Err(err.into()),
        }
    }

    let prelude_crc = u32::from_be_bytes([prelude[8], prelude[9], prelude[10], prelude[11]]);
    let computed_prelude_crc = crc32fast::hash(&prelude[..8]);
    if computed_prelude_crc != prelude_crc {
        return Err(anyhow!(
            "invalid event stream prelude crc: got {:08x}, want {:08x}",
            computed_prelude_crc,
            prelude_crc
        ));
    }

    let total_len = u32::from_be_bytes([prelude[0], prelude[1], prelude[2], prelude[3]]);
    let headers_len = u32::from_be_bytes([prelude[4], prelude[5], prelude[6], prelude[7]]);

    if total_len < 16 || total_len > MAX_FRAME_SIZE {
        return Err(anyhow!("invalid event stream frame size: {total_len}"));
    }

    let remaining_len = (total_len - 12) as usize;
    let mut remaining = vec![0u8; remaining_len];
    reader.read_exact(&mut remaining)?;

    let message_crc = u32::from_be_bytes([
        remaining[remaining.len() - 4],
        remaining[remaining.len() - 3],
        remaining[remaining.len() - 2],
        remaining[remaining.len() - 1],
    ]);

    let mut crc_input = Vec::with_capacity(prelude.len() + remaining.len() - 4);
    crc_input.extend_from_slice(&prelude);
    crc_input.extend_from_slice(&remaining[..remaining.len() - 4]);
    let computed_message_crc = crc32fast::hash(&crc_input);
    if computed_message_crc != message_crc {
        return Err(anyhow!(
            "invalid event stream message crc: got {:08x}, want {:08x}",
            computed_message_crc,
            message_crc
        ));
    }

    let headers_len = headers_len as usize;
    if headers_len > remaining.len().saturating_sub(4) {
        return Err(anyhow!("invalid event stream headers length"));
    }

    let headers = remaining[..headers_len].to_vec();
    let payload = remaining[headers_len..remaining.len() - 4].to_vec();
    Ok(Some((headers, payload)))
}

fn extract_frame_headers(headers: &[u8]) -> Result<(Option<String>, Option<String>)> {
    let mut i = 0usize;
    let mut message_type = None;
    let mut event_type = None;

    while i < headers.len() {
        let name_len = *headers
            .get(i)
            .ok_or_else(|| anyhow!("invalid header name length"))? as usize;
        i += 1;
        let name = std::str::from_utf8(
            headers
                .get(i..i + name_len)
                .ok_or_else(|| anyhow!("invalid header name bytes"))?,
        )?
        .to_string();
        i += name_len;

        let value_type = *headers
            .get(i)
            .ok_or_else(|| anyhow!("missing header value type"))?;
        i += 1;

        let value = match value_type {
            7 => {
                let len = u16::from_be_bytes([
                    *headers
                        .get(i)
                        .ok_or_else(|| anyhow!("missing string length high byte"))?,
                    *headers
                        .get(i + 1)
                        .ok_or_else(|| anyhow!("missing string length low byte"))?,
                ]) as usize;
                i += 2;
                let value = std::str::from_utf8(
                    headers
                        .get(i..i + len)
                        .ok_or_else(|| anyhow!("invalid string header bytes"))?,
                )?
                .to_string();
                i += len;
                value
            }
            0 | 1 => String::new(),
            2 => {
                i += 1;
                String::new()
            }
            3 => {
                i += 2;
                String::new()
            }
            4 => {
                i += 4;
                String::new()
            }
            5 | 8 => {
                i += 8;
                String::new()
            }
            6 => {
                let len = u16::from_be_bytes([
                    *headers
                        .get(i)
                        .ok_or_else(|| anyhow!("missing bytes length high byte"))?,
                    *headers
                        .get(i + 1)
                        .ok_or_else(|| anyhow!("missing bytes length low byte"))?,
                ]) as usize;
                i += 2 + len;
                String::new()
            }
            9 => {
                i += 16;
                String::new()
            }
            _ => {
                return Err(anyhow!(
                    "unsupported event stream header type: {value_type}"
                ))
            }
        };

        match name.as_str() {
            ":message-type" => message_type = Some(value),
            ":event-type" | ":exception-type" => event_type = Some(value),
            _ => {}
        }
    }

    Ok((message_type, event_type))
}

pub fn collect_content(events: &[KiroEvent]) -> String {
    events
        .iter()
        .filter_map(|event| match event {
            KiroEvent::Content(content) => content.content.as_deref(),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

pub fn collect_thinking(events: &[KiroEvent]) -> String {
    events
        .iter()
        .filter_map(|event| match event {
            KiroEvent::Reasoning(reasoning) => reasoning.text.as_deref(),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

pub fn collect_tool_uses(events: &[KiroEvent]) -> Vec<ToolUseCompleteEvent> {
    events
        .iter()
        .filter_map(|event| match event {
            KiroEvent::ToolUseComplete(tool_use) => Some(tool_use.clone()),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collect_content() {
        let events = vec![
            KiroEvent::Content(ContentEvent {
                content: Some("Hello".to_string()),
                model_id: Some("claude-sonnet-4.6".to_string()),
            }),
            KiroEvent::Content(ContentEvent {
                content: Some(" world".to_string()),
                model_id: Some("claude-sonnet-4.6".to_string()),
            }),
        ];

        assert_eq!(collect_content(&events), "Hello world");
    }
}
