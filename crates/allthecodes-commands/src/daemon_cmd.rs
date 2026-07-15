//! `/daemon` command -- view/control the daemon process.
//!
//! Subcommands:
//! - `status` (default): show daemon URL and running state
//! - `stop`: request daemon shutdown
//! - `bridge`: list, resume, create, or release bridge sessions

use allthecodes_types::kairos::{
    KairosConfigScope, KairosControlAction, KairosControlRequest, KairosControlResult,
    KairosFeatureProfilePatch, KairosRuntimeSnapshot,
};
use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{OnceLock, RwLock};

use crate::{CommandContext, CommandHandler, CommandResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonWorkerSummary {
    pub worker_id: String,
    pub kind: String,
    pub pid: Option<u32>,
    pub status: String,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonProcessState {
    pub pid: u32,
    pub health_url: String,
    pub workers: Vec<DaemonWorkerSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DaemonStatusSnapshot {
    Running(DaemonProcessState),
    Stale(DaemonProcessState),
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonBridgeSessionSummary {
    pub session_id: String,
    pub workspace_key: String,
    pub cwd: PathBuf,
    pub account_id: Option<String>,
    pub profile: Option<String>,
    pub assistant_session_id: Option<String>,
    pub remote_session_key: Option<String>,
    pub last_run_id: Option<String>,
    pub last_ack_at: Option<DateTime<Utc>>,
    pub lease_owner: Option<String>,
    pub lease_expires_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Copy)]
pub struct DaemonCommandRuntime {
    pub status_snapshot: fn() -> Result<DaemonStatusSnapshot>,
    pub state_path: fn() -> PathBuf,
    pub request_shutdown: fn(&str) -> Result<()>,
    pub list_bridge_sessions: fn() -> Result<Vec<DaemonBridgeSessionSummary>>,
    pub get_bridge_session: fn(&str) -> Result<Option<DaemonBridgeSessionSummary>>,
    pub resume_bridge_session: fn(&str) -> Result<DaemonBridgeSessionSummary>,
    pub new_bridge_session: fn(&Path) -> Result<DaemonBridgeSessionSummary>,
    pub release_bridge_session: fn(&str) -> Result<Option<DaemonBridgeSessionSummary>>,
    pub kairos_snapshot: fn(&Path) -> Result<KairosRuntimeSnapshot>,
    pub configure_kairos:
        fn(&Path, KairosConfigScope, &KairosFeatureProfilePatch) -> Result<KairosRuntimeSnapshot>,
    pub control_kairos: fn(KairosControlRequest) -> KairosControlFuture,
}

pub type KairosControlFuture =
    Pin<Box<dyn Future<Output = Result<KairosControlResult>> + Send + 'static>>;

static DAEMON_RUNTIME: OnceLock<RwLock<Option<DaemonCommandRuntime>>> = OnceLock::new();

pub fn set_daemon_command_runtime(runtime: DaemonCommandRuntime) {
    let slot = DAEMON_RUNTIME.get_or_init(|| RwLock::new(None));
    if let Ok(mut guard) = slot.write() {
        *guard = Some(runtime);
    }
}

fn daemon_runtime() -> Result<DaemonCommandRuntime> {
    let Some(slot) = DAEMON_RUNTIME.get() else {
        anyhow::bail!(
            "Daemon command runtime is unavailable; install DaemonCommandRuntime adapter"
        );
    };
    let Ok(guard) = slot.read() else {
        anyhow::bail!("Daemon command runtime lock is poisoned");
    };
    guard.as_ref().copied().ok_or_else(|| {
        anyhow::anyhow!(
            "Daemon command runtime is unavailable; install DaemonCommandRuntime adapter"
        )
    })
}

pub struct DaemonCmdHandler;

#[async_trait]
impl CommandHandler for DaemonCmdHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let parts: Vec<&str> = args.split_whitespace().collect();
        match parts.as_slice() {
            [] => show_status(ctx),
            [cmd] if cmd.eq_ignore_ascii_case("status") => show_status(ctx),
            [cmd] if cmd.eq_ignore_ascii_case("stop") => {
                request_control(KairosControlAction::Stop, ctx).await
            }
            [cmd] if cmd.eq_ignore_ascii_case("start") => {
                request_control(KairosControlAction::Start, ctx).await
            }
            [cmd] if cmd.eq_ignore_ascii_case("restart") => {
                request_control(KairosControlAction::Restart, ctx).await
            }
            [cmd] if cmd.eq_ignore_ascii_case("bridge") => bridge_sessions(),
            [cmd, sub]
                if cmd.eq_ignore_ascii_case("bridge") && sub.eq_ignore_ascii_case("sessions") =>
            {
                bridge_sessions()
            }
            [cmd, sub]
                if cmd.eq_ignore_ascii_case("bridge") && sub.eq_ignore_ascii_case("status") =>
            {
                bridge_sessions()
            }
            [cmd, sub, session_id]
                if cmd.eq_ignore_ascii_case("bridge") && sub.eq_ignore_ascii_case("status") =>
            {
                bridge_status(session_id)
            }
            [cmd, sub, session_id]
                if cmd.eq_ignore_ascii_case("bridge") && sub.eq_ignore_ascii_case("resume") =>
            {
                bridge_resume(session_id)
            }
            [cmd, sub] if cmd.eq_ignore_ascii_case("bridge") && sub.eq_ignore_ascii_case("new") => {
                bridge_new(&ctx.cwd)
            }
            [cmd, sub, session_id]
                if cmd.eq_ignore_ascii_case("bridge") && sub.eq_ignore_ascii_case("release") =>
            {
                bridge_release(session_id)
            }
            _ => Ok(CommandResult::Output(format!(
                "Unknown subcommand: '{}'\n{}",
                args.trim(),
                daemon_usage()
            ))),
        }
    }
}

/// Show daemon status information.
fn show_status(_ctx: &CommandContext) -> Result<CommandResult> {
    let runtime = daemon_runtime()?;
    let output = match (runtime.status_snapshot)()? {
        DaemonStatusSnapshot::Running(state) => {
            let workers = if state.workers.is_empty() {
                "Workers:    0".to_string()
            } else {
                let mut lines = vec![format!("Workers:    {}", state.workers.len())];
                for worker in &state.workers {
                    let pid = worker
                        .pid
                        .map(|pid| pid.to_string())
                        .unwrap_or_else(|| "-".to_string());
                    lines.push(format!(
                        "  - {} kind={} pid={} status={}",
                        worker.worker_id, worker.kind, pid, worker.status
                    ));
                }
                lines.join("\n")
            };
            format!(
                "=== Daemon Status ===\n\
                 Running:    yes\n\
                 PID:        {}\n\
                 Health URL: {}\n\
                 State file: {}\n\
                 {}",
                state.pid,
                state.health_url,
                (runtime.state_path)().display(),
                workers
            )
        }
        DaemonStatusSnapshot::Stale(state) => format!(
            "=== Daemon Status ===\n\
             Running:    stale\n\
             Last PID:   {}\n\
             State file: {}",
            state.pid,
            (runtime.state_path)().display()
        ),
        DaemonStatusSnapshot::Stopped => format!(
            "=== Daemon Status ===\n\
             Running:    no\n\
             State file: {}",
            (runtime.state_path)().display()
        ),
    };
    Ok(CommandResult::Output(output))
}

async fn request_control(
    action: KairosControlAction,
    ctx: &CommandContext,
) -> Result<CommandResult> {
    let runtime = daemon_runtime()?;
    let result = (runtime.control_kairos)(KairosControlRequest {
        action,
        cwd: Some(ctx.cwd.display().to_string()),
        ..KairosControlRequest::default()
    })
    .await?;
    Ok(CommandResult::Output(format_control_result(&result)))
}

pub fn kairos_snapshot(cwd: &Path) -> Result<KairosRuntimeSnapshot> {
    let runtime = daemon_runtime()?;
    (runtime.kairos_snapshot)(cwd)
}

pub fn configure_kairos(
    cwd: &Path,
    scope: KairosConfigScope,
    patch: &KairosFeatureProfilePatch,
) -> Result<KairosRuntimeSnapshot> {
    let runtime = daemon_runtime()?;
    (runtime.configure_kairos)(cwd, scope, patch)
}

pub async fn control_kairos(request: KairosControlRequest) -> Result<KairosControlResult> {
    let runtime = daemon_runtime()?;
    (runtime.control_kairos)(request).await
}

pub fn format_kairos_snapshot(snapshot: &KairosRuntimeSnapshot) -> String {
    let mut lines = vec![
        "=== KAIROS Status ===".to_string(),
        format!("Lifecycle:       {:?}", snapshot.lifecycle),
        format!("Desired:         {}", profile_summary(&snapshot.desired)),
        format!("Effective:       {}", profile_summary(&snapshot.effective)),
        format!(
            "Running:         {}",
            snapshot
                .running
                .as_ref()
                .map(profile_summary)
                .unwrap_or_else(|| "none".to_string())
        ),
        format!("Restart needed:  {}", snapshot.restart_required),
        format!("Workers:         {}", snapshot.workers.len()),
        format!(
            "Sources:         enabled={:?} brief={:?} channels={:?} push={:?} github={:?} proactive={:?}",
            snapshot.sources.enabled,
            snapshot.sources.brief,
            snapshot.sources.channels,
            snapshot.sources.push_notifications,
            snapshot.sources.github_webhooks,
            snapshot.sources.proactive
        ),
    ];
    if let Some(supervisor) = &snapshot.supervisor {
        lines.push(format!("Supervisor:      pid={}", supervisor.pid));
        lines.push(format!("Health:          {}", supervisor.health_url));
    }
    if let Some(transition) = &snapshot.last_transition {
        lines.push(format!(
            "Last transition: {:?}->{:?}",
            transition.from, transition.to
        ));
        if let Some(message) = &transition.message {
            lines.push(format!("Message:         {message}"));
        }
    }
    for diagnostic in &snapshot.diagnostics {
        lines.push(format!(
            "Diagnostic:      {}: {}",
            diagnostic.code, diagnostic.message
        ));
    }
    lines.join("\n")
}

fn format_control_result(result: &KairosControlResult) -> String {
    format!(
        "KAIROS {:?}: {}\n{}",
        result.action,
        if result.changed {
            "changed"
        } else {
            "unchanged"
        },
        format_kairos_snapshot(&result.snapshot)
    )
}

fn profile_summary(profile: &allthecodes_types::kairos::KairosFeatureProfile) -> String {
    format!(
        "enabled={} brief={} channels={} push={} github={} proactive={}",
        profile.enabled,
        profile.brief,
        profile.channels,
        profile.push_notifications,
        profile.github_webhooks,
        profile.proactive
    )
}

fn bridge_sessions() -> Result<CommandResult> {
    let runtime = daemon_runtime()?;
    let sessions = (runtime.list_bridge_sessions)()?;
    Ok(CommandResult::Output(format_bridge_session_list(&sessions)))
}

fn bridge_status(session_id: &str) -> Result<CommandResult> {
    let runtime = daemon_runtime()?;
    let output = match (runtime.get_bridge_session)(session_id)? {
        Some(session) => format_bridge_session_detail(&session),
        None => format!("Bridge session not found: {session_id}"),
    };
    Ok(CommandResult::Output(output))
}

fn bridge_resume(session_id: &str) -> Result<CommandResult> {
    let runtime = daemon_runtime()?;
    let session = (runtime.resume_bridge_session)(session_id)?;
    Ok(CommandResult::Output(format!(
        "Bridge session resumed: {}\n{}",
        session.session_id,
        format_bridge_session_detail(&session)
    )))
}

fn bridge_new(cwd: &Path) -> Result<CommandResult> {
    let runtime = daemon_runtime()?;
    let session = (runtime.new_bridge_session)(cwd)?;
    Ok(CommandResult::Output(format!(
        "Bridge session created: {}\n{}",
        session.session_id,
        format_bridge_session_detail(&session)
    )))
}

fn bridge_release(session_id: &str) -> Result<CommandResult> {
    let runtime = daemon_runtime()?;
    let output = match (runtime.release_bridge_session)(session_id)? {
        Some(session) => format!(
            "Bridge session released: {}\n{}",
            session.session_id,
            format_bridge_session_detail(&session)
        ),
        None => format!("Bridge session not found: {session_id}"),
    };
    Ok(CommandResult::Output(output))
}

fn format_bridge_session_list(sessions: &[DaemonBridgeSessionSummary]) -> String {
    if sessions.is_empty() {
        return "=== Bridge Sessions ===\nnone".to_string();
    }
    let mut lines = vec![format!(
        "=== Bridge Sessions ===\nCount: {}",
        sessions.len()
    )];
    for session in sessions {
        lines.push(format!(
            "  - {} cwd={} assistant={} remote={} run={} lease={}",
            session.session_id,
            session.cwd.display(),
            optional_value(session.assistant_session_id.as_deref()),
            optional_value(session.remote_session_key.as_deref()),
            optional_value(session.last_run_id.as_deref()),
            lease_summary(session),
        ));
    }
    lines.join("\n")
}

fn format_bridge_session_detail(session: &DaemonBridgeSessionSummary) -> String {
    format!(
        "=== Bridge Session ===\n\
         Session:     {}\n\
         Workspace:   {}\n\
         CWD:         {}\n\
         Account:     {}\n\
         Profile:     {}\n\
         Assistant:   {}\n\
         Remote:      {}\n\
         Last run:    {}\n\
         Last ack:    {}\n\
         Lease:       {}",
        session.session_id,
        session.workspace_key,
        session.cwd.display(),
        optional_value(session.account_id.as_deref()),
        optional_value(session.profile.as_deref()),
        optional_value(session.assistant_session_id.as_deref()),
        optional_value(session.remote_session_key.as_deref()),
        optional_value(session.last_run_id.as_deref()),
        session
            .last_ack_at
            .map(|value| value.to_rfc3339())
            .unwrap_or_else(|| "-".to_string()),
        lease_summary(session),
    )
}

fn lease_summary(session: &DaemonBridgeSessionSummary) -> String {
    match (&session.lease_owner, session.lease_expires_at) {
        (Some(owner), Some(expires_at)) => {
            format!("owner={} expires={}", owner, expires_at.to_rfc3339())
        }
        (Some(owner), None) => format!("owner={owner}"),
        (None, _) => "none".to_string(),
    }
}

fn optional_value(value: Option<&str>) -> &str {
    value.filter(|value| !value.is_empty()).unwrap_or("-")
}

fn daemon_usage() -> &'static str {
    "Usage:\n  \
       /daemon                         -- show daemon status\n  \
       /daemon status                  -- show daemon status\n  \
       /daemon stop                    -- stop the daemon\n  \
       /daemon start                   -- start the daemon and wait for readiness\n  \
       /daemon restart                 -- restart the daemon and wait for readiness\n  \
       /daemon bridge sessions         -- list bridge sessions\n  \
       /daemon bridge status [id]      -- show bridge session status\n  \
       /daemon bridge resume <id>      -- refresh a bridge session lease\n  \
       /daemon bridge new              -- create a new bridge session\n  \
       /daemon bridge release <id>     -- release a bridge session lease"
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
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

    fn install_test_runtime() {
        set_daemon_command_runtime(DaemonCommandRuntime {
            status_snapshot: || Ok(DaemonStatusSnapshot::Stopped),
            state_path: || allthecodes_config::paths::daemon_dir().join("supervisor.json"),
            request_shutdown: |_| Ok(()),
            list_bridge_sessions: test_list_bridge_sessions,
            get_bridge_session: test_get_bridge_session,
            resume_bridge_session: test_resume_bridge_session,
            new_bridge_session: test_new_bridge_session,
            release_bridge_session: test_release_bridge_session,
            kairos_snapshot: |_| Ok(test_kairos_snapshot()),
            configure_kairos: |_, _, _| Ok(test_kairos_snapshot()),
            control_kairos: |request| {
                Box::pin(async move {
                    Ok(KairosControlResult {
                        action: request.action,
                        changed: true,
                        operation_id: Some("test-operation".to_string()),
                        snapshot: test_kairos_snapshot(),
                    })
                })
            },
        });
    }

    fn test_kairos_snapshot() -> KairosRuntimeSnapshot {
        KairosRuntimeSnapshot {
            lifecycle: allthecodes_types::kairos::KairosLifecycleState::Ready,
            desired: allthecodes_types::kairos::KairosFeatureProfile {
                enabled: true,
                proactive: true,
                ..Default::default()
            },
            effective: allthecodes_types::kairos::KairosFeatureProfile {
                enabled: true,
                proactive: true,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn test_bridge_summary(session_id: &str, cwd: PathBuf) -> DaemonBridgeSessionSummary {
        DaemonBridgeSessionSummary {
            session_id: session_id.to_string(),
            workspace_key: format!("cwd={}|account=acct_1|profile=default", cwd.display()),
            cwd,
            account_id: Some("acct_1".to_string()),
            profile: Some("default".to_string()),
            assistant_session_id: Some("assistant-session-1".to_string()),
            remote_session_key: Some("remote:http:abc".to_string()),
            last_run_id: Some("run_bridge123".to_string()),
            last_ack_at: Some(Utc::now()),
            lease_owner: Some("owner-a".to_string()),
            lease_expires_at: Some(Utc::now() + chrono::Duration::seconds(30)),
        }
    }

    fn test_list_bridge_sessions() -> Result<Vec<DaemonBridgeSessionSummary>> {
        Ok(vec![test_bridge_summary(
            "bridge-session-1",
            PathBuf::from("/workspace"),
        )])
    }

    fn test_get_bridge_session(session_id: &str) -> Result<Option<DaemonBridgeSessionSummary>> {
        Ok((session_id == "bridge-session-1")
            .then(|| test_bridge_summary(session_id, PathBuf::from("/workspace"))))
    }

    fn test_resume_bridge_session(session_id: &str) -> Result<DaemonBridgeSessionSummary> {
        Ok(test_bridge_summary(session_id, PathBuf::from("/workspace")))
    }

    fn test_new_bridge_session(cwd: &Path) -> Result<DaemonBridgeSessionSummary> {
        Ok(test_bridge_summary("new-bridge-session", cwd.to_path_buf()))
    }

    fn test_release_bridge_session(session_id: &str) -> Result<Option<DaemonBridgeSessionSummary>> {
        let mut summary = test_bridge_summary(session_id, PathBuf::from("/workspace"));
        summary.lease_owner = None;
        summary.lease_expires_at = None;
        Ok(Some(summary))
    }

    fn test_ctx() -> CommandContext {
        CommandContext {
            messages: Vec::new(),
            cwd: PathBuf::from("/test"),
            app_state: Default::default(),
            session_id: SessionId::from_string("test-session"),
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_status_without_state_reports_not_running() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        install_test_runtime();
        let handler = DaemonCmdHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("status", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => assert!(text.contains("Running:    no")),
            _ => panic!("Expected Output"),
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_start_and_restart_use_typed_controller() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        install_test_runtime();
        let handler = DaemonCmdHandler;
        let mut ctx = test_ctx();

        for input in &["start", "restart"] {
            let result = handler.execute(input, &mut ctx).await.unwrap();
            match result {
                CommandResult::Output(text) => {
                    assert!(
                        text.contains("KAIROS"),
                        "missing control result for {input}"
                    );
                    assert!(text.contains("Lifecycle:       Ready"));
                }
                _ => panic!("Expected Output for input '{}'", input),
            }
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn daemon_bridge_sessions_lists_runtime_sessions() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        install_test_runtime();
        let handler = DaemonCmdHandler;
        let mut ctx = test_ctx();

        let result = handler.execute("bridge sessions", &mut ctx).await.unwrap();

        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Bridge Sessions"));
                assert!(text.contains("bridge-session-1"));
                assert!(text.contains("assistant-session-1"));
                assert!(text.contains("remote:http:abc"));
            }
            _ => panic!("Expected Output"),
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn daemon_bridge_resume_uses_runtime() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        install_test_runtime();
        let handler = DaemonCmdHandler;
        let mut ctx = test_ctx();

        let result = handler
            .execute("bridge resume bridge-session-1", &mut ctx)
            .await
            .unwrap();

        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Bridge session resumed: bridge-session-1"));
                assert!(text.contains("run_bridge123"));
            }
            _ => panic!("Expected Output"),
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn daemon_bridge_new_uses_context_cwd() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        install_test_runtime();
        let handler = DaemonCmdHandler;
        let mut ctx = test_ctx();

        let result = handler.execute("bridge new", &mut ctx).await.unwrap();

        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Bridge session created: new-bridge-session"));
                assert!(text.contains("/test"));
            }
            _ => panic!("Expected Output"),
        }
    }
}
