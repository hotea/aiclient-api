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
    let converted = stream.filter_map(move |result| {
        let model = model.clone();
        async move {
            match result {
                Ok(bytes) => {
                    let converted_bytes = match format {
                        OutputFormat::OpenAI => chunk_to_openai(&bytes, &model),
                        OutputFormat::Anthropic => chunk_to_anthropic(&bytes, &model),
                    };

                    if converted_bytes.is_empty() {
                        return None;
                    }

                    // Parse SSE lines and emit events
                    let text = match std::str::from_utf8(&converted_bytes) {
                        Ok(s) => s.to_string(),
                        Err(_) => return None,
                    };

                    // Extract data from SSE formatted content
                    let data_lines: Vec<&str> = text
                        .lines()
                        .filter_map(|line| line.strip_prefix("data: "))
                        .collect();

                    if data_lines.is_empty() {
                        return None;
                    }

                    // For simplicity, emit the first data chunk
                    // In production, this could be split into multiple events
                    let data = data_lines.join("\n");
                    Some(Ok::<Event, Infallible>(Event::default().data(data)))
                }
                Err(e) => {
                    error!("Stream error: {}", e);
                    None
                }
            }
        }
    });

    Sse::new(converted).keep_alive(KeepAlive::default())
}
