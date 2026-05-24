//! LSP client — manages a single language server subprocess.
//!
//! Lifecycle: `start()` ->initialize handshake ->request/notify ->file sync — `shutdown()`.

use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use serde::Serialize;
use serde_json::Value;
use tokio::process::Command;
use tracing::{debug, warn};

use super::transport::{self, JsonRpcTransport};
use super::types::{DiagnosticRange, DocumentChange, DocumentSyncState, LspDiagnostic, LspEvent};
use super::LspServerConfig;

// ---------------------------------------------------------------------------
// LSP Client
// ---------------------------------------------------------------------------

/// An LSP client managing one language server subprocess.
pub struct LspClient {
    transport: JsonRpcTransport,
    child: tokio::process::Child,
    next_id: u64,
    /// The language this server handles (e.g. "rust", "python").
    pub language_id: String,
    _root_uri: String,
    _initialized: bool,
    open_files: HashMap<String, OpenDocument>,
}

#[derive(Debug, Clone)]
struct OpenDocument {
    language_id: String,
    version: i32,
    text: String,
}

impl LspClient {
    /// Start a language server, perform the initialize handshake, and return a
    /// ready-to-use client.
    ///
    /// 1. Spawns the server subprocess (stdin/stdout piped, stderr null).
    /// 2. Sends `initialize` with root URI, process ID, and minimal capabilities.
    /// 3. Waits for the initialize response (30 s timeout).
    /// 4. Sends `initialized` notification.
    pub async fn start(config: &LspServerConfig, root_path: &Path) -> Result<Self> {
        let root_uri = path_to_uri(root_path);

        debug!(
            language = %config.language_id,
            command = %config.command,
            root = %root_uri,
            "starting LSP server"
        );

        let mut command = Command::new(&config.command);
        command.args(&config.args);
        if !config.env.is_empty() {
            command.envs(&config.env);
        }
        command.current_dir(root_path);
        let mut child = command
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| {
                format!(
                    "failed to spawn LSP server '{}' for language '{}'",
                    config.command, config.language_id
                )
            })?;

        let stdin = child
            .stdin
            .take()
            .context("LSP server subprocess has no stdin")?;
        let stdout = child
            .stdout
            .take()
            .context("LSP server subprocess has no stdout")?;

        let mut transport = JsonRpcTransport::new(stdin, stdout);

        // -- initialize request ------------------------------------------------

        let init_params = serde_json::json!({
            "processId": std::process::id(),
            "rootUri": root_uri,
            "capabilities": {
                "textDocument": {
                    "synchronization": {
                        "dynamicRegistration": false,
                        "willSave": false,
                        "willSaveWaitUntil": false,
                        "didSave": true
                    },
                    "publishDiagnostics": {
                        "relatedInformation": true,
                        "tagSupport": { "valueSet": [1, 2] },
                        "versionSupport": true,
                        "codeDescriptionSupport": true,
                        "dataSupport": false
                    },
                    "definition": { "dynamicRegistration": false },
                    "implementation": { "dynamicRegistration": false },
                    "references": { "dynamicRegistration": false },
                    "hover": {
                        "dynamicRegistration": false,
                        "contentFormat": ["markdown", "plaintext"]
                    },
                    "documentSymbol": {
                        "dynamicRegistration": false,
                        "hierarchicalDocumentSymbolSupport": true
                    },
                    "callHierarchy": { "dynamicRegistration": false },
                    "completion": {
                        "dynamicRegistration": false,
                        "completionItem": {
                            "snippetSupport": true,
                            "documentationFormat": ["markdown", "plaintext"],
                            "deprecatedSupport": true,
                            "preselectSupport": true,
                            "tagSupport": { "valueSet": [1] }
                        }
                    }
                },
                "workspace": {
                    "symbol": { "dynamicRegistration": false },
                    "configuration": false,
                    "workspaceFolders": false
                },
                "general": {
                    "positionEncodings": ["utf-16"]
                }
            },
            "initializationOptions": config.init_options.clone()
        });

        let req = transport::make_request(0, "initialize", init_params);
        transport
            .send(&req)
            .await
            .context("failed to send initialize request")?;

        let response = transport
            .recv_timeout(Duration::from_secs(30))
            .await
            .context("initialize response timed out")?;

        if let Some(err) = transport::extract_error(&response) {
            bail!("initialize handshake failed: {err}");
        }

        debug!(
            language = %config.language_id,
            "LSP server initialize response received"
        );

        // -- initialized notification ------------------------------------------

        let notif = transport::make_notification("initialized", serde_json::json!({}));
        transport
            .send(&notif)
            .await
            .context("failed to send initialized notification")?;

        Ok(Self {
            transport,
            child,
            next_id: 1, // 0 was used for initialize
            language_id: config.language_id.clone(),
            _root_uri: root_uri,
            _initialized: true,
            open_files: HashMap::new(),
        })
    }

    // -----------------------------------------------------------------------
    // Request / Notify
    // -----------------------------------------------------------------------

    /// Send a JSON-RPC request and wait for the matching response.
    ///
    /// Server-initiated notifications received while waiting are silently
    /// skipped (they carry no `id` field).
    pub async fn request<P: Serialize>(&mut self, method: &str, params: P) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;

        let params_value =
            serde_json::to_value(params).context("failed to serialize request params")?;
        let req = transport::make_request(id, method, params_value);
        self.transport
            .send(&req)
            .await
            .with_context(|| format!("failed to send request '{method}'"))?;

        // Wait for response with matching id, skipping server notifications.
        loop {
            let msg =
                self.transport.recv().await.with_context(|| {
                    format!("failed to receive response for '{method}' (id={id})")
                })?;

            // Server notifications have no `id` — skip them.
            let msg_id = match msg.get("id") {
                Some(v) => v,
                None => {
                    self.handle_server_notification(&msg);
                    continue;
                }
            };

            if msg.get("method").is_some() {
                self.handle_server_request(&msg).await?;
                continue;
            }

            // Check if this is the response we're waiting for.
            if msg_id.as_u64() == Some(id) {
                if let Some(err) = transport::extract_error(&msg) {
                    bail!("{method}: {err}");
                }
                return Ok(msg.get("result").cloned().unwrap_or(Value::Null));
            }

            // Response for a different id — log and keep waiting.
            debug!(
                expected_id = id,
                actual_id = %msg_id,
                "received response with unexpected id, skipping"
            );
        }
    }

    /// Send a JSON-RPC notification (fire-and-forget, no response expected).
    pub async fn notify<P: Serialize>(&mut self, method: &str, params: P) -> Result<()> {
        let params_value =
            serde_json::to_value(params).context("failed to serialize notification params")?;
        let notif = transport::make_notification(method, params_value);
        self.transport
            .send(&notif)
            .await
            .with_context(|| format!("failed to send notification '{method}'"))
    }

    // -----------------------------------------------------------------------
    // File synchronization
    // -----------------------------------------------------------------------

    /// Ensure a file is open on the server via `textDocument/didOpen`.
    ///
    /// If the file was already opened, this is a no-op. The file contents are
    /// read from disk and the language ID is detected from the extension.
    pub async fn ensure_file_open(&mut self, uri: &str) -> Result<()> {
        if self.open_files.contains_key(uri) {
            return Ok(());
        }

        let path = uri_to_path(uri).context("cannot convert URI to file path")?;
        let contents = tokio::fs::read_to_string(&path)
            .await
            .with_context(|| format!("failed to read file for didOpen: {path}"))?;

        let language_id = detect_language(&path).unwrap_or_else(|| self.language_id.clone());

        self.open_document(uri, Some(language_id), contents).await?;

        Ok(())
    }

    /// Open a document on the server with caller-provided content.
    pub async fn open_document(
        &mut self,
        uri: &str,
        language_id: Option<String>,
        text: String,
    ) -> Result<DocumentSyncState> {
        if let Some(existing) = self.open_files.get(uri) {
            return Ok(DocumentSyncState {
                uri: uri.to_string(),
                language_id: existing.language_id.clone(),
                version: existing.version,
            });
        }

        let language_id = language_id.unwrap_or_else(|| {
            uri_to_path(uri)
                .ok()
                .and_then(|path| detect_language(&path))
                .unwrap_or_else(|| self.language_id.clone())
        });

        let version = 1;
        self.notify(
            "textDocument/didOpen",
            serde_json::json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": language_id,
                    "version": version,
                    "text": text
                }
            }),
        )
        .await
        .with_context(|| format!("failed to send didOpen for {uri}"))?;

        let state = DocumentSyncState {
            uri: uri.to_string(),
            language_id: language_id.clone(),
            version,
        };
        self.open_files.insert(
            uri.to_string(),
            OpenDocument {
                language_id,
                version,
                text,
            },
        );
        debug!(uri, "opened file on LSP server");
        Ok(state)
    }

    /// Change an open document using either full-text or ranged incremental
    /// updates. Ranged updates are sent as LSP `didChange` content changes.
    pub async fn change_document(
        &mut self,
        uri: &str,
        full_text: Option<String>,
        changes: Vec<DocumentChange>,
        version: Option<i32>,
    ) -> Result<DocumentSyncState> {
        if !self.open_files.contains_key(uri) {
            let text = match full_text.clone() {
                Some(text) => text,
                None => {
                    let path = uri_to_path(uri).context("cannot convert URI to file path")?;
                    tokio::fs::read_to_string(&path)
                        .await
                        .with_context(|| format!("failed to read file for didOpen: {path}"))?
                }
            };
            self.open_document(uri, None, text).await?;
        }

        let document = self
            .open_files
            .get(uri)
            .cloned()
            .with_context(|| format!("document was not opened: {uri}"))?;

        let next_version = version.unwrap_or(document.version.saturating_add(1));
        let (next_text, content_changes) = if let Some(text) = full_text {
            (
                text.clone(),
                vec![serde_json::json!({
                    "text": text
                })],
            )
        } else {
            let mut text = document.text.clone();
            let mut content_changes = Vec::with_capacity(changes.len());
            for change in changes {
                apply_document_change(&mut text, &change)?;
                let mut content_change = serde_json::json!({
                    "range": {
                        "start": {
                            "line": change.range.start_line.saturating_sub(1),
                            "character": change.range.start_character.saturating_sub(1)
                        },
                        "end": {
                            "line": change.range.end_line.saturating_sub(1),
                            "character": change.range.end_character.saturating_sub(1)
                        }
                    },
                    "text": change.text
                });
                if let Some(range_length) = change.range_length {
                    content_change["rangeLength"] = serde_json::json!(range_length);
                }
                content_changes.push(content_change);
            }
            (text, content_changes)
        };

        if content_changes.is_empty() {
            return Ok(DocumentSyncState {
                uri: uri.to_string(),
                language_id: document.language_id,
                version: document.version,
            });
        }

        self.notify(
            "textDocument/didChange",
            serde_json::json!({
                "textDocument": {
                    "uri": uri,
                    "version": next_version
                },
                "contentChanges": content_changes
            }),
        )
        .await
        .with_context(|| format!("failed to send didChange for {uri}"))?;

        let state = DocumentSyncState {
            uri: uri.to_string(),
            language_id: document.language_id.clone(),
            version: next_version,
        };
        self.open_files.insert(
            uri.to_string(),
            OpenDocument {
                language_id: document.language_id,
                version: next_version,
                text: next_text,
            },
        );
        crate::clear_delivered_diagnostics(uri);
        debug!(uri, version = next_version, "changed file on LSP server");
        Ok(state)
    }

    /// Notify the server that a live document has been saved.
    pub async fn save_document(
        &mut self,
        uri: &str,
        text: Option<String>,
    ) -> Result<DocumentSyncState> {
        if !self.open_files.contains_key(uri) {
            let contents = if let Some(text) = text.clone() {
                text
            } else {
                let path = uri_to_path(uri).context("cannot convert URI to file path")?;
                tokio::fs::read_to_string(&path)
                    .await
                    .with_context(|| format!("failed to read file for didOpen: {path}"))?
            };
            self.open_document(uri, None, contents).await?;
        }

        if let Some(text) = text {
            self.change_document(uri, Some(text), Vec::new(), None)
                .await?;
        }

        let document = self
            .open_files
            .get(uri)
            .cloned()
            .with_context(|| format!("document was not opened: {uri}"))?;

        self.notify(
            "textDocument/didSave",
            serde_json::json!({
                "textDocument": { "uri": uri },
                "text": document.text
            }),
        )
        .await
        .with_context(|| format!("failed to send didSave for {uri}"))?;

        Ok(DocumentSyncState {
            uri: uri.to_string(),
            language_id: document.language_id,
            version: document.version,
        })
    }

    /// Close a document on the language server and stop tracking its version.
    pub async fn close_document(&mut self, uri: &str) -> Result<Option<DocumentSyncState>> {
        let Some(document) = self.open_files.remove(uri) else {
            return Ok(None);
        };

        self.notify(
            "textDocument/didClose",
            serde_json::json!({
                "textDocument": { "uri": uri }
            }),
        )
        .await
        .with_context(|| format!("failed to send didClose for {uri}"))?;

        Ok(Some(DocumentSyncState {
            uri: uri.to_string(),
            language_id: document.language_id,
            version: document.version,
        }))
    }

    /// Best-effort notification drain used after document sync notifications.
    ///
    /// The current transport is request-oriented, so editor sync commands call
    /// this to ingest passive publishDiagnostics notifications without waiting
    /// for a later tool request.
    pub async fn drain_notifications(&mut self, max_wait: Duration) -> Result<usize> {
        let deadline = Instant::now() + max_wait;
        let mut count = 0;
        loop {
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                break;
            };
            if remaining.is_zero() {
                break;
            }
            let wait = remaining.min(Duration::from_millis(75));
            match self.transport.recv_timeout(wait).await {
                Ok(msg) => {
                    if msg.get("id").is_some() && msg.get("method").is_some() {
                        self.handle_server_request(&msg).await?;
                    } else {
                        self.handle_server_notification(&msg);
                    }
                    count += 1;
                }
                Err(_) => break,
            }
        }
        Ok(count)
    }

    pub fn open_files_count(&self) -> usize {
        self.open_files.len()
    }

    fn handle_server_notification(&mut self, msg: &serde_json::Value) {
        let method_str = msg.get("method").and_then(|m| m.as_str()).unwrap_or("?");
        match method_str {
            "textDocument/publishDiagnostics" => {
                if let Some(params) = msg.get("params") {
                    let event = parse_diagnostics_notification(params);
                    crate::record_diagnostics_event(event);
                }
            }
            "workspace/configuration" => {
                debug!(
                    "received workspace/configuration notification without an id; no JSON-RPC response can be sent"
                );
            }
            _ => {
                debug!(method = method_str, "skipping server notification");
            }
        }
    }

    async fn handle_server_request(&mut self, msg: &serde_json::Value) -> Result<()> {
        let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("?");
        let id = msg.get("id").cloned().unwrap_or(Value::Null);
        let result = match method {
            "workspace/configuration" => workspace_configuration_result(msg),
            "client/registerCapability" | "client/unregisterCapability" => Value::Null,
            _ => {
                debug!(method, "replying null to unsupported LSP server request");
                Value::Null
            }
        };
        let response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result
        });
        self.transport
            .send(&response)
            .await
            .with_context(|| format!("failed to respond to LSP server request '{method}'"))
    }

    // -----------------------------------------------------------------------
    // Lifecycle helpers
    // -----------------------------------------------------------------------

    /// Check whether the server subprocess is still running.
    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Gracefully shut down the server: send `shutdown` request, then `exit`
    /// notification. If the server does not respond within 5 seconds, kill it.
    pub async fn shutdown(mut self) -> Result<()> {
        debug!(language = %self.language_id, "shutting down LSP server");

        // Send shutdown request with a 5-second timeout.
        let id = self.next_id;
        self.next_id += 1;
        let req = transport::make_request(id, "shutdown", Value::Null);

        let shutdown_result = async {
            self.transport.send(&req).await?;
            self.transport.recv_timeout(Duration::from_secs(5)).await
        };

        match tokio::time::timeout(Duration::from_secs(5), shutdown_result).await {
            Ok(Ok(_)) => {
                debug!("LSP server acknowledged shutdown");
            }
            Ok(Err(e)) => {
                warn!(error = %e, "shutdown request failed, killing server");
                let _ = self.child.kill().await;
                return Ok(());
            }
            Err(_) => {
                warn!("shutdown request timed out, killing server");
                let _ = self.child.kill().await;
                return Ok(());
            }
        }

        // Send exit notification.
        let notif = transport::make_notification("exit", Value::Null);
        let _ = self.transport.send(&notif).await;

        // Give the process a moment to exit, then force-kill if needed.
        match tokio::time::timeout(Duration::from_secs(2), self.child.wait()).await {
            Ok(Ok(status)) => {
                debug!(%status, "LSP server exited");
            }
            _ => {
                warn!("LSP server did not exit after notification, killing");
                let _ = self.child.kill().await;
            }
        }

        Ok(())
    }
}

fn workspace_configuration_result(msg: &serde_json::Value) -> Value {
    let len = msg
        .get("params")
        .and_then(|p| p.get("items"))
        .and_then(|items| items.as_array())
        .map(|items| items.len())
        .unwrap_or(0);
    Value::Array(vec![Value::Null; len])
}

impl Drop for LspClient {
    fn drop(&mut self) {
        // Best-effort kill — we cannot await here but `start_kill` sends SIGKILL
        // immediately on Unix / TerminateProcess on Windows.
        let _ = self.child.start_kill();
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Convert a filesystem path to a `file://` URI string.
///
/// On Windows, backslashes are normalized to forward slashes and the path
/// is prefixed with `file:///` (three slashes before the drive letter).
/// On Unix, the path is prefixed with `file://` (two slashes, then the
/// leading `/` of the absolute path gives the standard three).
fn path_to_uri(path: &Path) -> String {
    let s = path.to_string_lossy();
    let normalized = s.replace('\\', "/");

    if normalized.starts_with('/') {
        // Unix absolute path: /home/user/... ->file:///home/user/...
        format!("file://{normalized}")
    } else {
        // Windows absolute path: C:/Users/... ->file:///C:/Users/...
        format!("file:///{normalized}")
    }
}

/// Strip the `file://` or `file:///` prefix from a URI and return the
/// filesystem path.
fn uri_to_path(uri: &str) -> Result<String> {
    if let Some(rest) = uri.strip_prefix("file:///") {
        #[cfg(windows)]
        {
            // On Windows: file:///C:/foo ->C:\foo
            Ok(rest.replace('/', "\\"))
        }
        #[cfg(not(windows))]
        {
            // On Unix: file:///home/user ->/home/user
            Ok(format!("/{rest}"))
        }
    } else if let Some(rest) = uri.strip_prefix("file://") {
        // file://host/path — treat everything after `//` as path.
        Ok(rest.to_string())
    } else {
        bail!("not a file:// URI: {uri}")
    }
}

/// Detect the language ID from a file path's extension.
fn detect_language(path: &str) -> Option<String> {
    let ext = Path::new(path).extension()?.to_str()?;
    let lang = match ext {
        "rs" => "rust",
        "ts" | "tsx" => "typescript",
        "js" | "jsx" => "javascript",
        "py" => "python",
        "go" => "go",
        "java" => "java",
        "c" | "h" => "c",
        "cpp" | "cc" | "cxx" | "hpp" => "cpp",
        "cs" => "csharp",
        "rb" => "ruby",
        "swift" => "swift",
        "kt" | "kts" => "kotlin",
        "lua" => "lua",
        "sh" | "bash" => "shellscript",
        "json" => "json",
        "yaml" | "yml" => "yaml",
        "toml" => "toml",
        "xml" => "xml",
        "html" | "htm" => "html",
        "css" => "css",
        "sql" => "sql",
        "md" | "markdown" => "markdown",
        _ => return None,
    };
    Some(lang.to_string())
}

fn apply_document_change(text: &mut String, change: &DocumentChange) -> Result<()> {
    let start = byte_index_for_position(
        text,
        change.range.start_line.saturating_sub(1),
        change.range.start_character.saturating_sub(1),
    )?;
    let end = byte_index_for_position(
        text,
        change.range.end_line.saturating_sub(1),
        change.range.end_character.saturating_sub(1),
    )?;

    if start > end {
        bail!("invalid document change range: start is after end");
    }
    text.replace_range(start..end, &change.text);
    Ok(())
}

fn byte_index_for_position(text: &str, line: u32, utf16_character: u32) -> Result<usize> {
    let mut current_line = 0u32;
    let mut line_start = 0usize;

    for (idx, ch) in text.char_indices() {
        if current_line == line {
            break;
        }
        if ch == '\n' {
            current_line += 1;
            line_start = idx + ch.len_utf8();
        }
    }

    if current_line != line {
        if current_line.saturating_add(1) == line && text.ends_with('\n') {
            return Ok(text.len());
        }
        bail!("line {} is out of bounds", line + 1);
    }

    let line_text = &text[line_start..];
    let line_end_offset = line_text.find('\n').unwrap_or(line_text.len());
    let line_slice = &line_text[..line_end_offset];

    let mut utf16_seen = 0u32;
    for (offset, ch) in line_slice.char_indices() {
        if utf16_seen == utf16_character {
            return Ok(line_start + offset);
        }
        utf16_seen = utf16_seen.saturating_add(ch.len_utf16() as u32);
        if utf16_seen > utf16_character {
            bail!(
                "character {} splits a UTF-16 code point on line {}",
                utf16_character + 1,
                line + 1
            );
        }
    }

    if utf16_seen == utf16_character {
        Ok(line_start + line_slice.len())
    } else {
        bail!(
            "character {} is out of bounds on line {}",
            utf16_character + 1,
            line + 1
        )
    }
}

/// Parse a `textDocument/publishDiagnostics` notification into an LspEvent.
fn parse_diagnostics_notification(params: &serde_json::Value) -> LspEvent {
    let uri = params["uri"].as_str().unwrap_or_default().to_string();
    let diagnostics = params["diagnostics"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|val| {
                    let range = val.get("range")?;
                    Some(LspDiagnostic {
                        range: DiagnosticRange {
                            start_line: range["start"]["line"].as_u64()? as u32 + 1,
                            start_character: range["start"]["character"].as_u64()? as u32 + 1,
                            end_line: range["end"]["line"].as_u64()? as u32 + 1,
                            end_character: range["end"]["character"].as_u64()? as u32 + 1,
                        },
                        severity: match val["severity"].as_u64() {
                            Some(1) => "error",
                            Some(2) => "warning",
                            Some(3) => "info",
                            Some(4) => "hint",
                            _ => "unknown",
                        }
                        .to_string(),
                        message: val["message"].as_str()?.to_string(),
                        source: val["source"].as_str().map(|s| s.to_string()),
                        code: val.get("code").and_then(|c| {
                            c.as_str()
                                .map(|s| s.to_string())
                                .or_else(|| c.as_u64().map(|n| n.to_string()))
                        }),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    LspEvent::DiagnosticsPublished { uri, diagnostics }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- path_to_uri -------------------------------------------------------

    #[test]
    fn test_path_to_uri_unix_style() {
        let path = Path::new("/home/user/project/src/main.rs");
        let uri = path_to_uri(path);
        assert_eq!(uri, "file:///home/user/project/src/main.rs");
    }

    #[test]
    fn test_path_to_uri_windows_style() {
        // Simulate a Windows path with forward slashes (as path_to_uri normalizes).
        let path = Path::new("C:/Users/dev/project/src/main.rs");
        let uri = path_to_uri(path);
        assert_eq!(uri, "file:///C:/Users/dev/project/src/main.rs");
    }

    #[test]
    fn test_path_to_uri_windows_backslashes() {
        // On Windows, Path::new with backslashes is native.
        // path_to_uri should normalize to forward slashes.
        let path = Path::new("C:\\Users\\dev\\project");
        let uri = path_to_uri(path);
        assert!(
            uri.starts_with("file:///"),
            "expected file:/// prefix, got: {uri}"
        );
        assert!(
            !uri.contains('\\'),
            "URI should not contain backslashes: {uri}"
        );
    }

    // -- uri_to_path -------------------------------------------------------

    #[test]
    fn test_uri_to_path_unix() {
        let result = uri_to_path("file:///home/user/file.rs").unwrap();
        #[cfg(windows)]
        assert_eq!(result, "home\\user\\file.rs");
        #[cfg(not(windows))]
        assert_eq!(result, "/home/user/file.rs");
    }

    #[test]
    fn test_uri_to_path_windows() {
        let result = uri_to_path("file:///C:/Users/dev/file.rs").unwrap();
        #[cfg(windows)]
        assert_eq!(result, "C:\\Users\\dev\\file.rs");
        #[cfg(not(windows))]
        assert_eq!(result, "/C:/Users/dev/file.rs");
    }

    #[test]
    fn test_uri_to_path_not_file_uri() {
        let result = uri_to_path("https://example.com");
        assert!(result.is_err());
    }

    #[test]
    fn test_uri_to_path_file_host() {
        // file://host/share/path — uncommon but valid.
        let result = uri_to_path("file://server/share/file.txt").unwrap();
        assert_eq!(result, "server/share/file.txt");
    }

    // -- detect_language ---------------------------------------------------

    #[test]
    fn test_detect_language_rust() {
        assert_eq!(detect_language("src/main.rs"), Some("rust".into()));
    }

    #[test]
    fn test_detect_language_typescript() {
        assert_eq!(detect_language("app.ts"), Some("typescript".into()));
        assert_eq!(detect_language("App.tsx"), Some("typescript".into()));
    }

    #[test]
    fn test_detect_language_javascript() {
        assert_eq!(detect_language("index.js"), Some("javascript".into()));
        assert_eq!(detect_language("component.jsx"), Some("javascript".into()));
    }

    #[test]
    fn test_detect_language_python() {
        assert_eq!(detect_language("script.py"), Some("python".into()));
    }

    #[test]
    fn test_detect_language_cpp() {
        assert_eq!(detect_language("main.cpp"), Some("cpp".into()));
        assert_eq!(detect_language("lib.hpp"), Some("cpp".into()));
        assert_eq!(detect_language("module.cc"), Some("cpp".into()));
    }

    #[test]
    fn test_detect_language_c() {
        assert_eq!(detect_language("main.c"), Some("c".into()));
        assert_eq!(detect_language("header.h"), Some("c".into()));
    }

    #[test]
    fn test_detect_language_various() {
        assert_eq!(detect_language("main.go"), Some("go".into()));
        assert_eq!(detect_language("App.java"), Some("java".into()));
        assert_eq!(detect_language("Gemfile.rb"), Some("ruby".into()));
        assert_eq!(detect_language("config.json"), Some("json".into()));
        assert_eq!(detect_language("style.css"), Some("css".into()));
        assert_eq!(detect_language("page.html"), Some("html".into()));
        assert_eq!(detect_language("data.yaml"), Some("yaml".into()));
        assert_eq!(detect_language("query.sql"), Some("sql".into()));
    }

    #[test]
    fn test_detect_language_unknown() {
        assert_eq!(detect_language("data.csv"), None);
        assert_eq!(detect_language("Makefile"), None);
        assert_eq!(detect_language("file.xyz"), None);
    }

    #[test]
    fn test_detect_language_nested_path() {
        assert_eq!(
            detect_language("/home/user/project/src/lib.rs"),
            Some("rust".into())
        );
    }

    // -- round-trip --------------------------------------------------------

    #[test]
    fn test_uri_roundtrip_unix_path() {
        let original = Path::new("/tmp/project/src/main.rs");
        let uri = path_to_uri(original);
        let back = uri_to_path(&uri).unwrap();
        #[cfg(not(windows))]
        assert_eq!(back, "/tmp/project/src/main.rs");
        #[cfg(windows)]
        assert_eq!(back, "tmp\\project\\src\\main.rs");
    }

    #[test]
    fn test_uri_roundtrip_windows_path() {
        let original = Path::new("C:/dev/project/src/main.rs");
        let uri = path_to_uri(original);
        let back = uri_to_path(&uri).unwrap();
        #[cfg(windows)]
        assert_eq!(back, "C:\\dev\\project\\src\\main.rs");
        #[cfg(not(windows))]
        assert_eq!(back, "/C:/dev/project/src/main.rs");
    }

    // -- parse_diagnostics_notification ---------------------------------------

    #[test]
    fn parse_diagnostics_notification_parses_valid_params() {
        let params = serde_json::json!({
            "uri": "file:///src/main.rs",
            "diagnostics": [{
                "range": {
                    "start": {"line": 10, "character": 4},
                    "end": {"line": 10, "character": 12}
                },
                "severity": 1,
                "message": "unused variable",
                "source": "rust-analyzer",
                "code": "E0599"
            }]
        });
        let event = parse_diagnostics_notification(&params);
        match event {
            LspEvent::DiagnosticsPublished { uri, diagnostics } => {
                assert_eq!(uri, "file:///src/main.rs");
                assert_eq!(diagnostics.len(), 1);
                assert_eq!(diagnostics[0].severity, "error");
                assert_eq!(diagnostics[0].range.start_line, 11); // 0-based -> 1-based
                assert_eq!(diagnostics[0].code.as_deref(), Some("E0599"));
            }
            _ => panic!("expected DiagnosticsPublished"),
        }
    }

    #[test]
    fn parse_diagnostics_notification_handles_empty() {
        let params = serde_json::json!({
            "uri": "file:///empty.rs",
            "diagnostics": []
        });
        let event = parse_diagnostics_notification(&params);
        match event {
            LspEvent::DiagnosticsPublished { diagnostics, .. } => {
                assert!(diagnostics.is_empty());
            }
            _ => panic!("expected DiagnosticsPublished"),
        }
    }

    #[test]
    fn workspace_configuration_request_returns_null_array_per_item() {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "workspace/configuration",
            "params": {
                "items": [
                    {"section": "rust-analyzer"},
                    {"section": "typescript"}
                ]
            }
        });

        assert_eq!(
            workspace_configuration_result(&msg),
            serde_json::json!([null, null])
        );
    }

    #[test]
    fn workspace_configuration_request_without_items_returns_empty_array() {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 8,
            "method": "workspace/configuration",
            "params": {}
        });

        assert_eq!(workspace_configuration_result(&msg), serde_json::json!([]));
    }

    #[test]
    fn apply_document_change_replaces_ascii_range() {
        let mut text = "fn main() {\n    let x = 1;\n}\n".to_string();
        let change = DocumentChange {
            range: crate::types::SourceRange {
                start_line: 2,
                start_character: 9,
                end_line: 2,
                end_character: 10,
            },
            range_length: None,
            text: "answer".to_string(),
        };
        apply_document_change(&mut text, &change).unwrap();
        assert!(text.contains("let answer = 1;"));
    }

    #[test]
    fn byte_index_for_position_counts_utf16_units() {
        let text = "a😀b\n";
        assert_eq!(byte_index_for_position(text, 0, 0).unwrap(), 0);
        assert_eq!(byte_index_for_position(text, 0, 1).unwrap(), 1);
        assert_eq!(byte_index_for_position(text, 0, 3).unwrap(), 5);
        assert!(byte_index_for_position(text, 0, 2).is_err());
    }
}
