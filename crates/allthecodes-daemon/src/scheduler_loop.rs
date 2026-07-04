//! Persistent scheduled-task firing loop.

use std::sync::atomic::Ordering;
use std::time::Duration;

use anyhow::Result;
use chrono::{Local, NaiveDate};
use futures::StreamExt;
use serde_json::json;
use tracing::{debug, info, warn};

use allthecodes_config::features::{self, Feature};
use allthecodes_engine::types::config::QuerySource;
use allthecodes_services::scheduler::{SchedulerStore, TaskPayload};

use super::automation_state::AutomationStatus;
use super::state::{DaemonState, SseEvent};

const SCHEDULER_TICK_INTERVAL_MS: u64 = 15_000;

pub async fn scheduler_loop(state: DaemonState) {
    let store = SchedulerStore::open_default();
    let mut interval = tokio::time::interval(Duration::from_millis(SCHEDULER_TICK_INTERVAL_MS));
    info!(
        "scheduled task loop started (interval: {}ms)",
        SCHEDULER_TICK_INTERVAL_MS
    );

    interval.tick().await;
    loop {
        interval.tick().await;

        let automation = super::automation_state::snapshot(&state);
        if matches!(
            automation.status,
            AutomationStatus::Running
                | AutomationStatus::Sleeping
                | AutomationStatus::NeedsInput
                | AutomationStatus::Blocked
        ) {
            debug!(
                status = automation.status.as_str(),
                sleeping_until = ?automation.sleeping_until.map(|until| until.to_rfc3339()),
                reason = ?automation.reason,
                "scheduled task tick skipped: automation state is not idle"
            );
            continue;
        }

        if let Err(err) = run_kairos_dream_tick_for_date(Local::now().date_naive()) {
            warn!(error = %err, "failed to run KAIROS dream tick");
        }

        let due = match store.due_tasks() {
            Ok(tasks) => tasks,
            Err(err) => {
                warn!(error = %err, "failed to read due scheduled tasks");
                continue;
            }
        };

        if let Some(task) = due.into_iter().next() {
            if state.is_query_running.swap(true, Ordering::SeqCst) {
                continue;
            }

            let prompt = match &task.payload {
                TaskPayload::Prompt(text) | TaskPayload::SlashCommand(text) => text.clone(),
            };
            let task_id = task.id.clone();
            let task_name = task.name.clone();
            let payload_kind = task.payload.kind_label();

            state.broadcast(SseEvent {
                id: String::new(),
                event_type: "scheduled_task_start".to_string(),
                data: json!({
                    "task_id": task_id.as_str(),
                    "name": task_name,
                    "payload_kind": payload_kind,
                    "prompt": prompt,
                }),
            });

            let state_clone = state.clone();
            tokio::spawn(async move {
                let engine = state_clone.engine.clone();
                let stream = engine.submit_message(&prompt, QuerySource::ScheduledTask);
                tokio::pin!(stream);
                while let Some(sdk_msg) = stream.next().await {
                    if let Some(event) =
                        super::routes::sdk_message_to_sse(&sdk_msg, task_id.as_str())
                    {
                        state_clone.broadcast(event);
                    }
                }

                match SchedulerStore::open_default().record_fired(&task_id) {
                    Ok(updated) => {
                        state_clone.broadcast(SseEvent {
                            id: String::new(),
                            event_type: "scheduled_task_complete".to_string(),
                            data: json!({
                                "task_id": task_id.as_str(),
                                "next_run_at": updated.next_run_at.to_rfc3339(),
                            }),
                        });
                    }
                    Err(err) => {
                        warn!(task_id = %task_id, error = %err, "failed to record scheduled task fire");
                        state_clone.broadcast(SseEvent {
                            id: String::new(),
                            event_type: "scheduled_task_error".to_string(),
                            data: json!({
                                "task_id": task_id.as_str(),
                                "error": err.to_string(),
                            }),
                        });
                    }
                }
                state_clone.is_query_running.store(false, Ordering::SeqCst);
            });
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

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_config::features::FeatureFlags;
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
}
