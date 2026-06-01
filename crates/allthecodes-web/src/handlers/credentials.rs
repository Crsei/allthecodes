//! Credential and OAuth handlers — status, start flow, poll flow.

use axum::extract::Path as AxumPath;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use allthecodes_auth::resolve_auth;

#[derive(Serialize)]
pub struct CredentialSummary {
    pub provider: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_name: Option<String>,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
}

#[derive(Serialize)]
pub struct CredentialStatusResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    pub credentials: Vec<CredentialSummary>,
}

#[derive(Serialize)]
pub struct OAuthStartResponse {
    pub flow_id: String,
    pub provider: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verification_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interval_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Deserialize)]
pub struct OAuthStartRequest {
    pub provider: String,
    #[serde(default)]
    pub profile_id: Option<String>,
}

#[derive(Serialize)]
pub struct OAuthPollResponse {
    pub flow_id: String,
    pub provider: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credential: Option<CredentialSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// GET /api/credentials — Return credential status.
pub async fn credentials_handler() -> impl IntoResponse {
    let auth = resolve_auth();
    let mut credentials = Vec::new();

    if auth.is_authenticated() {
        let subject = auth.api_key().map(|k| {
            if k.len() > 8 {
                format!("{}...{}", &k[..4], &k[k.len()-4..])
            } else {
                "configured".to_string()
            }
        });

        credentials.push(CredentialSummary {
            provider: "anthropic".to_string(),
            provider_id: None,
            provider_name: Some("Anthropic".to_string()),
            status: "configured".to_string(),
            subject,
            expires_at: None,
            updated_at: None,
            profile_id: None,
        });
    }

    Json(CredentialStatusResponse {
        profile_id: None,
        credentials,
    })
}

/// POST /api/oauth/{provider}/start — Start an OAuth flow.
pub async fn oauth_start_handler(
    AxumPath(provider): AxumPath<String>,
    Json(_req): Json<OAuthStartRequest>,
) -> Response {
    Json(OAuthStartResponse {
        flow_id: format!("oauth-{}", Utc::now().timestamp()),
        provider,
        status: "failed".to_string(),
        verification_uri: None,
        user_code: None,
        expires_at: None,
        interval_ms: None,
        message: Some("OAuth flow is not available in the web UI yet. Use the CLI `/login` command instead.".to_string()),
    }).into_response()
}

/// POST /api/oauth/{provider}/poll — Poll OAuth flow status.
pub async fn oauth_poll_handler(
    AxumPath(provider): AxumPath<String>,
) -> Response {
    Json(OAuthPollResponse {
        flow_id: String::new(),
        provider,
        status: "failed".to_string(),
        credential: None,
        message: Some("OAuth flow is not available in the web UI yet.".to_string()),
    }).into_response()
}
