//! SSE (Server-Sent Events) handler for the KAIROS daemon.
//!
//! Frontends connect to `GET /events?client_id=…&last_event_id=…` and receive
//! a continuous stream of [`SseEvent`]s.  On reconnection the client can pass
//! `last_event_id` to receive any events it missed while disconnected.

use std::convert::Infallible;

use axum::extract::{Query, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use futures::stream::{self, Stream};
use futures::StreamExt;
use serde::Deserialize;
use serde_json::json;
use tracing::info;

use super::{
    state::{DaemonState, SseEvent},
    supervisor::ASSISTANT_WORKER_ID,
};
use allthecodes_server::ConnectionId;

/// Query parameters for the SSE endpoint.
#[derive(Debug, Deserialize)]
pub struct SseQuery {
    pub client_id: String,
    pub last_event_id: Option<String>,
    pub after_seq: Option<u64>,
}

/// `GET /events` -- open an SSE stream.
///
/// The handler:
/// 1. Creates an unbounded channel for this client.
/// 2. Replays any missed events since `last_event_id` (if provided).
/// 3. Registers the client in `DaemonState::clients`.
/// 4. Returns an axum `Sse` stream that forwards events from the channel.
pub async fn sse_handler(
    State(state): State<DaemonState>,
    Query(query): Query<SseQuery>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    info!(client_id = query.client_id, "SSE client connected");

    let connection_id = ConnectionId::next();
    let replay_after = replay_after_from_query(&query);

    // Register before replay so events published during connection setup are
    // queued live. The replay high-watermark below is used to drop duplicates.
    let rx = state.register_sse_client(query.client_id.clone(), connection_id.clone());

    // Replay missed events.
    let (replay_high_watermark, mut replay_events) = buffered_replay_events(&state, replay_after);

    for event in super::protocol_store()
        .read_worker_events(ASSISTANT_WORKER_ID)
        .unwrap_or_default()
    {
        replay_events.push(SseEvent {
            id: event.event_id,
            event_type: format!("daemon_{}", event.event_type),
            data: json!({
                "worker_id": event.worker_id,
                "command_id": event.command_id,
                "event_type": event.event_type,
                "data": event.data,
                "created_at": event.created_at,
            }),
        });
    }

    // Build the SSE stream.
    let replay_stream = stream::iter(replay_events.into_iter().map(sse_to_event));
    let live_state = state.clone();
    let live_client_id = query.client_id;
    let live_connection_id = connection_id;
    let live_stream = stream::unfold((rx, false), move |(mut rx, lag_sent)| {
        let state = live_state.clone();
        let client_id = live_client_id.clone();
        let connection_id = live_connection_id.clone();
        async move {
            loop {
                match rx.recv().await {
                    Some(event) if event.seq <= replay_high_watermark => continue,
                    Some(event) => return Some((sse_to_event(event.message), (rx, lag_sent))),
                    None if !lag_sent => {
                        if let Some(skipped) = state.take_disconnect_lag(&connection_id) {
                            return Some((
                                sse_to_event(state.live_lagged_event(skipped)),
                                (rx, true),
                            ));
                        }
                        state.unregister_sse_client(&client_id, &connection_id);
                        return None;
                    }
                    None => {
                        state.unregister_sse_client(&client_id, &connection_id);
                        return None;
                    }
                }
            }
        }
    });
    let stream = replay_stream.chain(live_stream);

    Sse::new(stream).keep_alive(KeepAlive::default())
}

fn replay_after_from_query(query: &SseQuery) -> Option<u64> {
    query.after_seq.or_else(|| {
        query
            .last_event_id
            .as_deref()
            .and_then(|id| id.parse().ok())
    })
}

fn buffered_replay_events(state: &DaemonState, replay_after: Option<u64>) -> (u64, Vec<SseEvent>) {
    let replay = state.replay_after(replay_after);
    let replay_high_watermark = replay.high_watermark;
    let mut replay_events = Vec::new();
    if let Some(lagged) = state.lagged_event(&replay.status) {
        replay_events.push(lagged);
    }
    replay_events.extend(replay.events.into_iter().map(|event| event.message));
    (replay_high_watermark, replay_events)
}

fn sse_to_event(sse_event: SseEvent) -> Result<Event, Infallible> {
    let event = Event::default()
        .event(sse_event.event_type)
        .json_data(sse_event.data)
        .unwrap_or_else(|_| Event::default());
    let event = if sse_event.id.is_empty() {
        event
    } else {
        event.id(sse_event.id)
    };
    Ok(event)
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_config::features::FeatureFlags;
    use allthecodes_engine::lifecycle::QueryEngine;
    use allthecodes_engine::types::config::QueryEngineConfig;
    use std::sync::Arc;

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
            verification_policy: None,
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

    fn event(label: &str) -> SseEvent {
        SseEvent {
            id: String::new(),
            event_type: "system_info".to_string(),
            data: json!({ "label": label }),
        }
    }

    #[test]
    fn after_seq_takes_precedence_over_legacy_last_event_id() {
        let query = SseQuery {
            client_id: "client-1".to_string(),
            last_event_id: Some("1".to_string()),
            after_seq: Some(3),
        };

        assert_eq!(replay_after_from_query(&query), Some(3));
    }

    #[test]
    fn invalid_legacy_last_event_id_starts_at_live_tail() {
        let query = SseQuery {
            client_id: "client-1".to_string(),
            last_event_id: Some("not-a-seq".to_string()),
            after_seq: None,
        };

        assert_eq!(replay_after_from_query(&query), None);
    }

    #[test]
    fn buffered_replay_uses_after_seq_and_reports_high_watermark() {
        let state = make_daemon_state();
        state.broadcast(event("one"));
        state.broadcast(event("two"));
        state.broadcast(event("three"));

        let (high_watermark, events) = buffered_replay_events(&state, Some(1));

        assert_eq!(high_watermark, 3);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].id, "2");
        assert_eq!(events[0].data["label"], "two");
        assert_eq!(events[1].id, "3");
        assert_eq!(events[1].data["label"], "three");
    }

    #[test]
    fn buffered_replay_emits_lagged_event_for_compacted_cursor() {
        let state = make_daemon_state();
        state.event_log.append_with(|seq| SseEvent {
            id: seq.to_string(),
            event_type: "test".to_string(),
            data: json!({ "n": 1 }),
        });
        for n in 2..=1002u64 {
            state.event_log.append_with(|seq| SseEvent {
                id: seq.to_string(),
                event_type: "test".to_string(),
                data: json!({ "n": n }),
            });
        }

        let (high_watermark, events) = buffered_replay_events(&state, Some(0));

        assert_eq!(high_watermark, 1002);
        assert_eq!(events[0].event_type, "lagged");
        assert_eq!(events[0].id, "");
        assert_eq!(events[0].data["requested_after_seq"], 0);
        assert_eq!(events[0].data["oldest_seq"], 3);
        assert_eq!(events[0].data["latest_seq"], 1002);
        assert_eq!(events[0].data["skipped"], 2);
        assert_eq!(events[1].id, "3");
    }
}
