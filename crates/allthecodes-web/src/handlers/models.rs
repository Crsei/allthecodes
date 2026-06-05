//! Model registry handlers — list, update, set default.

use axum::extract::{Path as AxumPath, State};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::handlers::admin::persist_setting;
use crate::handlers::providers::{
    configured_provider_models, provider_for_model, update_configured_model,
};
use crate::handlers::{ApiError, SettingsResponse};
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

    let mut models: Vec<ModelSummary> = configured_provider_models();
    if models.is_empty() {
        models = available
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
    } else {
        for model in &mut models {
            model.default = Some(model.id == *current_model);
        }
    }

    Json(ModelRegistryResponse {
        profile_id: None,
        default_model_id: Some(current_model.clone()).filter(|m| !m.is_empty()),
        models,
    })
}

/// PATCH /api/models/{id} — Update a model's properties.
pub async fn models_update_handler(
    AxumPath(id): AxumPath<String>,
    State(state): State<WebState>,
    Json(req): Json<ModelUpdateRequest>,
) -> Response {
    if let Err(error) = update_configured_model(&id, req.alias, req.context_window, req.visible) {
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: format!("Failed to update model: {error}"),
                code: "model_update_failed".into(),
            }),
        )
            .into_response();
    }

    models_list_handler(State(state)).await.into_response()
}

/// POST /api/models/default — Set the default model.
pub async fn models_set_default_handler(
    State(state): State<WebState>,
    Json(req): Json<SetDefaultModelRequest>,
) -> Response {
    if let Some((provider_id, enabled)) = provider_for_model(&req.model_id) {
        if !enabled {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                Json(ApiError {
                    error: format!(
                        "Provider '{}' must be enabled before model '{}' can run",
                        provider_id, req.model_id
                    ),
                    code: "provider_disabled".into(),
                }),
            )
                .into_response();
        }
    }

    if let Err(error) = persist_setting(&state, "model", json!(req.model_id.clone())) {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: format!("Failed to set default model: {error}"),
                code: "model_default_failed".into(),
            }),
        )
            .into_response();
    }

    Json(SettingsResponse {
        ok: true,
        message: format!("Default model set to {}", req.model_id),
    })
    .into_response()
}
