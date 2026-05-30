//! Peer discovery and remote trigger product tools.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use allthecodes_types::message::AssistantMessage;
use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use url::Url;
use uuid::Uuid;

use crate::tool::{
    PermissionResult, Tool, ToolProgress, ToolResult, ToolUseContext, ValidationResult,
};

pub(super) struct ListPeersTool;
pub(super) struct RemoteTriggerTool;

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

#[derive(Debug, Clone, Serialize)]
pub(super) struct PeerInfo {
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
pub(super) struct DaemonStateFile {
    status: String,
    pid: u32,
    cwd: PathBuf,
    port: u16,
    health_url: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct ControlTokenFile {
    token: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ConfiguredPeer {
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
pub(super) struct RemoteTarget {
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
pub(super) struct RemoteTriggerAudit {
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
    use serial_test::serial;

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    #[test]
    fn normalize_daemon_url_rejects_credentials() {
        assert!(normalize_daemon_url("http://user:pass@127.0.0.1:19836").is_err());
        assert_eq!(
            normalize_daemon_url("http://127.0.0.1:19836/").unwrap(),
            "http://127.0.0.1:19836"
        );
    }

    #[test]
    #[serial]
    fn list_peers_reads_local_daemon_state() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path().to_str().unwrap());
        let daemon = home.path().join("daemon");
        fs::create_dir_all(&daemon).unwrap();
        fs::write(
            daemon.join("supervisor.json"),
            json!({
                "status": "running",
                "pid": 999_999,
                "cwd": "/workspace/project",
                "port": 19836,
                "health_url": "http://127.0.0.1:19836/health"
            })
            .to_string(),
        )
        .unwrap();
        fs::write(
            daemon.join("control-token.json"),
            json!({ "token": "secret-token" }).to_string(),
        )
        .unwrap();

        let peers = list_peers(false).unwrap();

        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].name, "local-daemon");
        assert_eq!(peers[0].url, "http://127.0.0.1:19836");
        assert_eq!(peers[0].status.as_deref(), Some("running"));
        assert!(peers[0].has_token);
    }

    #[test]
    #[serial]
    fn configured_peers_support_array_and_token_env() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path().to_str().unwrap());
        let _token = EnvGuard::set("ALLTHECODES_TEST_PEER_TOKEN", "peer-secret");
        fs::write(
            home.path().join("peers.json"),
            json!([
                {
                    "name": "build-box",
                    "url": "http://127.0.0.1:19837",
                    "token_env": "ALLTHECODES_TEST_PEER_TOKEN",
                    "cwd": "/workspace/build"
                }
            ])
            .to_string(),
        )
        .unwrap();

        let peers = list_peers(false).unwrap();
        let target = resolve_remote_target(&json!({ "peer": "build-box" })).unwrap();

        assert_eq!(peers.len(), 1);
        assert_eq!(
            peers[0].address,
            "allthecodes-daemon:http://127.0.0.1:19837"
        );
        assert!(peers[0].has_token);
        assert_eq!(target.name.as_deref(), Some("build-box"));
        assert_eq!(target.url, "http://127.0.0.1:19837");
        assert_eq!(target.token.as_deref(), Some("peer-secret"));
    }

    #[test]
    #[serial]
    fn remote_trigger_audit_appends_ndjson_without_token() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path().to_str().unwrap());

        let audit = append_remote_trigger_audit(
            "http://127.0.0.1:19836",
            true,
            202,
            &json!({ "accepted": true }),
        )
        .unwrap();
        let raw = fs::read_to_string(home.path().join("remote-trigger-audit.ndjson")).unwrap();

        assert!(raw.contains(&audit.audit_id));
        assert!(raw.contains("\"status\":202"));
        assert!(!raw.contains("secret"));
    }
}
