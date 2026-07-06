use std::fs;
use std::path::Path;

use allthecodes_config::features::{self, FeatureFlags};
use chrono::Utc;

use super::management::{daemon_start_feature_enabled, parse_port};
use super::paths::health_url;
use super::storage::{ensure_daemon_dir, write_state};
use super::types::{DEFAULT_DAEMON_PORT, SCHEMA_VERSION};
use super::*;
use serial_test::serial;

struct EnvGuard {
    key: &'static str,
    previous: Option<String>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &Path) -> Self {
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

struct FeatureOverrideGuard(Option<FeatureFlags>);

impl FeatureOverrideGuard {
    fn set(flags: FeatureFlags) -> Self {
        let previous = features::runtime_override();
        features::set_runtime_override(flags);
        Self(previous)
    }
}

impl Drop for FeatureOverrideGuard {
    fn drop(&mut self) {
        match self.0.take() {
            Some(flags) => features::set_runtime_override(flags),
            None => features::clear_runtime_override(),
        }
    }
}

fn bridge_state(
    session_id: &str,
    cwd: &Path,
    workspace_key: &str,
    updated_at: chrono::DateTime<Utc>,
) -> DaemonBridgeSessionState {
    DaemonBridgeSessionState {
        schema_version: SCHEMA_VERSION,
        session_id: session_id.to_string(),
        account_id: Some("acct_1".to_string()),
        profile: Some("default".to_string()),
        cwd: cwd.to_path_buf(),
        assistant_worker_id: "assistant-session".to_string(),
        last_poll_cursor: None,
        last_ack_at: None,
        updated_at,
        workspace_key: workspace_key.to_string(),
        terminal_id: None,
        remote_session_key: None,
        assistant_session_id: None,
        last_run_id: None,
        lease_owner: None,
        lease_expires_at: None,
    }
}

#[test]
#[serial]
fn write_and_read_state_uses_allthecodes_home() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
    let cwd = temp.path().join("workspace");
    fs::create_dir_all(&cwd).unwrap();

    let state = write_started(19999, &cwd).unwrap();
    assert!(state_path().starts_with(temp.path().join("daemon")));
    let read_back = read_state().unwrap().unwrap();

    assert_eq!(state.pid, std::process::id());
    assert_eq!(read_back.port, 19999);
    assert_eq!(read_back.cwd, cwd);
    assert_eq!(read_back.schema_version, SCHEMA_VERSION);
    assert_eq!(read_back.command_kind.as_deref(), Some("daemon-supervisor"));
    assert_eq!(
        read_back.binary_version.as_deref(),
        Some(env!("CARGO_PKG_VERSION"))
    );
    assert!(read_back.binary_path.is_some());
    assert!(read_back.log_path.is_some());
    assert!(read_back.ready_url.is_some());
    assert!(read_back.process_start_key.is_some());
    assert!(state_path().starts_with(temp.path()));
}

#[test]
#[serial]
fn read_state_accepts_v1_json_without_process_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
    let cwd = temp.path().join("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let now = Utc::now();
    atomic_write_json(
        &state_path(),
        &serde_json::json!({
            "schema_version": 1,
            "status": "running",
            "pid": std::process::id(),
            "cwd": cwd,
            "port": DEFAULT_DAEMON_PORT,
            "health_url": health_url(DEFAULT_DAEMON_PORT),
            "started_at": now,
            "updated_at": now,
            "shutdown_requested": false,
            "workers": [],
        }),
    )
    .unwrap();

    let read_back = read_state().unwrap().unwrap();
    assert_eq!(read_back.cwd, temp.path().join("workspace"));
    assert_eq!(read_back.port, DEFAULT_DAEMON_PORT);
    assert_eq!(read_back.schema_version, 1);
    assert!(read_back.process_start_key.is_none());
}

#[cfg(feature = "sqlite-storage")]
#[test]
#[serial]
fn daemon_state_falls_back_to_json_when_sqlite_path_is_blocked() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
    fs::create_dir_all(temp.path().join("state").join("state_5.sqlite")).unwrap();

    let token = write_control_token().unwrap();
    assert!(control_token_path().exists());
    assert_eq!(read_control_token().unwrap().unwrap().token, token.token);
}

#[test]
#[serial]
fn shutdown_request_sets_state_flag() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
    write_started(DEFAULT_DAEMON_PORT, temp.path()).unwrap();

    request_shutdown("test").unwrap();
    let state = read_state().unwrap().unwrap();

    assert!(shutdown_requested());
    assert!(state.shutdown_requested);
}

#[test]
fn current_process_is_alive() {
    assert!(process_is_alive(std::process::id()));
    assert!(super::platform::process_start_key(std::process::id()).is_some());
}

#[test]
fn parses_port_from_management_args() {
    let args = vec![
        "daemon".to_string(),
        "start".to_string(),
        "--port".to_string(),
        "20100".to_string(),
    ];
    assert_eq!(parse_port(&args), Some(20100));
}

#[test]
#[serial]
fn daemon_start_gate_accepts_standalone_proactive_feature() {
    let _features = FeatureOverrideGuard::set(FeatureFlags {
        proactive: true,
        ..FeatureFlags::all_disabled()
    });

    assert!(daemon_start_feature_enabled());
}

#[test]
#[serial]
fn worker_state_updates_supervisor_summary() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
    let cwd = temp.path().join("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let log_path = worker_log_path("assistant/session:1");

    write_worker_running(
        "assistant/session:1",
        "assistant-session",
        std::process::id(),
        &cwd,
        &log_path,
        2,
        true,
    )
    .unwrap();
    write_supervisor_heartbeat(DEFAULT_DAEMON_PORT, &cwd).unwrap();

    let state = read_state().unwrap().unwrap();
    assert_eq!(state.workers.len(), 1);
    assert_eq!(state.workers[0].worker_id, "assistant/session:1");
    assert_eq!(state.workers[0].status, "running");
    assert!(worker_state_path("assistant/session:1").ends_with("assistant_session_1.json"));
}

#[test]
#[serial]
fn worker_heartbeat_preserves_restart_count() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
    let log_path = worker_log_path("assistant-session-1");
    write_worker_running(
        "assistant-session-1",
        "assistant-session",
        std::process::id(),
        temp.path(),
        &log_path,
        3,
        true,
    )
    .unwrap();

    write_worker_heartbeat("assistant-session-1").unwrap();
    let state = read_worker_state("assistant-session-1").unwrap().unwrap();

    assert_eq!(state.restart_count, 3);
    assert_eq!(state.status, DaemonWorkerStatus::Running);
    assert!(state.last_heartbeat_at.is_some());
}

#[test]
#[serial]
fn control_token_is_created_and_cleared_with_daemon_state() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());

    write_started(DEFAULT_DAEMON_PORT, temp.path()).unwrap();
    let token = read_control_token().unwrap().unwrap();
    assert!(verify_control_token(&token.token).unwrap());
    assert!(!verify_control_token("wrong").unwrap());

    write_stopped(DEFAULT_DAEMON_PORT, temp.path()).unwrap();
    assert!(read_control_token().unwrap().is_none());
}

#[test]
#[serial]
fn sleep_state_tracks_active_and_expired_sleep() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());

    let active = write_sleep_state(60, "waiting").unwrap();
    assert_eq!(active.reason.as_deref(), Some("waiting"));
    assert!(active_sleep_state().unwrap().is_some());

    write_sleep_state_until(Utc::now() - chrono::Duration::seconds(1), "expired").unwrap();
    assert!(active_sleep_state().unwrap().is_none());
    assert!(read_sleep_state().unwrap().is_none());
}

fn dead_test_pid() -> u32 {
    (1_000_000u32..4_194_303u32)
        .rev()
        .find(|pid| !process_is_alive(*pid))
        .expect("dead test pid")
}

#[test]
#[serial]
fn stale_cleanup_removes_dead_supervisor_state() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
    let cwd = temp.path().join("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let now = Utc::now();
    let state = DaemonProcessState {
        schema_version: SCHEMA_VERSION,
        status: DaemonRunStatus::Running,
        pid: dead_test_pid(),
        cwd: cwd.clone(),
        port: DEFAULT_DAEMON_PORT,
        health_url: health_url(DEFAULT_DAEMON_PORT),
        command_kind: None,
        binary_version: None,
        binary_path: None,
        log_path: None,
        ready_url: None,
        process_start_key: None,
        started_at: now,
        updated_at: now,
        shutdown_requested: true,
        workers: Vec::new(),
    };
    write_state(&state).unwrap();
    write_control_token().unwrap();
    request_shutdown("test").unwrap();
    write_sleep_state_until(Utc::now() - chrono::Duration::seconds(1), "expired").unwrap();

    let report = cleanup_stale_state_before_start().unwrap();

    assert_eq!(report.supervisor_pid, Some(dead_test_pid()));
    assert!(report.supervisor_state_removed);
    assert!(report.control_token_removed);
    assert!(report.shutdown_request_removed);
    assert!(report.expired_sleep_state_removed);
    assert!(!state_path().exists());
    assert!(!control_token_path().exists());
    assert!(!shutdown_request_path().exists());
    assert!(!sleep_state_path().exists());
}

#[test]
#[serial]
fn stale_cleanup_preserves_live_supervisor_state() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
    write_started(DEFAULT_DAEMON_PORT, temp.path()).unwrap();

    let report = cleanup_stale_state_before_start().unwrap();

    assert_eq!(report, StaleStateCleanupReport::default());
    assert!(state_path().exists());
    assert!(control_token_path().exists());
}

#[test]
#[serial]
fn status_snapshot_marks_dead_supervisor_stale() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
    let now = Utc::now();
    write_state(&DaemonProcessState {
        schema_version: SCHEMA_VERSION,
        status: DaemonRunStatus::Running,
        pid: dead_test_pid(),
        cwd: temp.path().to_path_buf(),
        port: DEFAULT_DAEMON_PORT,
        health_url: health_url(DEFAULT_DAEMON_PORT),
        command_kind: None,
        binary_version: None,
        binary_path: None,
        log_path: None,
        ready_url: None,
        process_start_key: None,
        started_at: now,
        updated_at: now,
        shutdown_requested: false,
        workers: Vec::new(),
    })
    .unwrap();

    match status_snapshot().unwrap() {
        DaemonStatusSnapshot::Stale(state) => assert_eq!(state.pid, dead_test_pid()),
        other => panic!("expected stale snapshot, got {other:?}"),
    }
}

#[test]
#[serial]
fn status_snapshot_marks_reused_pid_stale() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
    let now = Utc::now();
    write_state(&DaemonProcessState {
        schema_version: SCHEMA_VERSION,
        status: DaemonRunStatus::Running,
        pid: std::process::id(),
        cwd: temp.path().to_path_buf(),
        port: DEFAULT_DAEMON_PORT,
        health_url: health_url(DEFAULT_DAEMON_PORT),
        command_kind: None,
        binary_version: None,
        binary_path: None,
        log_path: None,
        ready_url: None,
        process_start_key: Some("definitely-not-this-process".to_string()),
        started_at: now,
        updated_at: now,
        shutdown_requested: false,
        workers: Vec::new(),
    })
    .unwrap();

    match status_snapshot().unwrap() {
        DaemonStatusSnapshot::Stale(state) => assert_eq!(state.pid, std::process::id()),
        other => panic!("expected stale snapshot, got {other:?}"),
    }
}

#[test]
fn terminate_refuses_mismatched_identity() {
    let error = terminate_process_tree(std::process::id(), Some("definitely-not-this-process"))
        .expect_err("identity mismatch should fail before signaling");
    assert!(error.to_string().contains("identity mismatch"));
}

#[test]
#[serial]
fn tail_log_limits_bytes_and_rejects_outside_daemon_dir() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
    ensure_daemon_dir().unwrap();
    let log = daemon_dir().join("supervisor.log");
    fs::write(&log, "0123456789abcdef").unwrap();

    assert_eq!(tail_log(&log, Some(4)).unwrap(), "cdef");

    let outside = temp.path().join("outside.log");
    fs::write(&outside, "secret").unwrap();
    let error = tail_log(&outside, Some(16)).expect_err("outside path rejected");
    assert!(error.to_string().contains("outside daemon dir"));
}

#[test]
#[serial]
fn stale_cleanup_removes_dead_worker_and_retains_live_worker() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
    let cwd = temp.path().join("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let now = Utc::now();
    write_state(&DaemonProcessState {
        schema_version: SCHEMA_VERSION,
        status: DaemonRunStatus::Running,
        pid: dead_test_pid(),
        cwd: cwd.clone(),
        port: DEFAULT_DAEMON_PORT,
        health_url: health_url(DEFAULT_DAEMON_PORT),
        command_kind: None,
        binary_version: None,
        binary_path: None,
        log_path: None,
        ready_url: None,
        process_start_key: None,
        started_at: now,
        updated_at: now,
        shutdown_requested: false,
        workers: Vec::new(),
    })
    .unwrap();
    write_worker_running(
        "live-worker",
        "assistant-session",
        std::process::id(),
        &cwd,
        &worker_log_path("live-worker"),
        0,
        true,
    )
    .unwrap();
    write_worker_running(
        "dead-worker",
        "assistant-session",
        dead_test_pid(),
        &cwd,
        &worker_log_path("dead-worker"),
        0,
        true,
    )
    .unwrap();
    let empty_path = worker_state_path("empty-worker");
    fs::write(&empty_path, "").unwrap();

    let report = cleanup_stale_state_before_start().unwrap();

    assert!(worker_state_path("live-worker").exists());
    assert!(!worker_state_path("dead-worker").exists());
    assert!(!empty_path.exists());
    assert_eq!(report.live_worker_states_retained.len(), 1);
    assert_eq!(
        report.live_worker_states_retained[0].worker_id,
        "live-worker"
    );
    assert_eq!(report.worker_states_removed.len(), 2);
}

#[test]
#[serial]
fn bridge_session_v1_migrates_to_v2_with_workspace_key() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
    let cwd = temp.path().join("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let now = Utc::now();

    let raw = serde_json::json!({
        "schema_version": 1,
        "session_id": "legacy-session",
        "account_id": "acct_1",
        "profile": "default",
        "cwd": cwd,
        "assistant_worker_id": "assistant-session",
        "last_poll_cursor": "cursor-1",
        "last_ack_at": now,
        "updated_at": now,
    });

    let migrated = migrate_bridge_session_state(raw).unwrap();

    assert_eq!(migrated.schema_version, SCHEMA_VERSION);
    assert_eq!(migrated.session_id, "legacy-session");
    assert_eq!(migrated.last_poll_cursor.as_deref(), Some("cursor-1"));
    assert!(migrated.workspace_key.contains("acct_1"));
    assert!(migrated.workspace_key.contains("default"));
    assert!(migrated.terminal_id.is_none());
    assert!(migrated.remote_session_key.is_none());
    assert!(migrated.assistant_session_id.is_none());
    assert!(migrated.last_run_id.is_none());
    assert!(migrated.lease_owner.is_none());
    assert!(migrated.lease_expires_at.is_none());
}

#[test]
#[serial]
fn list_bridge_session_states_returns_all_valid_sessions() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
    let cwd = temp.path().join("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let now = Utc::now();

    write_bridge_session_state(&bridge_state("session-a", &cwd, "workspace-a", now)).unwrap();
    write_bridge_session_state(&bridge_state("session-b", &cwd, "workspace-b", now)).unwrap();

    let sessions = list_bridge_session_states().unwrap();

    let ids: Vec<_> = sessions
        .iter()
        .map(|session| session.session_id.as_str())
        .collect();
    assert_eq!(ids, vec!["session-a", "session-b"]);
}

#[test]
#[serial]
fn find_bridge_session_by_workspace_key_returns_newest_match() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
    let cwd = temp.path().join("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let older = Utc::now() - chrono::Duration::minutes(5);
    let newer = Utc::now();

    write_bridge_session_state(&bridge_state("old", &cwd, "workspace-key", older)).unwrap();
    write_bridge_session_state(&bridge_state("new", &cwd, "workspace-key", newer)).unwrap();
    write_bridge_session_state(&bridge_state("other", &cwd, "other-key", newer)).unwrap();

    let matched =
        find_bridge_session_by_workspace_key("workspace-key", Some("acct_1"), Some("default"))
            .unwrap()
            .unwrap();

    assert_eq!(matched.session_id, "new");
}

#[test]
#[serial]
fn bridge_session_state_paths_stay_under_allthecodes_daemon_bridge() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
    let cwd = temp.path().join("workspace");
    fs::create_dir_all(&cwd).unwrap();

    write_bridge_session_state(&bridge_state(
        "session/with:unsafe chars",
        &cwd,
        "workspace-key",
        Utc::now(),
    ))
    .unwrap();

    let bridge_root = daemon_dir().join("bridge");
    assert!(bridge_session_state_path("session/with:unsafe chars").starts_with(&bridge_root));
    assert!(bridge_session_inbox_path("session/with:unsafe chars").starts_with(&bridge_root));
    assert!(bridge_session_state_path("session/with:unsafe chars").exists());
    assert!(bridge_session_inbox_path("session/with:unsafe chars")
        .parent()
        .unwrap()
        .starts_with(&bridge_root));
}
