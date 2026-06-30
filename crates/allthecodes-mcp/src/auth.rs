//! OAuth support for remote MCP transports.
//!
//! The on-disk settings file stores only OAuth metadata. Access and refresh
//! tokens live in `{ALLTHECODES_HOME|~/.allthecodes}/mcp-oauth.json` so they stay
//! isolated from upstream Codex paths and are never echoed through settings,
//! command output, or IPC config events.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::Utc;
use rand::RngCore;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::warn;
use url::Url;

use super::{McpOAuthConfig, McpServerConfig};

const DEFAULT_CLIENT_ID: &str = "allthecodes";
const DEFAULT_CALLBACK_PORT: u16 = 1455;
const TOKEN_EXPIRY_SKEW_SECS: i64 = 60;

/// Interactive authorization state returned to the command/UI layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpOAuthStart {
    pub authorization_url: String,
    pub state: String,
    pub redirect_uri: String,
    pub token_store_path: PathBuf,
}

/// Redacted credential status for command and IPC surfaces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpOAuthCredentialStatus {
    pub configured: bool,
    pub authorized: bool,
    pub expired: bool,
    pub can_refresh: bool,
    pub token_store_path: PathBuf,
}

/// Token persisted for one MCP server. `Debug` is intentionally redacted.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredMcpOAuthToken {
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(default = "default_token_type")]
    pub token_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scopes: Vec<String>,
    pub authorization_server: String,
    pub token_endpoint: String,
    pub client_id: String,
}

impl fmt::Debug for StoredMcpOAuthToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StoredMcpOAuthToken")
            .field("access_token", &"<redacted>")
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "<redacted>"),
            )
            .field("token_type", &self.token_type)
            .field("expires_at", &self.expires_at)
            .field("scopes", &self.scopes)
            .field("authorization_server", &self.authorization_server)
            .field("token_endpoint", &self.token_endpoint)
            .field("client_id", &self.client_id)
            .finish()
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct OAuthStore {
    #[serde(default)]
    servers: BTreeMap<String, StoredMcpOAuthToken>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct OAuthPendingStore {
    #[serde(default)]
    pending: BTreeMap<String, PendingMcpOAuthAuthorization>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingMcpOAuthAuthorization {
    server_name: String,
    server_url: Option<String>,
    state: String,
    code_verifier: String,
    redirect_uri: String,
    client_id: String,
    scopes: Vec<String>,
    authorization_endpoint: String,
    token_endpoint: String,
    authorization_server: String,
    created_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
struct ProtectedResourceMetadata {
    #[serde(default)]
    authorization_servers: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct AuthorizationServerMetadata {
    #[serde(default)]
    issuer: Option<String>,
    authorization_endpoint: String,
    token_endpoint: String,
    #[serde(default)]
    scopes_supported: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct OAuthTokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    token_type: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
    #[serde(default)]
    scope: Option<String>,
}

fn default_token_type() -> String {
    "Bearer".to_string()
}

/// Return the MCP OAuth token store path.
pub fn token_store_path() -> PathBuf {
    allthecodes_config::paths::data_root().join("mcp-oauth.json")
}

fn pending_store_path() -> PathBuf {
    allthecodes_config::paths::data_root().join("mcp-oauth-pending.json")
}

/// Return a redacted representation suitable for logs or command output.
pub fn redact_secret(_value: &str) -> &'static str {
    "<redacted>"
}

/// Build an authorization header from a stored MCP OAuth token.
///
/// Returns `Ok(None)` when the server is not OAuth-configured or has no usable
/// stored token. Expired tokens are refreshed when a refresh token is available.
pub async fn authorization_header(config: &McpServerConfig) -> Result<Option<String>> {
    if config.oauth.is_none() {
        return Ok(None);
    }

    let key = server_auth_key(config);
    let mut store = read_oauth_store()?;
    let Some(mut token) = store.servers.get(&key).cloned() else {
        return Ok(None);
    };

    if token_is_expired(&token) {
        match refresh_stored_token(config, &token).await {
            Ok(refreshed) => {
                token = refreshed;
                store.servers.insert(key, token.clone());
                write_oauth_store(&store)?;
            }
            Err(err) => {
                warn!(
                    server = %config.name,
                    error = %err,
                    "MCP OAuth token refresh failed; reconnect will require auth"
                );
                return Ok(None);
            }
        }
    }

    validate_header_token(&token.token_type, &token.access_token)?;
    Ok(Some(format!("{} {}", token.token_type, token.access_token)))
}

/// Start a manual OAuth authorization flow for one MCP server.
pub async fn start_authorization(config: &McpServerConfig) -> Result<McpOAuthStart> {
    let oauth = require_oauth_config(config)?;
    validate_oauth_config(oauth)?;
    let metadata = discover_authorization_server_metadata(config).await?;
    validate_metadata(&metadata)?;

    let client_id = oauth
        .client_id
        .clone()
        .unwrap_or_else(|| DEFAULT_CLIENT_ID.to_string());
    let redirect_uri = redirect_uri(oauth);
    let scopes = requested_scopes(oauth, &metadata);
    let state = generate_random_urlsafe();
    let code_verifier = generate_random_urlsafe();
    let code_challenge = pkce_challenge(&code_verifier);

    let mut authorization_url = Url::parse(&metadata.authorization_endpoint)
        .context("invalid OAuth authorization endpoint")?;
    {
        let mut pairs = authorization_url.query_pairs_mut();
        pairs
            .append_pair("response_type", "code")
            .append_pair("client_id", &client_id)
            .append_pair("redirect_uri", &redirect_uri)
            .append_pair("state", &state)
            .append_pair("code_challenge", &code_challenge)
            .append_pair("code_challenge_method", "S256");
        if !scopes.is_empty() {
            pairs.append_pair("scope", &scopes.join(" "));
        }
        // Use oauth_resource from config, fall back to MCP server URL
        let resource = oauth
            .oauth_resource
            .as_deref()
            .or_else(|| config.url.as_deref());
        if let Some(resource) = resource {
            pairs.append_pair("resource", resource);
        }
    }

    let pending = PendingMcpOAuthAuthorization {
        server_name: config.name.clone(),
        server_url: config.url.clone(),
        state: state.clone(),
        code_verifier,
        redirect_uri: redirect_uri.clone(),
        client_id,
        scopes,
        authorization_endpoint: metadata.authorization_endpoint,
        token_endpoint: metadata.token_endpoint,
        authorization_server: metadata.issuer.unwrap_or_default(),
        created_at: now_timestamp(),
    };
    save_pending_authorization(config, pending)?;

    Ok(McpOAuthStart {
        authorization_url: authorization_url.to_string(),
        state,
        redirect_uri,
        token_store_path: token_store_path(),
    })
}

/// Complete a previously-started manual OAuth authorization flow.
pub async fn complete_authorization(
    config: &McpServerConfig,
    code: &str,
    state: Option<&str>,
) -> Result<StoredMcpOAuthToken> {
    let code = code.trim();
    if code.is_empty() || code.contains('\r') || code.contains('\n') {
        bail!("OAuth authorization code must be non-empty and single-line");
    }

    let key = server_auth_key(config);
    let mut pending_store = read_pending_store()?;
    let pending = pending_store.pending.remove(&key).ok_or_else(|| {
        anyhow::anyhow!("no pending MCP OAuth authorization for `{}`", config.name)
    })?;
    if let Some(state) = state {
        if state != pending.state {
            bail!("OAuth state mismatch for MCP server `{}`", config.name);
        }
    }
    write_pending_store(&pending_store)?;

    let response = exchange_code_for_token(config, &pending, code).await?;
    let token = token_from_response(&pending, response)?;
    let mut store = read_oauth_store()?;
    store.servers.insert(key, token.clone());
    write_oauth_store(&store)?;
    Ok(token)
}

/// Remove stored MCP OAuth credentials for one server.
pub fn clear_stored_token(config: &McpServerConfig) -> Result<bool> {
    let key = server_auth_key(config);
    let mut store = read_oauth_store()?;
    let removed = store.servers.remove(&key).is_some();
    write_oauth_store(&store)?;

    let mut pending = read_pending_store()?;
    pending.pending.remove(&key);
    write_pending_store(&pending)?;

    Ok(removed)
}

/// Return a redacted, non-refreshing credential status snapshot.
pub fn credential_status(config: &McpServerConfig) -> Result<McpOAuthCredentialStatus> {
    let store = read_oauth_store()?;
    let token = store.servers.get(&server_auth_key(config)).cloned();
    Ok(McpOAuthCredentialStatus {
        configured: config.oauth.is_some(),
        authorized: token.is_some(),
        expired: token.as_ref().map(token_is_expired).unwrap_or(false),
        can_refresh: token
            .as_ref()
            .and_then(|t| t.refresh_token.as_ref())
            .is_some(),
        token_store_path: token_store_path(),
    })
}

fn require_oauth_config(config: &McpServerConfig) -> Result<&McpOAuthConfig> {
    config.oauth.as_ref().ok_or_else(|| {
        anyhow::anyhow!(
            "MCP server `{}` has no OAuth configuration; add oauth metadata to its MCP config",
            config.name
        )
    })
}

fn validate_oauth_config(oauth: &McpOAuthConfig) -> Result<()> {
    if let Some(url) = oauth.auth_server_metadata_url.as_deref() {
        let parsed = Url::parse(url).context("invalid OAuth authorization-server metadata URL")?;
        validate_oauth_endpoint_url(&parsed, "OAuth authorization-server metadata URL")?;
    }
    if let Some(client_id) = oauth.client_id.as_deref() {
        if client_id.trim().is_empty() || client_id.contains('\r') || client_id.contains('\n') {
            bail!("OAuth clientId must be a non-empty single-line value");
        }
    }
    if let Some(scopes) = &oauth.scopes {
        for scope in scopes {
            if scope.trim().is_empty() || scope.chars().any(char::is_whitespace) {
                bail!("OAuth scopes must be non-empty single tokens");
            }
        }
    }
    Ok(())
}

async fn discover_authorization_server_metadata(
    config: &McpServerConfig,
) -> Result<AuthorizationServerMetadata> {
    let oauth = require_oauth_config(config)?;
    if let Some(url) = oauth.auth_server_metadata_url.as_deref() {
        let metadata_url = Url::parse(url)?;
        let http_client = oauth_http_client_for_url(&metadata_url)?;
        return fetch_auth_server_metadata(&http_client, &metadata_url).await;
    }

    let server_url = config
        .url
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("OAuth discovery requires the MCP server URL"))?;
    let origin = origin_url(&Url::parse(server_url).context("invalid MCP server URL")?)?;
    let protected_resource_url = origin.join("/.well-known/oauth-protected-resource")?;
    let http_client = oauth_http_client_for_url(&protected_resource_url)?;
    if let Some(resource) =
        fetch_protected_resource_metadata(&http_client, &protected_resource_url).await?
    {
        if let Some(first) = resource.authorization_servers.first() {
            let metadata_url = authorization_server_metadata_url(first)?;
            return fetch_auth_server_metadata(&http_client, &metadata_url).await;
        }
    }

    let fallback = origin.join("/.well-known/oauth-authorization-server")?;
    fetch_auth_server_metadata(&http_client, &fallback).await
}

async fn fetch_protected_resource_metadata(
    http_client: &reqwest::Client,
    url: &Url,
) -> Result<Option<ProtectedResourceMetadata>> {
    validate_oauth_endpoint_url(url, "OAuth protected-resource metadata URL")?;
    let response = http_client
        .get(url.clone())
        .header(
            "User-Agent",
            allthecodes_config::user_agent::mcp_user_agent(),
        )
        .send()
        .await
        .with_context(|| format!("failed to fetch MCP OAuth resource metadata from {}", url))?;
    if response.status() == StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !response.status().is_success() {
        return Ok(None);
    }
    response
        .json::<ProtectedResourceMetadata>()
        .await
        .map(Some)
        .with_context(|| format!("failed to parse MCP OAuth resource metadata from {}", url))
}

async fn fetch_auth_server_metadata(
    http_client: &reqwest::Client,
    url: &Url,
) -> Result<AuthorizationServerMetadata> {
    validate_oauth_endpoint_url(url, "OAuth authorization-server metadata URL")?;
    let response = http_client
        .get(url.clone())
        .header(
            "User-Agent",
            allthecodes_config::user_agent::mcp_user_agent(),
        )
        .send()
        .await
        .with_context(|| {
            format!(
                "failed to fetch MCP OAuth authorization-server metadata from {}",
                url
            )
        })?;
    if !response.status().is_success() {
        bail!(
            "MCP OAuth authorization-server metadata fetch returned HTTP {}",
            response.status().as_u16()
        );
    }
    let metadata = response
        .json::<AuthorizationServerMetadata>()
        .await
        .context("failed to parse MCP OAuth authorization-server metadata")?;
    validate_metadata(&metadata)?;
    Ok(metadata)
}

fn validate_metadata(metadata: &AuthorizationServerMetadata) -> Result<()> {
    let authorization_endpoint = Url::parse(&metadata.authorization_endpoint)
        .context("invalid OAuth authorization endpoint")?;
    let token_endpoint =
        Url::parse(&metadata.token_endpoint).context("invalid OAuth token endpoint")?;
    validate_oauth_endpoint_url(&authorization_endpoint, "OAuth authorization endpoint")?;
    validate_oauth_endpoint_url(&token_endpoint, "OAuth token endpoint")?;
    Ok(())
}

async fn exchange_code_for_token(
    config: &McpServerConfig,
    pending: &PendingMcpOAuthAuthorization,
    code: &str,
) -> Result<OAuthTokenResponse> {
    let token_endpoint = Url::parse(&pending.token_endpoint).context("invalid OAuth token URL")?;
    validate_oauth_endpoint_url(&token_endpoint, "OAuth token endpoint")?;
    let mut form = vec![
        ("grant_type".to_string(), "authorization_code".to_string()),
        ("code".to_string(), code.to_string()),
        ("redirect_uri".to_string(), pending.redirect_uri.clone()),
        ("client_id".to_string(), pending.client_id.clone()),
        ("code_verifier".to_string(), pending.code_verifier.clone()),
    ];
    if let Some(resource) = resource_for_tokens(config) {
        form.push(("resource".to_string(), resource.to_string()));
    }
    post_token_form(&token_endpoint, form).await
}

async fn refresh_stored_token(
    config: &McpServerConfig,
    token: &StoredMcpOAuthToken,
) -> Result<StoredMcpOAuthToken> {
    let refresh_token = token
        .refresh_token
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("MCP OAuth token has expired and has no refresh token"))?;
    let token_endpoint = Url::parse(&token.token_endpoint).context("invalid OAuth token URL")?;
    validate_oauth_endpoint_url(&token_endpoint, "OAuth token endpoint")?;

    let mut form = vec![
        ("grant_type".to_string(), "refresh_token".to_string()),
        ("refresh_token".to_string(), refresh_token.to_string()),
        ("client_id".to_string(), token.client_id.clone()),
    ];
    if !token.scopes.is_empty() {
        form.push(("scope".to_string(), token.scopes.join(" ")));
    }
    if let Some(resource) = resource_for_tokens(config) {
        form.push(("resource".to_string(), resource.to_string()));
    }

    let response = post_token_form(&token_endpoint, form).await?;
    let pending = PendingMcpOAuthAuthorization {
        server_name: config.name.clone(),
        server_url: config.url.clone(),
        state: String::new(),
        code_verifier: String::new(),
        redirect_uri: String::new(),
        client_id: token.client_id.clone(),
        scopes: token.scopes.clone(),
        authorization_endpoint: String::new(),
        token_endpoint: token.token_endpoint.clone(),
        authorization_server: token.authorization_server.clone(),
        created_at: now_timestamp(),
    };
    let mut refreshed = token_from_response(&pending, response)?;
    if refreshed.refresh_token.is_none() {
        refreshed.refresh_token = token.refresh_token.clone();
    }
    Ok(refreshed)
}

async fn post_token_form(
    token_endpoint: &Url,
    form: Vec<(String, String)>,
) -> Result<OAuthTokenResponse> {
    let body = encode_form(form);
    let response = oauth_http_client_for_url(token_endpoint)?
        .post(token_endpoint.clone())
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Accept", "application/json")
        .header(
            "User-Agent",
            allthecodes_config::user_agent::mcp_user_agent(),
        )
        .body(body)
        .send()
        .await
        .with_context(|| {
            format!(
                "failed to POST MCP OAuth token request to {}",
                token_endpoint
            )
        })?;
    if !response.status().is_success() {
        bail!(
            "MCP OAuth token endpoint returned HTTP {}",
            response.status().as_u16()
        );
    }
    response
        .json::<OAuthTokenResponse>()
        .await
        .context("failed to parse MCP OAuth token response")
}

fn token_from_response(
    pending: &PendingMcpOAuthAuthorization,
    response: OAuthTokenResponse,
) -> Result<StoredMcpOAuthToken> {
    if response.access_token.trim().is_empty()
        || response.access_token.contains('\r')
        || response.access_token.contains('\n')
    {
        bail!("MCP OAuth token response did not include a valid access_token");
    }
    let token_type = response
        .token_type
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(default_token_type);
    validate_header_token(&token_type, &response.access_token)?;

    let scopes = response
        .scope
        .map(|scope| {
            scope
                .split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| pending.scopes.clone());
    let expires_at = response
        .expires_in
        .and_then(|seconds| (seconds > 0).then(|| now_timestamp() + seconds));

    Ok(StoredMcpOAuthToken {
        access_token: response.access_token,
        refresh_token: response.refresh_token,
        token_type,
        expires_at,
        scopes,
        authorization_server: pending.authorization_server.clone(),
        token_endpoint: pending.token_endpoint.clone(),
        client_id: pending.client_id.clone(),
    })
}

fn requested_scopes(oauth: &McpOAuthConfig, metadata: &AuthorizationServerMetadata) -> Vec<String> {
    oauth
        .scopes
        .clone()
        .unwrap_or_else(|| metadata.scopes_supported.clone())
}

fn redirect_uri(oauth: &McpOAuthConfig) -> String {
    let port = oauth.callback_port.unwrap_or(DEFAULT_CALLBACK_PORT);
    format!("http://127.0.0.1:{port}/mcp/oauth/callback")
}

fn save_pending_authorization(
    config: &McpServerConfig,
    pending: PendingMcpOAuthAuthorization,
) -> Result<()> {
    let mut store = read_pending_store()?;
    store.pending.insert(server_auth_key(config), pending);
    write_pending_store(&store)
}

fn read_oauth_store() -> Result<OAuthStore> {
    read_json_or_default(token_store_path())
}

fn write_oauth_store(store: &OAuthStore) -> Result<()> {
    write_json_atomic(token_store_path(), store)
}

fn read_pending_store() -> Result<OAuthPendingStore> {
    read_json_or_default(pending_store_path())
}

fn write_pending_store(store: &OAuthPendingStore) -> Result<()> {
    write_json_atomic(pending_store_path(), store)
}

fn read_json_or_default<T>(path: PathBuf) -> Result<T>
where
    T: for<'de> Deserialize<'de> + Default,
{
    if !path.exists() {
        return Ok(T::default());
    }
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    if content.trim().is_empty() {
        return Ok(T::default());
    }
    serde_json::from_str(&content).with_context(|| format!("failed to parse {}", path.display()))
}

fn write_json_atomic<T>(path: PathBuf, value: &T) -> Result<()>
where
    T: Serialize,
{
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let tmp = path.with_extension("json.tmp");
    let pretty =
        serde_json::to_string_pretty(value).context("failed to serialize MCP OAuth data")?;
    std::fs::write(&tmp, pretty).with_context(|| format!("failed to write {}", tmp.display()))?;
    std::fs::rename(&tmp, &path)
        .with_context(|| format!("failed to rename {} -> {}", tmp.display(), path.display()))?;
    Ok(())
}

fn server_auth_key(config: &McpServerConfig) -> String {
    let endpoint = config
        .url
        .as_deref()
        .or(config.command.as_deref())
        .unwrap_or_default();
    format!("{}|{}|{}", config.name, config.transport, endpoint)
}

fn token_is_expired(token: &StoredMcpOAuthToken) -> bool {
    token
        .expires_at
        .map(|expires_at| expires_at <= now_timestamp() + TOKEN_EXPIRY_SKEW_SECS)
        .unwrap_or(false)
}

fn now_timestamp() -> i64 {
    Utc::now().timestamp()
}

fn generate_random_urlsafe() -> String {
    let mut buf = [0_u8; 32];
    rand::thread_rng().fill_bytes(&mut buf);
    URL_SAFE_NO_PAD.encode(buf)
}

fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn encode_form(fields: Vec<(String, String)>) -> String {
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    for (key, value) in fields {
        serializer.append_pair(&key, &value);
    }
    serializer.finish()
}

fn validate_header_token(token_type: &str, access_token: &str) -> Result<()> {
    if token_type.contains('\r')
        || token_type.contains('\n')
        || token_type.contains('\0')
        || access_token.contains('\r')
        || access_token.contains('\n')
        || access_token.contains('\0')
    {
        bail!("MCP OAuth token contains invalid header characters");
    }
    Ok(())
}

fn oauth_http_client_for_url(url: &Url) -> Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none());
    if is_loopback_url(url) {
        builder = builder.no_proxy();
    }
    builder
        .build()
        .context("failed to build MCP OAuth HTTP client")
}

fn origin_url(url: &Url) -> Result<Url> {
    let mut origin = url.clone();
    origin.set_path("");
    origin.set_query(None);
    origin.set_fragment(None);
    Ok(origin)
}

fn authorization_server_metadata_url(issuer: &str) -> Result<Url> {
    let issuer = Url::parse(issuer).context("invalid OAuth authorization server issuer URL")?;
    validate_oauth_endpoint_url(&issuer, "OAuth authorization server issuer URL")?;
    if issuer
        .path()
        .contains("/.well-known/oauth-authorization-server")
    {
        return Ok(issuer);
    }
    let mut metadata = issuer;
    metadata.set_path("/.well-known/oauth-authorization-server");
    metadata.set_query(None);
    metadata.set_fragment(None);
    Ok(metadata)
}

fn validate_oauth_endpoint_url(url: &Url, label: &str) -> Result<()> {
    if url.scheme() == "https" {
        if url.host_str().is_none() {
            bail!("{label} must include a host");
        }
        if !url.username().is_empty() || url.password().is_some() {
            bail!("{label} must not include embedded credentials");
        }
        return Ok(());
    }

    if url.scheme() == "http" && is_loopback_url(url) {
        return Ok(());
    }

    bail!("{label} must use https:// unless it targets loopback development");
}

fn is_loopback_url(url: &Url) -> bool {
    matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "::1"))
}

/// Resolve the OAuth `resource` parameter (RFC 8707) for token requests.
/// Uses `oauth_resource` from the OAuth config when set, falling back to
/// the MCP server URL.
fn resource_for_tokens(config: &McpServerConfig) -> Option<String> {
    config
        .oauth
        .as_ref()
        .and_then(|o| o.oauth_resource.clone())
        .or_else(|| config.url.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::TempDir;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    fn oauth_config(url: String) -> McpServerConfig {
        McpServerConfig {
            name: "remote".to_string(),
            transport: "sse".to_string(),
            command: None,
            args: None,
            url: Some("https://mcp.example.com/sse".to_string()),
            headers: None,
            oauth: Some(McpOAuthConfig {
                client_id: Some("test-client".to_string()),
                callback_port: Some(18888),
                auth_server_metadata_url: Some(url),
                scopes: Some(vec!["tools.read".to_string()]),
                oauth_resource: None,
            }),
            env: None,
            browser_mcp: None,
            disabled: None,
            bearer_token_env_var: None,
            env_http_headers: None,
            auth: None,
        }
    }

    async fn read_http_request(stream: &mut TcpStream) -> (String, String) {
        let mut buffer = Vec::new();
        let header_end = loop {
            if let Some(index) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
                break index;
            }
            let mut chunk = [0_u8; 512];
            let read = stream.read(&mut chunk).await.unwrap();
            assert!(read > 0, "connection closed before HTTP headers completed");
            buffer.extend_from_slice(&chunk[..read]);
        };

        let head = String::from_utf8(buffer[..header_end].to_vec()).unwrap();
        let content_length = head
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or(0);
        let body_start = header_end + 4;
        let mut body = buffer[body_start..].to_vec();
        while body.len() < content_length {
            let mut chunk = vec![0_u8; content_length - body.len()];
            let read = stream.read(&mut chunk).await.unwrap();
            assert!(read > 0, "connection closed before HTTP body completed");
            body.extend_from_slice(&chunk[..read]);
        }
        body.truncate(content_length);
        (head, String::from_utf8(body).unwrap())
    }

    async fn write_json_response(stream: &mut TcpStream, body: serde_json::Value) {
        let body = body.to_string();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).await.unwrap();
        stream.flush().await.unwrap();
    }

    #[test]
    fn oauth_config_round_trips_without_tokens() {
        let config: McpServerConfig = serde_json::from_value(serde_json::json!({
            "type": "sse",
            "url": "https://mcp.example.com/sse",
            "oauth": {
                "clientId": "client-1",
                "callbackPort": 1234,
                "authServerMetadataUrl": "https://auth.example.com/.well-known/oauth-authorization-server",
                "scopes": ["tools.read"]
            }
        }))
        .unwrap();
        assert_eq!(
            config.oauth.as_ref().unwrap().client_id.as_deref(),
            Some("client-1")
        );
        let value = serde_json::to_value(&config).unwrap();
        assert!(value["oauth"].get("accessToken").is_none());
    }

    #[test]
    #[serial]
    fn token_store_uses_cc_rust_home() {
        let temp = TempDir::new().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path().to_str().unwrap());
        assert_eq!(token_store_path(), temp.path().join("mcp-oauth.json"));
    }

    #[test]
    fn stored_token_debug_redacts_secrets() {
        let token = StoredMcpOAuthToken {
            access_token: "access-secret".to_string(),
            refresh_token: Some("refresh-secret".to_string()),
            token_type: "Bearer".to_string(),
            expires_at: None,
            scopes: Vec::new(),
            authorization_server: "https://auth.example.com".to_string(),
            token_endpoint: "https://auth.example.com/token".to_string(),
            client_id: "client".to_string(),
        };
        let debug = format!("{token:?}");
        assert!(!debug.contains("access-secret"));
        assert!(!debug.contains("refresh-secret"));
        assert!(debug.contains("<redacted>"));
    }

    #[tokio::test]
    #[serial]
    async fn authorization_header_uses_stored_bearer_without_refresh() {
        let temp = TempDir::new().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path().to_str().unwrap());
        let config =
            oauth_config("http://127.0.0.1:9/.well-known/oauth-authorization-server".to_string());
        let mut store = OAuthStore::default();
        store.servers.insert(
            server_auth_key(&config),
            StoredMcpOAuthToken {
                access_token: "stored-access".to_string(),
                refresh_token: None,
                token_type: "Bearer".to_string(),
                expires_at: Some(now_timestamp() + 3600),
                scopes: vec!["tools.read".to_string()],
                authorization_server: "http://127.0.0.1:9".to_string(),
                token_endpoint: "http://127.0.0.1:9/token".to_string(),
                client_id: "test-client".to_string(),
            },
        );
        write_oauth_store(&store).unwrap();

        let header = authorization_header(&config).await.unwrap().unwrap();
        assert_eq!(header, "Bearer stored-access");
    }

    #[tokio::test]
    #[serial]
    async fn oauth_start_and_complete_with_loopback_auth_server() {
        let temp = TempDir::new().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path().to_str().unwrap());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let metadata_url = format!(
            "http://127.0.0.1:{}/.well-known/oauth-authorization-server",
            addr.port()
        );
        let config = oauth_config(metadata_url);

        let server = tokio::spawn(async move {
            let (mut metadata_stream, _) = listener.accept().await.unwrap();
            let (head, body) = read_http_request(&mut metadata_stream).await;
            assert!(head.starts_with("GET /.well-known/oauth-authorization-server "));
            assert!(body.is_empty());
            write_json_response(
                &mut metadata_stream,
                serde_json::json!({
                    "issuer": format!("http://127.0.0.1:{}", addr.port()),
                    "authorization_endpoint": format!("http://127.0.0.1:{}/authorize", addr.port()),
                    "token_endpoint": format!("http://127.0.0.1:{}/token", addr.port()),
                    "scopes_supported": ["tools.read"]
                }),
            )
            .await;

            let (mut token_stream, _) = listener.accept().await.unwrap();
            let (head, body) = read_http_request(&mut token_stream).await;
            assert!(head.starts_with("POST /token "));
            assert!(body.contains("grant_type=authorization_code"));
            assert!(body.contains("code=returned-code"));
            assert!(body.contains("code_verifier="));
            write_json_response(
                &mut token_stream,
                serde_json::json!({
                    "access_token": "new-access",
                    "refresh_token": "new-refresh",
                    "token_type": "Bearer",
                    "expires_in": 3600,
                    "scope": "tools.read"
                }),
            )
            .await;
        });

        let start = start_authorization(&config).await.unwrap();
        assert!(start.authorization_url.contains("/authorize?"));
        assert!(start.authorization_url.contains("code_challenge="));
        assert!(start.authorization_url.contains("scope=tools.read"));
        assert!(!start.authorization_url.contains("code_verifier"));

        let token = complete_authorization(&config, "returned-code", Some(&start.state))
            .await
            .unwrap();
        assert_eq!(token.access_token, "new-access");
        assert_eq!(
            authorization_header(&config).await.unwrap().unwrap(),
            "Bearer new-access"
        );

        server.await.unwrap();
    }

    #[tokio::test]
    #[serial]
    async fn expired_token_refreshes_through_token_endpoint() {
        let temp = TempDir::new().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path().to_str().unwrap());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let config =
            oauth_config("http://127.0.0.1:9/.well-known/oauth-authorization-server".to_string());

        let mut store = OAuthStore::default();
        store.servers.insert(
            server_auth_key(&config),
            StoredMcpOAuthToken {
                access_token: "expired-access".to_string(),
                refresh_token: Some("refresh-me".to_string()),
                token_type: "Bearer".to_string(),
                expires_at: Some(now_timestamp() - 10),
                scopes: vec!["tools.read".to_string()],
                authorization_server: format!("http://127.0.0.1:{}", addr.port()),
                token_endpoint: format!("http://127.0.0.1:{}/token", addr.port()),
                client_id: "test-client".to_string(),
            },
        );
        write_oauth_store(&store).unwrap();

        let server = tokio::spawn(async move {
            let (mut token_stream, _) = listener.accept().await.unwrap();
            let (head, body) = read_http_request(&mut token_stream).await;
            assert!(head.starts_with("POST /token "));
            assert!(body.contains("grant_type=refresh_token"));
            assert!(body.contains("refresh_token=refresh-me"));
            write_json_response(
                &mut token_stream,
                serde_json::json!({
                    "access_token": "refreshed-access",
                    "token_type": "Bearer",
                    "expires_in": 3600
                }),
            )
            .await;
        });

        let header = authorization_header(&config).await.unwrap().unwrap();
        assert_eq!(header, "Bearer refreshed-access");
        let store = read_oauth_store().unwrap();
        let refreshed = store.servers.get(&server_auth_key(&config)).unwrap();
        assert_eq!(refreshed.refresh_token.as_deref(), Some("refresh-me"));

        server.await.unwrap();
    }

    #[test]
    #[serial]
    fn clear_token_removes_entry_and_pending() {
        let temp = TempDir::new().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path().to_str().unwrap());
        let config =
            oauth_config("http://127.0.0.1:9/.well-known/oauth-authorization-server".to_string());
        let key = server_auth_key(&config);
        let mut store = OAuthStore::default();
        store.servers.insert(
            key.clone(),
            StoredMcpOAuthToken {
                access_token: "access".to_string(),
                refresh_token: None,
                token_type: "Bearer".to_string(),
                expires_at: None,
                scopes: Vec::new(),
                authorization_server: "http://127.0.0.1:9".to_string(),
                token_endpoint: "http://127.0.0.1:9/token".to_string(),
                client_id: "test-client".to_string(),
            },
        );
        write_oauth_store(&store).unwrap();

        let mut pending = OAuthPendingStore::default();
        pending.pending.insert(
            key,
            PendingMcpOAuthAuthorization {
                server_name: config.name.clone(),
                server_url: config.url.clone(),
                state: "state".to_string(),
                code_verifier: "verifier".to_string(),
                redirect_uri: "http://127.0.0.1:18888/mcp/oauth/callback".to_string(),
                client_id: "test-client".to_string(),
                scopes: Vec::new(),
                authorization_endpoint: "http://127.0.0.1:9/authorize".to_string(),
                token_endpoint: "http://127.0.0.1:9/token".to_string(),
                authorization_server: "http://127.0.0.1:9".to_string(),
                created_at: now_timestamp(),
            },
        );
        write_pending_store(&pending).unwrap();

        assert!(clear_stored_token(&config).unwrap());
        assert!(read_oauth_store().unwrap().servers.is_empty());
        assert!(read_pending_store().unwrap().pending.is_empty());
    }

    #[test]
    fn oauth_metadata_rejects_non_loopback_http() {
        let err = validate_oauth_endpoint_url(
            &Url::parse("http://auth.example.com/.well-known/oauth-authorization-server").unwrap(),
            "metadata",
        )
        .unwrap_err();
        assert!(err.to_string().contains("https"));
    }
}
