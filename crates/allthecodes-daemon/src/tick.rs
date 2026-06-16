//! Proactive tick loop — periodically triggers autonomous model execution.

use std::sync::atomic::Ordering;
use std::time::Duration;

use chrono::Local;
use futures::StreamExt;
use serde_json::json;
use tracing::{debug, info, warn};

use allthecodes_engine::types::config::QuerySource;

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

        // Skip if query running
        if state.is_query_running.load(Ordering::SeqCst) {
            debug!("tick skipped: query running");
            continue;
        }

        // Skip if sleeping
        if state.engine.is_sleeping() {
            debug!("tick skipped: sleeping");
            continue;
        }
        match super::process_state::active_sleep_state() {
            Ok(Some(sleep)) => {
                debug!(
                    sleeping_until = %sleep.sleeping_until.to_rfc3339(),
                    reason = ?sleep.reason,
                    "tick skipped: daemon sleep state active"
                );
                continue;
            }
            Ok(None) => {}
            Err(err) => warn!(error = %err, "failed to read daemon sleep state"),
        }

        let now = Local::now();
        let focus = state.terminal_focus();
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
