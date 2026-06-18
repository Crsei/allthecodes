//! `/remote` command -- local gateway status and control.

use std::fs;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{OnceLock, RwLock};

use allthecodes_gateway::{
    AdapterProvider, AdapterState, AdapterStatus, GatewayConfig, GatewayDiagnostic,
    GatewayPersistence, GatewayStore, OutputReadBatch, RunEvent, RunId, RunMeta, SessionKeyPolicy,
};
use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

use crate::{CommandContext, CommandHandler, CommandResult};

pub type RemoteFuture<T> =
    Pin<Box<dyn Future<Output = Result<T, GatewayDiagnostic>> + Send + 'static>>;

#[derive(Clone, Copy)]
pub struct RemoteGatewayAdapter {
    pub capabilities: fn() -> RemoteFuture<GatewayCapabilitiesSnapshot>,
    pub adapters: fn() -> RemoteFuture<Vec<AdapterStatus>>,
    pub connect_adapter: fn(AdapterProvider) -> RemoteFuture<AdapterStatus>,
    pub test_adapter_message: fn(AdapterProvider, String, String) -> RemoteFuture<AdapterStatus>,
    pub show_run: fn(RunId) -> RemoteFuture<RunMeta>,
    pub run_output: fn(RunId) -> RemoteFuture<OutputReadBatch>,
    pub run_timeline: fn(RunId) -> RemoteFuture<Vec<RunEvent>>,
    pub stop_run: fn(RunId) -> RemoteFuture<GatewayRunActionResponse>,
}

static REMOTE_GATEWAY_ADAPTER: OnceLock<RwLock<Option<RemoteGatewayAdapter>>> = OnceLock::new();

pub fn set_remote_gateway_adapter(adapter: RemoteGatewayAdapter) {
    let slot = REMOTE_GATEWAY_ADAPTER.get_or_init(|| RwLock::new(None));
    if let Ok(mut guard) = slot.write() {
        *guard = Some(adapter);
    }
}

#[derive(Debug, Clone)]
pub enum LocalGatewayDaemonStatus {
    Running {
        pid: u32,
        base_url: String,
        health_url: String,
    },
    Stale {
        pid: u32,
    },
    Stopped,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayCapabilitiesSnapshot {
    pub version: String,
    pub auth_mode: Value,
    pub supports_steer: bool,
    pub max_running: usize,
    pub max_queued: usize,
    pub endpoints: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayRunActionResponse {
    pub run_id: RunId,
    pub status: allthecodes_gateway::RunStatus,
    pub action: Value,
    #[serde(default)]
    pub diagnostic: Option<GatewayDiagnostic>,
}

#[derive(Debug, Clone)]
pub struct GatewayPaths {
    pub gateway_dir: PathBuf,
    pub runs_dir: PathBuf,
    pub adapters_dir: PathBuf,
    pub config_path: PathBuf,
    pub token_path: PathBuf,
}

pub struct RemoteHandler;

#[async_trait]
impl CommandHandler for RemoteHandler {
    async fn execute(&self, args: &str, _ctx: &mut CommandContext) -> Result<CommandResult> {
        let mut parts = args.split_whitespace();
        let sub = parts.next().unwrap_or("status").to_ascii_lowercase();
        let output = match sub.as_str() {
            "" | "status" => render_status().await,
            "adapters" => render_adapters().await,
            "connect" => connect_adapter(parts.next()).await,
            "test-message" => {
                let provider = parts.next();
                let target = parts.next();
                let text = parts.collect::<Vec<_>>().join(" ");
                test_message(provider, target, text).await
            }
            "runs" => render_runs(parse_limit(parts.collect())),
            "show" => show_run(parts.next()).await,
            "events" => show_events(parts.next(), parse_limit(parts.collect())).await,
            "timeline" => show_timeline(parts.next(), parse_limit(parts.collect())).await,
            "stop" => stop_run(parts.next()).await,
            "doctor" => render_doctor().await,
            "help" | "--help" | "-h" => usage(),
            other => format!("Unknown /remote subcommand '{}'.\n\n{}", other, usage()),
        };
        Ok(CommandResult::Output(redact_text(&output)))
    }
}

async fn render_status() -> String {
    let mut lines = vec!["Remote Gateway Status".to_string()];
    match daemon_status() {
        Ok(LocalGatewayDaemonStatus::Running {
            pid,
            base_url,
            health_url,
        }) => {
            lines.push(format!("Daemon: running (pid={pid})"));
            lines.push(format!("Gateway: {}", base_url));
            lines.push(format!("Health: {}", health_url));
            match remote_capabilities().await {
                Ok(cap) => {
                    lines.push(format!("Version: {}", cap.version));
                    lines.push(format!("Auth: {}", cap.auth_mode));
                    lines.push(format!("Steer: {}", cap.supports_steer));
                    lines.push(format!(
                        "Capacity: running={} queued={}",
                        cap.max_running, cap.max_queued
                    ));
                    lines.push(format!("Endpoints: {}", cap.endpoints.len()));
                }
                Err(diag) => lines.push(format_diagnostic("Capabilities", &diag)),
            }
        }
        Ok(LocalGatewayDaemonStatus::Stale { pid }) => {
            lines.push(format!("Daemon: stale (pid={pid})"));
            lines.push("Gateway: unavailable".to_string());
            lines.push("Action: restart the daemon with `claude daemon restart`.".to_string());
        }
        Ok(LocalGatewayDaemonStatus::Stopped) => {
            lines.push("Daemon: stopped".to_string());
            lines.push("Gateway: unavailable".to_string());
            lines.push(
                "Action: start the daemon with `FEATURE_KAIROS=1 claude daemon start`.".to_string(),
            );
        }
        Err(error) => lines.push(format!("Daemon: error ({})", error)),
    }
    lines.join("\n")
}

pub(crate) async fn render_adapters() -> String {
    match remote_adapters().await {
        Ok(adapters) => render_adapter_statuses(&adapters),
        Err(diag) => render_gateway_diagnostic("Adapters", &diag),
    }
}

async fn connect_adapter(provider: Option<&str>) -> String {
    let Some(provider) = parse_provider(provider) else {
        return "Usage: /remote connect <telegram|lark>".to_string();
    };
    match remote_connect_adapter(provider).await {
        Ok(status) => render_adapter_statuses(&[status]),
        Err(diag) => render_gateway_diagnostic("Connect", &diag),
    }
}

async fn test_message(provider: Option<&str>, target: Option<&str>, text: String) -> String {
    let Some(provider) = parse_provider(provider) else {
        return "Usage: /remote test-message <telegram|lark> <target> [text]".to_string();
    };
    let Some(target) = target.filter(|value| !value.trim().is_empty()) else {
        return "Usage: /remote test-message <telegram|lark> <target> [text]".to_string();
    };
    let text = if text.trim().is_empty() {
        "allthecodes remote gateway test".to_string()
    } else {
        text
    };
    match remote_test_adapter_message(provider, target.to_string(), text).await {
        Ok(status) => render_adapter_statuses(&[status]),
        Err(diag) => render_gateway_diagnostic("Test message", &diag),
    }
}

fn render_runs(limit: usize) -> String {
    match list_local_runs(limit) {
        Ok(runs) if runs.is_empty() => "Remote runs: none".to_string(),
        Ok(runs) => {
            let mut lines = vec![format!("Remote runs (latest {})", runs.len())];
            for run in runs {
                lines.push(format!(
                    "{}  status={:?}  session={}  updated={}",
                    run.run_id, run.status, run.session_key, run.updated_at_ms
                ));
            }
            lines.join("\n")
        }
        Err(error) => format!("Remote runs unavailable: {}", error),
    }
}

async fn show_run(raw_run_id: Option<&str>) -> String {
    let Some(run_id) = parse_run_id(raw_run_id) else {
        return "Usage: /remote show <run_id>".to_string();
    };
    match remote_show_run(run_id.clone()).await {
        Ok(meta) => render_run_meta(&meta),
        Err(_) => match load_local_run(&run_id) {
            Ok(meta) => render_run_meta(&meta),
            Err(diag) => render_gateway_diagnostic("Run", &diag),
        },
    }
}

async fn show_events(raw_run_id: Option<&str>, limit: usize) -> String {
    let Some(run_id) = parse_run_id(raw_run_id) else {
        return "Usage: /remote events <run_id> [--limit N]".to_string();
    };
    let output = match remote_run_output(run_id.clone()).await {
        Ok(output) => Ok(output),
        Err(_) => read_local_output(&run_id),
    };
    match output {
        Ok(output) => render_output_events(&output, limit),
        Err(diag) => render_gateway_diagnostic("Events", &diag),
    }
}

async fn show_timeline(raw_run_id: Option<&str>, limit: usize) -> String {
    let Some(run_id) = parse_run_id(raw_run_id) else {
        return "Usage: /remote timeline <run_id> [--limit N]".to_string();
    };
    let events = match remote_run_timeline(run_id.clone()).await {
        Ok(events) => Ok(events),
        Err(_) => read_local_timeline(&run_id),
    };
    match events {
        Ok(events) => render_timeline_events(&events, limit),
        Err(diag) => render_gateway_diagnostic("Timeline", &diag),
    }
}

async fn stop_run(raw_run_id: Option<&str>) -> String {
    let Some(run_id) = parse_run_id(raw_run_id) else {
        return "Usage: /remote stop <run_id>".to_string();
    };
    match remote_stop_run(run_id.clone()).await {
        Ok(response) => format!(
            "Stop requested for {}: status={:?} action={}{}",
            response.run_id,
            response.status,
            response.action,
            response
                .diagnostic
                .as_ref()
                .map(|diag| format!("\n{}", format_diagnostic("Diagnostic", diag)))
                .unwrap_or_default()
        ),
        Err(diag) => render_gateway_diagnostic("Stop", &diag),
    }
}

async fn render_doctor() -> String {
    let paths = gateway_paths();
    let mut lines = vec!["Remote Gateway Doctor".to_string()];
    lines.push(render_status().await);
    lines.push(format!("Gateway dir: {}", paths.gateway_dir.display()));
    lines.push(format!("Runs dir: {}", paths.runs_dir.display()));
    lines.push(format!("Adapters dir: {}", paths.adapters_dir.display()));
    lines.push(format!("Config path: {}", paths.config_path.display()));
    lines.push(format!(
        "Control token path: {}",
        paths.token_path.display()
    ));
    lines.join("\n")
}

fn render_adapter_statuses(adapters: &[AdapterStatus]) -> String {
    let mut lines = vec![format!("Remote adapters ({})", adapters.len())];
    for adapter in adapters {
        lines.push(format!(
            "{}  configured={}  state={}  message={}",
            adapter.provider.as_str(),
            adapter.configured,
            adapter_state(adapter.state),
            adapter.message
        ));
        if let Some(diag) = &adapter.diagnostic {
            lines.push(format_diagnostic("  Diagnostic", diag));
        }
    }
    lines.join("\n")
}

fn render_run_meta(meta: &allthecodes_gateway::RunMeta) -> String {
    let mut lines = vec![format!("Remote run {}", meta.run_id)];
    lines.push(format!("Status: {:?}", meta.status));
    lines.push(format!("Session: {}", meta.session_key));
    lines.push(format!("Created: {}", meta.created_at_ms));
    lines.push(format!("Updated: {}", meta.updated_at_ms));
    lines.push(format!("Source: {:?}", meta.request.source.transport));
    lines.push(format!("Thread: {}", meta.request.source.thread_id));
    lines.push(format!("Prompt: {}", first_line(&meta.request.prompt, 120)));
    lines.join("\n")
}

fn render_output_events(output: &OutputReadBatch, limit: usize) -> String {
    if output.events.is_empty() {
        return format!(
            "Remote run output: none (state={:?} next_seq={})",
            output.state, output.next_seq
        );
    }
    let start = output.events.len().saturating_sub(limit);
    let mut lines = vec![format!(
        "Remote run output (showing {}, state={:?}, next_seq={}, truncated={})",
        output.events.len() - start,
        output.state,
        output.next_seq,
        output.truncated
    )];
    for event in &output.events[start..] {
        lines.push(format!(
            "#{} {:?} {}",
            event.seq,
            event.stream,
            first_line(&event.chunk, 160)
        ));
    }
    lines.join("\n")
}

fn render_timeline_events(events: &[RunEvent], limit: usize) -> String {
    if events.is_empty() {
        return "Remote run timeline: none".to_string();
    }
    let start = events.len().saturating_sub(limit);
    let mut lines = vec![format!(
        "Remote run timeline (showing {})",
        events.len() - start
    )];
    for event in &events[start..] {
        lines.push(format!(
            "#{} {} {:?}",
            event.sequence, event.timestamp_ms, event.kind
        ));
    }
    lines.join("\n")
}

fn render_gateway_diagnostic(title: &str, diag: &GatewayDiagnostic) -> String {
    format!(
        "{} failed\n{}",
        title,
        format_diagnostic("Diagnostic", diag)
    )
}

fn format_diagnostic(label: &str, diag: &GatewayDiagnostic) -> String {
    let mut line = format!(
        "{}: code={} message={} action={}",
        label, diag.code, diag.message, diag.action
    );
    if let Some(context) = &diag.context {
        line.push_str(&format!(" context={}", context));
    }
    line
}

fn parse_provider(raw: Option<&str>) -> Option<AdapterProvider> {
    raw.and_then(|value| AdapterProvider::parse(&value.to_ascii_lowercase()).ok())
}

fn parse_run_id(raw: Option<&str>) -> Option<RunId> {
    raw.and_then(|value| RunId::from_string(value).ok())
}

fn parse_limit(args: Vec<&str>) -> usize {
    args.windows(2)
        .find(|pair| pair[0] == "--limit" || pair[0] == "-n")
        .and_then(|pair| pair[1].parse::<usize>().ok())
        .unwrap_or(20)
        .clamp(1, 100)
}

fn adapter_state(state: AdapterState) -> &'static str {
    match state {
        AdapterState::Unconfigured => "unconfigured",
        AdapterState::Connected => "connected",
        AdapterState::ConnectedOutboundOnly => "connected_outbound_only",
        AdapterState::Blocked => "blocked",
        AdapterState::Failed => "failed",
    }
}

fn first_line(value: &str, max: usize) -> String {
    let line = value.lines().next().unwrap_or(value).trim();
    if line.chars().count() <= max {
        return line.to_string();
    }
    format!("{}...", line.chars().take(max).collect::<String>())
}

fn daemon_status() -> Result<LocalGatewayDaemonStatus> {
    crate::runtime::remote_daemon_status().map_err(anyhow::Error::msg)
}

async fn remote_capabilities() -> Result<GatewayCapabilitiesSnapshot, GatewayDiagnostic> {
    let Some(adapter) = remote_gateway_adapter() else {
        return Err(gateway_unavailable("Capabilities"));
    };
    (adapter.capabilities)().await
}

async fn remote_adapters() -> Result<Vec<AdapterStatus>, GatewayDiagnostic> {
    let Some(adapter) = remote_gateway_adapter() else {
        return Err(gateway_unavailable("Adapters"));
    };
    (adapter.adapters)().await
}

async fn remote_connect_adapter(
    provider: AdapterProvider,
) -> Result<AdapterStatus, GatewayDiagnostic> {
    let Some(adapter) = remote_gateway_adapter() else {
        return Err(gateway_unavailable("Connect"));
    };
    (adapter.connect_adapter)(provider).await
}

async fn remote_test_adapter_message(
    provider: AdapterProvider,
    target: String,
    text: String,
) -> Result<AdapterStatus, GatewayDiagnostic> {
    let Some(adapter) = remote_gateway_adapter() else {
        return Err(gateway_unavailable("Test message"));
    };
    (adapter.test_adapter_message)(provider, target, text).await
}

async fn remote_show_run(run_id: RunId) -> Result<RunMeta, GatewayDiagnostic> {
    let Some(adapter) = remote_gateway_adapter() else {
        return Err(gateway_unavailable("Run"));
    };
    (adapter.show_run)(run_id).await
}

async fn remote_run_output(run_id: RunId) -> Result<OutputReadBatch, GatewayDiagnostic> {
    let Some(adapter) = remote_gateway_adapter() else {
        return Err(gateway_unavailable("Events"));
    };
    (adapter.run_output)(run_id).await
}

async fn remote_run_timeline(run_id: RunId) -> Result<Vec<RunEvent>, GatewayDiagnostic> {
    let Some(adapter) = remote_gateway_adapter() else {
        return Err(gateway_unavailable("Timeline"));
    };
    (adapter.run_timeline)(run_id).await
}

async fn remote_stop_run(run_id: RunId) -> Result<GatewayRunActionResponse, GatewayDiagnostic> {
    let Some(adapter) = remote_gateway_adapter() else {
        return Err(gateway_unavailable("Stop"));
    };
    (adapter.stop_run)(run_id).await
}

fn remote_gateway_adapter() -> Option<RemoteGatewayAdapter> {
    crate::runtime::ensure_runtime_installed();
    REMOTE_GATEWAY_ADAPTER
        .get()
        .and_then(|slot| slot.read().ok().and_then(|guard| *guard))
}

fn gateway_unavailable(title: &str) -> GatewayDiagnostic {
    GatewayDiagnostic::new(
        "gateway_adapter_unavailable",
        format!("{title} cannot reach the local gateway adapter."),
        "Install the remote gateway runtime adapter from the root process.",
    )
}

fn list_local_runs(limit: usize) -> Result<Vec<RunMeta>> {
    let persistence = GatewayPersistence::default();
    let runs_dir = persistence.runs_dir.clone();
    if !runs_dir.exists() {
        return Ok(Vec::new());
    }

    let store = GatewayStore::new(persistence, SessionKeyPolicy::default());
    let mut runs = Vec::new();
    for entry in
        fs::read_dir(&runs_dir).with_context(|| format!("failed to read {}", runs_dir.display()))?
    {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let Ok(run_id) = RunId::from_string(name) else {
            continue;
        };
        if let Ok(meta) = store.load_run(&run_id) {
            runs.push(meta);
        }
    }
    runs.sort_by(|left, right| right.updated_at_ms.cmp(&left.updated_at_ms));
    runs.truncate(limit);
    Ok(runs)
}

fn load_local_run(run_id: &RunId) -> Result<RunMeta, GatewayDiagnostic> {
    GatewayStore::new(GatewayPersistence::default(), SessionKeyPolicy::default())
        .load_run(run_id)
        .map_err(|error| error.into_diagnostic())
}

fn read_local_output(run_id: &RunId) -> Result<OutputReadBatch, GatewayDiagnostic> {
    GatewayStore::new(GatewayPersistence::default(), SessionKeyPolicy::default())
        .read_output_events(run_id, None, 64 * 1024)
        .map_err(|error| error.into_diagnostic())
}

fn read_local_timeline(run_id: &RunId) -> Result<Vec<RunEvent>, GatewayDiagnostic> {
    GatewayStore::new(GatewayPersistence::default(), SessionKeyPolicy::default())
        .read_timeline_events(run_id, None, 100)
        .map_err(|error| error.into_diagnostic())
}

fn gateway_paths() -> GatewayPaths {
    let config = GatewayConfig::default();
    GatewayPaths {
        gateway_dir: config.persistence.gateway_dir,
        runs_dir: config.persistence.runs_dir,
        adapters_dir: config.persistence.adapters_dir,
        config_path: GatewayConfig::default_config_path(),
        token_path: crate::runtime::remote_token_path(),
    }
}

fn redact_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut redact_next = false;
    for part in value.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }

        if redact_next {
            out.push_str("<redacted>");
            redact_next = false;
            continue;
        }

        let lower = part.to_ascii_lowercase();
        if lower == "bearer" {
            out.push_str(part);
            redact_next = true;
            continue;
        }

        if let Some(redacted) = redact_key_value(part) {
            out.push_str(&redacted);
        } else {
            out.push_str(part);
        }
    }
    out
}

fn redact_key_value(part: &str) -> Option<String> {
    let split_at = part.find('=').or_else(|| part.find(':'))?;
    let (key, rest) = part.split_at(split_at);
    if !is_sensitive_key(key) {
        return None;
    }

    let delimiter = rest.chars().next().unwrap_or('=');
    Some(format!("{key}{delimiter}<redacted>"))
}

fn is_sensitive_key(key: &str) -> bool {
    let normalized = key
        .trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_' && ch != '-')
        .to_ascii_lowercase()
        .replace('-', "_");
    normalized == "authorization"
        || normalized == "apikey"
        || normalized == "password"
        || normalized.contains("token")
        || normalized.contains("secret")
        || normalized.contains("credential")
        || normalized.contains("signature")
        || normalized.ends_with("_key")
}

fn usage() -> String {
    "Usage:\n  /remote status\n  /remote adapters\n  /remote connect <telegram|lark>\n  /remote test-message <telegram|lark> <target> [text]\n  /remote runs [--limit N]\n  /remote show <run_id>\n  /remote events <run_id> [--limit N]\n  /remote timeline <run_id> [--limit N]\n  /remote stop <run_id>\n  /remote doctor".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
    use allthecodes_gateway::{RemoteSource, RemoteTransport, RunPolicy, RunRequest};
    use std::path::PathBuf;

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &std::path::Path) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(previous) = &self.previous {
                std::env::set_var(self.key, previous);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    fn ctx() -> CommandContext {
        CommandContext {
            messages: Vec::new(),
            cwd: PathBuf::from("/test"),
            app_state: Default::default(),
            session_id: SessionId::from_string("remote-test"),
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn status_reports_stopped_without_token_leak() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        let mut ctx = ctx();
        let result = RemoteHandler.execute("status", &mut ctx).await.unwrap();

        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Daemon: stopped"));
                assert!(!text.to_ascii_lowercase().contains("bearer"));
                assert!(!text.contains("raw-token"));
            }
            _ => panic!("expected output"),
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn runs_lists_local_gateway_store() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        let store = allthecodes_gateway::GatewayStore::default_with_policy(
            allthecodes_gateway::SessionKeyPolicy::default(),
        );
        let created = store.create_run(request()).unwrap().meta().run_id.clone();

        let mut ctx = ctx();
        let result = RemoteHandler
            .execute("runs --limit 5", &mut ctx)
            .await
            .unwrap();

        match result {
            CommandResult::Output(text) => {
                assert!(text.contains(created.as_str()));
                assert!(!text.contains("raw-token"));
            }
            _ => panic!("expected output"),
        }
    }

    #[tokio::test]
    async fn unknown_subcommand_prints_usage() {
        let mut ctx = ctx();
        let result = RemoteHandler.execute("wat", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Unknown /remote"));
                assert!(text.contains("/remote status"));
            }
            _ => panic!("expected output"),
        }
    }

    fn request() -> RunRequest {
        RunRequest {
            prompt: "hello from /remote".to_string(),
            source: RemoteSource::new(
                RemoteTransport::Local,
                "local",
                "workspace",
                "remote-cmd-test",
                "user",
                "thread",
            )
            .with_metadata("token", "raw-token"),
            policy: RunPolicy::default(),
            idempotency_key: None,
        }
    }
}
