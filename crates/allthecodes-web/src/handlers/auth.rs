//! Authentication handlers — status, login, logout, refresh.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};

use allthecodes_auth::resolve_auth;

use crate::handlers::ApiError;

#[derive(Serialize)]
pub struct AuthStatusResponse {
    pub authenticated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_required: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

#[derive(Deserialize)]
pub struct LoginRequest {
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
}

#[derive(Serialize)]
pub struct LoginResponse {
    pub authenticated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bearer_token: Option<String>,
}

/// GET /api/auth/status — Return current authentication status.
pub async fn auth_status_handler() -> impl IntoResponse {
    // Use allthecodes-auth to resolve current auth state
    let auth = resolve_auth();
    let authenticated = auth.is_authenticated();
    let subject = auth.api_key().map(|k| {
        if k.len() > 8 {
            format!("{}...{}", &k[..4], &k[k.len() - 4..])
        } else {
            "unknown".to_string()
        }
    });
    let expires_at = None;

    Json(AuthStatusResponse {
        authenticated,
        auth_required: Some(false), // local/anonymous mode
        subject,
        expires_at,
        profile_id: None,
        session_id: None,
    })
}

/// POST /api/auth/login — Authenticate with an API key or token.
pub async fn auth_login_handler(Json(req): Json<LoginRequest>) -> Response {
    // Accept API key from token field
    if let Some(token) = &req.token {
        if allthecodes_auth::api_key::validate_api_key(token) {
            match allthecodes_auth::api_key::store_api_key(token) {
                Ok(_) => {
                    return Json(LoginResponse {
                        authenticated: true,
                        session_id: None,
                        expires_at: None,
                        subject: Some(format!("{}...{}", &token[..4], &token[token.len() - 4..])),
                        profile_id: None,
                        access_token: None,
                        bearer_token: Some(token.clone()),
                    })
                    .into_response();
                }
                Err(e) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ApiError {
                            error: format!("Failed to store API key: {}", e),
                            code: "internal_error".into(),
                        }),
                    )
                        .into_response();
                }
            }
        }
        // Also try OpenAI key validation
        if allthecodes_auth::api_key::validate_openai_api_key(token) {
            match allthecodes_auth::api_key::store_openai_api_key(token) {
                Ok(_) => {
                    return Json(LoginResponse {
                        authenticated: true,
                        session_id: None,
                        expires_at: None,
                        subject: Some(format!(
                            "openai:{}...{}",
                            &token[..4],
                            &token[token.len() - 4..]
                        )),
                        profile_id: None,
                        access_token: None,
                        bearer_token: Some(token.clone()),
                    })
                    .into_response();
                }
                Err(e) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ApiError {
                            error: format!("Failed to store API key: {}", e),
                            code: "internal_error".into(),
                        }),
                    )
                        .into_response();
                }
            }
        }
    }

    (
        StatusCode::BAD_REQUEST,
        Json(ApiError {
            error: "Invalid API key format".into(),
            code: "validation_error".into(),
        }),
    )
        .into_response()
}

/// POST /api/auth/logout — Clear authentication.
pub async fn auth_logout_handler() -> impl IntoResponse {
    let _ = allthecodes_auth::oauth_logout();
    let _ = allthecodes_auth::api_key::remove_api_key();
    let _ = allthecodes_auth::api_key::remove_openai_api_key();
    StatusCode::OK
}

/// POST /api/auth/refresh — Refresh the session state.
pub async fn auth_refresh_handler() -> impl IntoResponse {
    auth_status_handler().await
}
