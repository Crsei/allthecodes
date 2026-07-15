//! Persistent scheduled-task worker loop.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use chrono::{Local, NaiveDate, Utc};
use serde_json::json;
use tracing::{debug, info, warn};

use allthecodes_config::features::{self, Feature};
use allthecodes_services::scheduler::{
    migrate_default_scheduler_data, SchedulerCommandDispatcher, SchedulerDispatchReceipt,
    SchedulerDispatchRequest, SchedulerDispatcherError, SchedulerRunTriggerSource,
    SchedulerService, TaskPayload,
};

use super::state::DaemonState;
use crate::protocol::{DaemonCommand, DaemonCommandKind};
use crate::supervisor::ASSISTANT_WORKER_ID;

pub const SCHEDULER_TICK_INTERVAL_MS: u64 = 15_000;

/// Daemon-owned implementation of the scheduler enqueue capability. Keeping
/// this adapter here lets Web composition inject a narrow dispatcher without
/// exposing the daemon protocol store itself.
#[derive(Debug, Default)]
pub struct DaemonSchedulerDispatcher;

impl SchedulerCommandDispatcher for DaemonSchedulerDispatcher {
    fn dispatch(
        &self,
        request: SchedulerDispatchRequest,
    ) -> Result<SchedulerDispatchReceipt, SchedulerDispatcherError> {
        let prompt = match &request.task.payload {
            TaskPayload::Prompt(text) | TaskPayload::SlashCommand(text) => text.clone(),
        };
        let source = match request.trigger_source {
            SchedulerRunTriggerSource::Manual => "manual_job",
            SchedulerRunTriggerSource::Scheduled => "scheduled_task",
        };
        let command = crate::protocol_store()
            .enqueue_command(
                ASSISTANT_WORKER_ID,
                DaemonCommandKind::Submit,
                json!({
                    "text": prompt,
                    "message_id": format!("scheduler-run-{}", request.run_id.as_str()),
                    "source": source,
                    "scheduled_task": {
                        "task_id": request.task.id.as_str(),
                        "name": request.task.name,
                        "payload_kind": request.task.payload.kind_label(),
                        "next_run_at": request.task.next_run_at.to_rfc3339(),
                        "definition_revision": request.task.revision,
                    },
                    "scheduler_run": {
                        "run_id": request.run_id.as_str(),
                        "trigger_source": request.trigger_source.as_str(),
                        "scheduled_for": request.scheduled_for.map(|value| value.to_rfc3339()),
                        "idempotency_key": request.idempotency_key.clone(),
                        "profile_id": request.task.metadata.profile_id,
                        "session_id": request.task.metadata.session_id,
                        "working_directory": request.task.metadata.working_directory,
                    },
                }),
                Some(request.idempotency_key.clone()),
            )
            .map_err(|error| SchedulerDispatcherError::EnqueueFailed(error.to_string()))?;

        if command
            .payload
            .pointer("/scheduler_run/run_id")
            .and_then(serde_json::Value::as_str)
            != Some(request.run_id.as_str())
        {
            return Err(SchedulerDispatcherError::Rejected(
                "idempotency key is already bound to a different scheduler run".to_string(),
            ));
        }
        Ok(SchedulerDispatchReceipt {
            run_id: request.run_id,
            command_id: command.command_id,
            accepted_at: command.created_at,
            idempotency_key: request.idempotency_key,
        })
    }
}

pub async fn scheduler_loop(state: DaemonState) {
    let mut interval = tokio::time::interval(Duration::from_millis(SCHEDULER_TICK_INTERVAL_MS));
    info!(
        "scheduled task loop started (interval: {}ms)",
        SCHEDULER_TICK_INTERVAL_MS
    );

    interval.tick().await;
    loop {
        interval.tick().await;

        if super::automation_state::autonomous_worker_blocked() {
            debug!("scheduled task tick skipped: automation state is not idle");
            continue;
        }
        if !scheduled_dispatch_enabled_by_hermes(&state.engine.app_state().settings) {
            debug!("scheduled task tick skipped: Hermes runtime disabled");
            continue;
        }

        if let Err(err) = run_kairos_dream_tick_for_date(Local::now().date_naive()) {
            warn!(error = %err, "failed to run KAIROS dream tick");
        }

        match enqueue_due_scheduled_task_unchecked() {
            Ok(Some(command)) => {
                info!(
                    command_id = %command.command_id,
                    "scheduled task queued assistant command"
                );
            }
            Ok(None) => {}
            Err(error) => warn!(error = %error, "failed to queue due scheduled task"),
        }
    }
}

pub(crate) fn run_kairos_dream_tick_for_date(
    date: NaiveDate,
) -> Result<Option<std::path::PathBuf>> {
    if !features::enabled(Feature::Kairos) {
        return Ok(None);
    }
    crate::dream::run_daily_dream_once(date)
}

pub fn enqueue_due_scheduled_task_once() -> Result<Option<DaemonCommand>> {
    if super::automation_state::autonomous_worker_blocked() {
        return Ok(None);
    }
    enqueue_due_scheduled_task_unchecked()
}

fn enqueue_due_scheduled_task_unchecked() -> Result<Option<DaemonCommand>> {
    let service =
        SchedulerService::open_default().with_dispatcher(Arc::new(DaemonSchedulerDispatcher));
    migrate_default_scheduler_data(&service)?;
    let Some(dispatched) = service.dispatch_due_once(Utc::now())? else {
        return Ok(None);
    };
    crate::protocol_store()
        .read_command(ASSISTANT_WORKER_ID, &dispatched.accepted.receipt.command_id)?
        .map(Some)
        .ok_or_else(|| {
            anyhow!(
                "accepted scheduler command '{}' is missing from daemon store",
                dispatched.accepted.receipt.command_id
            )
        })
}

fn scheduled_dispatch_enabled_by_hermes(
    settings: &allthecodes_engine::types::app_state::SettingsJson,
) -> bool {
    settings.hermes_enabled.unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_config::features::FeatureFlags;
    use allthecodes_services::scheduler::{
        Interval, ScheduledTask, SchedulerDispatchRequest, SchedulerDispatcherError, SchedulerKind,
        SchedulerRunFailureCode, SchedulerRunId, SchedulerRunQuery, SchedulerRunStatus,
        SchedulerRunStore, SchedulerRunTriggerSource, SchedulerService, SchedulerStore,
        TaskPayload,
    };
    use chrono::{Datelike, TimeZone};
    use serial_test::serial;

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: impl AsRef<std::path::Path>) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value.as_ref());
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

    struct FeatureGuard(Option<FeatureFlags>);

    impl FeatureGuard {
        fn enable_kairos() -> Self {
            let previous = features::runtime_override();
            features::set_runtime_override(FeatureFlags {
                kairos: true,
                proactive: true,
                ..FeatureFlags::all_disabled()
            });
            Self(previous)
        }
    }

    impl Drop for FeatureGuard {
        fn drop(&mut self) {
            match self.0.take() {
                Some(previous) => features::set_runtime_override(previous),
                None => features::clear_runtime_override(),
            }
        }
    }

    fn write_daily_log(date: NaiveDate, body: &str) {
        let now = Local
            .with_ymd_and_hms(date.year(), date.month(), date.day(), 12, 0, 0)
            .single()
            .expect("test date should map to local time");
        let path = allthecodes_config::paths::daily_log_path(now);
        std::fs::create_dir_all(path.parent().expect("daily log has parent")).unwrap();
        std::fs::write(path, body).unwrap();
    }

    fn write_sleep_state() {
        let now = chrono::Utc::now();
        let state = crate::process_state::DaemonSleepState {
            schema_version: 2,
            sleeping_until: now + chrono::Duration::seconds(60),
            reason: Some("test sleep".to_string()),
            updated_at: now,
        };
        let path = allthecodes_config::paths::daemon_dir().join("sleep-state.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, serde_json::to_vec_pretty(&state).unwrap()).unwrap();
    }

    #[test]
    #[serial]
    fn kairos_dream_tick_writes_once_when_enabled() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let _features = FeatureGuard::enable_kairos();
        let date = NaiveDate::from_ymd_opt(2026, 7, 4).unwrap();
        write_daily_log(date, "- Task: scheduler dream hook.\n");

        let first = run_kairos_dream_tick_for_date(date)
            .unwrap()
            .expect("dream tick writes memory");
        assert!(first.ends_with("memory/dream/2026-07-04.md"));
        assert!(run_kairos_dream_tick_for_date(date).unwrap().is_none());
    }

    #[test]
    #[serial]
    fn scheduler_worker_enqueues_due_task_without_running_engine() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let store = SchedulerStore::open_default();
        let now = chrono::Utc::now();
        let mut task = ScheduledTask::new(
            SchedulerKind::LocalCron,
            "nightly",
            "60s",
            Interval::from_seconds(60),
            TaskPayload::Prompt("summarize queue".to_string()),
            now,
        );
        task.next_run_at = now - chrono::Duration::seconds(5);
        let task = store.add(task).unwrap();

        let command = enqueue_due_scheduled_task_once()
            .unwrap()
            .expect("scheduler should enqueue due task");

        assert_eq!(command.target_worker_id, ASSISTANT_WORKER_ID);
        assert_eq!(command.kind, DaemonCommandKind::Submit);
        assert_eq!(command.payload["source"], "scheduled_task");
        assert_eq!(
            command.payload["scheduled_task"]["task_id"],
            task.id.as_str()
        );
        assert_eq!(command.payload["text"], "summarize queue");
        assert!(store.get(&task.id).unwrap().last_run_at.is_some());

        let run_store = SchedulerRunStore::open_default();
        let queued = run_store.query(&SchedulerRunQuery::default()).unwrap();
        assert_eq!(queued.runs.len(), 1);
        assert_eq!(queued.runs[0].status, SchedulerRunStatus::Queued);
        assert_eq!(
            queued.runs[0].command_id.as_deref(),
            Some(command.command_id.as_str())
        );

        let protocol = crate::protocol_store();
        let claimed = protocol
            .claim_next_pending_command(ASSISTANT_WORKER_ID, "assistant")
            .unwrap()
            .expect("scheduler command should be claimable");
        assert_eq!(
            run_store.get(&queued.runs[0].id).unwrap().status,
            SchedulerRunStatus::Running
        );
        protocol.mark_command_handled(claimed).unwrap();
        assert_eq!(
            run_store.get(&queued.runs[0].id).unwrap().status,
            SchedulerRunStatus::Completed
        );
    }

    #[test]
    #[serial]
    fn scheduler_worker_skips_while_sleeping() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let store = SchedulerStore::open_default();
        let now = chrono::Utc::now();
        let mut task = ScheduledTask::new(
            SchedulerKind::LocalCron,
            "sleepy",
            "60s",
            Interval::from_seconds(60),
            TaskPayload::Prompt("do not run while sleeping".to_string()),
            now,
        );
        task.next_run_at = now - chrono::Duration::seconds(5);
        let task = store.add(task).unwrap();
        write_sleep_state();

        assert!(enqueue_due_scheduled_task_once().unwrap().is_none());
        assert!(store.get(&task.id).unwrap().last_run_at.is_none());
        assert!(crate::protocol_store()
            .read_worker_commands(ASSISTANT_WORKER_ID)
            .unwrap()
            .is_empty());
    }

    #[test]
    #[serial]
    fn scheduler_dispatch_rejects_idempotency_collision_with_unrelated_command() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let now = chrono::Utc::now();
        let task = ScheduledTask::new(
            SchedulerKind::LocalCron,
            "collision",
            "60s",
            Interval::from_seconds(60),
            TaskPayload::Prompt("scheduler payload".to_string()),
            now,
        );
        let idempotency_key = "shared-but-not-scheduler".to_string();
        crate::protocol_store()
            .enqueue_command(
                ASSISTANT_WORKER_ID,
                DaemonCommandKind::Submit,
                json!({ "text": "unrelated payload" }),
                Some(idempotency_key.clone()),
            )
            .unwrap();

        let error = DaemonSchedulerDispatcher
            .dispatch(SchedulerDispatchRequest {
                run_id: SchedulerRunId::new(),
                task,
                trigger_source: SchedulerRunTriggerSource::Manual,
                scheduled_for: None,
                idempotency_key,
                requested_at: now,
            })
            .unwrap_err();

        assert!(matches!(error, SchedulerDispatcherError::Rejected(_)));
        assert_eq!(
            crate::protocol_store()
                .read_worker_commands(ASSISTANT_WORKER_ID)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    #[serial]
    fn manual_scheduler_command_failure_updates_canonical_history() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let now = chrono::Utc::now();
        let task = SchedulerStore::open_default()
            .add(ScheduledTask::new(
                SchedulerKind::LocalCron,
                "manual failure",
                "60s",
                Interval::from_seconds(60),
                TaskPayload::Prompt("fail truthfully".to_string()),
                now,
            ))
            .unwrap();
        let service =
            SchedulerService::open_default().with_dispatcher(Arc::new(DaemonSchedulerDispatcher));
        let accepted = service
            .trigger_manual(&task.id, task.revision, "manual-worker-failure", now)
            .unwrap();

        let protocol = crate::protocol_store();
        let claimed = protocol
            .claim_next_pending_command(ASSISTANT_WORKER_ID, "assistant")
            .unwrap()
            .expect("manual scheduler command should be claimable");
        protocol
            .mark_command_failed(claimed, "worker execution failed")
            .unwrap();

        let failed = SchedulerRunStore::open_default()
            .get(&accepted.run.id)
            .unwrap();
        assert_eq!(failed.status, SchedulerRunStatus::Failed);
        let failure = failed.failure.expect("failed run has typed failure");
        assert_eq!(failure.code, SchedulerRunFailureCode::ExecutionFailed);
        assert_eq!(failure.message, "worker execution failed");
    }

    #[test]
    fn scheduled_dispatch_requires_hermes_enabled() {
        let mut settings = allthecodes_engine::types::app_state::SettingsJson::default();
        assert!(!scheduled_dispatch_enabled_by_hermes(&settings));

        settings.hermes_enabled = Some(true);
        assert!(scheduled_dispatch_enabled_by_hermes(&settings));
    }
}
