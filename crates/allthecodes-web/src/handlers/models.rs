//! Model registry handlers — list, update, set default.

use axum::extract::{Path as AxumPath, State};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::handlers::SettingsResponse;
use crate::state::WebState;

#[derive(Serialize)]
pub struct ModelSummary {
    pub id: String,
    pub provider_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    pub visible: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_tools: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_vision: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,
}

#[derive(Serialize)]
pub struct ModelRegistryResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_model_id: Option<String>,
    pub models: Vec<ModelSummary>,
}

#[derive(Deserialize)]
pub struct ModelUpdateRequest {
    #[serde(default)]
    pub visible: Option<bool>,
    #[serde(default)]
    pub alias: Option<String>,
    #[serde(default)]
    pub context_window: Option<u32>,
}

#[derive(Deserialize)]
pub struct SetDefaultModelRequest {
    pub model_id: String,
}

/// GET /api/models — List the model registry.
pub async fn models_list_handler(State(state): State<WebState>) -> impl IntoResponse {
    let engine = state.engine();
    let app_state = engine.app_state();
    let available = &app_state.settings.available_models;
    let current_model = &app_state.main_loop_model;

    let models: Vec<ModelSummary> = available
        .iter()
        .map(|m| ModelSummary {
            id: m.clone(),
            provider_id: "default".to_string(),
            provider_name: None,
            display_name: Some(m.clone()),
            alias: None,
            visible: true,
            default: Some(m == current_model),
            context_window: None,
            max_output_tokens: None,
            supports_tools: None,
            supports_vision: None,
            updated_at: None,
        })
        .collect();

    Json(ModelRegistryResponse {
        profile_id: None,
        default_model_id: Some(current_model.clone()).filter(|m| !m.is_empty()),
        models,
    })
}

/// PATCH /api/models/{id} — Update a model's properties.
pub async fn models_update_handler(
    AxumPath(_id): AxumPath<String>,
    Json(_req): Json<ModelUpdateRequest>,
) -> Response {
    // Model update is a no-op in this initial implementation
    // The engine's available_models list is read-only from settings
    Json(ModelRegistryResponse {
        profile_id: None,
        default_model_id: None,
        models: Vec::new(),
    })
    .into_response()
}

/// POST /api/models/default — Set the default model.
pub async fn models_set_default_handler(
    State(state): State<WebState>,
    Json(req): Json<SetDefaultModelRequest>,
) -> Response {
    state.engine().update_app_state(|s| {
        s.main_loop_model = req.model_id.clone();
        s.settings.model = Some(req.model_id.clone());
    });

    Json(SettingsResponse {
        ok: true,
        message: format!("Default model set to {}", req.model_id),
    })
    .into_response()
}
