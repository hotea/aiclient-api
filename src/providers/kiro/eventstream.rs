use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentEvent {
    pub content: Option<String>,
    #[serde(rename = "modelId")]
    pub model_id: Option<String>,
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
pub struct ContextUsageEvent {
    #[serde(rename = "contextUsagePercentage")]
    pub context_usage_percentage: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeteringEvent {
    pub unit: Option<String>,
    #[serde(rename = "unitPlural")]
    pub unit_plural: Option<String>,
    pub usage: Option<f64>,
}

#[derive(Debug, Clone)]
pub enum KiroEvent {
    Content(ContentEvent),
    ToolUse(ToolUseEvent),
    ContextUsage(ContextUsageEvent),
    Metering(MeteringEvent),
    Unknown,
}

/// Parse AWS event stream binary data and extract Kiro events
///
/// AWS event-stream format is a simple pattern matching approach:
/// We search for JSON objects in the binary data that match our event types
pub fn parse_event_stream(data: &[u8]) -> Result<Vec<KiroEvent>> {
    let mut events = Vec::new();

    // Convert to string to search for JSON patterns
    // The AWS event stream has binary headers, but JSON payloads
    let text = String::from_utf8_lossy(data);
    let mut search_start = 0;

    while search_start < text.len() {
        // Look for JSON object starts
        let content_start = text[search_start..]
            .find(r#"{"content":"#)
            .map(|p| search_start + p);
        let name_start = text[search_start..]
            .find(r#"{"name":"#)
            .map(|p| search_start + p);
        let input_start = text[search_start..]
            .find(r#"{"input":"#)
            .map(|p| search_start + p);
        let stop_start = text[search_start..]
            .find(r#"{"stop":"#)
            .map(|p| search_start + p);
        let context_start = text[search_start..]
            .find(r#"{"contextUsagePercentage":"#)
            .map(|p| search_start + p);
        let unit_start = text[search_start..]
            .find(r#"{"unit":"#)
            .map(|p| search_start + p);

        // Find the earliest JSON start
        let candidates: Vec<usize> = vec![
            content_start,
            name_start,
            input_start,
            stop_start,
            context_start,
            unit_start,
        ]
        .into_iter()
        .flatten()
        .collect();

        if candidates.is_empty() {
            break;
        }

        let json_start = *candidates.iter().min().unwrap();

        // Find matching closing brace
        let json_end = find_json_end(&text, json_start);

        if json_end.is_none() {
            // Incomplete JSON, move to next position
            search_start = json_start + 1;
            continue;
        }

        let json_end = json_end.unwrap();
        let json_str = &text[json_start..=json_end];

        tracing::debug!("Found JSON: {}", json_str);

        // Try parsing as different event types
        if let Ok(content) = serde_json::from_str::<ContentEvent>(json_str) {
            if content.content.is_some() {
                events.push(KiroEvent::Content(content));
            }
        } else if let Ok(tool_use) = serde_json::from_str::<ToolUseEvent>(json_str) {
            if tool_use.name.is_some() || tool_use.input.is_some() {
                events.push(KiroEvent::ToolUse(tool_use));
            }
        } else if let Ok(context) = serde_json::from_str::<ContextUsageEvent>(json_str) {
            if context.context_usage_percentage.is_some() {
                events.push(KiroEvent::ContextUsage(context));
            }
        } else if let Ok(metering) = serde_json::from_str::<MeteringEvent>(json_str) {
            if metering.usage.is_some() {
                events.push(KiroEvent::Metering(metering));
            }
        } else {
            tracing::warn!("Unknown event format: {}", json_str);
        }

        search_start = json_end + 1;
    }

    Ok(events)
}

/// Find the matching closing brace for a JSON object starting at json_start
fn find_json_end(text: &str, json_start: usize) -> Option<usize> {
    let chars: Vec<char> = text[json_start..].chars().collect();
    let mut brace_count = 0;
    let mut in_string = false;
    let mut escape_next = false;

    for (i, ch) in chars.iter().enumerate() {
        if escape_next {
            escape_next = false;
            continue;
        }

        match ch {
            '\\' => escape_next = true,
            '"' => in_string = !in_string,
            '{' if !in_string => brace_count += 1,
            '}' if !in_string => {
                brace_count -= 1;
                if brace_count == 0 {
                    return Some(json_start + i);
                }
            }
            _ => {}
        }
    }

    None
}

/// Collect all content from events into a single string
pub fn collect_content(events: &[KiroEvent]) -> String {
    events
        .iter()
        .filter_map(|event| {
            if let KiroEvent::Content(content) = event {
                content.content.as_ref()
            } else {
                None
            }
        })
        .map(|s| s.as_str())
        .collect::<Vec<_>>()
        .join("")
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
