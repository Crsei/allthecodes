use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use allthecodes_types::kairos::{
    KairosAutomationSnapshot, KairosConfigScope, KairosControlAction, KairosControlRequest,
    KairosControlResult, KairosFeatureProfile, KairosFeatureProfilePatch, KairosLifecycleState,
    KairosLifecycleTransition, KairosRuntimeSnapshot, KairosSupervisorSnapshot,
    KairosWorkerSnapshot,
};
use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{operation_lock, readiness};

use super::paths::{daemon_dir, health_url, kairos_lifecycle_path};
use super::platform::{configure_detached, process_matches_record, process_start_key};
use super::storage::{
    cleanup_stale_state_before_start, ensure_daemon_dir, read_proactive_state, read_sleep_state,
    read_worker_states, request_shutdown, status_snapshot, write_stopped,
};
use super::types::{DaemonProcessState, DaemonStatusSnapshot, ProcessIdentityStatus};

const LIFECYCLE_SCHEMA_VERSION: u32 = 1;
const DEFAULT_DAEMON_PORT: u16 = 19836;
const SHUTDOWN_REQUEST_GRACE_PERIOD: Duration = Duration::from_secs(60);
const TERMINATE_GRACE_PERIOD: Duration = Duration::from_secs(5);
const FINAL_EXIT_GRACE_PERIOD: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Serialize, Deserialize)]
struct KairosLifecycleRecord {
    schema_version: u32,
    lifecycle: KairosLifecycleState,
    #[serde(default)]
    running_profile: Option<KairosFeatureProfile>,
    #[serde(default)]
    last_transition: Option<KairosLifecycleTransition>,
}

#[derive(Debug, Clone, Default)]
pub struct LocalKairosController;

impl LocalKairosController {
    pub fn snapshot(&self, cwd: &Path) -> Result<KairosRuntimeSnapshot> {
        kairos_snapshot(cwd)
    }

    pub fn configure(
        &self,
        cwd: &Path,
        scope: KairosConfigScope,
        patch: &KairosFeatureProfilePatch,
    ) -> Result<KairosRuntimeSnapshot> {
        configure_kairos(cwd, scope, patch)
    }

    pub fn control(&self, request: KairosControlRequest) -> Result<KairosControlResult> {
        let cwd = request
            .cwd
            .as_deref()
            .map(PathBuf::from)
            .map(Ok)
            .unwrap_or_else(std::env::current_dir)?;
        let operation = request.action;
        operation_lock::with_operation_lock(action_name(operation), &cwd, || {
            self.control_locked(&cwd, request)
        })
    }

    fn control_locked(
        &self,
        cwd: &Path,
        request: KairosControlRequest,
    ) -> Result<KairosControlResult> {
        let operation_id = std::env::var("ALLTHECODES_KAIROS_OPERATION_ID")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let changed = match request.action {
            KairosControlAction::Start | KairosControlAction::Reconcile => {
                let snapshot = kairos_snapshot(cwd)?;
                if matches!(snapshot.lifecycle, KairosLifecycleState::Ready) {
                    if snapshot.restart_required && request.action == KairosControlAction::Reconcile
                    {
                        stop_locked(cwd, &operation_id, KairosControlAction::Restart)?;
                        start_locked(cwd, &request, &operation_id, KairosControlAction::Restart)?;
                        true
                    } else {
                        false
                    }
                } else {
                    start_locked(cwd, &request, &operation_id, request.action)?;
                    true
                }
            }
            KairosControlAction::Stop => stop_locked(cwd, &operation_id, request.action)?,
            KairosControlAction::Restart => {
                let _ = stop_locked(cwd, &operation_id, request.action)?;
                start_locked(cwd, &request, &operation_id, request.action)?;
                true
            }
        };
        Ok(KairosControlResult {
            action: request.action,
            changed,
            operation_id: Some(operation_id),
            snapshot: kairos_snapshot(cwd)?,
        })
    }
}

pub fn configure_kairos(
    cwd: &Path,
    scope: KairosConfigScope,
    patch: &KairosFeatureProfilePatch,
) -> Result<KairosRuntimeSnapshot> {
    allthecodes_config::settings::update_kairos_settings(cwd, scope, patch)?;
    kairos_snapshot(cwd)
}

pub fn kairos_snapshot(cwd: &Path) -> Result<KairosRuntimeSnapshot> {
    let loaded = allthecodes_config::settings::load_effective(cwd)?;
    let resolution = allthecodes_config::features::resolve_loaded_kairos(&loaded);
    let lifecycle_record = read_lifecycle_record()?;
    let daemon = status_snapshot()?;
    let (lifecycle, supervisor) = match &daemon {
        DaemonStatusSnapshot::Running(state) => (
            lifecycle_record
                .as_ref()
                .map(|record| record.lifecycle)
                .filter(|state| {
                    matches!(
                        state,
                        KairosLifecycleState::Starting
                            | KairosLifecycleState::Restarting
                            | KairosLifecycleState::Ready
                    )
                })
                .unwrap_or(KairosLifecycleState::Ready),
            Some(KairosSupervisorSnapshot {
                pid: state.pid,
                health_url: state.health_url.clone(),
                started_at: Some(state.started_at.to_rfc3339()),
            }),
        ),
        DaemonStatusSnapshot::Stale(_) => (KairosLifecycleState::Stale, None),
        DaemonStatusSnapshot::Stopped => (
            lifecycle_record
                .as_ref()
                .map(|record| record.lifecycle)
                .filter(|state| matches!(state, KairosLifecycleState::Failed))
                .unwrap_or(KairosLifecycleState::Stopped),
            None,
        ),
    };
    let running = lifecycle_record
        .as_ref()
        .and_then(|record| record.running_profile.clone());
    let restart_required = running
        .as_ref()
        .is_some_and(|running| resolution.effective.differs_for_restart(running));
    let workers = read_worker_states()?
        .into_iter()
        .map(|worker| KairosWorkerSnapshot {
            worker_id: worker.worker_id,
            kind: worker.kind,
            pid: worker.pid,
            status: worker.status.as_str().to_string(),
            restart_count: worker.restart_count,
            updated_at: Some(worker.updated_at.to_rfc3339()),
        })
        .collect();
    let automation = automation_snapshot()?;

    Ok(KairosRuntimeSnapshot {
        desired: resolution.desired,
        effective: resolution.effective,
        running,
        sources: resolution.sources,
        diagnostics: resolution.diagnostics,
        lifecycle,
        restart_required,
        supervisor,
        workers,
        automation,
        last_transition: lifecycle_record.and_then(|record| record.last_transition),
        extensions: Default::default(),
    })
}

/// Spawn an independent CLI helper for lifecycle operations that would stop
/// the daemon currently serving the request. The helper acquires the same
/// controller lock and therefore preserves the normal lifecycle semantics.
pub fn spawn_control_helper(
    action: KairosControlAction,
    cwd: &Path,
    port: Option<u16>,
    operation_id: Option<&str>,
) -> Result<u32> {
    let subcommand = match action {
        KairosControlAction::Start | KairosControlAction::Reconcile => "start",
        KairosControlAction::Stop => "stop",
        KairosControlAction::Restart => "restart",
    };
    let exe = std::env::current_exe().context("failed to resolve current executable")?;
    ensure_daemon_dir()?;
    let log_path = daemon_dir().join("control-helper.log");
    let log_file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .with_context(|| format!("failed to open control helper log {}", log_path.display()))?;
    let log_file_err = log_file
        .try_clone()
        .with_context(|| format!("failed to clone control helper log {}", log_path.display()))?;
    let mut command = Command::new(exe);
    command
        .arg("daemon")
        .arg(subcommand)
        .current_dir(cwd)
        .env("ALLTHECODES_DAEMON_CONTROL_DELAY_MS", "250")
        .stdin(Stdio::null())
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(log_file_err));
    if let Some(port) = port {
        command.arg("--port").arg(port.to_string());
    }
    if let Some(operation_id) = operation_id {
        command.env("ALLTHECODES_KAIROS_OPERATION_ID", operation_id);
    }
    configure_detached(&mut command);
    let child = command
        .spawn()
        .with_context(|| format!("failed to spawn KAIROS {subcommand} helper"))?;
    Ok(child.id())
}

fn start_locked(
    cwd: &Path,
    request: &KairosControlRequest,
    operation_id: &str,
    action: KairosControlAction,
) -> Result<()> {
    cleanup_stale_state_before_start()?;
    if matches!(status_snapshot()?, DaemonStatusSnapshot::Running(_)) {
        return Ok(());
    }
    let snapshot = kairos_snapshot(cwd)?;
    if !snapshot.effective.enabled && !snapshot.effective.proactive {
        anyhow::bail!("KAIROS is disabled; enable KAIROS or standalone proactive first");
    }
    let transition_state = if action == KairosControlAction::Restart {
        KairosLifecycleState::Restarting
    } else {
        KairosLifecycleState::Starting
    };
    write_transition(
        operation_id,
        action,
        snapshot.lifecycle,
        transition_state,
        None,
        None,
        None,
    )?;

    let port = request.port.unwrap_or(DEFAULT_DAEMON_PORT);
    let exe = std::env::current_exe().context("failed to resolve current executable")?;
    let log_path = daemon_dir().join("supervisor.log");
    ensure_daemon_dir()?;
    let log_file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .with_context(|| format!("failed to open daemon log {}", log_path.display()))?;
    let log_file_err = log_file
        .try_clone()
        .with_context(|| format!("failed to clone daemon log {}", log_path.display()))?;
    let mut command = Command::new(exe);
    command
        .arg("--daemon")
        .arg("--port")
        .arg(port.to_string())
        .current_dir(cwd)
        .env_remove("ALLTHECODES_KAIROS_OPERATION_ID")
        .env_remove("ALLTHECODES_DAEMON_CONTROL_DELAY_MS")
        .stdin(Stdio::null())
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(log_file_err));
    configure_detached(&mut command);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            write_transition(
                operation_id,
                action,
                transition_state,
                KairosLifecycleState::Failed,
                None,
                Some("kairos_spawn_failed".to_string()),
                Some(error.to_string()),
            )?;
            return Err(error).context("failed to spawn daemon supervisor");
        }
    };
    let timeout = Duration::from_millis(
        request
            .readiness_timeout_ms
            .unwrap_or(readiness::DEFAULT_READY_TIMEOUT.as_millis() as u64),
    );
    if let Err(error) =
        readiness::wait_for_ready_with(port, timeout, readiness::DEFAULT_READY_POLL_INTERVAL)
    {
        let pid = child.id();
        let start_key = process_start_key(pid);
        let _ = super::platform::terminate_process_tree(pid, start_key.as_deref());
        let _ = child.wait();
        write_transition(
            operation_id,
            action,
            transition_state,
            KairosLifecycleState::Failed,
            None,
            Some("kairos_readiness_timeout".to_string()),
            Some(error.to_string()),
        )?;
        return Err(error.into());
    }
    write_transition(
        operation_id,
        action,
        transition_state,
        KairosLifecycleState::Ready,
        Some(snapshot.effective),
        None,
        Some(format!(
            "daemon ready: pid={} health={}",
            child.id(),
            health_url(port)
        )),
    )
}

fn stop_locked(cwd: &Path, operation_id: &str, action: KairosControlAction) -> Result<bool> {
    let DaemonStatusSnapshot::Running(state) = status_snapshot()? else {
        write_transition(
            operation_id,
            action,
            KairosLifecycleState::Stopped,
            KairosLifecycleState::Stopped,
            None,
            None,
            Some("daemon was already stopped".to_string()),
        )?;
        return Ok(false);
    };
    let from = read_lifecycle_record()?
        .map(|record| record.lifecycle)
        .unwrap_or(KairosLifecycleState::Ready);
    write_transition(
        operation_id,
        action,
        from,
        KairosLifecycleState::Stopping,
        None,
        None,
        None,
    )?;
    request_shutdown("KAIROS controller stop")?;
    if !wait_until_not_matching(&state, SHUTDOWN_REQUEST_GRACE_PERIOD)? {
        super::platform::send_soft_terminate(state.pid, state.process_start_key.as_deref())?;
        if !wait_until_not_matching(&state, TERMINATE_GRACE_PERIOD)? {
            super::platform::send_force_kill(state.pid, state.process_start_key.as_deref())?;
            if !wait_until_not_matching(&state, FINAL_EXIT_GRACE_PERIOD)? {
                anyhow::bail!(
                    "daemon pid={} still appears alive after force kill",
                    state.pid
                );
            }
        }
    }
    write_transition(
        operation_id,
        action,
        KairosLifecycleState::Stopping,
        KairosLifecycleState::Stopped,
        None,
        None,
        Some(format!("daemon stopped: pid={}", state.pid)),
    )?;
    let _ = cwd;
    Ok(true)
}

fn wait_until_not_matching(state: &DaemonProcessState, timeout: Duration) -> Result<bool> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let identity = process_matches_record(state.pid, state.process_start_key.as_deref());
        if !identity.is_current_process_record() {
            match identity {
                ProcessIdentityStatus::Dead => {
                    write_stopped(state.port, &state.cwd)?;
                }
                ProcessIdentityStatus::Mismatched { .. } => {}
                _ => {}
            }
            return Ok(true);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Ok(false)
}

fn automation_snapshot() -> Result<Option<KairosAutomationSnapshot>> {
    let proactive = read_proactive_state()?;
    let sleep = read_sleep_state()?;
    if proactive.is_none() && sleep.is_none() {
        return Ok(None);
    }
    Ok(Some(KairosAutomationSnapshot {
        status: if sleep.is_some() {
            "sleeping".to_string()
        } else {
            "standby".to_string()
        },
        proactive_active: proactive.as_ref().is_some_and(|state| state.active),
        query_running: false,
        pending_input: false,
        terminal_focus: false,
        next_tick_at: proactive
            .and_then(|state| state.next_tick_at)
            .map(|value| value.to_rfc3339()),
        sleeping_until: sleep
            .as_ref()
            .map(|state| state.sleeping_until.to_rfc3339()),
        reason: sleep.and_then(|state| state.reason),
    }))
}

fn read_lifecycle_record() -> Result<Option<KairosLifecycleRecord>> {
    let path = kairos_lifecycle_path();
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(&path)
        .with_context(|| format!("failed to read KAIROS lifecycle {}", path.display()))?;
    serde_json::from_slice(&bytes)
        .with_context(|| format!("failed to parse KAIROS lifecycle {}", path.display()))
        .map(Some)
}

#[allow(clippy::too_many_arguments)]
fn write_transition(
    operation_id: &str,
    action: KairosControlAction,
    from: KairosLifecycleState,
    to: KairosLifecycleState,
    running_profile: Option<KairosFeatureProfile>,
    error_code: Option<String>,
    message: Option<String>,
) -> Result<()> {
    let previous = read_lifecycle_record()?;
    let record = KairosLifecycleRecord {
        schema_version: LIFECYCLE_SCHEMA_VERSION,
        lifecycle: to,
        running_profile: running_profile.or_else(|| {
            previous
                .as_ref()
                .and_then(|record| record.running_profile.clone())
                .filter(|_| to != KairosLifecycleState::Stopped)
        }),
        last_transition: Some(KairosLifecycleTransition {
            operation_id: operation_id.to_string(),
            action,
            from,
            to,
            timestamp: Utc::now().to_rfc3339(),
            error_code,
            message,
        }),
    };
    super::atomic_write_json(&kairos_lifecycle_path(), &record)
}

fn action_name(action: KairosControlAction) -> &'static str {
    match action {
        KairosControlAction::Start => "kairos-start",
        KairosControlAction::Stop => "kairos-stop",
        KairosControlAction::Restart => "kairos-restart",
        KairosControlAction::Reconcile => "kairos-reconcile",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    struct EnvGuard(Option<String>);

    impl EnvGuard {
        fn set(path: &Path) -> Self {
            let previous = std::env::var("ALLTHECODES_HOME").ok();
            std::env::set_var("ALLTHECODES_HOME", path);
            Self(previous)
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.0 {
                Some(value) => std::env::set_var("ALLTHECODES_HOME", value),
                None => std::env::remove_var("ALLTHECODES_HOME"),
            }
        }
    }

    #[test]
    #[serial]
    fn stopped_snapshot_includes_persisted_profile_and_sources() {
        let home = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set(home.path());
        configure_kairos(
            workspace.path(),
            KairosConfigScope::Local,
            &KairosFeatureProfilePatch {
                enabled: Some(true),
                brief: Some(true),
                ..KairosFeatureProfilePatch::default()
            },
        )
        .unwrap();

        let snapshot = kairos_snapshot(workspace.path()).unwrap();
        assert_eq!(snapshot.lifecycle, KairosLifecycleState::Stopped);
        assert!(snapshot.desired.enabled);
        assert!(snapshot.effective.brief);
        assert_eq!(
            snapshot.sources.enabled,
            allthecodes_types::kairos::KairosValueSource::Local
        );
    }
}
