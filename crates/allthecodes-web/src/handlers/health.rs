//! Web health check handlers.

use allthecodes_protocol::v1::health::HealthResponse;
use allthecodes_protocol::{ApiError as ProtocolApiError, ApiMethod, NoParams};
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
    HandlerRegistry::new().handle(ApiMethod::Health, get(health_handler))
}

pub async fn health_handler(State(state): State<WebState>) -> Response {
    rest_processor_response::<HealthProcessor>(state, ApiMethod::Health, NoParams {}).await
}

#[derive(Clone)]
pub struct HealthProcessor {
    state: WebState,
}

impl From<WebState> for HealthProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for HealthProcessor {
    type Request = NoParams;
    type Response = HealthResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "health"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    async fn handle(&self, _params: Self::Request) -> Result<Self::Response, Self::Error> {
        self.state
            .web_ui_store
            .health()
            .await
            .map_err(|error| ProtocolApiError::Internal {
                message: format!("Web UI store health check failed: {error}"),
            })?;

        Ok(HealthResponse {
            status: "ok".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            db: "connected".to_string(),
        })
    }
}
