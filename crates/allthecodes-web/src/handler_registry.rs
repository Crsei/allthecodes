use allthecodes_protocol::{ApiMethod, ALL_ENDPOINTS};
use axum::routing::{get, post, MethodRouter};
use axum::{Json, Router};
use serde::Serialize;
use tracing::error;

use crate::handlers;
use crate::state::WebState;

#[derive(Clone)]
pub struct HandlerEntry {
    pub operation: ApiMethod,
    pub router: MethodRouter<WebState>,
}

#[derive(Clone, Default)]
pub struct HandlerRegistry {
    entries: Vec<HandlerEntry>,
}

impl HandlerRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn handle(mut self, operation: ApiMethod, router: MethodRouter<WebState>) -> Self {
        self.entries.push(HandlerEntry { operation, router });
        self
    }

    pub fn entries(&self) -> &[HandlerEntry] {
        &self.entries
    }

    pub fn validate_registered_endpoints(&self) -> Result<(), MissingEndpointDefinition> {
        for entry in &self.entries {
            if protocol_endpoint(entry.operation).is_none() {
                return Err(MissingEndpointDefinition {
                    operation: entry.operation,
                });
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MissingEndpointDefinition {
    pub operation: ApiMethod,
}

pub fn session_handlers() -> HandlerRegistry {
    HandlerRegistry::new()
        .handle(ApiMethod::SessionList, get(handlers::sessions_list_handler))
        .handle(ApiMethod::SessionCreate, post(handlers::session_new_handler))
        .handle(ApiMethod::SessionDetail, get(handlers::session_detail_handler))
        .handle(
            ApiMethod::SessionResume,
            post(handlers::session_resume_handler),
        )
        .handle(
            ApiMethod::SessionArchive,
            post(handlers::session_archive_handler),
        )
}

pub fn register_protocol_routes(
    mut router: Router<WebState>,
    registry: &HandlerRegistry,
) -> Router<WebState> {
    for entry in registry.entries() {
        let Some(endpoint) = protocol_endpoint(entry.operation) else {
            error!(
                "registered handler has no protocol endpoint definition: {:?}",
                entry.operation
            );
            continue;
        };

        router = router.route(endpoint.path, entry.router.clone());
    }

    router
}

pub async fn protocol_routes_handler() -> Json<Vec<ProtocolRouteInfo>> {
    Json(
        ALL_ENDPOINTS
            .iter()
            .map(|endpoint| ProtocolRouteInfo {
                operation: format!("{:?}", endpoint.operation),
                http_method: endpoint.http_method,
                path: endpoint.path,
            })
            .collect(),
    )
}

fn protocol_endpoint(operation: ApiMethod) -> Option<&'static allthecodes_protocol::ApiEndpoint> {
    ALL_ENDPOINTS
        .iter()
        .find(|endpoint| endpoint.operation == operation)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProtocolRouteInfo {
    pub operation: String,
    pub http_method: &'static str,
    pub path: &'static str,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_registry_covers_current_protocol_endpoints() {
        let registry = session_handlers();

        registry
            .validate_registered_endpoints()
            .expect("session registry should only reference protocol endpoints");

        let registered: Vec<ApiMethod> = registry
            .entries()
            .iter()
            .map(|entry| entry.operation)
            .collect();
        let declared: Vec<ApiMethod> = ALL_ENDPOINTS
            .iter()
            .map(|endpoint| endpoint.operation)
            .collect();

        assert_eq!(registered, declared);
    }
}
