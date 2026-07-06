use std::collections::HashSet;
use std::sync::atomic::Ordering;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use allthecodes_config::features::Feature;
use allthecodes_services::proactive::ProactiveStatus;

use crate::process_state;
use crate::protocol::{DaemonCommandKind, DaemonCommandStatus, DaemonEventKind};
use crate::state::DaemonState;
use crate::supervisor::ASSISTANT_WORKER_ID;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutomationStatus {
    Standby,
    Sleeping,
    Running,
    Blocked,
    NeedsInput,
}

impl AutomationStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Standby => "standby",
            Self::Sleeping => "sleeping",
            Self::Running => "running",
            Self::Blocked => "blocked",
            Self::NeedsInput => "needs_input",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutomationState {
    pub status: AutomationStatus,
    pub sleeping_until: Option<DateTime<Utc>>,
    pub reason: Option<String>,
    pub next_tick_at: Option<DateTime<Utc>>,
    pub proactive_active: bool,
    pub terminal_focus: bool,
    pub query_running: bool,
    pub pending_input: bool,
}

impl AutomationState {
    pub fn external_metadata(&self) -> Value {
        serde_json::json!({
            "status": self.status.as_str(),
            "sleeping_until": self.sleeping_until.map(|value| value.to_rfc3339()),
            "reason": self.reason.clone(),
            "next_tick_at": self.next_tick_at.map(|value| value.to_rfc3339()),
            "proactive_active": self.proactive_active,
            "terminal_focus": self.terminal_focus,
            "query_running": self.query_running,
            "pending_input": self.pending_input,
        })
    }
}

pub fn snapshot(state: &DaemonState) -> AutomationState {
    let daemon_sleep = process_state::active_sleep_state()
        .ok()
        .flatten()
        .map(|sleep| (sleep.sleeping_until, sleep.reason));
    let engine_sleeping = state.engine.is_sleeping();
    let query_running = state.is_query_running.load(Ordering::SeqCst) || assistant_command_active();
    let pending_input = pending_input_active();
    let proactive = allthecodes_services::proactive::global_controller().snapshot();
    let daemon_proactive = process_state::read_proactive_state().ok().flatten();
    let sleeping = daemon_sleep.is_some() || engine_sleeping;
    let status = if pending_input {
        AutomationStatus::NeedsInput
    } else if sleeping {
        AutomationStatus::Sleeping
    } else if query_running {
        AutomationStatus::Running
    } else {
        AutomationStatus::Standby
    };
    let (sleeping_until, reason) = match daemon_sleep {
        Some((sleeping_until, reason)) => (Some(sleeping_until), reason),
        None => (None, None),
    };

    AutomationState {
        status,
        sleeping_until,
        reason,
        next_tick_at: proactive.next_tick_at.or_else(|| {
            daemon_proactive
                .as_ref()
                .and_then(|state| state.next_tick_at)
        }),
        proactive_active: proactive.status == ProactiveStatus::Active
            || state.features.proactive
            || daemon_proactive
                .as_ref()
                .map(|state| state.active)
                .unwrap_or(false)
            || allthecodes_config::features::enabled(Feature::Proactive),
        terminal_focus: state.terminal_focus(),
        query_running,
        pending_input,
    }
}

pub fn snapshot_from_process_state() -> AutomationState {
    let daemon_sleep = process_state::active_sleep_state()
        .ok()
        .flatten()
        .map(|sleep| (sleep.sleeping_until, sleep.reason));
    let query_running = assistant_command_active();
    let pending_input = pending_input_active();
    let proactive = allthecodes_services::proactive::global_controller().snapshot();
    let daemon_proactive = process_state::read_proactive_state().ok().flatten();
    let sleeping = daemon_sleep.is_some();
    let status = if pending_input {
        AutomationStatus::NeedsInput
    } else if sleeping {
        AutomationStatus::Sleeping
    } else if query_running {
        AutomationStatus::Running
    } else {
        AutomationStatus::Standby
    };
    let (sleeping_until, reason) = match daemon_sleep {
        Some((sleeping_until, reason)) => (Some(sleeping_until), reason),
        None => (None, None),
    };

    AutomationState {
        status,
        sleeping_until,
        reason,
        next_tick_at: proactive.next_tick_at.or_else(|| {
            daemon_proactive
                .as_ref()
                .and_then(|state| state.next_tick_at)
        }),
        proactive_active: proactive.status == ProactiveStatus::Active
            || daemon_proactive
                .as_ref()
                .map(|state| state.active)
                .unwrap_or(false)
            || allthecodes_config::features::enabled(Feature::Proactive),
        terminal_focus: false,
        query_running,
        pending_input,
    }
}

pub(crate) fn assistant_command_active() -> bool {
    active_submit_count() > 0
}

pub(crate) fn autonomous_worker_blocked() -> bool {
    process_state::active_sleep_state().ok().flatten().is_some()
        || assistant_command_active()
        || pending_input_active()
}

pub(crate) fn active_submit_count() -> usize {
    crate::protocol_store()
        .read_worker_commands(ASSISTANT_WORKER_ID)
        .map(|commands| {
            commands
                .into_iter()
                .filter(|command| {
                    command.kind == DaemonCommandKind::Submit
                        && matches!(
                            command.status,
                            DaemonCommandStatus::Pending | DaemonCommandStatus::Acked
                        )
                })
                .count()
        })
        .unwrap_or(0)
}

pub(crate) fn pending_input_active() -> bool {
    let mut permissions = HashSet::new();
    let mut questions = HashSet::new();
    let events = crate::protocol_store()
        .read_worker_events(ASSISTANT_WORKER_ID)
        .unwrap_or_default();

    for event in events {
        match DaemonEventKind::parse(&event.event_type) {
            DaemonEventKind::PermissionRequest => {
                if let Some(id) = payload_id(&event.data, &["tool_use_id", "request_id", "id"]) {
                    permissions.insert(id);
                }
            }
            DaemonEventKind::AskUserQuestion => {
                if let Some(id) = payload_id(&event.data, &["request_id", "id", "question_id"]) {
                    questions.insert(id);
                }
            }
            DaemonEventKind::Unknown(kind) if kind == "permission_response" => {
                if let Some(id) = payload_id(&event.data, &["tool_use_id", "request_id", "id"]) {
                    permissions.remove(&id);
                }
            }
            DaemonEventKind::Unknown(kind) if kind == "ask_user_response" => {
                if let Some(id) = payload_id(&event.data, &["request_id", "id", "question_id"]) {
                    questions.remove(&id);
                }
            }
            _ => {}
        }
    }

    !permissions.is_empty() || !questions.is_empty()
}

fn payload_id(payload: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .filter_map(|key| payload.get(*key).and_then(Value::as_str))
        .find(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::Path;
    use std::sync::Arc;

    use allthecodes_config::features::{self, FeatureFlags};
    use allthecodes_engine::lifecycle::QueryEngine;
    use allthecodes_engine::types::config::QueryEngineConfig;
    use chrono::Utc;
    use serde_json::json;
    use serial_test::serial;

    use crate::process_state::DaemonSleepState;
    use crate::protocol::{DaemonCommandKind, DaemonEventKind};
    use crate::state::DaemonState;
    use crate::supervisor::ASSISTANT_WORKER_ID;

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
            match &self.previous {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    fn make_daemon_state() -> DaemonState {
        make_daemon_state_with_features(FeatureFlags::all_disabled())
    }

    fn make_daemon_state_with_features(features: FeatureFlags) -> DaemonState {
        let engine = Arc::new(QueryEngine::new(QueryEngineConfig {
            cwd: ".".to_string(),
            tools: vec![],
            custom_system_prompt: None,
            append_system_prompt: None,
            user_specified_model: None,
            fallback_model: None,
            max_turns: None,
            max_budget_usd: None,
            task_budget: None,
            verbose: false,
            initial_messages: None,
            commands: vec![],
            thinking_config: None,
            json_schema: None,
            replay_user_messages: false,
            persist_session: false,
            resolved_model: None,
            auto_save_session: false,
            agent_context: None,
        }));
        DaemonState::new(engine, Arc::new(features), 19836)
    }

    fn write_config_sleep_state(reason: &str) -> DaemonSleepState {
        let now = Utc::now();
        let state = DaemonSleepState {
            schema_version: 2,
            sleeping_until: now + chrono::Duration::seconds(60),
            reason: Some(reason.to_string()),
            updated_at: now,
        };
        let path = allthecodes_config::paths::daemon_dir().join("sleep-state.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, serde_json::to_vec_pretty(&state).unwrap()).unwrap();
        state
    }

    #[test]
    #[serial]
    fn snapshot_reports_standby_when_idle() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let state = make_daemon_state();

        let current = snapshot(&state);

        assert_eq!(current.status, AutomationStatus::Standby);
        assert!(!current.query_running);
        assert!(!current.pending_input);
        assert!(!current.terminal_focus);
        assert!(current.sleeping_until.is_none());
        assert!(current.reason.is_none());
        assert!(current.next_tick_at.is_none());
        assert!(!current.proactive_active);
    }

    #[test]
    #[serial]
    fn external_metadata_includes_public_gateway_fields() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let state = make_daemon_state();

        let metadata = snapshot(&state).external_metadata();

        assert_eq!(metadata["status"], "standby");
        assert_eq!(metadata["proactive_active"], false);
        assert!(metadata.get("next_tick_at").is_some());
        assert_eq!(metadata["query_running"], false);
        assert_eq!(metadata["pending_input"], false);
        assert_eq!(metadata["terminal_focus"], false);
    }

    #[test]
    #[serial]
    fn snapshot_reports_proactive_controller_next_tick() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let controller = allthecodes_services::proactive::global_controller();
        controller.activate("automation_state_test");
        let expected_next_tick_at = controller.snapshot().next_tick_at;
        let state = make_daemon_state();

        let current = snapshot(&state);
        controller.deactivate("automation_state_test_cleanup");

        assert_eq!(current.next_tick_at, expected_next_tick_at);
        assert!(current.proactive_active);
    }

    #[test]
    #[serial]
    fn snapshot_reports_proactive_active_from_worker_feature() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        allthecodes_services::proactive::global_controller()
            .deactivate("automation_state_test_cleanup");
        let state = make_daemon_state_with_features(FeatureFlags {
            proactive: true,
            ..FeatureFlags::all_disabled()
        });

        let current = snapshot(&state);

        assert!(current.next_tick_at.is_none());
        assert!(current.proactive_active);
    }

    #[test]
    #[serial]
    fn process_snapshot_reports_next_tick_written_by_proactive_worker() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let mut flags = FeatureFlags::all_disabled();
        flags.proactive = true;
        features::set_runtime_override(flags);

        crate::tick::enqueue_proactive_tick_once(chrono::Local::now(), false)
            .unwrap()
            .expect("tick command");
        let current = snapshot_from_process_state();
        features::clear_runtime_override();

        assert!(current.proactive_active);
        assert!(current.next_tick_at.is_some());
    }

    #[test]
    #[serial]
    fn snapshot_reports_sleep_state_written_to_config_daemon_dir() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let expected = write_config_sleep_state("waiting");
        let state = make_daemon_state();

        let current = snapshot(&state);

        assert_eq!(current.status, AutomationStatus::Sleeping);
        assert_eq!(current.sleeping_until, Some(expected.sleeping_until));
        assert_eq!(current.reason.as_deref(), Some("waiting"));
    }

    #[test]
    #[serial]
    fn snapshot_reports_running_for_active_submit_command() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        crate::protocol_store()
            .enqueue_command(
                ASSISTANT_WORKER_ID,
                DaemonCommandKind::Submit,
                json!({ "text": "hello" }),
                None,
            )
            .unwrap();
        let state = make_daemon_state();

        let current = snapshot(&state);

        assert_eq!(current.status, AutomationStatus::Running);
        assert!(current.query_running);
        assert!(!current.pending_input);
    }

    #[test]
    #[serial]
    fn snapshot_reports_needs_input_for_pending_permission() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        crate::protocol_store()
            .append_event(
                ASSISTANT_WORKER_ID,
                None,
                DaemonEventKind::PermissionRequest.as_str(),
                json!({
                    "request_id": "toolu_1",
                    "tool_use_id": "toolu_1",
                    "tool_name": "Bash",
                }),
            )
            .unwrap();
        let state = make_daemon_state();

        let current = snapshot(&state);

        assert_eq!(current.status, AutomationStatus::NeedsInput);
        assert!(current.pending_input);
    }

    #[test]
    #[serial]
    fn snapshot_clears_pending_permission_after_response() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        crate::protocol_store()
            .append_event(
                ASSISTANT_WORKER_ID,
                None,
                DaemonEventKind::PermissionRequest.as_str(),
                json!({
                    "request_id": "toolu_1",
                    "tool_use_id": "toolu_1",
                    "tool_name": "Bash",
                }),
            )
            .unwrap();
        crate::protocol_store()
            .append_event(
                ASSISTANT_WORKER_ID,
                None,
                "permission_response",
                json!({
                    "tool_use_id": "toolu_1",
                    "decision": "allow",
                }),
            )
            .unwrap();
        let state = make_daemon_state();

        let current = snapshot(&state);

        assert_eq!(current.status, AutomationStatus::Standby);
        assert!(!current.pending_input);
    }

    #[test]
    #[serial]
    fn snapshot_reports_needs_input_for_pending_ask_user_question() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        crate::protocol_store()
            .append_event(
                ASSISTANT_WORKER_ID,
                None,
                DaemonEventKind::AskUserQuestion.as_str(),
                json!({
                    "request_id": "question-1",
                    "question": "Which branch?",
                }),
            )
            .unwrap();
        let state = make_daemon_state();

        let current = snapshot(&state);

        assert_eq!(current.status, AutomationStatus::NeedsInput);
        assert!(current.pending_input);
    }
}
