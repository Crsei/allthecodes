//! Shared API dispatcher for transport adapters.
//!
//! REST handlers, direct in-process calls, and future API WebSocket adapters
//! should converge here once their DTOs are stable. PTY, MCP, browser native
//! host, and daemon SSE remain outside this dispatcher boundary.

use async_trait::async_trait;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use tracing::Instrument;

use allthecodes_protocol::v1;
use allthecodes_protocol::{
    ApiError, ApiMethod, ClientRequest, ClientResponse, MessageProcessor, NoParams,
    ServerNotification, API_METADATA,
};

use crate::handlers;
use crate::processors::{dispatch_processor, protocol_error_response, Processor};
use crate::state::WebState;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ApiConnectionId(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ApiTransportKind {
    Rest,
    JsonRpcWebSocket,
    Direct,
    IpcBridge,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiRequestContext {
    pub connection_id: Option<ApiConnectionId>,
    pub transport: ApiTransportKind,
}

impl ApiRequestContext {
    pub fn rest() -> Self {
        Self {
            connection_id: None,
            transport: ApiTransportKind::Rest,
        }
    }

    pub fn direct() -> Self {
        Self {
            connection_id: None,
            transport: ApiTransportKind::Direct,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ApiDispatcherMigrationState {
    Dispatched,
    LegacyRestHandler,
    DedicatedTransport,
    LegacyIpcBridge,
}

pub(crate) const DISPATCHED_OPERATIONS: &[ApiMethod] = &[
    ApiMethod::Capabilities,
    ApiMethod::SessionList,
    ApiMethod::SessionDetail,
    ApiMethod::SessionResume,
    ApiMethod::SessionArchive,
];

const DEDICATED_TRANSPORT_OPERATIONS: &[ApiMethod] = &[
    ApiMethod::TerminalProfiles,
    ApiMethod::TerminalSessionsList,
    ApiMethod::TerminalSessionsCreate,
    ApiMethod::TerminalSessionDetail,
    ApiMethod::TerminalSessionDelete,
    ApiMethod::TerminalSessionWs,
    ApiMethod::TuiWs,
];

pub(crate) fn dispatcher_migration_state(operation: ApiMethod) -> ApiDispatcherMigrationState {
    if DISPATCHED_OPERATIONS.contains(&operation) {
        ApiDispatcherMigrationState::Dispatched
    } else if DEDICATED_TRANSPORT_OPERATIONS.contains(&operation) {
        ApiDispatcherMigrationState::DedicatedTransport
    } else if operation == ApiMethod::IpcWs {
        ApiDispatcherMigrationState::LegacyIpcBridge
    } else {
        ApiDispatcherMigrationState::LegacyRestHandler
    }
}

#[derive(Clone)]
pub struct ApiDispatcher {
    state: WebState,
    default_context: ApiRequestContext,
}

impl ApiDispatcher {
    pub fn new(state: WebState) -> Self {
        Self {
            state,
            default_context: ApiRequestContext::direct(),
        }
    }

    pub fn with_default_context(mut self, context: ApiRequestContext) -> Self {
        self.default_context = context;
        self
    }

    pub async fn dispatch(
        &self,
        context: ApiRequestContext,
        request: ClientRequest,
    ) -> Result<ClientResponse, ApiError> {
        dispatch(self.state.clone(), context, request).await
    }
}

#[async_trait]
impl MessageProcessor for ApiDispatcher {
    async fn process_request(&self, request: ClientRequest) -> Result<ClientResponse, ApiError> {
        self.dispatch(self.default_context.clone(), request).await
    }

    async fn process_notification(&self, notification: ServerNotification) -> Result<(), ApiError> {
        match notification {}
    }
}

pub async fn dispatch(
    state: WebState,
    context: ApiRequestContext,
    request: ClientRequest,
) -> Result<ClientResponse, ApiError> {
    match request {
        ClientRequest::Capabilities(params) => {
            let response = dispatch_tracked_processor::<handlers::CapabilitiesProcessor>(
                state,
                context,
                ApiMethod::Capabilities,
                params,
            )
            .await?;
            Ok(ClientResponse::Capabilities(response))
        }
        ClientRequest::SessionList(NoParams {}) => {
            let response = dispatch_tracked_processor::<handlers::SessionListProcessor>(
                state,
                context,
                ApiMethod::SessionList,
                NoParams {},
            )
            .await?;
            Ok(ClientResponse::SessionList(map_session_list(response)))
        }
        ClientRequest::SessionDetail(params) => {
            let response = dispatch_tracked_processor::<handlers::SessionDetailProcessor>(
                state,
                context,
                ApiMethod::SessionDetail,
                params,
            )
            .await?;
            Ok(ClientResponse::SessionDetail(map_session_detail(response)))
        }
        ClientRequest::SessionResume(params) => {
            let response = dispatch_tracked_processor::<handlers::SessionResumeProcessor>(
                state,
                context,
                ApiMethod::SessionResume,
                params,
            )
            .await?;
            Ok(ClientResponse::SessionResume(v1::SessionResumeResponse {
                id: response.session_id,
                resumed: true,
            }))
        }
        ClientRequest::SessionArchive(params) => {
            let id = params.id.clone();
            dispatch_tracked_processor::<handlers::SessionArchiveProcessor>(
                state,
                context,
                ApiMethod::SessionArchive,
                params,
            )
            .await?;
            Ok(ClientResponse::SessionArchive(v1::SessionArchiveResponse {
                id,
                archived: true,
            }))
        }
        other => Err(ApiError::NotImplemented {
            capability: format!("{:?}", other.method()),
        }),
    }
}

pub(crate) async fn rest_processor_response<P>(
    state: WebState,
    operation: ApiMethod,
    params: P::Request,
) -> Response
where
    P: Processor + From<WebState>,
    P::Response: Serialize,
{
    match dispatch_rest_processor::<P>(state, operation, params).await {
        Ok(response) => Json(response).into_response(),
        Err(error) => protocol_error_response(error),
    }
}

pub(crate) async fn dispatch_rest_processor<P>(
    state: WebState,
    operation: ApiMethod,
    params: P::Request,
) -> Result<P::Response, ApiError>
where
    P: Processor + From<WebState>,
{
    dispatch_tracked_processor::<P>(state, ApiRequestContext::rest(), operation, params).await
}

async fn dispatch_tracked_processor<P>(
    state: WebState,
    context: ApiRequestContext,
    operation: ApiMethod,
    params: P::Request,
) -> Result<P::Response, ApiError>
where
    P: Processor + From<WebState>,
{
    let span = tracing::info_span!(
        "api.dispatch",
        method = ?operation,
        transport = ?context.transport,
        connection_id = context.connection_id.as_ref().map(|id| id.0.as_str()),
    );

    async move {
        if dispatcher_migration_state(operation) != ApiDispatcherMigrationState::Dispatched {
            return Err(ApiError::NotImplemented {
                capability: format!("{operation:?}"),
            });
        }

        if let Some(reason) = experimental_reason(operation) {
            if !experimental_apis_enabled() {
                return Err(ApiError::Experimental(reason.to_string()));
            }
        }

        dispatch_processor(P::from(state), params).await
    }
    .instrument(span)
    .await
}

fn experimental_reason(operation: ApiMethod) -> Option<&'static str> {
    API_METADATA
        .iter()
        .find(|metadata| metadata.endpoint.operation == operation)
        .and_then(|metadata| metadata.experimental)
}

fn experimental_apis_enabled() -> bool {
    std::env::var("ALLTHECODES_ENABLE_EXPERIMENTAL_API")
        .ok()
        .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "on"))
}

fn map_session_list(response: handlers::SessionListResponse) -> v1::SessionListResponse {
    v1::SessionListResponse {
        sessions: response
            .sessions
            .into_iter()
            .map(|session| v1::SessionSummary {
                id: session.session_id,
                title: Some(session.title),
                archived: false,
            })
            .collect(),
    }
}

fn map_session_detail(response: handlers::SessionDetailResponse) -> v1::SessionDetailResponse {
    v1::SessionDetailResponse {
        session: v1::SessionSummary {
            id: response.session_id,
            title: Some(response.title),
            archived: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    use allthecodes_engine::lifecycle::QueryEngine;
    use allthecodes_engine::types::config::QueryEngineConfig;
    use allthecodes_protocol::{DirectTransport, Transport};
    use axum::http::StatusCode;

    use super::*;

    fn make_web_state() -> WebState {
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
        WebState::new(engine, Arc::new(AtomicBool::new(false)))
    }

    #[tokio::test]
    async fn dispatcher_handles_capabilities_request() {
        let response = dispatch(
            make_web_state(),
            ApiRequestContext::direct(),
            ClientRequest::Capabilities(NoParams {}),
        )
        .await
        .expect("capabilities should dispatch");

        match response {
            ClientResponse::Capabilities(body) => {
                assert_eq!(body.capabilities.get("sessions"), Some(&true));
            }
            other => panic!("unexpected response: {other:?}"),
        }
    }

    #[tokio::test]
    async fn direct_transport_uses_api_dispatcher() {
        let transport = DirectTransport::new(ApiDispatcher::new(make_web_state()));
        let response = transport
            .send_request(ClientRequest::Capabilities(NoParams {}))
            .await
            .expect("direct dispatcher request should succeed");

        assert!(matches!(response, ClientResponse::Capabilities(_)));
    }

    #[test]
    fn migration_tracker_marks_dispatched_operations() {
        assert_eq!(DISPATCHED_OPERATIONS.len(), 5);

        for operation in DISPATCHED_OPERATIONS {
            assert_eq!(
                dispatcher_migration_state(*operation),
                ApiDispatcherMigrationState::Dispatched,
                "{operation:?} should enter ApiDispatcher"
            );
        }
    }

    #[test]
    fn migration_tracker_keeps_dedicated_transports_out() {
        for operation in [
            ApiMethod::TerminalProfiles,
            ApiMethod::TerminalSessionsList,
            ApiMethod::TerminalSessionsCreate,
            ApiMethod::TerminalSessionDetail,
            ApiMethod::TerminalSessionDelete,
            ApiMethod::TerminalSessionWs,
            ApiMethod::TuiWs,
        ] {
            assert_eq!(
                dispatcher_migration_state(operation),
                ApiDispatcherMigrationState::DedicatedTransport
            );
        }

        assert_eq!(
            dispatcher_migration_state(ApiMethod::IpcWs),
            ApiDispatcherMigrationState::LegacyIpcBridge
        );
    }

    #[tokio::test]
    async fn rest_processor_response_uses_dispatcher_lifecycle() {
        let response = rest_processor_response::<handlers::CapabilitiesProcessor>(
            make_web_state(),
            ApiMethod::Capabilities,
            NoParams {},
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn rest_processor_rejects_untracked_operations() {
        let result = dispatch_rest_processor::<handlers::CapabilitiesProcessor>(
            make_web_state(),
            ApiMethod::State,
            NoParams {},
        )
        .await;

        assert!(matches!(result, Err(ApiError::NotImplemented { .. })));
    }

    #[tokio::test]
    async fn dispatcher_routes_session_requests_through_processors() {
        let error = dispatch(
            make_web_state(),
            ApiRequestContext::direct(),
            ClientRequest::SessionDetail(v1::SessionDetailParams {
                id: "missing-session-for-dispatcher-test".to_string(),
            }),
        )
        .await
        .expect_err("missing session should surface as protocol not_found");

        assert!(matches!(
            error,
            ApiError::NotFound {
                entity: "session",
                ..
            }
        ));
    }

    #[tokio::test]
    async fn unsupported_request_returns_protocol_error() {
        let error = dispatch(
            make_web_state(),
            ApiRequestContext::direct(),
            ClientRequest::TerminalSessionsCreate(NoParams {}),
        )
        .await
        .expect_err("terminal transport must remain out of dispatcher");

        assert!(matches!(error, ApiError::NotImplemented { .. }));
    }
}
