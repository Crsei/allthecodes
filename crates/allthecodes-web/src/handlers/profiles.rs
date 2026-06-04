//! Profile CRUD handlers — list, create, detail, update, delete, switch, import, export.

use std::collections::HashMap;

use axum::extract::Path as AxumPath;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use allthecodes_config::settings::{
    load_global_config, write_user_settings, ProviderProfileSettings,
};

use crate::handlers::ApiError;

#[derive(Serialize, Clone)]
pub struct ProfileSummary {
    pub id: String,
    pub name: String,
    pub active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,
}

#[derive(Serialize)]
pub struct ProfileListResponse {
    pub active_profile_id: Option<String>,
    pub profiles: Vec<ProfileSummary>,
}

#[derive(Deserialize)]
pub struct ProfileCreateRequest {
    pub name: String,
}

#[derive(Deserialize)]
pub struct ProfileUpdateRequest {
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Deserialize)]
pub struct ProfileImportRequest {
    #[serde(default)]
    pub payload: Option<serde_json::Value>,
}

/// Load profiles from user settings, returning the active profile id and
/// a sorted list of profiles.
fn load_profile_list() -> (Option<String>, Vec<ProfileSummary>) {
    let settings = load_global_config().unwrap_or_default();
    let active_id = settings.active_auth_profile.clone();
    let mut profiles: Vec<ProfileSummary> = Vec::new();

    if let Some(auth_profiles) = settings.auth_profiles {
        for (id, _profile) in auth_profiles {
            let active = Some(&id) == active_id.as_ref();
            profiles.push(ProfileSummary {
                id: id.clone(),
                name: id.clone(),
                active,
                created_at: Some(Utc::now().timestamp()),
                updated_at: Some(Utc::now().timestamp()),
            });
        }
    }

    profiles.sort_by(|a, b| a.name.cmp(&b.name));
    (active_id, profiles)
}

fn save_profile_list(
    active_id: &Option<String>,
    profiles: &[ProfileSummary],
) -> Result<(), String> {
    let mut settings = load_global_config().unwrap_or_default();
    settings.active_auth_profile = active_id.clone();

    let mut auth_profiles = HashMap::new();
    for p in profiles {
        auth_profiles.insert(
            p.id.clone(),
            ProviderProfileSettings {
                backend: None,
                api_provider: None,
                model: None,
                available_models: None,
                model_capabilities: None,
                model_reasoning_effort: None,
                base_url: None,
                api_key: None,
                env: None,
                auth_source: None,
                extra: HashMap::new(),
            },
        );
    }
    settings.auth_profiles = Some(auth_profiles);

    write_user_settings(&settings).map_err(|e| e.to_string())?;
    Ok(())
}

/// GET /api/profiles — List all profiles.
pub async fn profiles_list_handler() -> impl IntoResponse {
    let (active_id, profiles) = load_profile_list();
    Json(ProfileListResponse {
        active_profile_id: active_id,
        profiles,
    })
}

/// POST /api/profiles — Create a new profile.
pub async fn profiles_create_handler(Json(req): Json<ProfileCreateRequest>) -> impl IntoResponse {
    if req.name.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: "Profile name cannot be empty".into(),
                code: "validation_error".into(),
            }),
        )
            .into_response();
    }

    let (active_id, mut profiles) = load_profile_list();

    // Check for duplicate
    if profiles.iter().any(|p| p.id == req.name.trim()) {
        return (
            StatusCode::CONFLICT,
            Json(ApiError {
                error: format!("Profile '{}' already exists", req.name),
                code: "conflict".into(),
            }),
        )
            .into_response();
    }

    let now = Utc::now().timestamp();
    profiles.push(ProfileSummary {
        id: req.name.trim().to_string(),
        name: req.name.trim().to_string(),
        active: false,
        created_at: Some(now),
        updated_at: Some(now),
    });

    match save_profile_list(&active_id, &profiles) {
        Ok(()) => Json(ProfileListResponse {
            active_profile_id: active_id,
            profiles,
        })
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: e,
                code: "internal_error".into(),
            }),
        )
            .into_response(),
    }
}

/// GET /api/profiles/{id} — Get a single profile detail.
pub async fn profiles_detail_handler(AxumPath(id): AxumPath<String>) -> impl IntoResponse {
    let (_, profiles) = load_profile_list();
    if let Some(profile) = profiles.into_iter().find(|p| p.id == id) {
        Json(profile).into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(ApiError {
                error: format!("Profile '{}' not found", id),
                code: "not_found".into(),
            }),
        )
            .into_response()
    }
}

/// PATCH /api/profiles/{id} — Update a profile.
pub async fn profiles_update_handler(
    AxumPath(id): AxumPath<String>,
    Json(req): Json<ProfileUpdateRequest>,
) -> impl IntoResponse {
    let (active_id, mut profiles) = load_profile_list();

    let profile = match profiles.iter_mut().find(|p| p.id == id) {
        Some(p) => p,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(ApiError {
                    error: format!("Profile '{}' not found", id),
                    code: "not_found".into(),
                }),
            )
                .into_response();
        }
    };

    if let Some(new_name) = &req.name {
        let trimmed = new_name.trim().to_string();
        if trimmed.is_empty() {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiError {
                    error: "Profile name cannot be empty".into(),
                    code: "validation_error".into(),
                }),
            )
                .into_response();
        }
        profile.id = trimmed.clone();
        profile.name = trimmed;
        profile.updated_at = Some(Utc::now().timestamp());
    }

    match save_profile_list(&active_id, &profiles) {
        Ok(()) => Json(ProfileListResponse {
            active_profile_id: active_id,
            profiles,
        })
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: e,
                code: "internal_error".into(),
            }),
        )
            .into_response(),
    }
}

/// DELETE /api/profiles/{id} — Delete a profile.
pub async fn profiles_delete_handler(AxumPath(id): AxumPath<String>) -> impl IntoResponse {
    let (active_id, mut profiles) = load_profile_list();

    let pos = match profiles.iter().position(|p| p.id == id) {
        Some(pos) => pos,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(ApiError {
                    error: format!("Profile '{}' not found", id),
                    code: "not_found".into(),
                }),
            )
                .into_response();
        }
    };

    profiles.remove(pos);

    let active_id = if active_id.as_deref() == Some(&id) {
        None
    } else {
        active_id
    };

    match save_profile_list(&active_id, &profiles) {
        Ok(()) => Json(ProfileListResponse {
            active_profile_id: active_id,
            profiles,
        })
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: e,
                code: "internal_error".into(),
            }),
        )
            .into_response(),
    }
}

/// POST /api/profiles/{id}/switch — Switch the active profile.
pub async fn profiles_switch_handler(AxumPath(id): AxumPath<String>) -> impl IntoResponse {
    let (_, mut profiles) = load_profile_list();

    if !profiles.iter().any(|p| p.id == id) {
        return (
            StatusCode::NOT_FOUND,
            Json(ApiError {
                error: format!("Profile '{}' not found", id),
                code: "not_found".into(),
            }),
        )
            .into_response();
    }

    // Update active flags
    for p in &mut profiles {
        p.active = p.id == id;
    }

    let new_active_id = Some(id.clone());
    match save_profile_list(&new_active_id, &profiles) {
        Ok(()) => Json(ProfileListResponse {
            active_profile_id: new_active_id,
            profiles,
        })
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: e,
                code: "internal_error".into(),
            }),
        )
            .into_response(),
    }
}

/// POST /api/profiles/import — Import a profile from a JSON payload.
pub async fn profiles_import_handler(Json(req): Json<ProfileImportRequest>) -> impl IntoResponse {
    let payload = match req.payload {
        Some(p) => p,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiError {
                    error: "Missing 'payload' field".into(),
                    code: "validation_error".into(),
                }),
            )
                .into_response();
        }
    };

    // Extract profile name from payload
    let name = payload
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("imported");
    let (active_id, mut profiles) = load_profile_list();

    let now = Utc::now().timestamp();
    profiles.push(ProfileSummary {
        id: name.to_string(),
        name: name.to_string(),
        active: false,
        created_at: Some(now),
        updated_at: Some(now),
    });

    match save_profile_list(&active_id, &profiles) {
        Ok(()) => Json(ProfileListResponse {
            active_profile_id: active_id,
            profiles,
        })
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: e,
                code: "internal_error".into(),
            }),
        )
            .into_response(),
    }
}

/// GET /api/profiles/{id}/export — Export a profile as JSON.
pub async fn profiles_export_handler(AxumPath(id): AxumPath<String>) -> Response {
    let (_, profiles) = load_profile_list();
    let profile = match profiles.into_iter().find(|p| p.id == id) {
        Some(p) => p,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(ApiError {
                    error: format!("Profile '{}' not found", id),
                    code: "not_found".into(),
                }),
            )
                .into_response();
        }
    };

    Json(profile).into_response()
}
