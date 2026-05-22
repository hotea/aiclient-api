use axum::response::sse::{Event, KeepAlive, Sse};
use bytes::Bytes;
use futures::Stream;
use futures::StreamExt;
use std::convert::Infallible;
use std::pin::Pin;
use tracing::error;

use crate::convert::stream::{chunk_to_anthropic, chunk_to_openai};
use crate::providers::OutputFormat;

/// Convert a provider byte stream into an SSE response.
/// Applies chunk conversion based on the target output format.
pub fn into_sse_response(
    stream: Pin<Box<dyn Stream<Item = anyhow::Result<Bytes>> + Send>>,
    format: OutputFormat,
    model: String,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let converted = stream
        .map(move |result| {
            let model = model.clone();
            match result {
                Ok(bytes) => {
                    let converted_bytes = match format {
                        OutputFormat::OpenAI => chunk_to_openai(&bytes, &model),
                        OutputFormat::Anthropic => chunk_to_anthropic(&bytes, &model),
                    };

                    if converted_bytes.is_empty() {
                        return Vec::new();
                    }

                    let text = match std::str::from_utf8(&converted_bytes) {
                        Ok(s) => s.to_string(),
                        Err(_) => return Vec::new(),
                    };

                    parse_sse_events(&text)
                        .into_iter()
                        .map(|(event_name, data)| {
                            let mut event = Event::default();
                            if let Some(name) = event_name {
                                event = event.event(name);
                            }
                            Ok::<Event, Infallible>(event.data(data))
                        })
                        .collect::<Vec<_>>()
                }
                Err(e) => {
                    error!("Stream error: {}", e);
                    Vec::new()
                }
            }
        })
        .flat_map(futures::stream::iter);

    Sse::new(converted).keep_alive(KeepAlive::default())
}

fn parse_sse_events(text: &str) -> Vec<(Option<String>, String)> {
    let mut events = Vec::new();
    let mut current_event: Option<String> = None;
    let mut current_data: Vec<String> = Vec::new();

    for line in text.lines() {
        if line.is_empty() {
            if !current_data.is_empty() {
                events.push((current_event.take(), current_data.join("\n")));
                current_data.clear();
            }
            continue;
        }

        if let Some(name) = line.strip_prefix("event: ") {
            current_event = Some(name.to_string());
            continue;
        }

        if let Some(data) = line.strip_prefix("data: ") {
            current_data.push(data.to_string());
        }
    }

    if !current_data.is_empty() {
        events.push((current_event, current_data.join("\n")));
    }

    events
}
