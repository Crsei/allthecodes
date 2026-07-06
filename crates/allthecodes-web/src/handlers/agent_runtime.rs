//! Agent runtime dashboard handlers.

use allthecodes_protocol::v1::agent_runtime::{
    AgentRuntimeDashboardQuery, AgentRuntimeDashboardResponse,
};
use allthecodes_protocol::ApiError as ProtocolApiError;
use allthecodes_protocol::ApiMethod;
use allthecodes_services::agent_runtime_history::load_dashboard_snapshot;
use async_trait::async_trait;
use axum::extract::{Query, State};
use axum::response::Response;
use axum::routing::get;

use crate::api_dispatcher::rest_processor_response;
use crate::handler_registry::HandlerRegistry;
use crate::processors::Processor;
use crate::serialization::SerializationLayer;
use crate::state::WebState;

#[derive(Clone)]
pub struct AgentRuntimeDashboardProcessor {
    state: WebState,
}

impl From<WebState> for AgentRuntimeDashboardProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for AgentRuntimeDashboardProcessor {
    type Request = AgentRuntimeDashboardQuery;
    type Response = AgentRuntimeDashboardResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "agent_runtime.dashboard"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    async fn handle(&self, mut query: Self::Request) -> Result<Self::Response, Self::Error> {
        if query.session_id.as_deref().is_none_or(str::is_empty) {
            query.session_id = Some(self.state.engine().current_session_id().to_string());
        }
        load_dashboard_snapshot(query).map_err(|error| ProtocolApiError::Internal {
            message: error.to_string(),
        })
    }
}

async fn dashboard_handler(
    State(state): State<WebState>,
    Query(query): Query<AgentRuntimeDashboardQuery>,
) -> Response {
    rest_processor_response::<AgentRuntimeDashboardProcessor>(
        state,
        ApiMethod::AgentRuntimeDashboard,
        query,
    )
    .await
}

pub(crate) fn handlers() -> HandlerRegistry {
    HandlerRegistry::new().handle(ApiMethod::AgentRuntimeDashboard, get(dashboard_handler))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::test_support::{make_web_state, temp_home};
    use allthecodes_services::agent_runtime_history::{
        persist_execution_record, AgentRuntimeAgentStatus,
    };
    use allthecodes_types::agent_runtime_record::AgentRuntimeExecutionRecord;

    #[tokio::test]
    #[serial_test::serial]
    async fn processor_returns_current_session_dashboard_by_default() {
        let (_home, _guard) = temp_home();
        let state = make_web_state();
        let session_id = state.engine().current_session_id().to_string();
        persist_execution_record(&AgentRuntimeExecutionRecord {
            session_id: session_id.clone(),
            agent_id: "agent-web".to_string(),
            tool: "shell".to_string(),
            ..AgentRuntimeExecutionRecord::default()
        })
        .expect("persist execution record");

        let snapshot = AgentRuntimeDashboardProcessor::from(state)
            .handle(AgentRuntimeDashboardQuery::default())
            .await
            .expect("dashboard snapshot");

        assert_eq!(snapshot.session_id.as_deref(), Some(session_id.as_str()));
        assert_eq!(snapshot.summary.total_agents, 1);
        assert_eq!(snapshot.agents[0].agent_id, "agent-web");
        assert_eq!(snapshot.agents[0].status, AgentRuntimeAgentStatus::Running);
    }
}
