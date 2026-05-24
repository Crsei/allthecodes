//! MCP stdio transport -- background reader loop and response dispatch.
//!
//! The reader task reads line-delimited JSON-RPC messages from the server's
//! stdout and dispatches responses to waiting request futures via oneshot channels.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use serde_json::Value;

type PendingRequest = oneshot::Sender<Result<Value>>;
type PendingRequests = Arc<Mutex<HashMap<u64, PendingRequest>>>;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, BufReader};
use tokio::sync::{oneshot, Mutex};
use tracing::{debug, info, warn};

use super::channel::parse_channel_notification;
use super::{JsonRpcResponse, McpRuntimeContext, McpSubsystemEvent};

/// Background task that reads JSON-RPC responses from the MCP server's stdout.
///
/// Each line is parsed as a JSON-RPC response and dispatched to the
/// corresponding pending request via its oneshot channel.
pub(crate) async fn reader_loop(
    stdout: tokio::process::ChildStdout,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value>>>>>,
    server_name: String,
    runtime: McpRuntimeContext,
) {
    let reader = BufReader::new(stdout);
    let mut lines = reader.lines();

    loop {
        match lines.next_line().await {
            Ok(Some(line)) => {
                let line = line.trim().to_string();
                if line.is_empty() {
                    continue;
                }

                // Try to parse as a JSON-RPC response
                match serde_json::from_str::<JsonRpcResponse>(&line) {
                    Ok(response) => {
                        dispatch_response(&pending, &server_name, response).await;
                    }
                    Err(_) => {
                        // Could be a notification from the server
                        match serde_json::from_str::<Value>(&line) {
                            Ok(val) => {
                                if val.get("id").is_some() {
                                    warn!(
                                        server = %server_name,
                                        line = %line,
                                        "MCP: received malformed response"
                                    );
                                } else if let Some(method) =
                                    val.get("method").and_then(|m| m.as_str())
                                {
                                    handle_json_notification(&server_name, method, &val, &runtime);
                                } else {
                                    debug!(
                                        server = %server_name,
                                        "MCP: received unknown JSON message"
                                    );
                                }
                            }
                            Err(e) => {
                                debug!(
                                    server = %server_name,
                                    error = %e,
                                    line = %line,
                                    "MCP: non-JSON line from server stdout"
                                );
                            }
                        }
                    }
                }
            }
            Ok(None) => {
                info!(server = %server_name, "MCP: server stdout closed (EOF)");
                break;
            }
            Err(e) => {
                warn!(
                    server = %server_name,
                    error = %e,
                    "MCP: error reading server stdout"
                );
                break;
            }
        }
    }

    // On exit, fail all pending requests
    let mut pending = pending.lock().await;
    for (id, sender) in pending.drain() {
        debug!(server = %server_name, id = id, "MCP: failing pending request (reader exited)");
        let _ = sender.send(Err(anyhow::anyhow!(
            "MCP server '{}' closed connection",
            server_name
        )));
    }
}

/// Background task that reads JSON-RPC responses from an HTTP SSE stream.
///
/// The MCP SSE transport sends an initial `endpoint` event containing the
/// HTTP POST target for client-to-server JSON-RPC messages. Later `message`
/// events carry normal JSON-RPC responses and notifications.
pub(crate) async fn sse_reader_loop<R>(
    mut reader: R,
    pending: PendingRequests,
    server_name: String,
    mut endpoint_sender: Option<oneshot::Sender<Result<String>>>,
    runtime: McpRuntimeContext,
) where
    R: AsyncBufRead + Unpin,
{
    let mut event_name = String::new();
    let mut data_lines: Vec<String> = Vec::new();

    loop {
        let mut line = String::new();
        match reader.read_line(&mut line).await {
            Ok(0) => {
                info!(server = %server_name, "MCP: SSE stream closed (EOF)");
                break;
            }
            Ok(_) => {
                let line = line.trim_end_matches(['\r', '\n']);
                if line.is_empty() {
                    handle_sse_event(
                        &server_name,
                        &pending,
                        &event_name,
                        &data_lines,
                        &mut endpoint_sender,
                        &runtime,
                    )
                    .await;
                    event_name.clear();
                    data_lines.clear();
                    continue;
                }

                if line.starts_with(':') {
                    continue;
                }

                let (field, value) = line.split_once(':').unwrap_or((line, ""));
                let value = value.strip_prefix(' ').unwrap_or(value);
                match field {
                    "event" => event_name = value.to_string(),
                    "data" => data_lines.push(value.to_string()),
                    _ => {}
                }
            }
            Err(e) => {
                warn!(
                    server = %server_name,
                    error = %e,
                    "MCP: error reading SSE stream"
                );
                break;
            }
        }
    }

    if let Some(sender) = endpoint_sender.take() {
        let _ = sender.send(Err(anyhow!(
            "MCP SSE server '{}' closed stream before endpoint event",
            server_name
        )));
    }

    let mut pending = pending.lock().await;
    for (id, sender) in pending.drain() {
        debug!(server = %server_name, id = id, "MCP: failing pending request (SSE stream exited)");
        let _ = sender.send(Err(anyhow!(
            "MCP SSE server '{}' closed connection",
            server_name
        )));
    }
}

/// Background reader for the optional Streamable HTTP GET SSE stream.
///
/// Unlike the legacy SSE transport reader, this stream is not the ownership
/// boundary for client-to-server requests. If it exits, in-flight POST
/// requests may still complete through their own response bodies, so pending
/// requests are not failed here.
pub(crate) async fn streamable_http_sse_reader_loop<R>(
    mut reader: R,
    pending: PendingRequests,
    server_name: String,
    runtime: McpRuntimeContext,
) where
    R: AsyncBufRead + Unpin,
{
    let mut event_name = String::new();
    let mut data_lines: Vec<String> = Vec::new();
    let mut endpoint_sender: Option<oneshot::Sender<Result<String>>> = None;

    loop {
        let mut line = String::new();
        match reader.read_line(&mut line).await {
            Ok(0) => {
                info!(
                    server = %server_name,
                    "MCP: Streamable HTTP GET stream closed (EOF)"
                );
                break;
            }
            Ok(_) => {
                let line = line.trim_end_matches(['\r', '\n']);
                if line.is_empty() {
                    handle_sse_event(
                        &server_name,
                        &pending,
                        &event_name,
                        &data_lines,
                        &mut endpoint_sender,
                        &runtime,
                    )
                    .await;
                    event_name.clear();
                    data_lines.clear();
                    continue;
                }

                if line.starts_with(':') {
                    continue;
                }

                let (field, value) = line.split_once(':').unwrap_or((line, ""));
                let value = value.strip_prefix(' ').unwrap_or(value);
                match field {
                    "event" => event_name = value.to_string(),
                    "data" => data_lines.push(value.to_string()),
                    _ => {}
                }
            }
            Err(e) => {
                warn!(
                    server = %server_name,
                    error = %e,
                    "MCP: error reading Streamable HTTP GET stream"
                );
                break;
            }
        }
    }
}

async fn handle_sse_event(
    server_name: &str,
    pending: &PendingRequests,
    event_name: &str,
    data_lines: &[String],
    endpoint_sender: &mut Option<oneshot::Sender<Result<String>>>,
    runtime: &McpRuntimeContext,
) {
    if data_lines.is_empty() {
        return;
    }

    let data = data_lines.join("\n");
    match event_name {
        "endpoint" => {
            if let Some(sender) = endpoint_sender.take() {
                let _ = sender.send(Ok(data));
            }
        }
        "" | "message" => match serde_json::from_str::<JsonRpcResponse>(&data) {
            Ok(response) => {
                dispatch_response(pending, server_name, response).await;
            }
            Err(_) => match serde_json::from_str::<Value>(&data) {
                Ok(val) => {
                    if val.get("id").is_some() {
                        warn!(
                            server = %server_name,
                            data = %data,
                            "MCP: received malformed SSE response"
                        );
                    } else if let Some(method) = val.get("method").and_then(|m| m.as_str()) {
                        handle_json_notification(server_name, method, &val, runtime);
                    } else {
                        debug!(
                            server = %server_name,
                            "MCP: received unknown SSE JSON message"
                        );
                    }
                }
                Err(e) => {
                    debug!(
                        server = %server_name,
                        error = %e,
                        data = %data,
                        "MCP: non-JSON SSE message"
                    );
                }
            },
        },
        other => {
            debug!(
                server = %server_name,
                event = other,
                "MCP: ignoring SSE event"
            );
        }
    }
}

fn handle_json_notification(
    server_name: &str,
    method: &str,
    value: &Value,
    runtime: &McpRuntimeContext,
) {
    if let Some(event) = notification_event(server_name, value) {
        debug!(
            server = %server_name,
            method = method,
            "MCP: routed server notification"
        );
        runtime.emit_event(event);
        return;
    }

    debug!(
        server = %server_name,
        method = method,
        "MCP: received server notification"
    );
}

pub(crate) fn notification_event(server_name: &str, value: &Value) -> Option<McpSubsystemEvent> {
    let method = value.get("method").and_then(|m| m.as_str())?;
    if method != "notifications/claude/channel" {
        return None;
    }

    let params = value.get("params").unwrap_or(&Value::Null);
    let notification = parse_channel_notification(params)?;
    Some(McpSubsystemEvent::ChannelNotification {
        server_name: server_name.to_string(),
        content: notification.content,
        meta: notification.meta,
    })
}

/// Dispatch a parsed JSON-RPC response to the corresponding pending request.
pub(crate) async fn dispatch_response(
    pending: &PendingRequests,
    server_name: &str,
    response: JsonRpcResponse,
) {
    let id = match response.id.as_u64() {
        Some(id) => id,
        None => {
            debug!(
                server = %server_name,
                id = ?response.id,
                "MCP: response has non-integer id, ignoring"
            );
            return;
        }
    };

    let mut pending = pending.lock().await;
    if let Some(sender) = pending.remove(&id) {
        let result = if let Some(error) = response.error {
            Err(anyhow::anyhow!(
                "MCP server '{}' returned error (code {}): {}",
                server_name,
                error.code,
                error.message
            ))
        } else {
            Ok(response.result.unwrap_or(Value::Null))
        };

        let _ = sender.send(result);
    } else {
        debug!(
            server = %server_name,
            id = id,
            "MCP: received response for unknown request id"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn notification_event_routes_channel_notification() {
        let value = json!({
            "jsonrpc": "2.0",
            "method": "notifications/claude/channel",
            "params": {
                "content": "Build finished",
                "meta": {"priority": "normal"}
            }
        });

        let event = notification_event("server-a", &value).unwrap();
        match event {
            McpSubsystemEvent::ChannelNotification {
                server_name,
                content,
                meta,
            } => {
                assert_eq!(server_name, "server-a");
                assert_eq!(content, "Build finished");
                assert_eq!(meta["priority"], "normal");
            }
            other => panic!("unexpected event: {:?}", other),
        }
    }

    #[test]
    fn notification_event_ignores_non_channel_notifications() {
        let value = json!({
            "jsonrpc": "2.0",
            "method": "notifications/progress",
            "params": {"content": "ignored"}
        });

        assert!(notification_event("server-a", &value).is_none());
    }

    #[test]
    fn notification_event_rejects_malformed_channel_payload() {
        let value = json!({
            "jsonrpc": "2.0",
            "method": "notifications/claude/channel",
            "params": {"content": 42}
        });

        assert!(notification_event("server-a", &value).is_none());
    }
}
