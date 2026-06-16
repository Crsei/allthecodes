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
    let replay_after = query.after_seq.or_else(|| {
        query
            .last_event_id
            .as_deref()
            .and_then(|id| id.parse().ok())
    });
    let mut replay_events = Vec::new();

    // Replay missed events.
    let replay = state.replay_after(replay_after);
    if let Some(lagged) = state.lagged_event(&replay.status) {
        replay_events.push(lagged);
    }
    replay_events.extend(replay.events.into_iter().map(|event| event.message));

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

    // Register client.
    let rx = state.register_sse_client(query.client_id.clone(), connection_id.clone());

    // Build the SSE stream.
    let replay_stream = stream::iter(replay_events.into_iter().map(sse_to_event));
    let live_state = state.clone();
    let live_client_id = query.client_id;
    let live_connection_id = connection_id;
    let live_stream = stream::unfold(rx, move |mut rx| {
        let state = live_state.clone();
        let client_id = live_client_id.clone();
        let connection_id = live_connection_id.clone();
        async move {
            match rx.recv().await {
                Some(event) => Some((sse_to_event(event.message), rx)),
                None => {
                    state.unregister_sse_client(&client_id, &connection_id);
                    None
                }
            }
        }
    });
    let stream = replay_stream.chain(live_stream);

    Sse::new(stream).keep_alive(KeepAlive::default())
}

fn sse_to_event(sse_event: SseEvent) -> Result<Event, Infallible> {
    let event = Event::default()
        .id(sse_event.id)
        .event(sse_event.event_type)
        .json_data(sse_event.data)
        .unwrap_or_else(|_| Event::default());
    Ok(event)
}
