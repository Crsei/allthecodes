//! SSE stream parsing for API responses.

use anyhow::{Context, Result};
use futures::Stream;

use crate::api::streaming::parse_sse_event;
use allthecodes_types::message::StreamEvent;

// ---------------------------------------------------------------------------
// SSE byte-stream parser
// ---------------------------------------------------------------------------

/// Parse an incoming HTTP byte stream (from `reqwest`) into a stream of
/// `StreamEvent` values.
///
/// The SSE wire format looks like:
/// ```text
/// event: message_start
/// data: {"type":"message_start","message":{...}}
///
/// event: content_block_start
/// data: {"type":"content_block_start","index":0,"content_block":{...}}
///
/// ```
///
/// Events are separated by blank lines (`\n\n`). Each event may have
/// `event:` and `data:` fields. We buffer incoming bytes and split on
/// line boundaries, accumulating `event` and `data` fields until a blank
/// line triggers parsing.
pub(crate) fn parse_sse_byte_stream<S>(
    byte_stream: S,
) -> impl Stream<Item = Result<StreamEvent>> + Send
where
    S: Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send + 'static,
{
    use futures::StreamExt;

    async_stream::try_stream! {
        let mut byte_stream = std::pin::pin!(byte_stream);
        let mut buffer = String::new();
        let mut current_event_type = String::new();
        let mut current_data = String::new();

        while let Some(chunk_result) = byte_stream.next().await {
            let chunk = chunk_result.context("error reading response chunk")?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            // Process complete lines from the buffer
            while let Some(line_end) = buffer.find('\n') {
                let line = buffer[..line_end].trim_end_matches('\r').to_string();
                buffer = buffer[line_end + 1..].to_string();

                if line.is_empty() {
                    // Blank line => dispatch accumulated event
                    if !current_event_type.is_empty() && !current_data.is_empty() {
                        if let Some(event) =
                            parse_sse_event(&current_event_type, &current_data)?
                        {
                            yield event;
                        }
                    }
                    current_event_type.clear();
                    current_data.clear();
                } else if let Some(rest) = line.strip_prefix("event:") {
                    current_event_type = rest.trim().to_string();
                } else if let Some(rest) = line.strip_prefix("data:") {
                    let data_part = rest.trim();
                    if !current_data.is_empty() {
                        current_data.push('\n');
                    }
                    current_data.push_str(data_part);
                }
                // Ignore other fields (id:, retry:, comments starting with ':')
            }
        }

        // Flush any remaining buffered event (in case the stream ends without
        // a trailing blank line)
        if !current_event_type.is_empty() && !current_data.is_empty() {
            if let Some(event) = parse_sse_event(&current_event_type, &current_data)? {
                yield event;
            }
        }
    }
}

/// Parse SSE-formatted text (for testing without network). Returns all events
/// found in the text.
#[cfg(test)]
pub fn parse_sse_text(text: &str) -> Result<Vec<StreamEvent>> {
    let mut events = Vec::new();
    let mut current_event_type = String::new();
    let mut current_data = String::new();

    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            // Blank line => dispatch
            if !current_event_type.is_empty() && !current_data.is_empty() {
                if let Some(event) = parse_sse_event(&current_event_type, &current_data)? {
                    events.push(event);
                }
            }
            current_event_type.clear();
            current_data.clear();
        } else if let Some(rest) = line.strip_prefix("event:") {
            current_event_type = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("data:") {
            let data_part = rest.trim();
            if !current_data.is_empty() {
                current_data.push('\n');
            }
            current_data.push_str(data_part);
        }
    }

    // Flush trailing event
    if !current_event_type.is_empty() && !current_data.is_empty() {
        if let Some(event) = parse_sse_event(&current_event_type, &current_data)? {
            events.push(event);
        }
    }

    Ok(events)
}
