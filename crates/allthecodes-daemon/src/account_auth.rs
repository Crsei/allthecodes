//! Desktop account authentication bridge.
//!
//! Electron opens the website authorization URL and gives the custom-scheme
//! callback back to this local daemon. The daemon owns PKCE state, exchanges the
//! authorization code with allthecodes.com, stores only the desktop refresh
//! token in the OS keychain, and keeps access tokens in memory.

use std::fs;

use allthecodes_auth::api_key::KEYCHAIN_SERVICE_NAME;
use allthecodes_auth::oauth::pkce;
use anyhow::{anyhow, Context, Result};
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use url::Url;

use crate::process_state;
use crate::state::DaemonState;

const DEFAULT_ACCOUNT_SITE_URL: &str = "https://allthecodes.com";
const DESKTOP_REDIRECT_URI: &str = "allthecodes://auth/callback";
const KEYCHAIN_ACCOUNT_DESKTOP_REFRESH_TOKEN: &str = "desktop-refresh-token";

#[derive(Debug, Clone, Default)]
pub struct AccountAuthMemory {
    pub pending: Option<PendingLogin>,
    pub session: Option<AccountAuthSession>,
}

#[derive(Debug, Clone)]
pub struct PendingLogin {
    pub account_site_url: String,
    pub redirect_uri: String,
    pub state: String,
    pub code_verifier: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountAuthSession {
    pub account_site_url: String,
    pub access_token: String,
    pub expires_at: String,
    pub user: Value,
    pub subscription: Value,
    pub entitlements: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AccountAuthMetadata {
    account_site_url: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginStartRequest {
    pub account_site_url: Option<String>,
    pub redirect_uri: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct LoginCompleteRequest {
    pub callback_url: Option<String>,
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

#[derive(Debug, Serialize)]
struct ExchangeRequest<'a> {
    code: &'a str,
    state: &'a str,
    code_verifier: &'a str,
    redirect_uri: &'a str,
}

#[derive(Debug, Serialize)]
struct RefreshRequest<'a> {
    refresh_token: &'a str,
}

#[derive(Debug, Serialize)]
struct LogoutRequest<'a> {
    refresh_token: &'a str,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    expires_at: String,
    user: Value,
    subscription: Value,
    entitlements: Value,
}

pub async fn login_start(
    State(state): State<DaemonState>,
    Json(body): Json<LoginStartRequest>,
) -> (StatusCode, Json<Value>) {
    let account_site_url = match normalize_account_site_url(
        body.account_site_url
            .as_deref()
            .unwrap_or(DEFAULT_ACCOUNT_SITE_URL),
    ) {
        Ok(url) => url,
        Err(err) => return error(StatusCode::BAD_REQUEST, err),
    };
    let redirect_uri = body.redirect_uri.as_deref().unwrap_or(DESKTOP_REDIRECT_URI);

    if redirect_uri != DESKTOP_REDIRECT_URI {
        return error(StatusCode::BAD_REQUEST, anyhow!("unsupported redirect_uri"));
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
        Err(err) => return error(StatusCode::BAD_REQUEST, err),
    };

    {
        let mut guard = state.account_auth.lock();
        guard.pending = Some(PendingLogin {
            account_site_url: account_site_url.clone(),
            redirect_uri: redirect_uri.to_string(),
            state: state_value,
            code_verifier,
        });
    }

    let _ = save_metadata(&AccountAuthMetadata { account_site_url });

    ok(json!({
        "status": "pending",
        "authorize_url": authorize_url,
        "authorization_url": authorize_url,
        "url": authorize_url,
    }))
}

pub async fn login_complete(
    State(state): State<DaemonState>,
    Json(body): Json<LoginCompleteRequest>,
) -> (StatusCode, Json<Value>) {
    if let Some(error_value) = body.error.as_deref().filter(|value| !value.is_empty()) {
        state.account_auth.lock().pending = None;
        return error(
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

    let (code, callback_state) = match login_complete_code_and_state(&body) {
        Ok(value) => value,
        Err(err) => return error(StatusCode::BAD_REQUEST, err),
    };

    let pending = {
        let guard = state.account_auth.lock();
        guard.pending.clone()
    };
    let Some(pending) = pending else {
        return error(StatusCode::BAD_REQUEST, anyhow!("no pending account login"));
    };

    if callback_state != pending.state {
        return error(
            StatusCode::BAD_REQUEST,
            anyhow!("account login state mismatch"),
        );
    }

    match exchange_code(&pending, &code).await {
        Ok(session) => {
            state.account_auth.lock().session = Some(session.clone());
            state.account_auth.lock().pending = None;
            ok(session_payload("authenticated", &session))
        }
        Err(err) => error(StatusCode::BAD_GATEWAY, err),
    }
}

pub async fn status(State(state): State<DaemonState>) -> (StatusCode, Json<Value>) {
    if let Some(session) = current_unexpired_session(&state) {
        return ok(session_payload("authenticated", &session));
    }

    match refresh_from_keychain(&state).await {
        Ok(Some(session)) => ok(session_payload("authenticated", &session)),
        Ok(None) => ok(json!({ "status": "signed_out" })),
        Err(err) => error(StatusCode::BAD_GATEWAY, err),
    }
}

pub async fn refresh(State(state): State<DaemonState>) -> (StatusCode, Json<Value>) {
    match refresh_from_keychain(&state).await {
        Ok(Some(session)) => ok(session_payload("authenticated", &session)),
        Ok(None) => error(
            StatusCode::UNAUTHORIZED,
            anyhow!("no desktop account session"),
        ),
        Err(err) => error(StatusCode::BAD_GATEWAY, err),
    }
}

pub async fn logout(State(state): State<DaemonState>) -> (StatusCode, Json<Value>) {
    let metadata = load_metadata().unwrap_or_else(|_| default_metadata());

    if let Ok(Some(refresh_token)) = load_refresh_token() {
        let _ = post_logout(&metadata.account_site_url, &refresh_token).await;
    }

    let _ = remove_refresh_token();
    state.account_auth.lock().session = None;
    state.account_auth.lock().pending = None;
    ok(json!({ "ok": true, "status": "signed_out" }))
}

fn current_unexpired_session(state: &DaemonState) -> Option<AccountAuthSession> {
    let session = state.account_auth.lock().session.clone()?;
    if access_token_expired(&session) {
        None
    } else {
        Some(session)
    }
}

async fn refresh_from_keychain(state: &DaemonState) -> Result<Option<AccountAuthSession>> {
    let Some(refresh_token) = load_refresh_token()? else {
        state.account_auth.lock().session = None;
        return Ok(None);
    };
    let metadata = load_metadata().unwrap_or_else(|_| default_metadata());
    let token = post_refresh(&metadata.account_site_url, &refresh_token).await?;
    let session = session_from_token_response(metadata.account_site_url, token)?;
    state.account_auth.lock().session = Some(session.clone());
    Ok(Some(session))
}

async fn exchange_code(pending: &PendingLogin, code: &str) -> Result<AccountAuthSession> {
    let url = endpoint_url(&pending.account_site_url, "/api/desktop-auth/exchange")?;
    let response = reqwest::Client::new()
        .post(url)
        .json(&ExchangeRequest {
            code,
            state: &pending.state,
            code_verifier: &pending.code_verifier,
            redirect_uri: &pending.redirect_uri,
        })
        .send()
        .await
        .context("failed to exchange desktop authorization code")?;
    let token = parse_token_response(response).await?;
    session_from_token_response(pending.account_site_url.clone(), token)
}

async fn post_refresh(account_site_url: &str, refresh_token: &str) -> Result<TokenResponse> {
    let url = endpoint_url(account_site_url, "/api/desktop-auth/refresh")?;
    let response = reqwest::Client::new()
        .post(url)
        .json(&RefreshRequest { refresh_token })
        .send()
        .await
        .context("failed to refresh desktop account session")?;
    parse_token_response(response).await
}

async fn post_logout(account_site_url: &str, refresh_token: &str) -> Result<()> {
    let url = endpoint_url(account_site_url, "/api/desktop-auth/logout")?;
    let _ = reqwest::Client::new()
        .post(url)
        .json(&LogoutRequest { refresh_token })
        .send()
        .await?;
    Ok(())
}

async fn parse_token_response(response: reqwest::Response) -> Result<TokenResponse> {
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

    serde_json::from_str::<TokenResponse>(&text).context("invalid desktop auth token response")
}

fn session_from_token_response(
    account_site_url: String,
    token: TokenResponse,
) -> Result<AccountAuthSession> {
    store_refresh_token(&token.refresh_token)?;
    save_metadata(&AccountAuthMetadata {
        account_site_url: account_site_url.clone(),
    })?;
    Ok(AccountAuthSession {
        account_site_url,
        access_token: token.access_token,
        expires_at: token.expires_at,
        user: token.user,
        subscription: token.subscription,
        entitlements: token.entitlements,
    })
}

fn login_complete_code_and_state(body: &LoginCompleteRequest) -> Result<(String, String)> {
    if let (Some(code), Some(state)) = (body.code.as_deref(), body.state.as_deref()) {
        if !code.is_empty() && !state.is_empty() {
            return Ok((code.to_string(), state.to_string()));
        }
    }

    let Some(callback_url) = body.callback_url.as_deref() else {
        return Err(anyhow!("missing account login callback_url"));
    };
    let url = Url::parse(callback_url).context("invalid account login callback_url")?;
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
    let mut url = endpoint_url(account_site_url, "/app/authorize")?;
    url.query_pairs_mut()
        .append_pair("state", state)
        .append_pair("code_challenge", code_challenge)
        .append_pair("redirect_uri", redirect_uri);
    Ok(url.to_string())
}

fn endpoint_url(account_site_url: &str, path: &str) -> Result<Url> {
    let mut url = Url::parse(account_site_url).context("invalid account site URL")?;
    url.set_path(path);
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

fn normalize_account_site_url(raw: &str) -> Result<String> {
    let url = Url::parse(raw).context("invalid account site URL")?;
    match url.scheme() {
        "http" | "https" => Ok(url.origin().ascii_serialization()),
        _ => Err(anyhow!("account_site_url must use http or https")),
    }
}

fn access_token_expired(session: &AccountAuthSession) -> bool {
    let Ok(expires_at) = DateTime::parse_from_rfc3339(&session.expires_at) else {
        return true;
    };
    Utc::now() >= expires_at.with_timezone(&Utc)
}

fn session_payload(status: &str, session: &AccountAuthSession) -> Value {
    json!({
        "status": status,
        "expires_at": session.expires_at,
        "user": session.user,
        "subscription": session.subscription,
        "entitlements": session.entitlements,
    })
}

fn metadata_path() -> std::path::PathBuf {
    process_state::daemon_dir().join("account-auth.json")
}

fn load_metadata() -> Result<AccountAuthMetadata> {
    let raw =
        fs::read_to_string(metadata_path()).context("failed to read account auth metadata")?;
    serde_json::from_str(&raw).context("failed to parse account auth metadata")
}

fn save_metadata(metadata: &AccountAuthMetadata) -> Result<()> {
    fs::create_dir_all(process_state::daemon_dir())
        .context("failed to create daemon account auth directory")?;
    fs::write(metadata_path(), serde_json::to_string_pretty(metadata)?)
        .context("failed to write account auth metadata")
}

fn default_metadata() -> AccountAuthMetadata {
    AccountAuthMetadata {
        account_site_url: std::env::var("ALLTHECODES_ACCOUNT_SITE_URL")
            .ok()
            .and_then(|value| normalize_account_site_url(&value).ok())
            .unwrap_or_else(|| DEFAULT_ACCOUNT_SITE_URL.to_string()),
    }
}

fn store_refresh_token(token: &str) -> Result<()> {
    let entry = keyring::Entry::new(
        KEYCHAIN_SERVICE_NAME,
        KEYCHAIN_ACCOUNT_DESKTOP_REFRESH_TOKEN,
    )?;
    entry.set_password(token)?;
    Ok(())
}

fn load_refresh_token() -> Result<Option<String>> {
    let entry = keyring::Entry::new(
        KEYCHAIN_SERVICE_NAME,
        KEYCHAIN_ACCOUNT_DESKTOP_REFRESH_TOKEN,
    )?;
    match entry.get_password() {
        Ok(token) => Ok(Some(token)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn remove_refresh_token() -> Result<()> {
    let entry = keyring::Entry::new(
        KEYCHAIN_SERVICE_NAME,
        KEYCHAIN_ACCOUNT_DESKTOP_REFRESH_TOKEN,
    )?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn ok(value: Value) -> (StatusCode, Json<Value>) {
    (StatusCode::OK, Json(value))
}

fn error(status: StatusCode, error: anyhow::Error) -> (StatusCode, Json<Value>) {
    (
        status,
        Json(json!({
            "status": "error",
            "error": error.to_string(),
        })),
    )
}
