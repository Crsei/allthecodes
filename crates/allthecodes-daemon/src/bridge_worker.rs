use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use allthecodes_gateway::{GatewayCommand, GatewayCommandKind, GatewayCommandSink};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::bridge_session::{
    refresh_bridge_session_lease, select_or_create_bridge_session, BridgeSessionIdentity,
    BridgeSessionLease, BridgeSessionReusePolicy,
};
use crate::gateway_bridge::GatewayDaemonBridge;
use crate::process_state::{self, DaemonBridgeSessionState};

const BRIDGE_WORK_SCHEMA_VERSION: u32 = 1;
const DEFAULT_BRIDGE_LEASE_TTL: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BridgeWorkKind {
    RemoteMessage,
    RemoteAbort,
    PermissionResponse,
    AskUserResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BridgeWorkItem {
    pub schema_version: u32,
    pub work_id: String,
    pub cursor: String,
    pub kind: BridgeWorkKind,
    pub run_id: String,
    pub session_key: String,
    pub payload: Value,
    pub idempotency_key: Option<String>,
    pub received_at: DateTime<Utc>,
}

impl BridgeWorkItem {
    pub fn remote_message(
        work_id: impl Into<String>,
        cursor: impl Into<String>,
        run_id: impl Into<String>,
        session_key: impl Into<String>,
        text: impl Into<String>,
        idempotency_key: Option<String>,
    ) -> Self {
        Self::new(
            work_id,
            cursor,
            BridgeWorkKind::RemoteMessage,
            run_id,
            session_key,
            json!({ "text": text.into() }),
            idempotency_key,
        )
    }

    pub fn remote_abort(
        work_id: impl Into<String>,
        cursor: impl Into<String>,
        run_id: impl Into<String>,
        session_key: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self::new(
            work_id,
            cursor,
            BridgeWorkKind::RemoteAbort,
            run_id,
            session_key,
            json!({ "reason": reason.into() }),
            None,
        )
    }

    pub fn permission_response(
        work_id: impl Into<String>,
        cursor: impl Into<String>,
        run_id: impl Into<String>,
        session_key: impl Into<String>,
        tool_use_id: impl Into<String>,
        approved: bool,
        reason: Option<String>,
    ) -> Self {
        let tool_use_id = tool_use_id.into();
        Self::new(
            work_id,
            cursor,
            BridgeWorkKind::PermissionResponse,
            run_id,
            session_key,
            json!({
                "tool_use_id": tool_use_id.clone(),
                "toolUseId": tool_use_id,
                "approved": approved,
                "decision": if approved { "allow" } else { "deny" },
                "reason": reason,
            }),
            None,
        )
    }

    pub fn ask_user_response(
        work_id: impl Into<String>,
        cursor: impl Into<String>,
        run_id: impl Into<String>,
        session_key: impl Into<String>,
        question_id: impl Into<String>,
        answer: impl Into<String>,
    ) -> Self {
        let question_id = question_id.into();
        let answer = answer.into();
        Self::new(
            work_id,
            cursor,
            BridgeWorkKind::AskUserResponse,
            run_id,
            session_key,
            json!({
                "request_id": question_id.clone(),
                "questionId": question_id,
                "answer": answer.clone(),
                "response": answer,
            }),
            None,
        )
    }

    fn new(
        work_id: impl Into<String>,
        cursor: impl Into<String>,
        kind: BridgeWorkKind,
        run_id: impl Into<String>,
        session_key: impl Into<String>,
        payload: Value,
        idempotency_key: Option<String>,
    ) -> Self {
        Self {
            schema_version: BRIDGE_WORK_SCHEMA_VERSION,
            work_id: work_id.into(),
            cursor: cursor.into(),
            kind,
            run_id: run_id.into(),
            session_key: session_key.into(),
            payload,
            idempotency_key,
            received_at: Utc::now(),
        }
    }

    fn idempotency_key(&self) -> String {
        self.idempotency_key
            .clone()
            .unwrap_or_else(|| format!("bridge:{}", self.work_id))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PollOutcome {
    pub processed: usize,
    pub acknowledged: usize,
    pub last_cursor: Option<String>,
}

#[derive(Debug, Clone)]
pub struct BridgeWorkerRuntime {
    session: DaemonBridgeSessionState,
    sink: GatewayDaemonBridge,
    lease_owner: String,
    lease_ttl: Duration,
}

impl BridgeWorkerRuntime {
    pub fn new(cwd: &Path) -> Result<Self> {
        let identity = bridge_session_identity_from_env(cwd);
        let policy = bridge_session_policy_from_env()?;
        Self::new_with_selection(
            identity,
            policy,
            BridgeSessionLease {
                owner: bridge_lease_owner(),
                ttl: DEFAULT_BRIDGE_LEASE_TTL,
                allow_stale_takeover: true,
            },
        )
    }

    pub fn new_with_selection(
        identity: BridgeSessionIdentity,
        policy: BridgeSessionReusePolicy,
        lease: BridgeSessionLease,
    ) -> Result<Self> {
        let lease_owner = lease.owner.clone();
        let lease_ttl = lease.ttl;
        let session = select_or_create_bridge_session(identity, policy, lease)?;
        Ok(Self::for_session_with_lease(
            session,
            lease_owner,
            lease_ttl,
        ))
    }

    pub fn for_session(session: DaemonBridgeSessionState) -> Self {
        Self::from_session_state(session)
    }

    pub fn from_session_state(session: DaemonBridgeSessionState) -> Self {
        let lease_owner = session
            .lease_owner
            .clone()
            .unwrap_or_else(bridge_lease_owner);
        Self::for_session_with_lease(session, lease_owner, DEFAULT_BRIDGE_LEASE_TTL)
    }

    fn for_session_with_lease(
        session: DaemonBridgeSessionState,
        lease_owner: String,
        lease_ttl: Duration,
    ) -> Self {
        let sink = GatewayDaemonBridge::for_worker_with_session(
            session.assistant_worker_id.clone(),
            Some(session.session_id.clone()),
            session.assistant_session_id.clone(),
        );
        Self {
            session,
            sink,
            lease_owner,
            lease_ttl,
        }
    }

    pub fn poll_once(&mut self) -> Result<PollOutcome> {
        let mut outcome = PollOutcome::default();
        for item in self.pending_work_items()? {
            self.sink
                .dispatch(self.gateway_command(&item))
                .map_err(|error| {
                    anyhow::anyhow!(
                        "failed to dispatch bridge work item {}: {}",
                        item.work_id,
                        error
                    )
                })?;
            outcome.processed += 1;
            outcome.acknowledged += 1;
            outcome.last_cursor = Some(item.cursor.clone());
            self.session.last_poll_cursor = Some(item.cursor);
            self.session.last_ack_at = Some(Utc::now());
            self.session.remote_session_key = Some(item.session_key);
            self.session.last_run_id = Some(item.run_id);
            self.session = process_state::write_bridge_session_state(&self.session)?;
            self.refresh_lease()?;
        }
        if outcome.processed == 0 {
            self.session = process_state::write_bridge_session_state(&self.session)?;
            self.refresh_lease()?;
            outcome.last_cursor = self.session.last_poll_cursor.clone();
        }
        Ok(outcome)
    }

    pub fn append_work_item(&self, item: BridgeWorkItem) -> Result<()> {
        let path = self.inbox_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("failed to open bridge inbox {}", path.display()))?;
        serde_json::to_writer(&mut file, &item)
            .with_context(|| format!("failed to encode bridge inbox item {}", item.work_id))?;
        file.write_all(b"\n")
            .with_context(|| format!("failed to write bridge inbox {}", path.display()))?;
        file.sync_all()
            .with_context(|| format!("failed to sync bridge inbox {}", path.display()))?;
        Ok(())
    }

    fn pending_work_items(&self) -> Result<Vec<BridgeWorkItem>> {
        let mut after_last_cursor = self.session.last_poll_cursor.is_none();
        let last_cursor = self.session.last_poll_cursor.as_deref();
        let mut pending = Vec::new();
        for item in self.read_work_items()? {
            if !after_last_cursor {
                if Some(item.cursor.as_str()) == last_cursor {
                    after_last_cursor = true;
                }
                continue;
            }
            pending.push(item);
        }
        Ok(pending)
    }

    fn read_work_items(&self) -> Result<Vec<BridgeWorkItem>> {
        let path = self.inbox_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let text = fs::read_to_string(&path)
            .with_context(|| format!("failed to read bridge inbox {}", path.display()))?;
        Ok(text
            .lines()
            .filter(|line| !line.trim().is_empty())
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect())
    }

    fn gateway_command(&self, item: &BridgeWorkItem) -> GatewayCommand {
        GatewayCommand {
            kind: match item.kind {
                BridgeWorkKind::RemoteMessage => GatewayCommandKind::Submit,
                BridgeWorkKind::RemoteAbort => GatewayCommandKind::Abort,
                BridgeWorkKind::PermissionResponse => GatewayCommandKind::PermissionResponse,
                BridgeWorkKind::AskUserResponse => GatewayCommandKind::AskUserResponse,
            },
            run_id: item.run_id.clone(),
            session_key: item.session_key.clone(),
            payload: item.payload.clone(),
            idempotency_key: Some(item.idempotency_key()),
        }
    }

    fn inbox_path(&self) -> PathBuf {
        process_state::bridge_session_inbox_path(&self.session.session_id)
    }

    fn refresh_lease(&mut self) -> Result<()> {
        self.session = refresh_bridge_session_lease(
            &self.session.session_id,
            &self.lease_owner,
            self.lease_ttl,
        )?;
        Ok(())
    }
}

fn bridge_session_identity_from_env(cwd: &Path) -> BridgeSessionIdentity {
    BridgeSessionIdentity {
        cwd: cwd.to_path_buf(),
        account_id: None,
        profile: None,
        terminal_id: non_empty_env("ALLTHECODES_BRIDGE_TERMINAL_ID"),
        remote_session_key: None,
    }
}

fn bridge_session_policy_from_env() -> Result<BridgeSessionReusePolicy> {
    let explicit_session_id = non_empty_env("ALLTHECODES_BRIDGE_SESSION_ID");
    let policy = non_empty_env("ALLTHECODES_BRIDGE_SESSION_POLICY")
        .unwrap_or_else(|| "reuse-workspace".to_string());
    match policy.trim().to_ascii_lowercase().as_str() {
        "reuse-workspace" | "reuse" => Ok(explicit_session_id
            .map(BridgeSessionReusePolicy::ExplicitSession)
            .unwrap_or(BridgeSessionReusePolicy::ReuseWorkspace)),
        "new" | "new-session" => Ok(BridgeSessionReusePolicy::NewSession),
        "explicit" => explicit_session_id
            .map(BridgeSessionReusePolicy::ExplicitSession)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "ALLTHECODES_BRIDGE_SESSION_POLICY=explicit requires ALLTHECODES_BRIDGE_SESSION_ID"
                )
            }),
        other => anyhow::bail!("unsupported ALLTHECODES_BRIDGE_SESSION_POLICY: {other}"),
    }
}

fn bridge_lease_owner() -> String {
    format!("daemon-pid-{}", std::process::id())
}

fn non_empty_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use chrono::Utc;
    use serial_test::serial;

    use crate::bridge_session::derive_workspace_key;
    use crate::process_state::DaemonBridgeSessionState;
    use crate::protocol::DaemonCommandKind;
    use crate::supervisor::ASSISTANT_WORKER_ID;

    use super::*;

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

        fn set_str(key: &'static str, value: &str) -> Self {
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

    fn session(cwd: &Path) -> DaemonBridgeSessionState {
        let workspace_key = derive_workspace_key(&BridgeSessionIdentity {
            cwd: cwd.to_path_buf(),
            account_id: Some("acct_1".to_string()),
            profile: Some("default".to_string()),
            terminal_id: None,
            remote_session_key: None,
        })
        .unwrap();
        DaemonBridgeSessionState {
            schema_version: 2,
            session_id: "bridge-session-1".to_string(),
            account_id: Some("acct_1".to_string()),
            profile: Some("default".to_string()),
            cwd: cwd.to_path_buf(),
            assistant_worker_id: ASSISTANT_WORKER_ID.to_string(),
            last_poll_cursor: None,
            last_ack_at: None,
            updated_at: Utc::now(),
            workspace_key,
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
    fn poll_once_persists_session_and_enqueues_submit_once() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let mut runtime = BridgeWorkerRuntime::for_session(session(home.path()));
        runtime
            .append_work_item(BridgeWorkItem::remote_message(
                "work-1",
                "cursor-1",
                "run_bridge123",
                "remote:http:abc",
                "hello from bridge",
                Some("delivery-1".to_string()),
            ))
            .unwrap();

        let outcome = runtime.poll_once().unwrap();
        let commands = crate::protocol_store()
            .read_worker_commands(ASSISTANT_WORKER_ID)
            .unwrap();
        let stored = crate::process_state::read_bridge_session_state("bridge-session-1")
            .unwrap()
            .unwrap();

        assert_eq!(outcome.processed, 1);
        assert_eq!(outcome.acknowledged, 1);
        assert_eq!(outcome.last_cursor.as_deref(), Some("cursor-1"));
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].kind, DaemonCommandKind::Submit);
        assert_eq!(commands[0].payload["text"], "hello from bridge");
        assert_eq!(commands[0].payload["gateway"]["runId"], "run_bridge123");
        assert_eq!(stored.last_poll_cursor.as_deref(), Some("cursor-1"));
        assert!(stored.last_ack_at.is_some());

        let second = runtime.poll_once().unwrap();
        let commands = crate::protocol_store()
            .read_worker_commands(ASSISTANT_WORKER_ID)
            .unwrap();
        assert_eq!(second.processed, 0);
        assert_eq!(commands.len(), 1);
    }

    #[test]
    #[serial]
    fn poll_once_maps_abort_permission_and_ask_user_work_units() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let mut runtime = BridgeWorkerRuntime::for_session(session(home.path()));
        runtime
            .append_work_item(BridgeWorkItem::remote_abort(
                "work-abort",
                "cursor-1",
                "run_bridge123",
                "remote:http:abc",
                "stop",
            ))
            .unwrap();
        runtime
            .append_work_item(BridgeWorkItem::permission_response(
                "work-perm",
                "cursor-2",
                "run_bridge123",
                "remote:http:abc",
                "toolu_1",
                true,
                Some("approved".to_string()),
            ))
            .unwrap();
        runtime
            .append_work_item(BridgeWorkItem::ask_user_response(
                "work-ask",
                "cursor-3",
                "run_bridge123",
                "remote:http:abc",
                "question-1",
                "yes",
            ))
            .unwrap();

        let outcome = runtime.poll_once().unwrap();
        let commands = crate::protocol_store()
            .read_worker_commands(ASSISTANT_WORKER_ID)
            .unwrap();

        assert_eq!(outcome.processed, 3);
        assert_eq!(outcome.acknowledged, 3);
        assert_eq!(commands.len(), 3);
        assert_eq!(commands[0].kind, DaemonCommandKind::Abort);
        assert_eq!(commands[0].payload["reason"], "stop");
        assert_eq!(commands[1].kind, DaemonCommandKind::PermissionResponse);
        assert_eq!(commands[1].payload["tool_use_id"], "toolu_1");
        assert_eq!(commands[1].payload["decision"], "allow");
        assert_eq!(commands[2].kind, DaemonCommandKind::AskUserResponse);
        assert_eq!(commands[2].payload["request_id"], "question-1");
        assert_eq!(commands[2].payload["answer"], "yes");
    }

    #[test]
    #[serial]
    fn bridge_worker_reuses_selected_session_after_restart() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let cwd = home.path().join("workspace");
        std::fs::create_dir_all(&cwd).unwrap();

        let mut first = BridgeWorkerRuntime::new(&cwd).unwrap();
        first
            .append_work_item(BridgeWorkItem::remote_message(
                "work-1",
                "cursor-1",
                "run_bridge123",
                "remote:http:abc",
                "hello from bridge",
                Some("delivery-1".to_string()),
            ))
            .unwrap();
        first.poll_once().unwrap();
        let selected = crate::process_state::list_bridge_session_states()
            .unwrap()
            .into_iter()
            .next()
            .unwrap();

        let mut second = BridgeWorkerRuntime::new(&cwd).unwrap();
        let outcome = second.poll_once().unwrap();
        let after_restart = crate::process_state::read_bridge_session_state(&selected.session_id)
            .unwrap()
            .unwrap();
        let commands = crate::protocol_store()
            .read_worker_commands(ASSISTANT_WORKER_ID)
            .unwrap();

        assert_eq!(after_restart.session_id, selected.session_id);
        assert_eq!(after_restart.last_poll_cursor.as_deref(), Some("cursor-1"));
        assert_eq!(outcome.processed, 0);
        assert_eq!(commands.len(), 1);
    }

    #[test]
    #[serial]
    fn bridge_worker_persists_remote_session_key_and_run_id() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let mut runtime = BridgeWorkerRuntime::from_session_state(session(home.path()));
        runtime
            .append_work_item(BridgeWorkItem::remote_message(
                "work-1",
                "cursor-1",
                "run_bridge123",
                "remote:http:abc",
                "hello from bridge",
                Some("delivery-1".to_string()),
            ))
            .unwrap();

        runtime.poll_once().unwrap();
        let stored = crate::process_state::read_bridge_session_state("bridge-session-1")
            .unwrap()
            .unwrap();

        assert_eq!(
            stored.remote_session_key.as_deref(),
            Some("remote:http:abc")
        );
        assert_eq!(stored.last_run_id.as_deref(), Some("run_bridge123"));
    }

    #[test]
    #[serial]
    fn bridge_worker_refreshes_lease_on_poll() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let mut state = session(home.path());
        state.lease_owner = Some("test-owner".to_string());
        state.lease_expires_at = Some(Utc::now() + chrono::Duration::seconds(1));
        let previous_expiry = state.lease_expires_at.unwrap();
        let mut runtime = BridgeWorkerRuntime::from_session_state(state);

        runtime.poll_once().unwrap();
        let stored = crate::process_state::read_bridge_session_state("bridge-session-1")
            .unwrap()
            .unwrap();

        assert_eq!(stored.lease_owner.as_deref(), Some("test-owner"));
        assert!(stored.lease_expires_at.unwrap() > previous_expiry);
    }

    #[test]
    #[serial]
    fn bridge_worker_includes_assistant_session_id_in_queued_gateway_context() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let mut state = session(home.path());
        state.assistant_session_id = Some("assistant-session-prev".to_string());
        let mut runtime = BridgeWorkerRuntime::from_session_state(state);
        runtime
            .append_work_item(BridgeWorkItem::remote_message(
                "work-1",
                "cursor-1",
                "run_bridge123",
                "remote:http:abc",
                "hello from bridge",
                Some("delivery-1".to_string()),
            ))
            .unwrap();

        runtime.poll_once().unwrap();
        let commands = crate::protocol_store()
            .read_worker_commands(ASSISTANT_WORKER_ID)
            .unwrap();

        assert_eq!(
            commands[0].payload["gateway"]["bridgeSessionId"],
            "bridge-session-1"
        );
        assert_eq!(
            commands[0].payload["gateway"]["assistantSessionId"],
            "assistant-session-prev"
        );
    }

    #[test]
    #[serial]
    fn bridge_worker_session_id_env_selects_explicit_session() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let _session_id = EnvGuard::set_str("ALLTHECODES_BRIDGE_SESSION_ID", "bridge-session-1");
        let cwd = home.path().join("workspace");
        std::fs::create_dir_all(&cwd).unwrap();
        crate::process_state::write_bridge_session_state(&session(&cwd)).unwrap();

        let runtime = BridgeWorkerRuntime::new(&cwd).unwrap();

        assert_eq!(runtime.session.session_id, "bridge-session-1");
        assert!(runtime.session.lease_owner.is_some());
    }

    #[test]
    #[serial]
    fn bridge_worker_new_policy_env_creates_distinct_sessions() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let _policy = EnvGuard::set_str("ALLTHECODES_BRIDGE_SESSION_POLICY", "new");
        let _terminal = EnvGuard::set_str("ALLTHECODES_BRIDGE_TERMINAL_ID", "terminal-a");
        let cwd = home.path().join("workspace");
        std::fs::create_dir_all(&cwd).unwrap();

        let first = BridgeWorkerRuntime::new(&cwd).unwrap();
        let second = BridgeWorkerRuntime::new(&cwd).unwrap();

        assert_ne!(first.session.session_id, second.session.session_id);
        assert_eq!(first.session.terminal_id.as_deref(), Some("terminal-a"));
        assert_eq!(second.session.terminal_id.as_deref(), Some("terminal-a"));
    }

    #[test]
    #[serial]
    fn bridge_worker_explicit_policy_env_requires_session_id() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let _policy = EnvGuard::set_str("ALLTHECODES_BRIDGE_SESSION_POLICY", "explicit");
        let cwd = home.path().join("workspace");
        std::fs::create_dir_all(&cwd).unwrap();

        let error = BridgeWorkerRuntime::new(&cwd)
            .expect_err("explicit policy without session id should fail");

        assert!(error
            .to_string()
            .contains("ALLTHECODES_BRIDGE_SESSION_POLICY=explicit requires"));
    }
}
