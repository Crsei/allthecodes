//! Capabilities discovery handlers.

use std::collections::HashMap;

use allthecodes_protocol::v1::capabilities::{
    BackendCapabilityFacts, BuildCapabilityFacts, CapabilityFeatureStatus, ProtocolCapabilityFacts,
    SchemaCapabilityFacts,
};
use allthecodes_protocol::ApiError as ProtocolApiError;
use allthecodes_protocol::ApiMethod;
use allthecodes_protocol::NoParams;
use allthecodes_protocol::API_METADATA;
use async_trait::async_trait;
use axum::extract::State;
use axum::response::Response;
use axum::routing::get;
use chrono::{SecondsFormat, Utc};

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

fn feature_status_map(
    capabilities: &HashMap<String, bool>,
) -> HashMap<String, CapabilityFeatureStatus> {
    capabilities
        .iter()
        .map(|(name, available)| {
            (
                name.clone(),
                CapabilityFeatureStatus {
                    available: *available,
                    reason: if *available {
                        None
                    } else {
                        Some("Not available in this backend build".to_string())
                    },
                },
            )
        })
        .collect()
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
        let capabilities = capabilities_map();
        Ok(Self::Response {
            features: Some(feature_status_map(&capabilities)),
            capabilities,
            backend: Some(BackendCapabilityFacts {
                name: "allthecodes".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                status: Some("running".to_string()),
            }),
            protocol: Some(ProtocolCapabilityFacts {
                version: "v1".to_string(),
                route_count: Some(API_METADATA.len()),
            }),
            schema: Some(SchemaCapabilityFacts {
                version: "v1".to_string(),
                format: Some("json-schema".to_string()),
            }),
            build: Some(BuildCapabilityFacts {
                package_version: env!("CARGO_PKG_VERSION").to_string(),
                target: option_env!("TARGET").map(str::to_string),
                profile: option_env!("PROFILE").map(str::to_string),
            }),
            checked_at: Some(Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_features_mirror_legacy_map() {
        let capabilities = capabilities_map();
        let features = feature_status_map(&capabilities);

        assert_eq!(
            features.get("chat").map(|feature| feature.available),
            Some(true)
        );
        assert_eq!(
            features.get("git").map(|feature| feature.available),
            Some(true)
        );
        assert!(!features.contains_key("allthecodes:update-check"));
    }
}
