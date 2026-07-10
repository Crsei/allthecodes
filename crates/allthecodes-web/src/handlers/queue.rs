//! Queue management handlers.
//!
//! Provides an in-memory queue of prompts that can be listed, added, edited,
//! removed, or immediately sent (flushed to the front of the submit queue).

use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post};
use axum::Json;
use chrono::Utc;
use futures::StreamExt;

use allthecodes_engine::types::config::QuerySource;
use allthecodes_protocol::v1::queue::{
    QueueAddRequest, QueueAddResponse, QueueListResponse, QueueRemoveResponse,
    QueueSendNowResponse, QueueUpdateRequest, QueueUpdateResponse, QueuedItem,
};
use allthecodes_protocol::ApiError as ProtocolApiError;
use allthecodes_protocol::ApiMethod;

use crate::api_dispatcher::rest_processor_response;
use crate::handler_registry::HandlerRegistry;
use crate::processors::Processor;
use crate::serialization::SerializationLayer;
use crate::state::{QueueEntry, WebState};

static QUEUE_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

fn new_queue_id() -> String {
    let now = Utc::now().timestamp_nanos_opt().unwrap_or(0);
    let counter = QUEUE_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("q-{now}-{counter}")
}

// ---------------------------------------------------------------------------
// Processor implementations
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct QueueListProcessor {
    state: WebState,
}

impl From<WebState> for QueueListProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for QueueListProcessor {
    type Request = allthecodes_protocol::NoParams;
    type Response = QueueListResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "queue.list"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    async fn handle(&self, _params: Self::Request) -> Result<Self::Response, Self::Error> {
        let queue = self.state.queue.read();
        let items: Vec<QueuedItem> = queue
            .iter()
            .enumerate()
            .map(|(i, item)| QueuedItem {
                id: item.id.clone(),
                text: item.text.clone(),
                session_id: item.session_id.clone(),
                position: i,
                created_at: item.created_at.clone(),
            })
            .collect();
        Ok(QueueListResponse { items })
    }
}

#[derive(Clone)]
pub struct QueueAddProcessor {
    state: WebState,
}

impl From<WebState> for QueueAddProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for QueueAddProcessor {
    type Request = QueueAddRequest;
    type Response = QueueAddResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "queue.add"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    async fn handle(&self, req: Self::Request) -> Result<Self::Response, Self::Error> {
        let mut queue = self.state.queue.write();
        let position = queue.len();
        let entry = QueueEntry {
            id: new_queue_id(),
            text: req.text,
            session_id: req.session_id,
            created_at: Utc::now().to_rfc3339(),
        };
        let response = QueuedItem {
            id: entry.id.clone(),
            text: entry.text.clone(),
            session_id: entry.session_id.clone(),
            position,
            created_at: entry.created_at.clone(),
        };
        queue.push_back(entry);
        Ok(QueueAddResponse { item: response })
    }
}

// ---------------------------------------------------------------------------
// Axum handler bridge functions
// ---------------------------------------------------------------------------

async fn list_handler(State(state): State<WebState>) -> Response {
    rest_processor_response::<QueueListProcessor>(
        state,
        ApiMethod::QueueList,
        allthecodes_protocol::NoParams {},
    )
    .await
}

async fn add_handler(State(state): State<WebState>, Json(req): Json<QueueAddRequest>) -> Response {
    rest_processor_response::<QueueAddProcessor>(state, ApiMethod::QueueAdd, req).await
}

async fn update_handler(
    State(state): State<WebState>,
    AxumPath(id): AxumPath<String>,
    Json(req): Json<QueueUpdateRequest>,
) -> Response {
    let mut queue = state.queue.write();
    if let Some(pos) = queue.iter().position(|e| e.id == id) {
        queue[pos].text = req.text;
        let item = QueuedItem {
            id: queue[pos].id.clone(),
            text: queue[pos].text.clone(),
            session_id: queue[pos].session_id.clone(),
            position: pos,
            created_at: queue[pos].created_at.clone(),
        };
        Json(QueueUpdateResponse { item }).into_response()
    } else {
        (StatusCode::NOT_FOUND, "queue item not found").into_response()
    }
}

async fn remove_handler(State(state): State<WebState>, AxumPath(id): AxumPath<String>) -> Response {
    let mut queue = state.queue.write();
    let original_len = queue.len();
    queue.retain(|e| e.id != id);
    let removed = queue.len() < original_len;
    Json(QueueRemoveResponse { ok: removed }).into_response()
}

async fn send_now_handler(
    State(state): State<WebState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let entry = {
        let queue = state.queue.read();
        match queue.iter().find(|e| e.id == id) {
            Some(entry) => entry.clone(),
            None => return (StatusCode::NOT_FOUND, "queue item not found").into_response(),
        }
    };

    if state.is_session_streaming(&entry.session_id) {
        return crate::api_errors::protocol_error_response(ProtocolApiError::EngineBusy)
            .into_response();
    }

    let engine = match state.engine_for_session(&entry.session_id) {
        Some(engine) => engine,
        None => {
            match crate::handlers::sessions::build_engine_for_session(&state, &entry.session_id) {
                Ok(engine) => {
                    state.cache_session_engine(engine.clone());
                    engine
                }
                Err(error) => {
                    return crate::api_errors::protocol_error_response(error).into_response();
                }
            }
        }
    };

    let removed = {
        let mut queue = state.queue.write();
        let Some(pos) = queue.iter().position(|e| e.id == id) else {
            return (StatusCode::NOT_FOUND, "queue item not found").into_response();
        };
        let Some(removed) = queue.remove(pos) else {
            return (StatusCode::NOT_FOUND, "queue item not found").into_response();
        };
        removed
    };

    let response = QueueSendNowResponse {
        item: QueuedItem {
            id: removed.id.clone(),
            text: removed.text.clone(),
            session_id: removed.session_id.clone(),
            position: 0,
            created_at: removed.created_at.clone(),
        },
    };

    let stream_state = state.clone();
    let stream_engine = engine.clone();
    let prompt = removed.text.clone();
    let session_id = removed.session_id.clone();
    stream_engine.reset_abort();
    stream_state.set_session_streaming(&session_id, true);
    tokio::spawn(async move {
        let mut stream = stream_engine.submit_message(&prompt, QuerySource::Sdk);
        while stream.next().await.is_some() {}
        stream_state.set_session_streaming(&session_id, false);
    });

    tracing::info!(
        queue_id = %removed.id,
        session_id = %removed.session_id,
        "queue.send_now: item submitted"
    );
    Json(response).into_response()
}

// ---------------------------------------------------------------------------
// Handler registry
// ---------------------------------------------------------------------------

pub fn handlers() -> HandlerRegistry {
    HandlerRegistry::new()
        .handle(ApiMethod::QueueList, get(list_handler))
        .handle(ApiMethod::QueueAdd, post(add_handler))
        .handle(ApiMethod::QueueUpdate, patch(update_handler))
        .handle(ApiMethod::QueueRemove, delete(remove_handler))
        .handle(ApiMethod::QueueSendNow, post(send_now_handler))
}
