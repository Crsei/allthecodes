//! Task status handlers.
//!
//! Task status handlers.
//!
//! Rich background/process task state is not exposed through the backend yet.

use async_trait::async_trait;
use axum::extract::{Path as AxumPath, State};
use axum::response::Response;
use axum::routing::get;

use allthecodes_protocol::v1::tasks::{TaskDetailParams, TaskDetailResponse, TaskListResponse};
use allthecodes_protocol::ApiError as ProtocolApiError;
use allthecodes_protocol::ApiMethod;

use crate::api_dispatcher::rest_processor_response;
use crate::handler_registry::HandlerRegistry;
use crate::processors::Processor;
use crate::serialization::SerializationLayer;
use crate::state::WebState;

// ---------------------------------------------------------------------------
// Processor implementations
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct TaskListProcessor {
    state: WebState,
}

impl From<WebState> for TaskListProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for TaskListProcessor {
    type Request = allthecodes_protocol::NoParams;
    type Response = TaskListResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "tasks.list"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    async fn handle(&self, _params: Self::Request) -> Result<Self::Response, Self::Error> {
        let _ = &self.state;
        Err(ProtocolApiError::NotImplemented {
            capability: "background_task_status_events".to_string(),
        })
    }
}

#[derive(Clone)]
pub struct TaskDetailProcessor {
    state: WebState,
}

impl From<WebState> for TaskDetailProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for TaskDetailProcessor {
    type Request = TaskDetailParams;
    type Response = TaskDetailResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "tasks.detail"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    async fn handle(&self, _params: Self::Request) -> Result<Self::Response, Self::Error> {
        let _ = &self.state;
        Err(ProtocolApiError::NotImplemented {
            capability: "background_task_status_events".to_string(),
        })
    }
}

// ---------------------------------------------------------------------------
// Axum handler bridge functions
// ---------------------------------------------------------------------------

async fn list_handler(State(state): State<WebState>) -> Response {
    rest_processor_response::<TaskListProcessor>(
        state,
        ApiMethod::TaskList,
        allthecodes_protocol::NoParams {},
    )
    .await
}

async fn detail_handler(State(state): State<WebState>, AxumPath(id): AxumPath<String>) -> Response {
    rest_processor_response::<TaskDetailProcessor>(
        state,
        ApiMethod::TaskDetail,
        TaskDetailParams { id },
    )
    .await
}

// ---------------------------------------------------------------------------
// Handler registry
// ---------------------------------------------------------------------------

pub fn handlers() -> HandlerRegistry {
    HandlerRegistry::new()
        .handle(ApiMethod::TaskList, get(list_handler))
        .handle(ApiMethod::TaskDetail, get(detail_handler))
}
