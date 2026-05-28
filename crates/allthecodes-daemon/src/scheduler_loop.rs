//! Persistent scheduled-task firing loop.

use std::sync::atomic::Ordering;
use std::time::Duration;

use futures::StreamExt;
use serde_json::json;
use tracing::{debug, info, warn};

use allthecodes_engine::types::config::QuerySource;
use allthecodes_services::scheduler::{SchedulerStore, TaskPayload};

use super::state::{next_event_id, DaemonState, SseEvent};

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

        let due = match store.due_tasks() {
            Ok(tasks) => tasks,
            Err(err) => {
                warn!(error = %err, "failed to read due scheduled tasks");
                continue;
            }
        };

        for task in due {
            if state.is_query_running.swap(true, Ordering::SeqCst) {
                break;
            }

            let prompt = match &task.payload {
                TaskPayload::Prompt(text) | TaskPayload::SlashCommand(text) => text.clone(),
            };
            let task_id = task.id.clone();
            let task_name = task.name.clone();
            let payload_kind = task.payload.kind_label();

            state.broadcast(SseEvent {
                id: next_event_id(),
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
                            id: next_event_id(),
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
                            id: next_event_id(),
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

            break;
        }
    }
}
