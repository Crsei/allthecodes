//! Capabilities discovery handlers.

use std::collections::HashMap;

use allthecodes_protocol::ApiError as ProtocolApiError;
use allthecodes_protocol::ApiMethod;
use allthecodes_protocol::NoParams;
use async_trait::async_trait;
use axum::extract::State;
use axum::response::Response;
use axum::routing::get;

use crate::api_dispatcher::rest_processor_response;
use crate::handler_registry::HandlerRegistry;
use crate::processors::Processor;
use crate::serialization::SerializationLayer;
use crate::state::WebState;

pub(crate) fn handlers() -> HandlerRegistry {
    HandlerRegistry::new().handle(ApiMethod::Capabilities, get(capabilities_handler))
}

pub async fn capabilities_handler(State(state): State<WebState>) -> Response {
    rest_processor_response::<CapabilitiesProcessor>(state, ApiMethod::Capabilities, NoParams {})
        .await
}

pub fn capabilities_map() -> HashMap<String, bool> {
    let mut caps = HashMap::new();
    // Ready capabilities
    caps.insert("chat".into(), true);
    caps.insert("sessions".into(), true);
    caps.insert("settings".into(), true);
    caps.insert("agents".into(), true);
    caps.insert("people".into(), true);
    caps.insert("hooks".into(), true);
    caps.insert("prompts".into(), true);
    caps.insert("mcp_servers".into(), true);
    caps.insert("plugins".into(), true);
    caps.insert("channels".into(), true);
    caps.insert("computer_use".into(), true);
    caps.insert("appshots".into(), true);
    caps.insert("activity_recorder".into(), true);
    caps.insert("chrome_relay".into(), true);
    caps.insert("git".into(), true);
    caps.insert("proxy".into(), true);
    caps.insert("debug".into(), true);
    caps.insert("state".into(), true);
    // Not yet implemented
    caps.insert("auth".into(), true);
    caps.insert("profiles".into(), true);
    caps.insert("gateways".into(), true);
    caps.insert("models".into(), true);
    caps.insert("providers".into(), true);
    caps.insert("credentials".into(), true);
    caps.insert("usage".into(), true);
    caps.insert("skills".into(), true);
    caps.insert("memory".into(), true);
    caps.insert("speech".into(), true);
    caps.insert("tts".into(), true);
    caps.insert("web_search".into(), true);
    caps.insert("network".into(), true);
    caps.insert("data".into(), true);
    caps.insert("token_savings".into(), true);
    caps.insert("kanban".into(), true);
    caps.insert("jobs".into(), true);
    caps.insert("group_chat".into(), true);
    caps.insert("files".into(), true);
    caps.insert("logs".into(), true);
    caps.insert("backend_services".into(), true);
    caps
}

#[derive(Clone)]
pub struct CapabilitiesProcessor {
    state: WebState,
}

impl From<WebState> for CapabilitiesProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for CapabilitiesProcessor {
    type Request = NoParams;
    type Response = allthecodes_protocol::v1::capabilities::CapabilityDiscoveryResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "capabilities"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    async fn handle(&self, _params: Self::Request) -> Result<Self::Response, Self::Error> {
        Ok(Self::Response {
            capabilities: capabilities_map(),
        })
    }
}
