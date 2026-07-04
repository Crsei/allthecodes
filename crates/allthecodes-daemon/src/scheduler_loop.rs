//! Persistent scheduled-task firing loop.

use std::sync::atomic::Ordering;
use std::time::Duration;

use chrono::Utc;
use futures::StreamExt;
use serde_json::json;
use tracing::{debug, info, warn};

use allthecodes_engine::types::config::{QuerySource, SubmitMessageOverrides};
use allthecodes_services::scheduler::{SchedulerStore, TaskPayload};
use allthecodes_tasks::{ScheduleSpec, ScheduledAgentTask};

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

        if state.is_query_running.load(Ordering::SeqCst) {
            debug!("scheduled task tick skipped: query running");
            continue;
        }
        if state.engine.is_sleeping() {
            debug!("scheduled task tick skipped: engine sleeping");
            continue;
        }
        if !scheduled_dispatch_enabled_by_hermes(&state.engine.app_state().settings) {
            debug!("scheduled task tick skipped: Hermes runtime disabled");
            continue;
        }

        if dispatch_due_agent_tasks(&state) {
            continue;
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

fn dispatch_due_agent_tasks(state: &DaemonState) -> bool {
    if state.is_query_running.swap(true, Ordering::SeqCst) {
        return false;
    }

    let due = match allthecodes_tasks::claim_due_scheduled_tasks(Utc::now()) {
        Ok(tasks) => tasks,
        Err(err) => {
            state.is_query_running.store(false, Ordering::SeqCst);
            warn!(error = %err, "failed to claim scheduled agent tasks");
            return false;
        }
    };

    if due.is_empty() {
        state.is_query_running.store(false, Ordering::SeqCst);
        return false;
    }

    let state_clone = state.clone();
    tokio::spawn(async move {
        for task in due {
            run_scheduled_agent_task(state_clone.clone(), task).await;
        }
        state_clone.is_query_running.store(false, Ordering::SeqCst);
    });

    true
}

fn scheduled_dispatch_enabled_by_hermes(
    settings: &allthecodes_engine::types::app_state::SettingsJson,
) -> bool {
    settings.hermes_enabled.unwrap_or(false)
}

async fn run_scheduled_agent_task(state: DaemonState, task: ScheduledAgentTask) {
    let request = allthecodes_gateway::scheduled_task_run_request(&task);
    let task_id = task.id.clone();
    let event_id = format!("scheduled:{}", task_id);

    state.broadcast(SseEvent {
        id: String::new(),
        event_type: "scheduled_task_start".to_string(),
        data: json!({
            "task_id": task_id.as_str(),
            "cwd": task.cwd.as_str(),
            "source": request.source.redacted_json(),
            "prompt": request.prompt.as_str(),
        }),
    });

    let overrides = SubmitMessageOverrides {
        system_prompt_append_parts: vec![scheduled_task_system_prompt_part(&task)],
        ..SubmitMessageOverrides::default()
    };
    let stream = state.engine.submit_message_with_overrides(
        &request.prompt,
        QuerySource::ScheduledTask,
        overrides,
    );
    tokio::pin!(stream);
    while let Some(sdk_msg) = stream.next().await {
        if let Some(event) = super::routes::sdk_message_to_sse(&sdk_msg, event_id.as_str()) {
            state.broadcast(event);
        }
    }

    state.broadcast(SseEvent {
        id: String::new(),
        event_type: "scheduled_task_complete".to_string(),
        data: json!({
            "task_id": task_id.as_str(),
        }),
    });
}

fn scheduled_task_system_prompt_part(task: &ScheduledAgentTask) -> String {
    let mut lines = vec![
        "<scheduled_task>".to_string(),
        format!("id: {}", task.id),
        format!("cwd: {}", task.cwd),
        format!("schedule_kind: {}", scheduled_schedule_kind(&task.schedule)),
        format!("enabled: {}", task.enabled),
    ];
    if let Some(last_run_at) = &task.last_run_at {
        lines.push(format!("last_run_at: {last_run_at}"));
    }
    if let Some(next_run_at) = &task.next_run_at {
        lines.push(format!("next_run_at: {next_run_at}"));
    }
    lines.push("</scheduled_task>".to_string());
    lines.join("\n")
}

fn scheduled_schedule_kind(schedule: &ScheduleSpec) -> &'static str {
    match schedule {
        ScheduleSpec::Interval { .. } => "interval",
        ScheduleSpec::Once { .. } => "once",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_tasks::{ScheduleSpec, ScheduledAgentTask};

    #[test]
    fn scheduled_task_system_prompt_includes_registry_metadata() {
        let task = ScheduledAgentTask {
            id: "daily-review".to_string(),
            prompt: "summarize today's work".to_string(),
            cwd: "/repo".to_string(),
            schedule: ScheduleSpec::Interval {
                every_seconds: 3600,
            },
            enabled: true,
            last_run_at: None,
            next_run_at: Some("2026-07-04T12:00:00+00:00".to_string()),
        };

        let section = scheduled_task_system_prompt_part(&task);

        assert!(section.contains("<scheduled_task>"));
        assert!(section.contains("id: daily-review"));
        assert!(section.contains("cwd: /repo"));
        assert!(section.contains("schedule_kind: interval"));
        assert!(section.contains("next_run_at: 2026-07-04T12:00:00+00:00"));
        assert!(!section.contains("summarize today's work"));
    }

    #[test]
    fn scheduled_dispatch_requires_hermes_enabled() {
        let mut settings = allthecodes_engine::types::app_state::SettingsJson::default();
        assert!(!scheduled_dispatch_enabled_by_hermes(&settings));

        settings.hermes_enabled = Some(true);
        assert!(scheduled_dispatch_enabled_by_hermes(&settings));
    }
}
