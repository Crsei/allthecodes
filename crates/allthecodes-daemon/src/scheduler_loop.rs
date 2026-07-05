//! Persistent scheduled-task worker loop.

use std::time::Duration;

use anyhow::Result;
use chrono::{Local, NaiveDate};
use serde_json::json;
use tracing::{debug, info, warn};

use allthecodes_config::features::{self, Feature};
use allthecodes_services::scheduler::{SchedulerStore, TaskPayload};

use super::state::DaemonState;
use crate::protocol::{DaemonCommand, DaemonCommandKind};
use crate::supervisor::ASSISTANT_WORKER_ID;

pub const SCHEDULER_TICK_INTERVAL_MS: u64 = 15_000;

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
    let store = SchedulerStore::open_default();
    let due = store.due_tasks()?;
    let Some(task) = due.into_iter().next() else {
        return Ok(None);
    };

    let prompt = match &task.payload {
        TaskPayload::Prompt(text) | TaskPayload::SlashCommand(text) => text.clone(),
    };
    let idempotency_key = format!(
        "scheduled_task:{}:{}",
        task.id.as_str(),
        task.next_run_at.timestamp_millis()
    );
    let command = crate::protocol_store().enqueue_command(
        ASSISTANT_WORKER_ID,
        DaemonCommandKind::Submit,
        json!({
            "text": prompt,
            "message_id": format!("scheduled-task-{}", task.id.as_str()),
            "source": "scheduled_task",
            "scheduled_task": {
                "task_id": task.id.as_str(),
                "name": task.name,
                "payload_kind": task.payload.kind_label(),
                "next_run_at": task.next_run_at.to_rfc3339(),
            },
        }),
        Some(idempotency_key),
    )?;
    store.record_fired(&task.id)?;
    Ok(Some(command))
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
        Interval, ScheduledTask, SchedulerKind, SchedulerStore, TaskPayload,
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
    fn scheduled_dispatch_requires_hermes_enabled() {
        let mut settings = allthecodes_engine::types::app_state::SettingsJson::default();
        assert!(!scheduled_dispatch_enabled_by_hermes(&settings));

        settings.hermes_enabled = Some(true);
        assert!(scheduled_dispatch_enabled_by_hermes(&settings));
    }
}
