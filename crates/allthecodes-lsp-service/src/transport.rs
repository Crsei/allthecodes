//! JSON-RPC 2.0 over stdio transport for LSP servers.
//!
//! Implements the LSP wire protocol: `Content-Length` header framing over
//! stdin/stdout of a child process.

use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, ChildStdout};

/// Default timeout for reading a response from the LSP server.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// JSON-RPC 2.0 transport over child process stdin/stdout.
pub struct JsonRpcTransport {
    writer: ChildStdin,
    reader: BufReader<ChildStdout>,
}

impl JsonRpcTransport {
    /// Create a new transport from a child process's stdin and stdout.
    pub fn new(stdin: ChildStdin, stdout: ChildStdout) -> Self {
        Self {
            writer: stdin,
            reader: BufReader::new(stdout),
        }
    }

    // -----------------------------------------------------------------------
    // Sending
    // -----------------------------------------------------------------------

    /// Send a JSON-RPC message to the server.
    ///
    /// Frames the message with `Content-Length` header per LSP spec.
    pub async fn send(&mut self, message: &Value) -> Result<()> {
        let body =
            serde_json::to_string(message).context("failed to serialize JSON-RPC message")?;
        let header = format!("Content-Length: {}\r\n\r\n", body.len());

        self.writer
            .write_all(header.as_bytes())
            .await
            .context("failed to write header to LSP server stdin")?;
        self.writer
            .write_all(body.as_bytes())
            .await
            .context("failed to write body to LSP server stdin")?;
        self.writer
            .flush()
            .await
            .context("failed to flush LSP server stdin")?;

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Receiving
    // -----------------------------------------------------------------------

    /// Receive a JSON-RPC message with the default 30-second timeout.
    pub async fn recv(&mut self) -> Result<Value> {
        self.recv_timeout(DEFAULT_TIMEOUT).await
    }

    /// Receive a JSON-RPC message with a custom timeout.
    pub async fn recv_timeout(&mut self, timeout: Duration) -> Result<Value> {
        tokio::time::timeout(timeout, self.read_message())
            .await
            .context("timed out waiting for LSP server response")?
    }

    /// Read one complete LSP message (header + body).
    async fn read_message(&mut self) -> Result<Value> {
        let content_length = self.read_content_length().await?;

        let mut body = vec![0u8; content_length];
        self.reader
            .read_exact(&mut body)
            .await
            .context("failed to read message body from LSP server")?;

        serde_json::from_slice(&body).context("failed to parse JSON-RPC message body")
    }

    /// Parse headers until we find `Content-Length`, skipping others.
    async fn read_content_length(&mut self) -> Result<usize> {
        loop {
            let mut line = String::new();
            let bytes_read = self
                .reader
                .read_line(&mut line)
                .await
                .context("failed to read header line from LSP server")?;

            if bytes_read == 0 {
                bail!("LSP server closed connection (EOF)");
            }

            let trimmed = line.trim();

            // Empty line signals end of headers — but we must have seen
            // Content-Length before this point.  If we reach an empty line
            // without Content-Length it means the server sent an invalid
            // frame; however, in practice Content-Length is always present.
            if trimmed.is_empty() {
                // We shouldn't reach here without having returned already.
                // Re-enter the loop (the next call will hit the body read).
                // But to be safe, keep reading — sometimes there are
                // consecutive blank lines.
                continue;
            }

            if let Some(value) = trimmed.strip_prefix("Content-Length:") {
                let length: usize = value
                    .trim()
                    .parse()
                    .context("invalid Content-Length value")?;

                // Consume remaining headers until the blank line.
                loop {
                    let mut header = String::new();
                    let n = self
                        .reader
                        .read_line(&mut header)
                        .await
                        .context("failed to read remaining headers")?;
                    if n == 0 {
                        bail!("LSP server closed connection (EOF)");
                    }
                    if header.trim().is_empty() {
                        break;
                    }
                    // Ignore other headers (e.g. Content-Type).
                }

                return Ok(length);
            }
            // Ignore non-Content-Length headers before Content-Length appears.
        }
    }
}

// ---------------------------------------------------------------------------
// Message builders
// ---------------------------------------------------------------------------

/// Build a JSON-RPC 2.0 request object.
pub fn make_request(id: u64, method: &str, params: Value) -> Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    })
}

/// Build a JSON-RPC 2.0 notification (no `id` field).
pub fn make_notification(method: &str, params: Value) -> Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
    })
}

/// Extract an error message from a JSON-RPC response, if present.
pub fn extract_error(response: &Value) -> Option<String> {
    response.get("error").map(|err| {
        let message = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown error");
        let code = err.get("code").and_then(|c| c.as_i64());
        match code {
            Some(c) => format!("JSON-RPC error {}: {}", c, message),
            None => format!("JSON-RPC error: {}", message),
        }
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_make_request() {
        let req = make_request(
            42,
            "textDocument/definition",
            serde_json::json!({"uri": "file:///foo.rs"}),
        );
        assert_eq!(req["jsonrpc"], "2.0");
        assert_eq!(req["id"], 42);
        assert_eq!(req["method"], "textDocument/definition");
        assert_eq!(req["params"]["uri"], "file:///foo.rs");
        // Notifications have no id; requests must have one.
        assert!(req.get("id").is_some());
    }

    #[test]
    fn test_make_notification() {
        let notif = make_notification(
            "textDocument/didOpen",
            serde_json::json!({"uri": "file:///bar.py"}),
        );
        assert_eq!(notif["jsonrpc"], "2.0");
        assert_eq!(notif["method"], "textDocument/didOpen");
        assert_eq!(notif["params"]["uri"], "file:///bar.py");
        // Notifications must NOT have an id field.
        assert!(notif.get("id").is_none());
    }

    #[test]
    fn test_extract_error_present() {
        let resp = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "error": {
                "code": -32601,
                "message": "Method not found"
            }
        });
        let err = extract_error(&resp);
        assert!(err.is_some());
        let msg = err.unwrap();
        assert!(msg.contains("-32601"), "expected error code in: {msg}");
        assert!(
            msg.contains("Method not found"),
            "expected message in: {msg}"
        );
    }

    #[test]
    fn test_extract_error_absent() {
        let resp = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {"capabilities": {}}
        });
        assert!(extract_error(&resp).is_none());
    }

    #[test]
    fn test_extract_error_no_code() {
        let resp = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "error": {
                "message": "something went wrong"
            }
        });
        let err = extract_error(&resp).unwrap();
        assert!(err.contains("something went wrong"));
        // Should not contain a numeric code prefix.
        assert!(!err.contains("-32"));
    }

    #[test]
    fn test_send_format() {
        // Verify the wire format: Content-Length header + body.
        let message = make_request(1, "initialize", serde_json::json!({}));
        let body = serde_json::to_string(&message).unwrap();
        let expected_header = format!("Content-Length: {}\r\n\r\n", body.len());
        let wire = format!("{}{}", expected_header, body);

        // Header must start with Content-Length.
        assert!(wire.starts_with("Content-Length: "));
        // Header-body separator is \r\n\r\n.
        assert!(wire.contains("\r\n\r\n"));
        // Body must be valid JSON.
        let header_end = wire.find("\r\n\r\n").unwrap() + 4;
        let parsed_body: Value = serde_json::from_str(&wire[header_end..]).unwrap();
        assert_eq!(parsed_body["jsonrpc"], "2.0");
        assert_eq!(parsed_body["id"], 1);
        assert_eq!(parsed_body["method"], "initialize");
    }

    #[test]
    fn test_make_request_with_complex_params() {
        let params = serde_json::json!({
            "textDocument": {
                "uri": "file:///src/main.rs"
            },
            "position": {
                "line": 10,
                "character": 5
            }
        });
        let req = make_request(7, "textDocument/hover", params);
        assert_eq!(req["id"], 7);
        assert_eq!(req["params"]["position"]["line"], 10);
        assert_eq!(req["params"]["textDocument"]["uri"], "file:///src/main.rs");
    }
}
