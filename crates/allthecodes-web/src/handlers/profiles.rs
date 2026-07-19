//! Profile CRUD handlers backed by the lossless provider-profile store.

use axum::extract::Path as AxumPath;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};

use allthecodes_config::settings::{
    ProviderProfileSettings, ProviderProfileStore, ProviderProfileStoreError,
};
use allthecodes_protocol::ApiError as ProtocolApiError;

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

fn list_response() -> Result<ProfileListResponse, anyhow::Error> {
    let store = ProviderProfileStore::global();
    let settings = store.load()?;
    let profiles = store
        .list()?
        .into_iter()
        .map(|profile| ProfileSummary {
            name: profile.id.clone(),
            id: profile.id,
            active: profile.active,
            created_at: None,
            updated_at: None,
        })
        .collect();
    Ok(ProfileListResponse {
        active_profile_id: settings.active_auth_profile,
        profiles,
    })
}

fn error_response(error: anyhow::Error) -> Response {
    let (status, body) = match error.downcast_ref::<ProviderProfileStoreError>() {
        Some(ProviderProfileStoreError::EmptyId) => (
            StatusCode::BAD_REQUEST,
            ProtocolApiError::BadRequest {
                code: "validation_error",
                message: error.to_string(),
            }
            .into_body(),
        ),
        Some(ProviderProfileStoreError::NotFound(id)) => (
            StatusCode::NOT_FOUND,
            ProtocolApiError::NotFound {
                entity: "profile",
                id: id.clone(),
            }
            .into_body(),
        ),
        Some(ProviderProfileStoreError::AlreadyExists(_))
        | Some(ProviderProfileStoreError::ActiveProfile(_))
        | Some(ProviderProfileStoreError::UnsupportedProvider(_)) => (
            StatusCode::CONFLICT,
            ProtocolApiError::Conflict {
                reason: error.to_string(),
            }
            .into_body(),
        ),
        Some(ProviderProfileStoreError::InvalidRecoveryPolicy(_)) => (
            StatusCode::BAD_REQUEST,
            ProtocolApiError::BadRequest {
                code: "validation_error",
                message: error.to_string(),
            }
            .into_body(),
        ),
        None => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ProtocolApiError::Internal {
                message: error.to_string(),
            }
            .into_body(),
        ),
    };
    (status, Json(body)).into_response()
}

fn refreshed_list() -> Response {
    match list_response() {
        Ok(response) => Json(response).into_response(),
        Err(error) => error_response(error),
    }
}

/// GET /api/profiles — List all profiles.
pub async fn profiles_list_handler() -> Response {
    refreshed_list()
}

/// POST /api/profiles — Create an empty, editable profile.
pub async fn profiles_create_handler(Json(req): Json<ProfileCreateRequest>) -> Response {
    match ProviderProfileStore::global().create(&req.name, ProviderProfileSettings::default()) {
        Ok(()) => refreshed_list(),
        Err(error) => error_response(error),
    }
}

/// GET /api/profiles/{id} — Return a redacted, lossless profile document.
pub async fn profiles_detail_handler(AxumPath(id): AxumPath<String>) -> Response {
    match ProviderProfileStore::global().get(&id) {
        Ok(profile) => Json(profile).into_response(),
        Err(error) => error_response(error),
    }
}

/// PATCH /api/profiles/{id} — Rename a profile without rebuilding its contents.
pub async fn profiles_update_handler(
    AxumPath(id): AxumPath<String>,
    Json(req): Json<ProfileUpdateRequest>,
) -> Response {
    let Some(name) = req.name else {
        return refreshed_list();
    };
    match ProviderProfileStore::global().rename(&id, &name) {
        Ok(()) => refreshed_list(),
        Err(error) => error_response(error),
    }
}

/// DELETE /api/profiles/{id} — Delete a non-active profile.
pub async fn profiles_delete_handler(AxumPath(id): AxumPath<String>) -> Response {
    match ProviderProfileStore::global().delete(&id) {
        Ok(()) => refreshed_list(),
        Err(error) => error_response(error),
    }
}

/// POST /api/profiles/{id}/switch — Activate a supported LLM profile.
pub async fn profiles_switch_handler(AxumPath(id): AxumPath<String>) -> Response {
    match ProviderProfileStore::global().activate(&id) {
        Ok(()) => refreshed_list(),
        Err(error) => error_response(error),
    }
}

/// POST /api/profiles/import — Import the actual profile payload without data loss.
pub async fn profiles_import_handler(Json(req): Json<ProfileImportRequest>) -> Response {
    let Some(mut payload) = req.payload else {
        return (
            StatusCode::BAD_REQUEST,
            Json(
                ProtocolApiError::BadRequest {
                    code: "validation_error",
                    message: "Missing 'payload' field".to_string(),
                }
                .into_body(),
            ),
        )
            .into_response();
    };
    let name = payload
        .get("name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("imported")
        .to_string();
    let profile_value = payload
        .get_mut("profile")
        .map(std::mem::take)
        .unwrap_or_else(|| {
            if let Some(object) = payload.as_object_mut() {
                object.remove("name");
            }
            payload
        });
    let profile = match serde_json::from_value::<ProviderProfileSettings>(profile_value) {
        Ok(profile) => profile,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(
                    ProtocolApiError::BadRequest {
                        code: "validation_error",
                        message: format!("Invalid profile payload: {error}"),
                    }
                    .into_body(),
                ),
            )
                .into_response();
        }
    };
    match ProviderProfileStore::global().create(&name, profile) {
        Ok(()) => refreshed_list(),
        Err(error) => error_response(error),
    }
}

/// GET /api/profiles/{id}/export — Export only the redacted profile document.
pub async fn profiles_export_handler(AxumPath(id): AxumPath<String>) -> Response {
    match ProviderProfileStore::global().get(&id) {
        Ok(profile) => Json(profile).into_response(),
        Err(error) => error_response(error),
    }
}
