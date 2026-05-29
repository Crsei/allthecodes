//! Product-experience tools for context, artifacts, peer discovery, and remote runs.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use url::Url;
use uuid::Uuid;

use crate::tool::{
    PermissionResult, Tool, ToolProgress, ToolResult, ToolUseContext, Tools, ValidationResult,
};
use allthecodes_types::message::{
    AssistantMessage, ContentBlock, InfoLevel, Message, MessageContent, SystemMessage,
    SystemSubtype, ToolResultContent,
};

pub fn tools() -> Tools {
    vec![
        Arc::new(CtxInspectTool),
        Arc::new(TerminalCaptureTool),
        Arc::new(ReviewArtifactTool),
        Arc::new(SnipTool),
        Arc::new(ListPeersTool),
        Arc::new(RemoteTriggerTool),
    ]
}

struct CtxInspectTool;
struct TerminalCaptureTool;
struct ReviewArtifactTool;
struct SnipTool;
struct ListPeersTool;
struct RemoteTriggerTool;

#[async_trait]
impl Tool for CtxInspectTool {
    fn name(&self) -> &str {
        "CtxInspect"
    }

    async fn description(&self, _input: &Value) -> String {
        "Inspect the current conversation context and estimated token usage.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Optional text filter." }
            }
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let query = input
            .get("query")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_ascii_lowercase);
        let app_state = (ctx.get_app_state)();
        let total_tokens = allthecodes_utils::messages::estimate_total_tokens(&ctx.messages);
        let mut by_kind: HashMap<&'static str, usize> = HashMap::new();
        let mut matches = Vec::new();

        for message in &ctx.messages {
            *by_kind.entry(message_kind(message)).or_default() += 1;
            if let Some(query) = query.as_deref() {
                let text = message_text(message);
                if text.to_ascii_lowercase().contains(query) {
                    matches.push(json!({
                        "uuid": message.uuid().to_string(),
                        "kind": message_kind(message),
                        "timestamp": message.timestamp(),
                        "preview": truncate_chars(&text, 240),
                    }));
                }
            }
        }

        let prompt_caching_enabled = !app_state.main_loop_model.starts_with("openai/")
            && !app_state.main_loop_model.starts_with("grok/")
            && !app_state.main_loop_model.starts_with("gemini/");
        let summary = format!(
            "Model context: {}\nEstimated tokens: {total_tokens}\nMessages: {}",
            app_state.main_loop_model,
            ctx.messages.len()
        );
        Ok(ToolResult {
            data: json!({
                "total_tokens": total_tokens,
                "message_count": ctx.messages.len(),
                "by_kind": by_kind,
                "context_window_model": app_state.main_loop_model,
                "prompt_caching_enabled": prompt_caching_enabled,
                "session_memory_enabled": !app_state.surfaced_memory_keys.is_empty(),
                "context_collapse_enabled": true,
                "summary": summary,
                "matches": matches,
            }),
            display_preview: Some(format!(
                "Context: {total_tokens} tokens, {} messages",
                ctx.messages.len()
            )),
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "Inspect current context usage, message counts, and optional text matches before deciding whether to snip or compact history.".to_string()
    }
}

#[async_trait]
impl Tool for TerminalCaptureTool {
    fn name(&self) -> &str {
        "TerminalCapture"
    }

    async fn description(&self, _input: &Value) -> String {
        "Capture recent shell tool output from the conversation.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "lines": { "type": "integer", "minimum": 1, "maximum": 1000 },
                "tool_use_id": { "type": "string" },
                "panel_id": { "type": "string" }
            }
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        let lines = input.get("lines").and_then(Value::as_u64).unwrap_or(50);
        if lines == 0 || lines > 1000 {
            return ValidationResult::Error {
                message: "'lines' must be between 1 and 1000".to_string(),
                error_code: 400,
            };
        }
        ValidationResult::Ok
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let lines = input.get("lines").and_then(Value::as_u64).unwrap_or(50) as usize;
        let requested_id = input.get("tool_use_id").and_then(Value::as_str);
        let shell_ids = shell_tool_use_ids(&ctx.messages);
        let captured =
            find_tool_result_text(&ctx.messages, requested_id, &shell_ids).ok_or_else(|| {
                anyhow!("No matching shell tool output found in conversation history")
            })?;
        let content = tail_lines(&captured.content, lines);
        let line_count = content.lines().count();
        Ok(ToolResult {
            data: json!({
                "content": content,
                "line_count": line_count,
                "tool_use_id": captured.tool_use_id,
                "source": captured.source_tool,
                "panel_id": input.get("panel_id").and_then(Value::as_str),
            }),
            display_preview: Some(format!("Captured {line_count} terminal line(s)")),
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "Capture recent shell output already present in this session. Use this to re-read command output without rerunning the command.".to_string()
    }
}

#[async_trait]
impl Tool for ReviewArtifactTool {
    fn name(&self) -> &str {
        "ReviewArtifact"
    }

    async fn description(&self, input: &Value) -> String {
        input
            .get("title")
            .and_then(Value::as_str)
            .map(|title| format!("Review artifact: {title}"))
            .unwrap_or_else(|| "Review an artifact with structured annotations.".to_string())
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "artifact": { "type": "string" },
                "title": { "type": "string" },
                "annotations": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "line": { "type": "integer", "minimum": 1 },
                            "message": { "type": "string" },
                            "severity": {
                                "type": "string",
                                "enum": ["info", "warning", "error", "suggestion"]
                            }
                        },
                        "required": ["message"]
                    }
                },
                "summary": { "type": "string" }
            },
            "required": ["artifact", "annotations"]
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        if input
            .get("artifact")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .is_empty()
            || !input.get("annotations").is_some_and(Value::is_array)
        {
            return ValidationResult::Error {
                message: "'artifact' and array 'annotations' are required".to_string(),
                error_code: 400,
            };
        }
        ValidationResult::Ok
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let annotations = input
            .get("annotations")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(ToolResult {
            data: json!({
                "artifact": input.get("artifact").cloned().unwrap_or(Value::Null),
                "title": input.get("title").cloned(),
                "annotations": annotations,
                "annotationCount": annotations.len(),
                "summary": input.get("summary").cloned(),
            }),
            display_preview: Some(format!(
                "Review complete: {} annotation(s)",
                annotations.len()
            )),
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "Present a structured review of a code snippet, document, or generated artifact with inline annotations and an optional summary.".to_string()
    }
}

#[async_trait]
impl Tool for SnipTool {
    fn name(&self) -> &str {
        "Snip"
    }

    async fn description(&self, _input: &Value) -> String {
        "Record intent to snip selected conversation messages from future context.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "message_ids": { "type": "array", "items": { "type": "string" } },
                "reason": { "type": "string" }
            },
            "required": ["message_ids"]
        })
    }

    fn is_destructive(&self, _input: &Value) -> bool {
        true
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        let valid = input
            .get("message_ids")
            .and_then(Value::as_array)
            .is_some_and(|ids| !ids.is_empty() && ids.iter().all(Value::is_string));
        if !valid {
            return ValidationResult::Error {
                message: "'message_ids' must be a non-empty array of strings".to_string(),
                error_code: 400,
            };
        }
        ValidationResult::Ok
    }

    async fn check_permissions(&self, input: &Value, _ctx: &ToolUseContext) -> PermissionResult {
        PermissionResult::Ask {
            message: format!(
                "Snip {} message(s) from future context?",
                input
                    .get("message_ids")
                    .and_then(Value::as_array)
                    .map(Vec::len)
                    .unwrap_or(0)
            ),
        }
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let requested: HashSet<String> = input
            .get("message_ids")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(ToOwned::to_owned)
            .collect();
        let matched = ctx
            .messages
            .iter()
            .filter(|message| requested.contains(&message.uuid().to_string()))
            .count();
        let summary = input
            .get("reason")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("Snipped messages")
            .to_string();
        let marker = Message::System(SystemMessage {
            uuid: Uuid::new_v4(),
            timestamp: Utc::now().timestamp_millis(),
            subtype: SystemSubtype::Informational {
                level: InfoLevel::Info,
            },
            content: format!(
                "Snip applied to {matched} message(s): {summary}. Future model context will omit the selected messages."
            ),
        });
        Ok(ToolResult {
            data: json!({
                "snipped_count": matched,
                "requested_count": requested.len(),
                "message_ids": requested.iter().cloned().collect::<Vec<_>>(),
                "summary": summary,
                "projection_applied": true,
            }),
            display_preview: Some(format!("Snip recorded for {matched} message(s)")),
            new_messages: vec![marker],
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "Mark selected conversation messages for snipping to reduce future context pressure. Include a concise reason that preserves important facts.".to_string()
    }
}

#[async_trait]
impl Tool for ListPeersTool {
    fn name(&self) -> &str {
        "ListPeers"
    }

    async fn description(&self, _input: &Value) -> String {
        "Discover local and configured allthecodes daemon peers.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "include_self": { "type": "boolean" }
            }
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let include_self = input
            .get("include_self")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let peers = list_peers(include_self)?;
        Ok(ToolResult {
            display_preview: Some(format!("Found {} allthecodes peer(s)", peers.len())),
            data: json!({ "peers": peers }),
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "List local daemon and configured remote allthecodes peers. Use the returned peer name or address with RemoteTrigger.".to_string()
    }
}

#[async_trait]
impl Tool for RemoteTriggerTool {
    fn name(&self) -> &str {
        "RemoteTrigger"
    }

    async fn description(&self, _input: &Value) -> String {
        "Trigger another allthecodes daemon instance with a prompt.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "prompt": { "type": "string" },
                "peer": { "type": "string" },
                "url": { "type": "string" },
                "token": { "type": "string" },
                "idempotency_key": { "type": "string" },
                "timeout_ms": { "type": "integer", "minimum": 1000, "maximum": 60000 }
            },
            "required": ["prompt"]
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        let has_prompt = input
            .get("prompt")
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty());
        let timeout = input
            .get("timeout_ms")
            .and_then(Value::as_u64)
            .unwrap_or(20_000);
        if !has_prompt || !(1000..=60_000).contains(&timeout) {
            return ValidationResult::Error {
                message: "'prompt' is required and 'timeout_ms' must be between 1000 and 60000"
                    .to_string(),
                error_code: 400,
            };
        }
        ValidationResult::Ok
    }

    async fn check_permissions(&self, input: &Value, _ctx: &ToolUseContext) -> PermissionResult {
        let target = input
            .get("url")
            .and_then(Value::as_str)
            .or_else(|| input.get("peer").and_then(Value::as_str))
            .unwrap_or("local daemon");
        PermissionResult::Ask {
            message: format!("Submit prompt to remote allthecodes peer {target}?"),
        }
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let prompt = input
            .get("prompt")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();
        let target = resolve_remote_target(&input)?;
        let token = input
            .get("token")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .or(target.token)
            .ok_or_else(|| anyhow!("RemoteTrigger requires a daemon control token"))?;
        let timeout = Duration::from_millis(
            input
                .get("timeout_ms")
                .and_then(Value::as_u64)
                .unwrap_or(20_000),
        );
        let endpoint = format!("{}/api/submit", target.url.trim_end_matches('/'));
        let idempotency_key = input
            .get("idempotency_key")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let request = reqwest::Client::builder()
            .timeout(timeout)
            .build()?
            .post(&endpoint)
            .header("x-allthecodes-daemon-token", token)
            .json(&json!({ "text": prompt, "idempotency_key": idempotency_key }));
        let mut abort_signal = ctx.abort_signal.clone();
        let response = tokio::select! {
            response = request.send() => response?,
            changed = abort_signal.changed() => {
                match changed {
                    Ok(()) if *abort_signal.borrow() => bail!("RemoteTrigger cancelled"),
                    _ => bail!("RemoteTrigger interrupted"),
                }
            }
        };
        let status = response.status().as_u16();
        let body = response
            .json::<Value>()
            .await
            .unwrap_or_else(|err| json!({ "parse_error": err.to_string() }));
        let ok = (200..300).contains(&status);
        let audit = append_remote_trigger_audit(&target.url, ok, status, &body)?;
        Ok(ToolResult {
            data: json!({
                "ok": ok,
                "status": status,
                "url": target.url,
                "peer": target.name,
                "response": body,
                "audit_id": audit.audit_id,
            }),
            display_preview: Some(format!("RemoteTrigger HTTP {status}")),
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "Submit a prompt to another allthecodes daemon instance. Discover targets with ListPeers, then pass a peer name or explicit URL.".to_string()
    }
}

#[derive(Debug)]
struct CapturedToolOutput {
    tool_use_id: String,
    source_tool: String,
    content: String,
}

fn shell_tool_use_ids(messages: &[Message]) -> HashMap<String, String> {
    let mut ids = HashMap::new();
    for message in messages {
        let Message::Assistant(assistant) = message else {
            continue;
        };
        for block in &assistant.content {
            if let ContentBlock::ToolUse { id, name, .. } = block {
                if matches!(name.as_str(), "Bash" | "PowerShell" | "REPL") {
                    ids.insert(id.clone(), name.clone());
                }
            }
        }
    }
    ids
}

fn find_tool_result_text(
    messages: &[Message],
    requested_id: Option<&str>,
    allowed_ids: &HashMap<String, String>,
) -> Option<CapturedToolOutput> {
    for message in messages.iter().rev() {
        let Message::User(user) = message else {
            continue;
        };
        let MessageContent::Blocks(blocks) = &user.content else {
            continue;
        };
        for block in blocks.iter().rev() {
            let ContentBlock::ToolResult {
                tool_use_id,
                content,
                ..
            } = block
            else {
                continue;
            };
            if requested_id.is_some_and(|id| id != tool_use_id) {
                continue;
            }
            let source_tool = allowed_ids.get(tool_use_id)?;
            return Some(CapturedToolOutput {
                tool_use_id: tool_use_id.clone(),
                source_tool: source_tool.clone(),
                content: tool_result_text(content),
            });
        }
    }
    None
}

fn tool_result_text(content: &ToolResultContent) -> String {
    match content {
        ToolResultContent::Text(text) => text.clone(),
        ToolResultContent::Blocks(blocks) => blocks
            .iter()
            .map(content_block_text)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

fn content_block_text(block: &ContentBlock) -> String {
    match block {
        ContentBlock::Text { text } => text.clone(),
        ContentBlock::ToolUse { name, input, .. }
        | ContentBlock::ServerToolUse { name, input, .. } => {
            format!("{name}: {}", truncate_chars(&input.to_string(), 200))
        }
        ContentBlock::ToolResult { content, .. } => tool_result_text(content),
        ContentBlock::Thinking { thinking, .. } => thinking.clone(),
        ContentBlock::RedactedThinking { .. } => "[redacted thinking]".to_string(),
        ContentBlock::ConnectorText { connector_text, .. } => connector_text.clone(),
        ContentBlock::Image { source } => format!("[image {}]", source.media_type),
    }
}

fn message_kind(message: &Message) -> &'static str {
    match message {
        Message::User(_) => "user",
        Message::Assistant(_) => "assistant",
        Message::System(_) => "system",
        Message::Progress(_) => "progress",
        Message::Attachment(_) => "attachment",
    }
}

fn message_text(message: &Message) -> String {
    match message {
        Message::User(user) => match &user.content {
            MessageContent::Text(text) => text.clone(),
            MessageContent::Blocks(blocks) => blocks
                .iter()
                .map(content_block_text)
                .collect::<Vec<_>>()
                .join("\n"),
        },
        Message::Assistant(assistant) => assistant
            .content
            .iter()
            .map(content_block_text)
            .collect::<Vec<_>>()
            .join("\n"),
        Message::System(system) => system.content.clone(),
        Message::Progress(progress) => progress.data.to_string(),
        Message::Attachment(attachment) => serde_json::to_string(&attachment.attachment)
            .unwrap_or_else(|_| "attachment".to_string()),
    }
}

fn tail_lines(content: &str, max_lines: usize) -> String {
    let mut lines = content.lines().rev().take(max_lines).collect::<Vec<_>>();
    lines.reverse();
    lines.join("\n")
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let take = max_chars.saturating_sub(3);
    let mut out = value.chars().take(take).collect::<String>();
    out.push_str("...");
    out
}

#[derive(Debug, Clone, Serialize)]
struct PeerInfo {
    name: String,
    address: String,
    url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<String>,
    local: bool,
    has_token: bool,
}

#[derive(Debug, Deserialize)]
struct DaemonStateFile {
    status: String,
    pid: u32,
    cwd: PathBuf,
    port: u16,
    health_url: String,
}

#[derive(Debug, Deserialize)]
struct ControlTokenFile {
    token: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ConfiguredPeer {
    name: String,
    url: String,
    #[serde(default)]
    token_env: Option<String>,
    #[serde(default)]
    token_path: Option<PathBuf>,
    #[serde(default)]
    cwd: Option<String>,
}

#[derive(Debug)]
struct RemoteTarget {
    name: Option<String>,
    url: String,
    token: Option<String>,
}

fn list_peers(include_self: bool) -> Result<Vec<PeerInfo>> {
    let mut peers = Vec::new();
    if let Some(local) = read_local_peer()? {
        if include_self || local.pid != Some(std::process::id()) {
            peers.push(local);
        }
    }
    for peer in configured_peers()? {
        let has_token = configured_peer_token(&peer).is_some();
        peers.push(PeerInfo {
            address: format!("allthecodes-daemon:{}", peer.url),
            url: peer.url,
            name: peer.name,
            cwd: peer.cwd,
            pid: None,
            status: Some("configured".to_string()),
            local: false,
            has_token,
        });
    }
    Ok(peers)
}

fn read_local_peer() -> Result<Option<PeerInfo>> {
    let state_path = daemon_dir().join("supervisor.json");
    if !state_path.exists() {
        return Ok(None);
    }
    let state: DaemonStateFile = read_json(&state_path)?;
    let url = if state.health_url.trim().is_empty() {
        format!("http://127.0.0.1:{}", state.port)
    } else {
        state
            .health_url
            .trim_end_matches("/health")
            .trim_end_matches('/')
            .to_string()
    };
    Ok(Some(PeerInfo {
        name: "local-daemon".to_string(),
        address: format!("allthecodes-daemon:{url}"),
        url,
        cwd: Some(state.cwd.display().to_string()),
        pid: Some(state.pid),
        status: Some(state.status),
        local: true,
        has_token: read_local_control_token()?.is_some(),
    }))
}

fn resolve_remote_target(input: &Value) -> Result<RemoteTarget> {
    if let Some(url) = input.get("url").and_then(Value::as_str) {
        return Ok(RemoteTarget {
            name: None,
            url: normalize_daemon_url(url)?,
            token: None,
        });
    }

    if let Some(peer) = input.get("peer").and_then(Value::as_str) {
        let peer = peer.trim();
        if peer == "local" || peer == "local-daemon" {
            let local = read_local_peer()?.ok_or_else(|| anyhow!("local daemon is not running"))?;
            return Ok(RemoteTarget {
                name: Some(local.name),
                url: local.url,
                token: read_local_control_token()?,
            });
        }
        if let Some(raw_url) = peer.strip_prefix("allthecodes-daemon:") {
            return Ok(RemoteTarget {
                name: None,
                url: normalize_daemon_url(raw_url)?,
                token: None,
            });
        }
        if peer.starts_with("http://") || peer.starts_with("https://") {
            return Ok(RemoteTarget {
                name: None,
                url: normalize_daemon_url(peer)?,
                token: None,
            });
        }
        for configured in configured_peers()? {
            if configured.name == peer {
                return Ok(RemoteTarget {
                    name: Some(configured.name.clone()),
                    url: normalize_daemon_url(&configured.url)?,
                    token: configured_peer_token(&configured),
                });
            }
        }
        bail!("unknown allthecodes peer '{peer}'. Use ListPeers to discover targets.");
    }

    let local = read_local_peer()?.ok_or_else(|| anyhow!("local daemon is not running"))?;
    Ok(RemoteTarget {
        name: Some(local.name),
        url: local.url,
        token: read_local_control_token()?,
    })
}

fn normalize_daemon_url(raw: &str) -> Result<String> {
    let url = Url::parse(raw.trim()).context("invalid daemon URL")?;
    match url.scheme() {
        "http" | "https" => {}
        scheme => bail!("unsupported daemon URL scheme '{scheme}'"),
    }
    if url.username() != "" || url.password().is_some() {
        bail!("daemon URL must not contain embedded credentials");
    }
    Ok(raw.trim().trim_end_matches('/').to_string())
}

fn configured_peers() -> Result<Vec<ConfiguredPeer>> {
    let path = data_root().join("peers.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let value: Value = read_json(&path)?;
    if value.is_array() {
        return serde_json::from_value(value)
            .with_context(|| format!("failed to parse {}", path.display()));
    }
    if let Some(peers) = value.get("peers") {
        return serde_json::from_value(peers.clone())
            .with_context(|| format!("failed to parse {}", path.display()));
    }
    bail!("{} must be an array or object with peers[]", path.display())
}

fn configured_peer_token(peer: &ConfiguredPeer) -> Option<String> {
    if let Some(env_name) = peer.token_env.as_deref().filter(|value| !value.is_empty()) {
        if let Ok(token) = std::env::var(env_name) {
            if !token.trim().is_empty() {
                return Some(token);
            }
        }
    }
    if let Some(path) = peer.token_path.as_deref() {
        if let Ok(token) = fs::read_to_string(path) {
            let token = token.trim().to_string();
            if !token.is_empty() {
                return Some(token);
            }
        }
    }
    None
}

fn read_local_control_token() -> Result<Option<String>> {
    let path = daemon_dir().join("control-token.json");
    if !path.exists() {
        return Ok(None);
    }
    let token: ControlTokenFile = read_json(&path)?;
    Ok((!token.token.trim().is_empty()).then_some(token.token))
}

#[derive(Debug, Serialize)]
struct RemoteTriggerAudit {
    audit_id: String,
    created_at: String,
    target_url: String,
    ok: bool,
    status: u16,
    response: Value,
}

fn append_remote_trigger_audit(
    target_url: &str,
    ok: bool,
    status: u16,
    response: &Value,
) -> Result<RemoteTriggerAudit> {
    let audit = RemoteTriggerAudit {
        audit_id: Uuid::new_v4().to_string(),
        created_at: Utc::now().to_rfc3339(),
        target_url: target_url.to_string(),
        ok,
        status,
        response: response.clone(),
    };
    let path = data_root().join("remote-trigger-audit.ndjson");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut line = serde_json::to_string(&audit)?;
    line.push('\n');
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("failed to open {}", path.display()))?
        .write_all(line.as_bytes())
        .with_context(|| format!("failed to append {}", path.display()))?;
    Ok(audit)
}

fn data_root() -> PathBuf {
    allthecodes_config::paths::data_root()
}

fn daemon_dir() -> PathBuf {
    allthecodes_config::paths::daemon_dir()
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("failed to parse {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_lines_returns_requested_suffix() {
        assert_eq!(tail_lines("a\nb\nc\nd", 2), "c\nd");
    }

    #[test]
    fn normalize_daemon_url_rejects_credentials() {
        assert!(normalize_daemon_url("http://user:pass@127.0.0.1:19836").is_err());
        assert_eq!(
            normalize_daemon_url("http://127.0.0.1:19836/").unwrap(),
            "http://127.0.0.1:19836"
        );
    }
}
