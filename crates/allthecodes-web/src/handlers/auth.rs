//! Authentication handlers — status, login, logout, refresh.

use std::fs;
use std::io::Write;
use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use allthecodes_auth::oauth::pkce;
use allthecodes_auth::resolve_auth;
use allthecodes_config::paths;

use allthecodes_protocol::ApiError as ProtocolApiError;

use crate::state::{AccountAuthSession, PendingAccountLogin, WebState};

const DEFAULT_ACCOUNT_SITE_URL: &str = "https://allthecodes.cc";
const DESKTOP_REDIRECT_URI: &str = "allthecodes://auth/callback";

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

#[derive(Debug, Deserialize)]
pub struct AccountLoginStartRequest {
    pub account_site_url: Option<String>,
    pub redirect_uri: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AccountLoginCompleteRequest {
    pub callback_url: Option<String>,
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

#[derive(Debug, Serialize)]
struct AccountExchangeRequest<'a> {
    code: &'a str,
    state: &'a str,
    code_verifier: &'a str,
    redirect_uri: &'a str,
}

#[derive(Debug, Serialize)]
struct AccountRefreshRequest<'a> {
    refresh_token: &'a str,
}

#[derive(Debug, Serialize)]
struct AccountLogoutRequest<'a> {
    refresh_token: &'a str,
}

#[derive(Debug, Deserialize)]
struct AccountTokenResponse {
    #[serde(default)]
    id_token: Option<String>,
    access_token: String,
    refresh_token: String,
    #[serde(default)]
    account_id: Option<String>,
    #[serde(default)]
    last_refresh: Option<String>,
    expires_at: String,
    user: Value,
    subscription: Value,
    entitlements: Value,
    #[serde(default)]
    credits: Value,
    #[serde(default)]
    agent_collaboration: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AccountAuthMetadata {
    account_site_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AccountAuthFile {
    auth_mode: String,
    tokens: AccountAuthTokens,
    last_refresh: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AccountAuthTokens {
    id_token: String,
    access_token: String,
    refresh_token: String,
    account_id: String,
}

#[derive(Debug, Deserialize)]
pub struct AccountBillingLedgerQuery {
    pub page: Option<u32>,
    pub page_size: Option<u32>,
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

/// POST /api/account-auth/login/start — Start desktop account auth.
pub async fn account_auth_login_start_handler(
    State(state): State<WebState>,
    Json(body): Json<AccountLoginStartRequest>,
) -> Response {
    let account_site_url = match normalize_account_site_url(
        body.account_site_url
            .as_deref()
            .unwrap_or(DEFAULT_ACCOUNT_SITE_URL),
    ) {
        Ok(url) => url,
        Err(err) => return account_error(StatusCode::BAD_REQUEST, err),
    };
    let redirect_uri = body.redirect_uri.as_deref().unwrap_or(DESKTOP_REDIRECT_URI);

    if redirect_uri != DESKTOP_REDIRECT_URI {
        return account_error(StatusCode::BAD_REQUEST, anyhow!("unsupported redirect_uri"));
    }

    let code_verifier = pkce::generate_code_verifier();
    let code_challenge = pkce::generate_code_challenge(&code_verifier);
    let state_value = pkce::generate_state();
    let authorize_url = match build_authorize_url(
        &account_site_url,
        &state_value,
        &code_challenge,
        redirect_uri,
    ) {
        Ok(url) => url,
        Err(err) => return account_error(StatusCode::BAD_REQUEST, err),
    };

    {
        let mut guard = state.account_auth.lock();
        guard.pending = Some(PendingAccountLogin {
            account_site_url: account_site_url.clone(),
            redirect_uri: redirect_uri.to_string(),
            state: state_value,
            code_verifier,
        });
    }

    let _ = save_account_metadata(&AccountAuthMetadata { account_site_url });

    account_ok(json!({
        "status": "pending",
        "authorize_url": authorize_url,
        "authorization_url": authorize_url,
        "url": authorize_url,
    }))
}

/// POST /api/account-auth/login/complete — Complete desktop account auth.
pub async fn account_auth_login_complete_handler(
    State(state): State<WebState>,
    Json(body): Json<AccountLoginCompleteRequest>,
) -> Response {
    if let Some(error_value) = body.error.as_deref().filter(|value| !value.is_empty()) {
        state.account_auth.lock().pending = None;
        return account_error(
            StatusCode::BAD_REQUEST,
            anyhow!(
                "{}{}",
                error_value,
                body.error_description
                    .as_deref()
                    .filter(|value| !value.is_empty())
                    .map(|value| format!(": {value}"))
                    .unwrap_or_default()
            ),
        );
    }

    let (code, callback_state) = match account_login_complete_code_and_state(&body) {
        Ok(value) => value,
        Err(err) => return account_error(StatusCode::BAD_REQUEST, err),
    };

    let pending = state.account_auth.lock().pending.clone();
    let Some(pending) = pending else {
        return account_error(StatusCode::BAD_REQUEST, anyhow!("no pending account login"));
    };

    if callback_state != pending.state {
        return account_error(
            StatusCode::BAD_REQUEST,
            anyhow!("account login state mismatch"),
        );
    }

    match exchange_account_code(&pending, &code).await {
        Ok(session) => {
            let mut guard = state.account_auth.lock();
            guard.session = Some(session.clone());
            guard.pending = None;
            account_ok(account_session_payload("authenticated", &session))
        }
        Err(err) => account_error(StatusCode::BAD_GATEWAY, err),
    }
}

/// GET /api/account-auth/status — Return desktop account status.
pub async fn account_auth_status_handler(State(state): State<WebState>) -> Response {
    if let Some(session) = current_unexpired_account_session(&state) {
        return account_ok(account_session_payload("authenticated", &session));
    }

    match refresh_from_stored_account_auth(&state).await {
        Ok(Some(session)) => account_ok(account_session_payload("authenticated", &session)),
        Ok(None) => Json(signed_out_account_status()).into_response(),
        Err(err) => account_error(StatusCode::BAD_GATEWAY, err),
    }
}

/// POST /api/account-auth/refresh — Refresh desktop account status.
pub async fn account_auth_refresh_handler(State(state): State<WebState>) -> Response {
    match refresh_from_stored_account_auth(&state).await {
        Ok(Some(session)) => account_ok(account_session_payload("authenticated", &session)),
        Ok(None) => account_error(
            StatusCode::UNAUTHORIZED,
            anyhow!("no desktop account session"),
        ),
        Err(err) => account_error(StatusCode::BAD_GATEWAY, err),
    }
}

/// POST /api/account-auth/logout — Clear desktop account state.
pub async fn account_auth_logout_handler(State(state): State<WebState>) -> Response {
    let metadata = load_account_metadata().unwrap_or_else(|_| default_account_metadata());

    if let Ok(Some(auth_file)) = load_account_auth_file() {
        let _ =
            post_account_logout(&metadata.account_site_url, &auth_file.tokens.refresh_token).await;
    }

    let _ = remove_account_auth_file();
    let mut guard = state.account_auth.lock();
    guard.session = None;
    guard.pending = None;
    account_ok(json!({ "ok": true, "status": "signed_out" }))
}

/// GET /api/account-auth/billing — Return desktop account billing snapshot.
pub async fn account_billing_snapshot_handler(State(state): State<WebState>) -> Response {
    proxy_account_billing_get(state, "/api/billing/me", Vec::new()).await
}

/// GET /api/account-auth/billing/ledger — Return desktop account billing ledger.
pub async fn account_billing_ledger_handler(
    State(state): State<WebState>,
    Query(query): Query<AccountBillingLedgerQuery>,
) -> Response {
    let mut params = Vec::new();
    if let Some(page) = query.page {
        params.push(("page".to_string(), page.to_string()));
    }
    if let Some(page_size) = query.page_size {
        params.push(("page_size".to_string(), page_size.to_string()));
    }
    proxy_account_billing_get(state, "/api/billing/ledger", params).await
}

/// GET /api/account-auth/billing/orders/:id — Return a desktop account billing order.
pub async fn account_billing_order_handler(
    State(state): State<WebState>,
    Path(order_id): Path<String>,
) -> Response {
    let path = format!("/api/billing/orders/{order_id}");
    proxy_account_billing_get(state, &path, Vec::new()).await
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

fn current_unexpired_account_session(state: &WebState) -> Option<AccountAuthSession> {
    let session = state.account_auth.lock().session.clone()?;
    if account_access_token_expired(&session) {
        None
    } else {
        Some(session)
    }
}

async fn refresh_from_stored_account_auth(state: &WebState) -> Result<Option<AccountAuthSession>> {
    let Some(auth_file) = load_account_auth_file()? else {
        state.account_auth.lock().session = None;
        return Ok(None);
    };
    let metadata = load_account_metadata().unwrap_or_else(|_| default_account_metadata());
    let token =
        post_account_refresh(&metadata.account_site_url, &auth_file.tokens.refresh_token).await?;
    let session = account_session_from_token_response(metadata.account_site_url, token)?;
    state.account_auth.lock().session = Some(session.clone());
    Ok(Some(session))
}

async fn exchange_account_code(
    pending: &PendingAccountLogin,
    code: &str,
) -> Result<AccountAuthSession> {
    let url = account_endpoint_url(&pending.account_site_url, "/api/desktop-auth/exchange")?;
    let response = reqwest::Client::new()
        .post(url)
        .json(&AccountExchangeRequest {
            code,
            state: &pending.state,
            code_verifier: &pending.code_verifier,
            redirect_uri: &pending.redirect_uri,
        })
        .send()
        .await
        .context("failed to exchange desktop authorization code")?;
    let token = parse_account_token_response(response).await?;
    account_session_from_token_response(pending.account_site_url.clone(), token)
}

async fn post_account_refresh(
    account_site_url: &str,
    refresh_token: &str,
) -> Result<AccountTokenResponse> {
    let url = account_endpoint_url(account_site_url, "/api/desktop-auth/refresh")?;
    let response = reqwest::Client::new()
        .post(url)
        .json(&AccountRefreshRequest { refresh_token })
        .send()
        .await
        .context("failed to refresh desktop account session")?;
    parse_account_token_response(response).await
}

async fn post_account_logout(account_site_url: &str, refresh_token: &str) -> Result<()> {
    let url = account_endpoint_url(account_site_url, "/api/desktop-auth/logout")?;
    let _ = reqwest::Client::new()
        .post(url)
        .json(&AccountLogoutRequest { refresh_token })
        .send()
        .await?;
    Ok(())
}

async fn parse_account_token_response(response: reqwest::Response) -> Result<AccountTokenResponse> {
    let status = response.status();
    let text = response.text().await.unwrap_or_default();

    if !status.is_success() {
        let message = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|value| {
                value
                    .get("error")
                    .or_else(|| value.get("message"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_else(|| format!("desktop auth service returned HTTP {status}"));
        return Err(anyhow!(message));
    }

    serde_json::from_str::<AccountTokenResponse>(&text)
        .context("invalid desktop auth token response")
}

fn account_session_from_token_response(
    account_site_url: String,
    token: AccountTokenResponse,
) -> Result<AccountAuthSession> {
    store_account_auth_file(&token)?;
    save_account_metadata(&AccountAuthMetadata {
        account_site_url: account_site_url.clone(),
    })?;
    Ok(AccountAuthSession {
        account_site_url,
        access_token: token.access_token,
        expires_at: token.expires_at,
        user: token.user,
        subscription: token.subscription,
        entitlements: token.entitlements,
        credits: token.credits,
        agent_collaboration: token.agent_collaboration,
    })
}

fn account_login_complete_code_and_state(
    body: &AccountLoginCompleteRequest,
) -> Result<(String, String)> {
    if let (Some(code), Some(state)) = (body.code.as_deref(), body.state.as_deref()) {
        if !code.is_empty() && !state.is_empty() {
            return Ok((code.to_string(), state.to_string()));
        }
    }

    let Some(callback_url) = body.callback_url.as_deref() else {
        return Err(anyhow!("missing account login callback_url"));
    };
    let url = reqwest::Url::parse(callback_url).context("invalid account login callback_url")?;
    let code = url
        .query_pairs()
        .find(|(key, _)| key == "code")
        .map(|(_, value)| value.to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("missing account login code"))?;
    let state = url
        .query_pairs()
        .find(|(key, _)| key == "state")
        .map(|(_, value)| value.to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("missing account login state"))?;
    Ok((code, state))
}

fn build_authorize_url(
    account_site_url: &str,
    state: &str,
    code_challenge: &str,
    redirect_uri: &str,
) -> Result<String> {
    let mut url = account_endpoint_url(account_site_url, "/login")?;
    url.query_pairs_mut()
        .append_pair("state", state)
        .append_pair("code_challenge", code_challenge)
        .append_pair("redirect_uri", redirect_uri);
    Ok(url.to_string())
}

fn account_endpoint_url(account_site_url: &str, path: &str) -> Result<reqwest::Url> {
    let mut url = reqwest::Url::parse(account_site_url).context("invalid account site URL")?;
    url.set_path(path);
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

fn normalize_account_site_url(raw: &str) -> Result<String> {
    let url = reqwest::Url::parse(raw).context("invalid account site URL")?;
    match url.scheme() {
        "http" | "https" => Ok(url.origin().ascii_serialization()),
        _ => Err(anyhow!("account_site_url must use http or https")),
    }
}

fn account_access_token_expired(session: &AccountAuthSession) -> bool {
    let Ok(expires_at) = DateTime::parse_from_rfc3339(&session.expires_at) else {
        return true;
    };
    Utc::now() >= expires_at.with_timezone(&Utc)
}

fn account_session_payload(status: &str, session: &AccountAuthSession) -> Value {
    json!({
        "status": status,
        "expires_at": session.expires_at,
        "user": session.user,
        "subscription": session.subscription,
        "entitlements": session.entitlements,
        "credits": session.credits,
        "agent_collaboration": session.agent_collaboration,
    })
}

async fn proxy_account_billing_get(
    state: WebState,
    path: &str,
    query: Vec<(String, String)>,
) -> Response {
    let session = match session_for_account_request(&state).await {
        Ok(Some(session)) => session,
        Ok(None) => {
            return account_error(
                StatusCode::UNAUTHORIZED,
                anyhow!("no desktop account session"),
            )
        }
        Err(err) => return account_error(StatusCode::BAD_GATEWAY, err),
    };

    match fetch_account_billing_get(&session, path, &query).await {
        Ok((StatusCode::UNAUTHORIZED, _)) => match refresh_from_stored_account_auth(&state).await {
            Ok(Some(refreshed)) => {
                match fetch_account_billing_get(&refreshed, path, &query).await {
                    Ok((status, value)) => (status, Json(value)).into_response(),
                    Err(err) => account_error(StatusCode::BAD_GATEWAY, err),
                }
            }
            Ok(None) => account_error(
                StatusCode::UNAUTHORIZED,
                anyhow!("no desktop account session"),
            ),
            Err(err) => account_error(StatusCode::BAD_GATEWAY, err),
        },
        Ok((status, value)) => (status, Json(value)).into_response(),
        Err(err) => account_error(StatusCode::BAD_GATEWAY, err),
    }
}

async fn session_for_account_request(state: &WebState) -> Result<Option<AccountAuthSession>> {
    if let Some(session) = current_unexpired_account_session(state) {
        return Ok(Some(session));
    }
    refresh_from_stored_account_auth(state).await
}

async fn fetch_account_billing_get(
    session: &AccountAuthSession,
    path: &str,
    query: &[(String, String)],
) -> Result<(StatusCode, Value)> {
    let mut url = account_endpoint_url(&session.account_site_url, path)?;
    if !query.is_empty() {
        url.query_pairs_mut().extend_pairs(
            query
                .iter()
                .map(|(key, value)| (key.as_str(), value.as_str())),
        );
    }
    let response = reqwest::Client::new()
        .get(url)
        .bearer_auth(&session.access_token)
        .send()
        .await
        .context("failed to load desktop account billing data")?;
    let status =
        StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let text = response.text().await.unwrap_or_default();
    let value = if text.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str::<Value>(&text).unwrap_or_else(|_| {
            json!({
                "error": text,
            })
        })
    };
    Ok((status, value))
}

fn account_metadata_path() -> PathBuf {
    paths::daemon_dir().join("account-auth.json")
}

fn load_account_metadata() -> Result<AccountAuthMetadata> {
    let raw = fs::read_to_string(account_metadata_path())
        .context("failed to read account auth metadata")?;
    serde_json::from_str(&raw).context("failed to parse account auth metadata")
}

fn save_account_metadata(metadata: &AccountAuthMetadata) -> Result<()> {
    fs::create_dir_all(paths::daemon_dir())
        .context("failed to create daemon account auth directory")?;
    fs::write(
        account_metadata_path(),
        serde_json::to_string_pretty(metadata)?,
    )
    .context("failed to write account auth metadata")
}

fn default_account_metadata() -> AccountAuthMetadata {
    AccountAuthMetadata {
        account_site_url: std::env::var("ALLTHECODES_ACCOUNT_SITE_URL")
            .ok()
            .and_then(|value| normalize_account_site_url(&value).ok())
            .unwrap_or_else(|| DEFAULT_ACCOUNT_SITE_URL.to_string()),
    }
}

fn store_account_auth_file(token: &AccountTokenResponse) -> Result<()> {
    let account_id = token
        .account_id
        .clone()
        .or_else(|| {
            token
                .user
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .ok_or_else(|| anyhow!("desktop auth response is missing account_id"))?;
    let last_refresh = token
        .last_refresh
        .clone()
        .unwrap_or_else(current_iso_timestamp);
    let auth_file = AccountAuthFile {
        auth_mode: "allthecodes".to_string(),
        tokens: AccountAuthTokens {
            id_token: token
                .id_token
                .clone()
                .unwrap_or_else(|| token.access_token.clone()),
            access_token: token.access_token.clone(),
            refresh_token: token.refresh_token.clone(),
            account_id,
        },
        last_refresh,
    };
    write_account_auth_file(&auth_file)
}

fn load_account_auth_file() -> Result<Option<AccountAuthFile>> {
    let path = account_auth_file_path();
    if !path.exists() {
        return Ok(None);
    }
    let raw =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    if raw.trim().is_empty() {
        return Ok(None);
    }
    let auth_file = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(Some(auth_file))
}

fn write_account_auth_file(auth_file: &AccountAuthFile) -> Result<()> {
    let path = account_auth_file_path();
    let parent = path
        .parent()
        .with_context(|| format!("path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;

    let tmp = path.with_extension(format!("json.tmp-{}", std::process::id()));
    {
        let mut file = create_private_file(&tmp)
            .with_context(|| format!("failed to create {}", tmp.display()))?;
        let bytes = serde_json::to_vec_pretty(auth_file)
            .context("failed to serialize account auth file")?;
        file.write_all(&bytes)
            .with_context(|| format!("failed to write {}", tmp.display()))?;
        file.write_all(b"\n")
            .with_context(|| format!("failed to write {}", tmp.display()))?;
        file.sync_all()
            .with_context(|| format!("failed to sync {}", tmp.display()))?;
    }
    set_private_permissions(&tmp)?;
    replace_file(&tmp, &path)
}

fn remove_account_auth_file() -> Result<()> {
    let path = account_auth_file_path();
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("failed to remove {}", path.display())),
    }
}

fn account_auth_file_path() -> PathBuf {
    paths::data_root().join("auth.json")
}

fn current_iso_timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(unix)]
fn create_private_file(path: &std::path::Path) -> std::io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn create_private_file(path: &std::path::Path) -> std::io::Result<fs::File> {
    fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
}

#[cfg(unix)]
fn set_private_permissions(path: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("failed to set private permissions on {}", path.display()))
}

#[cfg(not(unix))]
fn set_private_permissions(_path: &std::path::Path) -> Result<()> {
    Ok(())
}

fn replace_file(tmp: &std::path::Path, path: &std::path::Path) -> Result<()> {
    match fs::rename(tmp, path) {
        Ok(()) => Ok(()),
        Err(first_err) if path.exists() => {
            fs::remove_file(path)
                .with_context(|| format!("failed to replace {}", path.display()))?;
            fs::rename(tmp, path).with_context(|| {
                format!(
                    "failed to rename {} to {} after replace fallback: {first_err}",
                    tmp.display(),
                    path.display()
                )
            })
        }
        Err(err) => Err(err)
            .with_context(|| format!("failed to rename {} to {}", tmp.display(), path.display())),
    }
}

fn account_ok(value: Value) -> Response {
    (StatusCode::OK, Json(value)).into_response()
}

fn account_error(status: StatusCode, error: anyhow::Error) -> Response {
    (
        status,
        Json(json!({
            "status": "error",
            "error": error.to_string(),
        })),
    )
        .into_response()
}
