use std::collections::HashSet;
use std::sync::atomic::Ordering;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
    pub terminal_focus: bool,
    pub query_running: bool,
    pub pending_input: bool,
}

pub fn snapshot(state: &DaemonState) -> AutomationState {
    let daemon_sleep = process_state::active_sleep_state()
        .ok()
        .flatten()
        .map(|sleep| (sleep.sleeping_until, sleep.reason));
    let engine_sleeping = state.engine.is_sleeping();
    let query_running =
        state.is_query_running.load(Ordering::SeqCst) || assistant_command_active();
    let pending_input = pending_input_active();
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
        terminal_focus: state.terminal_focus(),
        query_running,
        pending_input,
    }
}

pub(crate) fn assistant_command_active() -> bool {
    active_submit_count() > 0
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

    use allthecodes_config::features::FeatureFlags;
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
        DaemonState::new(engine, Arc::new(FeatureFlags::all_disabled()), 19836)
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
