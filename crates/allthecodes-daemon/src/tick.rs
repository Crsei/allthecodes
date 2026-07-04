//! Proactive tick loop — periodically triggers autonomous model execution.

use std::sync::atomic::Ordering;
use std::time::Duration;

use chrono::Local;
use futures::StreamExt;
use serde_json::json;
use tracing::{debug, info};

use allthecodes_engine::types::config::QuerySource;

use super::automation_state::AutomationStatus;
use super::memory_log::append_log_entry;
use super::state::{DaemonState, SseEvent};

const DEFAULT_TICK_INTERVAL_MS: u64 = 30_000;

pub async fn tick_loop(state: DaemonState) {
    let mut interval = tokio::time::interval(Duration::from_millis(DEFAULT_TICK_INTERVAL_MS));
    info!(
        "proactive tick loop started (interval: {}ms)",
        DEFAULT_TICK_INTERVAL_MS
    );

    // Skip first immediate tick
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
                "tick skipped: automation state is not idle"
            );
            continue;
        }

        let now = Local::now();
        let focus = automation.terminal_focus;
        let today_log = super::memory_log::read_today_log();
        let tick_prompt = format!(
            "<tick_tag>\nLocal time: {}\nTerminal focus: {}\n</tick_tag>{}",
            now.format("%Y-%m-%d %H:%M:%S"),
            focus,
            if today_log.is_empty() {
                String::new()
            } else {
                format!("\n<daily_log>\n{}</daily_log>", today_log)
            },
        );

        debug!("proactive tick firing at {}", now.format("%H:%M:%S"));
        append_log_entry(&format!("proactive tick fired (focus={})", focus));

        // Notify frontend
        state.broadcast(SseEvent {
            id: String::new(),
            event_type: "autonomous_start".to_string(),
            data: json!({"source": "proactive_tick", "time": now.to_rfc3339()}),
        });

        // Submit to engine
        state.is_query_running.store(true, Ordering::SeqCst);
        let engine = state.engine.clone();
        let state_clone = state.clone();

        tokio::spawn(async move {
            let stream = engine.submit_message(&tick_prompt, QuerySource::ProactiveTick);
            tokio::pin!(stream);
            while let Some(sdk_msg) = stream.next().await {
                if let Some(event) = super::routes::sdk_message_to_sse(&sdk_msg, "tick") {
                    state_clone.broadcast(event);
                }
            }
            state_clone.is_query_running.store(false, Ordering::SeqCst);
        });
    }
}
