//! Authentication handlers — status, login, logout, refresh.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};

use allthecodes_auth::resolve_auth;

use allthecodes_protocol::ApiError as ProtocolApiError;

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

#[derive(Serialize)]
pub struct AccountAuthStatusResponse {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    pub user: Option<AccountAuthUser>,
    pub subscription: Option<AccountSubscription>,
    pub entitlements: Option<AccountEntitlements>,
    pub credits: Option<AccountCreditsSummary>,
    pub agent_collaboration: Option<AccountAgentCollaborationSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct AccountAuthUser {
    pub id: Option<String>,
    pub email: Option<String>,
}

#[derive(Serialize)]
pub struct AccountSubscription {
    pub status: Option<String>,
    pub provider: Option<String>,
    pub plan_code: Option<String>,
    pub price_id: Option<String>,
    pub current_period_start: Option<String>,
    pub current_period_end: Option<String>,
}

#[derive(Serialize)]
pub struct AccountEntitlements {
    pub plan: Option<String>,
    pub pro: bool,
}

#[derive(Serialize)]
pub struct AccountCreditsSummary {
    pub balance: i64,
    pub reserved_balance: i64,
    pub total_granted: i64,
    pub total_used: i64,
    pub expires_soon: i64,
    pub expiring_grants: Vec<AccountExpiringGrant>,
}

#[derive(Serialize)]
pub struct AccountExpiringGrant {
    pub id: Option<String>,
    pub remaining_credits: i64,
    pub expires_at: Option<String>,
}

#[derive(Serialize)]
pub struct AccountAgentCollaborationSummary {
    pub unit: Option<String>,
    pub limit: Option<i64>,
    pub used: i64,
    pub remaining: Option<i64>,
    pub current_period_start: Option<String>,
    pub current_period_end: Option<String>,
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
                        Json(
                            ProtocolApiError::Internal {
                                message: format!("Failed to store API key: {}", e),
                            }
                            .into_body(),
                        ),
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
                        Json(
                            ProtocolApiError::Internal {
                                message: format!("Failed to store API key: {}", e),
                            }
                            .into_body(),
                        ),
                    )
                        .into_response();
                }
            }
        }
    }

    (
        StatusCode::BAD_REQUEST,
        Json(
            ProtocolApiError::BadRequest {
                code: "validation_error",
                message: "Invalid API key format".into(),
            }
            .into_body(),
        ),
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

/// GET /api/account-auth/status — Return desktop account status.
pub async fn account_auth_status_handler() -> impl IntoResponse {
    Json(signed_out_account_status())
}

/// POST /api/account-auth/refresh — Refresh desktop account status.
pub async fn account_auth_refresh_handler() -> impl IntoResponse {
    Json(signed_out_account_status())
}

/// POST /api/account-auth/logout — Clear desktop account state.
pub async fn account_auth_logout_handler() -> impl IntoResponse {
    Json(signed_out_account_status())
}

fn signed_out_account_status() -> AccountAuthStatusResponse {
    AccountAuthStatusResponse {
        status: "signed_out".to_string(),
        expires_at: None,
        user: None,
        subscription: None,
        entitlements: Some(AccountEntitlements {
            plan: Some("free".to_string()),
            pro: false,
        }),
        credits: Some(AccountCreditsSummary {
            balance: 0,
            reserved_balance: 0,
            total_granted: 0,
            total_used: 0,
            expires_soon: 0,
            expiring_grants: Vec::new(),
        }),
        agent_collaboration: Some(AccountAgentCollaborationSummary {
            unit: Some("event".to_string()),
            limit: None,
            used: 0,
            remaining: None,
            current_period_start: None,
            current_period_end: None,
        }),
        error: None,
    }
}
