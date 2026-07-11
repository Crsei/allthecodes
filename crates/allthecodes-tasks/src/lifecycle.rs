//! Pure task lifecycle transitions shared by runtime task stores.

use serde_json::Value;

use crate::{
    AgentRuntimePhase, TaskEntry, TaskStatus, REMOTE_TASK_TYPE_ULTRAREVIEW, TASK_KIND_REMOTE_AGENT,
};

pub const REMOTE_REVIEW_TIMEOUT_MS: i64 = 30 * 60 * 1000;

pub fn recover_task_after_restart(entry: &mut TaskEntry, now_seconds: i64, now_ms: i64) -> bool {
    let mut changed = false;
    let status = normalize_loaded_status(entry.status);
    if status != entry.status {
        entry.previous_status = Some(entry.status);
        entry.status = status;
        changed = true;
    }

    if entry.status.should_interrupt_on_startup() {
        let remote_recoverable = is_remote_recoverable_task(entry);
        let recovered_status = if remote_recoverable {
            TaskStatus::Recoverable
        } else {
            TaskStatus::Interrupted
        };
        let mut recovered = false;

        if entry.status != recovered_status || entry.recovered_at.is_none() {
            if entry.status != recovered_status || entry.previous_status.is_none() {
                entry.previous_status = Some(entry.status);
            }
            entry.status = recovered_status;
            entry.recovered_at = Some(now_seconds);
            recovered = true;
        }

        if remote_recoverable && entry.poll_started_at != Some(now_ms) {
            entry.poll_started_at = Some(now_ms);
            recovered = true;
        }

        if !recovered {
            return changed;
        }
        entry.updated_at = now_seconds;
        if is_delegated_local_task(entry) {
            if let Some(activity) = entry.runtime_activity.as_mut() {
                activity.phase = AgentRuntimePhase::NeedsManualRecovery;
                activity.last_heartbeat_at_ms = now_ms;
            }
            if let Some(child_session_id) = delegated_child_session_id(entry) {
                let recovery = format!("/resume {child_session_id}");
                if !entry.output.contains(&recovery) {
                    if !entry.output.is_empty() && !entry.output.ends_with('\n') {
                        entry.output.push('\n');
                    }
                    entry.output.push_str(&format!(
                        "[Delegated child interrupted by restart; resume manually with {recovery}]"
                    ));
                }
            }
        }
        return true;
    }

    changed
}

pub fn is_remote_recoverable_task(entry: &TaskEntry) -> bool {
    !is_delegated_local_task(entry)
        && (entry.kind == TASK_KIND_REMOTE_AGENT
            || entry.remote_session_id.is_some()
            || entry.remote_task_type.is_some())
}

fn is_delegated_local_task(entry: &TaskEntry) -> bool {
    entry
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("delegate"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn delegated_child_session_id(entry: &TaskEntry) -> Option<String> {
    entry
        .runtime_activity
        .as_ref()
        .map(|activity| activity.child_session_id.clone())
        .filter(|id| !id.is_empty())
        .or_else(|| entry.remote_session_id.clone())
}

pub fn is_remote_review_task(entry: &TaskEntry) -> bool {
    entry.remote_task_type.as_deref() == Some(REMOTE_TASK_TYPE_ULTRAREVIEW)
        || entry
            .remote_task_metadata
            .as_ref()
            .and_then(|metadata| metadata.get("isRemoteReview"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
}

pub fn remote_review_timed_out(entry: &TaskEntry, now_ms: i64) -> bool {
    entry.status.is_active_for_output_wait()
        && is_remote_review_task(entry)
        && entry
            .poll_started_at
            .is_some_and(|started| now_ms.saturating_sub(started) > REMOTE_REVIEW_TIMEOUT_MS)
}

pub fn normalize_loaded_status(status: TaskStatus) -> TaskStatus {
    match status {
        TaskStatus::Stopped => TaskStatus::Cancelled,
        other => other,
    }
}

pub fn normalize_new_status(status: TaskStatus) -> TaskStatus {
    match status {
        TaskStatus::Stopped => TaskStatus::Cancelled,
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::TASK_KIND_TOOL;

    fn task(status: TaskStatus) -> TaskEntry {
        TaskEntry {
            id: "1".to_string(),
            kind: TASK_KIND_TOOL.to_string(),
            subject: "subject".to_string(),
            description: "description".to_string(),
            status,
            output: String::new(),
            output_summary: String::new(),
            output_bytes: 0,
            output_truncated: false,
            parent_id: None,
            depends_on: Vec::new(),
            owner: None,
            active_form: None,
            metadata: None,
            tool_use_id: None,
            agent_id: None,
            supervisor_id: None,
            isolation: None,
            worktree_path: None,
            worktree_branch: None,
            remote_task_type: None,
            remote_session_id: None,
            remote_task_metadata: None,
            poll_started_at: None,
            cancel_requested_at: None,
            recovered_at: None,
            previous_status: None,
            runtime_activity: None,
            created_at: 10,
            updated_at: 10,
        }
    }

    #[test]
    fn normalizes_legacy_stopped_status() {
        assert_eq!(
            normalize_loaded_status(TaskStatus::Stopped),
            TaskStatus::Cancelled
        );
        assert_eq!(
            normalize_new_status(TaskStatus::Stopped),
            TaskStatus::Cancelled
        );
    }

    #[test]
    fn local_active_task_recovers_to_interrupted() {
        let mut entry = task(TaskStatus::InProgress);

        assert!(recover_task_after_restart(&mut entry, 100, 100_000));
        assert_eq!(entry.status, TaskStatus::Interrupted);
        assert_eq!(entry.previous_status, Some(TaskStatus::InProgress));
        assert_eq!(entry.recovered_at, Some(100));
        assert_eq!(entry.poll_started_at, None);
        assert_eq!(entry.updated_at, 100);
    }

    #[test]
    fn delegated_restart_requires_manual_resume_and_keeps_worktree() {
        let mut entry = task(TaskStatus::InProgress);
        entry.kind = crate::TASK_KIND_LOCAL_AGENT.to_string();
        entry.metadata = Some(json!({ "delegate": true }));
        entry.remote_session_id = Some("child-session".to_string());
        entry.worktree_path = Some("/tmp/kept-worktree".to_string());
        entry.worktree_branch = Some("agent-worktree-child".to_string());
        entry.runtime_activity = Some(crate::AgentRuntimeActivity {
            phase: AgentRuntimePhase::Running,
            task_id: entry.id.clone(),
            agent_id: "child-session".to_string(),
            child_session_id: "child-session".to_string(),
            ..crate::AgentRuntimeActivity::default()
        });

        assert!(recover_task_after_restart(&mut entry, 20, 20_000));
        assert_eq!(entry.status, TaskStatus::Interrupted);
        assert_eq!(
            entry
                .runtime_activity
                .as_ref()
                .map(|activity| activity.phase),
            Some(AgentRuntimePhase::NeedsManualRecovery)
        );
        assert_eq!(entry.output.matches("/resume child-session").count(), 1);
        assert_eq!(entry.worktree_path.as_deref(), Some("/tmp/kept-worktree"));

        assert!(!recover_task_after_restart(&mut entry, 21, 21_000));
        assert_eq!(entry.output.matches("/resume child-session").count(), 1);
    }

    #[test]
    fn remote_active_task_recovers_to_recoverable_and_starts_polling() {
        let mut entry = task(TaskStatus::Pending);
        entry.kind = TASK_KIND_REMOTE_AGENT.to_string();
        entry.remote_session_id = Some("remote-session".to_string());

        assert!(recover_task_after_restart(&mut entry, 200, 200_000));
        assert_eq!(entry.status, TaskStatus::Recoverable);
        assert_eq!(entry.poll_started_at, Some(200_000));
        assert!(is_remote_recoverable_task(&entry));
    }

    #[test]
    fn remote_review_timeout_requires_active_review_past_limit() {
        let mut entry = task(TaskStatus::Recoverable);
        entry.remote_task_type = Some(REMOTE_TASK_TYPE_ULTRAREVIEW.to_string());
        entry.poll_started_at = Some(1_000);

        assert!(!remote_review_timed_out(
            &entry,
            1_000 + REMOTE_REVIEW_TIMEOUT_MS
        ));
        assert!(remote_review_timed_out(
            &entry,
            1_001 + REMOTE_REVIEW_TIMEOUT_MS
        ));

        entry.status = TaskStatus::Completed;
        assert!(!remote_review_timed_out(
            &entry,
            1_001 + REMOTE_REVIEW_TIMEOUT_MS
        ));
    }

    #[test]
    fn remote_review_metadata_alias_is_supported() {
        let mut entry = task(TaskStatus::Pending);
        entry.remote_task_metadata = Some(json!({"isRemoteReview": true}));

        assert!(is_remote_review_task(&entry));
    }
}
