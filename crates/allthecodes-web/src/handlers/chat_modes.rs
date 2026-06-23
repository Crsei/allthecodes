//! Chat mode bundle REST handlers and request-time resolution.

use std::path::Path;

use allthecodes_protocol::v1::chat_modes::ChatModeResourcesResponse as ProtocolChatModeResourcesResponse;
use allthecodes_protocol::v1::chat_modes::ChatModesResponse as ProtocolChatModesResponse;
use allthecodes_protocol::ApiMethod;
use allthecodes_protocol::{ApiError as ProtocolApiError, NoParams};
use allthecodes_services::chat_modes as service;
use async_trait::async_trait;
use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Json;
use serde::Deserialize;

pub use service::{
    chat_mode_preference_for_cwd_session, chat_mode_preference_for_session_info,
    normalize_mode_or_normal, normalize_optional_mode, ChatModeBundle, ChatModePreference,
    ModeActivation, NORMAL_CHAT_MODE_ID,
};

use crate::api_dispatcher::rest_processor_response;
use crate::api_errors::protocol_error_response;
use crate::handler_registry::HandlerRegistry;
use crate::processors::Processor;
use crate::serialization::SerializationLayer;
use crate::state::WebState;
use crate::workspace_metadata;

pub(crate) fn handlers() -> HandlerRegistry {
    HandlerRegistry::new()
        .handle(ApiMethod::ChatModesList, get(chat_modes_list_handler))
        .handle(
            ApiMethod::ChatModesResources,
            get(chat_modes_resources_handler),
        )
}

#[derive(Clone)]
pub struct ChatModesListProcessor {
    state: WebState,
}

impl From<WebState> for ChatModesListProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for ChatModesListProcessor {
    type Request = NoParams;
    type Response = ProtocolChatModesResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "chat_modes.list"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    async fn handle(&self, _params: Self::Request) -> Result<Self::Response, Self::Error> {
        service::refresh_plugin_contributed_skills(
            Path::new(self.state.engine().cwd()),
            Some(self.state.app_version()),
        );
        service_to_protocol(
            service::list_modes(Path::new(self.state.engine().cwd())).map_err(map_service_error)?,
        )
    }
}

#[derive(Clone)]
pub struct ChatModesResourcesProcessor {
    state: WebState,
}

impl From<WebState> for ChatModesResourcesProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for ChatModesResourcesProcessor {
    type Request = NoParams;
    type Response = ProtocolChatModeResourcesResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "chat_modes.resources"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    async fn handle(&self, _params: Self::Request) -> Result<Self::Response, Self::Error> {
        service::refresh_plugin_contributed_skills(
            Path::new(self.state.engine().cwd()),
            Some(self.state.app_version()),
        );
        service_to_protocol(
            service::list_resources(Path::new(self.state.engine().cwd()))
                .map_err(map_service_error)?,
        )
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatModeBundleUpsertRequest {
    pub bundle: ChatModeBundle,
}

pub async fn chat_modes_list_handler(State(state): State<WebState>) -> Response {
    rest_processor_response::<ChatModesListProcessor>(state, ApiMethod::ChatModesList, NoParams {})
        .await
}

pub async fn chat_modes_resources_handler(State(state): State<WebState>) -> Response {
    rest_processor_response::<ChatModesResourcesProcessor>(
        state,
        ApiMethod::ChatModesResources,
        NoParams {},
    )
    .await
}

pub async fn chat_modes_upsert_handler(
    State(state): State<WebState>,
    AxumPath(id): AxumPath<String>,
    Json(req): Json<ChatModeBundleUpsertRequest>,
) -> Response {
    let normalized_id = service::normalize_mode_id(&id);
    if normalized_id.is_empty() {
        return api_error_response(ProtocolApiError::BadRequest {
            code: "invalid_chat_mode",
            message: "mode id is required".to_string(),
        });
    }
    if service::normalize_mode_id(&req.bundle.id) != normalized_id {
        return api_error_response(ProtocolApiError::BadRequest {
            code: "invalid_chat_mode",
            message: "mode id does not match path".to_string(),
        });
    }

    match service::upsert_mode(
        Path::new(state.engine().cwd()),
        req.bundle,
        Some(state.app_version()),
    )
    .await
    {
        Ok(bundle) => Json(bundle).into_response(),
        Err(error) => api_error_response(map_service_error(error)),
    }
}

pub async fn chat_modes_enable_handler(
    State(state): State<WebState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    match service::enable_mode(
        Path::new(state.engine().cwd()),
        &id,
        Some(state.app_version()),
    )
    .await
    {
        Ok(bundle) => Json(bundle).into_response(),
        Err(error) => api_error_response(map_service_error(error)),
    }
}

pub async fn chat_modes_disable_handler(
    State(state): State<WebState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    match service::disable_mode(Path::new(state.engine().cwd()), &id) {
        Ok(bundle) => Json(bundle).into_response(),
        Err(error) => api_error_response(map_service_error(error)),
    }
}

pub async fn chat_modes_delete_handler(
    State(state): State<WebState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    match service::delete_mode(Path::new(state.engine().cwd()), &id) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => api_error_response(map_service_error(error)),
    }
}

pub fn resolve_mode_activation(
    state: &WebState,
    mode: Option<&str>,
) -> Result<Option<ModeActivation>, ProtocolApiError> {
    service::resolve_mode_activation(Path::new(state.engine().cwd()), mode)
        .map_err(map_service_error)
}

pub fn workspace_default_chat_mode(workspace_key: &str) -> String {
    workspace_metadata::load_metadata()
        .ok()
        .and_then(|metadata| metadata.get(workspace_key).cloned())
        .and_then(|metadata| normalize_optional_mode(metadata.default_chat_mode.as_deref()))
        .unwrap_or_else(|| NORMAL_CHAT_MODE_ID.to_string())
}

fn service_to_protocol<T, U>(value: T) -> Result<U, ProtocolApiError>
where
    T: serde::Serialize,
    U: serde::de::DeserializeOwned,
{
    serde_json::to_value(value)
        .and_then(serde_json::from_value)
        .map_err(|error| ProtocolApiError::Internal {
            message: error.to_string(),
        })
}

fn map_service_error(error: service::ChatModeError) -> ProtocolApiError {
    match error {
        service::ChatModeError::BadRequest { code, message } => {
            ProtocolApiError::BadRequest { code, message }
        }
        service::ChatModeError::Internal(message) => ProtocolApiError::Internal { message },
    }
}

fn api_error_response(error: ProtocolApiError) -> Response {
    protocol_error_response(error).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_exports_normalization_helpers() {
        assert_eq!(normalize_mode_or_normal(None), "normal");
        assert_eq!(
            normalize_optional_mode(Some("Eco Boost")),
            Some("eco-boost".into())
        );
    }
}
