//! SSE (Server-Sent Events) handler for the KAIROS daemon.
//!
//! Frontends connect to `GET /events?client_id=…&last_event_id=…` and receive
//! a continuous stream of [`SseEvent`]s.  On reconnection the client can pass
//! `last_event_id` to receive any events it missed while disconnected.

use std::convert::Infallible;

use axum::extract::{Query, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use futures::stream::Stream;
use serde::Deserialize;
use serde_json::json;
use tokio_stream::StreamExt;
use tracing::info;

use super::{
    state::{DaemonState, SseClient, SseEvent},
    supervisor::ASSISTANT_WORKER_ID,
};

/// Query parameters for the SSE endpoint.
#[derive(Debug, Deserialize)]
pub struct SseQuery {
    pub client_id: String,
    pub last_event_id: Option<String>,
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

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<SseEvent>();

    // Replay missed events.
    if let Some(ref last_id) = query.last_event_id {
        for event in state.events_since(last_id) {
            let _ = tx.send(event);
        }
    }
    for event in super::protocol_store()
        .read_worker_events(ASSISTANT_WORKER_ID)
        .unwrap_or_default()
    {
        let _ = tx.send(SseEvent {
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
    state.clients.write().insert(
        query.client_id.clone(),
        SseClient {
            client_id: query.client_id,
            tx,
            connected_at: std::time::Instant::now(),
        },
    );

    // Build the SSE stream.
    let stream = tokio_stream::wrappers::UnboundedReceiverStream::new(rx).map(|sse_event| {
        let event = Event::default()
            .id(sse_event.id)
            .event(sse_event.event_type)
            .json_data(sse_event.data)
            .unwrap_or_else(|_| Event::default());
        Ok(event)
    });

    Sse::new(stream).keep_alive(KeepAlive::default())
}
