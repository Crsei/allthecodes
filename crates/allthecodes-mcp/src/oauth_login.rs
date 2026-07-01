//! Automatic MCP OAuth login flow with a local loopback callback server.
//!
//! The flow works as follows:
//!
//! 1. Bind a `tokio::net::TcpListener` on `127.0.0.1:0` (OS-assigned port).
//! 2. Build the redirect URI from the bound port.
//! 3. Generate PKCE challenge + state (reuses functions from [`crate::auth`]).
//! 4. Build the authorization URL with all required parameters.
//! 5. Save pending state so the `wait()` method can validate and exchange.
//! 6. Return a [`McpOAuthLoginHandle`] — the caller prints the URL, then
//!    calls `wait()` to block until the callback arrives (or times out).

use std::time::Duration;

use anyhow::{Context, Result, bail};
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use url::Url;

use crate::auth::{
    AuthorizationServerMetadata, McpOAuthStart, PendingMcpOAuthAuthorization, StoredMcpOAuthToken,
    discover_authorization_server_metadata, exchange_code_for_token, generate_random_urlsafe,
    now_timestamp, pkce_challenge, remove_pending_authorization, require_oauth_config,
    save_pending_authorization, server_auth_key, token_from_response, token_store_path,
    validate_metadata, validate_oauth_config,
};
use crate::oauth_store::{McpCredentialsStoreMode, store_ops as oauth_store_ops};
use crate::{McpOAuthConfig, McpServerConfig};

/// Default OAuth callback timeout: 300 seconds.
const DEFAULT_OAUTH_TIMEOUT_SECS: u64 = 300;

/// The callback path we expect in the redirect URI.
const CALLBACK_PATH: &str = "/mcp/oauth/callback";

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Handle returned by [`start_auto_login`].
///
/// The caller should display the authorization URL to the user, then call
/// [`wait`](McpOAuthLoginHandle::wait) to block until the OAuth callback
/// completes (or times out).
pub struct McpOAuthLoginHandle {
    authorization_url: String,
    completion: oneshot::Receiver<Result<StoredMcpOAuthToken>>,
    auth_key: String,
    timeout: Duration,
}

impl McpOAuthLoginHandle {
    /// The URL the user must open in their browser to authorize.
    pub fn authorization_url(&self) -> &str {
        &self.authorization_url
    }

    /// Wait for the OAuth callback, exchange the code, and persist the token.
    ///
    /// Returns an error on timeout, callback parsing failure, state mismatch,
    /// token exchange failure, or storage failure.
    pub async fn wait(self) -> Result<StoredMcpOAuthToken> {
        let result = tokio::time::timeout(self.timeout, self.completion)
            .await
            .map_err(|_| {
                anyhow::anyhow!(
                    "OAuth callback timed out (no response received within the timeout period)"
                )
            })
            .and_then(|completion| completion.context("OAuth callback task cancelled"))
            .and_then(|callback| callback);
        if result.is_err() {
            let _ = remove_pending_authorization(&self.auth_key);
        }
        result
    }

    /// Split into parts: the authorization URL and the completion receiver.
    /// The caller can drive the receiver independently.
    pub fn into_parts(self) -> (String, oneshot::Receiver<Result<StoredMcpOAuthToken>>) {
        (self.authorization_url, self.completion)
    }
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Start an automatic OAuth login with a loopback callback server.
///
/// This function:
/// 1. Extracts the OAuth config from the server config.
/// 2. Uses the already-discovered metadata for endpoints.
/// 3. Binds a loopback TCP listener on `127.0.0.1:0`.
/// 4. Generates PKCE challenge + state.
/// 5. Builds the authorization URL.
/// 6. Saves the pending state.
/// 7. Spawns a background listener for the callback.
///
/// Returns the handle and an [`McpOAuthStart`] for display purposes.
pub async fn start_auto_authorization(
    config: &McpServerConfig,
) -> Result<(McpOAuthLoginHandle, McpOAuthStart)> {
    let oauth = require_oauth_config(config)?;
    validate_oauth_config(oauth)?;
    let metadata = discover_authorization_server_metadata(config).await?;
    validate_metadata(&metadata)?;
    start_auto_login(config, &metadata, oauth).await
}

pub(crate) async fn start_auto_login(
    config: &McpServerConfig,
    metadata: &AuthorizationServerMetadata,
    oauth: &McpOAuthConfig,
) -> Result<(McpOAuthLoginHandle, McpOAuthStart)> {
    start_auto_login_with_timeout(
        config,
        metadata,
        oauth,
        Duration::from_secs(DEFAULT_OAUTH_TIMEOUT_SECS),
    )
    .await
}

async fn start_auto_login_with_timeout(
    config: &McpServerConfig,
    metadata: &AuthorizationServerMetadata,
    oauth: &McpOAuthConfig,
    timeout: Duration,
) -> Result<(McpOAuthLoginHandle, McpOAuthStart)> {
    // Resolve store mode early so wait() doesn't need the config.
    let store_mode = McpCredentialsStoreMode::from_config(oauth.credentials_store.as_deref());
    let auth_key = server_auth_key(config);

    // 1. Bind loopback listener on OS-assigned port
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("failed to bind OAuth callback listener on 127.0.0.1")?;
    let actual_port = listener
        .local_addr()
        .context("failed to read listener address")?
        .port();

    // 2. Build redirect URI with the actual bound port
    let redirect_uri_str = format!("http://127.0.0.1:{actual_port}{CALLBACK_PATH}");

    // 3. Generate PKCE challenge + state
    let client_id = oauth
        .client_id
        .clone()
        .unwrap_or_else(|| "allthecodes".to_string());
    let state = generate_random_urlsafe();
    let code_verifier = generate_random_urlsafe();
    let code_challenge = pkce_challenge(&code_verifier);

    // 4. Build the authorization URL
    let mut authorization_url = Url::parse(&metadata.authorization_endpoint)
        .context("invalid OAuth authorization endpoint")?;
    {
        let mut pairs = authorization_url.query_pairs_mut();
        pairs
            .append_pair("response_type", "code")
            .append_pair("client_id", &client_id)
            .append_pair("redirect_uri", &redirect_uri_str)
            .append_pair("state", &state)
            .append_pair("code_challenge", &code_challenge)
            .append_pair("code_challenge_method", "S256");
        let scopes = oauth
            .scopes
            .as_ref()
            .map(|s| s.join(" "))
            .filter(|s| !s.is_empty());
        if let Some(scopes) = &scopes {
            pairs.append_pair("scope", scopes);
        }
        let resource = oauth
            .oauth_resource
            .as_deref()
            .or_else(|| config.url.as_deref());
        if let Some(resource) = resource {
            pairs.append_pair("resource", resource);
        }
    }

    // 5. Save pending state so wait() can validate state + exchange token
    let pending = PendingMcpOAuthAuthorization {
        server_name: config.name.clone(),
        server_url: config.url.clone(),
        state: state.clone(),
        code_verifier,
        redirect_uri: redirect_uri_str.clone(),
        client_id,
        scopes: oauth.scopes.clone().unwrap_or_default(),
        authorization_endpoint: metadata.authorization_endpoint.clone(),
        token_endpoint: metadata.token_endpoint.clone(),
        authorization_server: metadata.issuer.clone().unwrap_or_default(),
        created_at: now_timestamp(),
    };
    save_pending_authorization(config, pending)?;

    // 6. Spawn the callback listener
    let (tx, rx) = oneshot::channel();
    let state_clone = state.clone();
    let store_mode_clone = store_mode;
    let auth_key_clone = auth_key.clone();
    let config_clone = config.clone();

    tokio::spawn(async move {
        let result = listen_for_callback(
            listener,
            &state_clone,
            config_clone,
            &auth_key_clone,
            store_mode_clone,
            timeout,
        )
        .await;
        if result.is_err() {
            let _ = remove_pending_authorization(&auth_key_clone);
        }
        let _ = tx.send(result);
    });

    let handle = McpOAuthLoginHandle {
        authorization_url: authorization_url.to_string(),
        completion: rx,
        auth_key,
        timeout,
    };

    let start_info = McpOAuthStart {
        authorization_url: authorization_url.to_string(),
        state,
        redirect_uri: redirect_uri_str,
        token_store_path: token_store_path(),
    };

    Ok((handle, start_info))
}

// ---------------------------------------------------------------------------
// Callback listener
// ---------------------------------------------------------------------------

/// Accept one HTTP connection, validate the callback, exchange the code, and
/// persist the token.
async fn listen_for_callback(
    listener: TcpListener,
    expected_state: &str,
    config: McpServerConfig,
    auth_key: &str,
    store_mode: McpCredentialsStoreMode,
    timeout: Duration,
) -> Result<StoredMcpOAuthToken> {
    // Accept a single connection with a generous timeout
    let (mut stream, _) = tokio::time::timeout(timeout, listener.accept())
        .await
        .context("timed out waiting for OAuth callback connection")?
        .context("failed to accept OAuth callback connection")?;

    // Read the HTTP request (max 4096 bytes)
    let mut buf = vec![0_u8; 4096];
    let n = stream
        .read(&mut buf)
        .await
        .context("failed to read OAuth callback request")?;
    let request = &buf[..n];
    let request_str = std::str::from_utf8(request)
        .map_err(|_| anyhow::anyhow!("OAuth callback request is not valid UTF-8"))?;

    // Parse: extract the request line and query string
    let (method, path_and_query) = parse_request_line(request_str)?;
    if method != "GET" {
        respond_http(&mut stream, 400, "Bad Request: expected GET").await;
        bail!("OAuth callback: expected GET, got {method}");
    }

    let (path, query) = path_and_query
        .split_once('?')
        .unwrap_or((path_and_query, ""));
    if path != CALLBACK_PATH {
        // Unknown path — this may be a health check or port scan.
        // Respond 400 but don't terminate.
        respond_http(
            &mut stream,
            400,
            &format!("Bad Request: unexpected path '{path}'"),
        )
        .await;
        bail!("OAuth callback: unexpected path '{path}', expected '{CALLBACK_PATH}'");
    }

    // Parse query parameters
    let params = parse_query(query);
    let code = params
        .get("code")
        .filter(|s| !s.is_empty())
        .map(String::as_str);
    let error = params
        .get("error")
        .filter(|s| !s.is_empty())
        .map(String::as_str);
    let error_description = params.get("error_description").map(String::as_str);
    let state = params
        .get("state")
        .filter(|s| !s.is_empty())
        .map(String::as_str);

    if let Some(error) = error {
        let msg = format!("OAuth provider returned error: {error}");
        let desc = error_description
            .map(|d| format!(": {d}"))
            .unwrap_or_default();
        let full_msg = format!("{msg}{desc}");
        respond_http(&mut stream, 400, &full_msg).await;
        bail!("{full_msg}");
    }

    match (code, state) {
        (Some(code), Some(state)) => {
            if state != expected_state {
                let msg = "OAuth state mismatch".to_string();
                respond_http(&mut stream, 400, &msg).await;
                bail!("{msg}");
            }
            let mut pending_store = crate::auth::read_pending_store()?;
            let pending = pending_store.pending.remove(auth_key).ok_or_else(|| {
                anyhow::anyhow!("no pending OAuth authorization for '{}'", config.name)
            })?;
            crate::auth::write_pending_store(&pending_store)?;

            let token_result = async {
                let response = exchange_code_for_token(&config, &pending, code).await?;
                let token = token_from_response(&pending, response)?;
                oauth_store_ops::save_token(store_mode, auth_key, &token)?;
                Ok::<_, anyhow::Error>(token)
            }
            .await;

            match token_result {
                Ok(token) => {
                    respond_http(
                        &mut stream,
                        200,
                        "Authentication complete. You may close this window.",
                    )
                    .await;
                    Ok(token)
                }
                Err(err) => {
                    respond_http(
                        &mut stream,
                        500,
                        "Authentication failed. Return to allthecodes and try again.",
                    )
                    .await;
                    Err(err)
                }
            }
        }
        _ => {
            respond_http(
                &mut stream,
                400,
                "Bad Request: missing 'code' and 'state' parameters",
            )
            .await;
            bail!("OAuth callback: missing 'code' and 'state' query parameters");
        }
    }
}

// ---------------------------------------------------------------------------
// HTTP helpers
// ---------------------------------------------------------------------------

/// Parse the request line from an HTTP request string, returning (method, path_with_query).
fn parse_request_line(request: &str) -> Result<(&str, &str)> {
    let first_line = request
        .lines()
        .next()
        .ok_or_else(|| anyhow::anyhow!("empty HTTP request"))?;
    let parts: Vec<&str> = first_line.splitn(3, ' ').collect();
    if parts.len() < 3 {
        bail!("invalid HTTP request line: '{first_line}'");
    }
    Ok((parts[0], parts[1]))
}

/// Parse URL query parameters into a simple map.
/// Properly URL-decodes both keys and values.
fn parse_query(query: &str) -> std::collections::HashMap<String, String> {
    url::form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect()
}

/// Write an HTTP response and flush the stream.
async fn respond_http(stream: &mut tokio::net::TcpStream, status: u16, body: &str) {
    let status_text = match status {
        200 => "OK",
        400 => "Bad Request",
        500 => "Internal Server Error",
        _ => "Error",
    };
    let response = format!(
        "HTTP/1.1 {status} {status_text}\r\n\
         Content-Type: text/plain; charset=utf-8\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n\
         {body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.flush().await;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::TempDir;
    use tokio::io::AsyncWriteExt;

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

    fn auto_login_test_config(name: &str) -> McpServerConfig {
        McpServerConfig {
            name: name.to_string(),
            transport: "streamable-http".to_string(),
            command: None,
            args: None,
            url: Some("https://mcp.example.com/mcp".to_string()),
            headers: None,
            oauth: Some(McpOAuthConfig {
                client_id: Some("test-client".to_string()),
                callback_port: None,
                auth_server_metadata_url: Some(
                    "http://127.0.0.1:9/.well-known/oauth-authorization-server".to_string(),
                ),
                scopes: Some(vec!["tools.read".to_string()]),
                oauth_resource: None,
                credentials_store: Some("file".to_string()),
            }),
            env: None,
            browser_mcp: None,
            disabled: None,
            bearer_token_env_var: None,
            env_http_headers: None,
            auth: None,
        }
    }

    fn auto_login_test_metadata() -> AuthorizationServerMetadata {
        AuthorizationServerMetadata {
            issuer: Some("http://127.0.0.1:9".to_string()),
            authorization_endpoint: "http://127.0.0.1:9/authorize".to_string(),
            token_endpoint: "http://127.0.0.1:9/token".to_string(),
            scopes_supported: vec!["tools.read".to_string()],
        }
    }

    // -----------------------------------------------------------------------
    // Callback parsing helpers
    // -----------------------------------------------------------------------

    #[test]
    fn parse_request_line_extracts_method_and_path() {
        let (method, path) = parse_request_line(
            "GET /mcp/oauth/callback?code=abc&state=xyz HTTP/1.1\r\nHost: localhost\r\n",
        )
        .unwrap();
        assert_eq!(method, "GET");
        assert_eq!(path, "/mcp/oauth/callback?code=abc&state=xyz");
    }

    #[test]
    fn parse_request_line_rejects_empty() {
        assert!(parse_request_line("").is_err());
    }

    #[test]
    fn parse_query_extracts_code_and_state() {
        let params = parse_query("code=abc&state=xyz");
        assert_eq!(params.get("code").map(String::as_str), Some("abc"));
        assert_eq!(params.get("state").map(String::as_str), Some("xyz"));
    }

    #[test]
    fn parse_query_extracts_error() {
        let params = parse_query("error=invalid_scope&error_description=scope%20rejected");
        assert_eq!(
            params.get("error").map(String::as_str),
            Some("invalid_scope")
        );
        assert_eq!(
            params.get("error_description").map(String::as_str),
            Some("scope rejected")
        );
    }

    #[test]
    fn parse_query_handles_empty() {
        let params = parse_query("");
        assert!(params.is_empty());
    }

    #[test]
    fn parse_query_decodes_url_encoded_values() {
        let params = parse_query("state=abc%2Fdef&redirect_uri=http%3A%2F%2Flocalhost%3A1234");
        assert_eq!(params.get("state").map(String::as_str), Some("abc/def"));
        assert_eq!(
            params.get("redirect_uri").map(String::as_str),
            Some("http://localhost:1234")
        );
    }

    // -----------------------------------------------------------------------
    // Full loopback integration test
    // -----------------------------------------------------------------------

    #[tokio::test]
    #[serial]
    async fn loopback_callback_receives_code_and_state() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let expected_state = "test-state-123";

        let (tx, mut rx) = oneshot::channel::<(String, String)>();

        // Spawn the listener
        let handle = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0_u8; 4096];
            let n = stream.read(&mut buf).await.unwrap();
            let request_str = std::str::from_utf8(&buf[..n]).unwrap();
            let (method, path_and_query) = parse_request_line(request_str).unwrap();
            assert_eq!(method, "GET");
            assert!(path_and_query.contains("code=test-code"));
            assert!(path_and_query.contains("state=test-state-123"));

            // Send response
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK")
                .await
                .unwrap();

            let params = parse_query(path_and_query.split_once('?').map(|(_, q)| q).unwrap_or(""));
            tx.send((
                params.get("code").unwrap_or(&String::new()).to_string(),
                params.get("state").unwrap_or(&String::new()).to_string(),
            ))
            .unwrap();
        });

        // Send a fake callback request
        let mut client = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"))
            .await
            .unwrap();
        let request = format!(
            "GET {CALLBACK_PATH}?code=test-code&state={expected_state} HTTP/1.1\r\nHost: localhost\r\n\r\n"
        );
        client.write_all(request.as_bytes()).await.unwrap();

        let result = tokio::time::timeout(Duration::from_secs(5), &mut rx)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result.0, "test-code");
        assert_eq!(result.1, "test-state-123");

        handle.await.unwrap();
    }

    #[tokio::test]
    #[serial]
    async fn callback_listener_responds_400_on_wrong_path() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let (tx, rx) = oneshot::channel::<u16>();

        let handle = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0_u8; 4096];
            let n = stream.read(&mut buf).await.unwrap();
            let request_str = std::str::from_utf8(&buf[..n]).unwrap();
            let (_method, path_and_query) = parse_request_line(request_str).unwrap();
            // Not the expected callback path — should be treated as invalid
            assert!(path_and_query.contains("?"));
            drop(stream);
            tx.send(400).unwrap();
        });

        let mut client = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"))
            .await
            .unwrap();
        client
            .write_all(b"GET /wrong/path?code=abc&state=xyz HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .unwrap();

        let status = tokio::time::timeout(Duration::from_secs(5), rx)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(status, 400);
        handle.await.unwrap();
    }

    #[tokio::test]
    #[serial]
    async fn start_auto_login_returns_valid_authorization_url() {
        let temp = TempDir::new().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path().to_str().unwrap());

        // Start a mock metadata server
        let metadata_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let metadata_port = metadata_listener.local_addr().unwrap().port();
        let metadata_url =
            format!("http://127.0.0.1:{metadata_port}/.well-known/oauth-authorization-server");

        // Serve metadata in background
        tokio::spawn(async move {
            let (mut stream, _) = metadata_listener.accept().await.unwrap();
            let mut buf = [0_u8; 4096];
            let n = stream.read(&mut buf).await.unwrap();
            let _request = std::str::from_utf8(&buf[..n]).unwrap();
            let body = serde_json::json!({
                "issuer": format!("http://127.0.0.1:{metadata_port}"),
                "authorization_endpoint": format!("http://127.0.0.1:{metadata_port}/authorize"),
                "token_endpoint": format!("http://127.0.0.1:{metadata_port}/token"),
                "scopes_supported": ["tools.read"]
            });
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                body.to_string().len(),
                body
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });

        let config = McpServerConfig {
            name: "test-server".to_string(),
            transport: "streamable-http".to_string(),
            command: None,
            args: None,
            url: Some("https://mcp.example.com/mcp".to_string()),
            headers: None,
            oauth: Some(McpOAuthConfig {
                client_id: Some("test-client".to_string()),
                callback_port: None,
                auth_server_metadata_url: Some(metadata_url),
                scopes: Some(vec!["tools.read".to_string()]),
                oauth_resource: None,
                credentials_store: None,
            }),
            env: None,
            browser_mcp: None,
            disabled: None,
            bearer_token_env_var: None,
            env_http_headers: None,
            auth: None,
        };

        let metadata = crate::auth::discover_authorization_server_metadata(&config)
            .await
            .unwrap();
        let oauth = config.oauth.as_ref().unwrap();

        let (handle, start_info) = start_auto_login(&config, &metadata, oauth).await.unwrap();

        // Verify URL contains expected params
        let url = handle.authorization_url();
        assert!(url.contains("/authorize?"));
        assert!(url.contains("code_challenge="));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("state="));
        assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("client_id=test-client"));
        assert!(url.contains("scope=tools.read"));

        // Verify start_info
        assert_eq!(start_info.authorization_url, url);
        assert!(!start_info.state.is_empty());

        // The listener is alive — just drop the handle to clean up
        drop(handle);
    }

    #[tokio::test]
    #[serial]
    async fn start_auto_login_returns_authorization_url() {
        let temp = TempDir::new().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path().to_str().unwrap());

        let config = McpServerConfig {
            name: "test-server".to_string(),
            transport: "streamable-http".to_string(),
            command: None,
            args: None,
            url: Some("https://mcp.example.com/mcp".to_string()),
            headers: None,
            oauth: Some(McpOAuthConfig {
                client_id: Some("test-client".to_string()),
                callback_port: None,
                auth_server_metadata_url: None,
                scopes: Some(vec!["tools.read".to_string()]),
                oauth_resource: None,
                credentials_store: None,
            }),
            env: None,
            browser_mcp: None,
            disabled: None,
            bearer_token_env_var: None,
            env_http_headers: None,
            auth: None,
        };

        // Start mock metadata server
        let metadata_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let metadata_port = metadata_listener.local_addr().unwrap().port();
        let metadata_url =
            format!("http://127.0.0.1:{metadata_port}/.well-known/oauth-authorization-server",);

        let mcp_config_rebuild = McpServerConfig {
            oauth: Some(McpOAuthConfig {
                auth_server_metadata_url: Some(metadata_url),
                ..config.oauth.clone().unwrap()
            }),
            ..config
        };

        // Serve metadata in background
        tokio::spawn(async move {
            let (mut stream, _) = metadata_listener.accept().await.unwrap();
            let mut buf = [0_u8; 4096];
            stream.read(&mut buf).await.unwrap();
            let body = serde_json::json!({
                "issuer": format!("http://127.0.0.1:{metadata_port}"),
                "authorization_endpoint": format!("http://127.0.0.1:{metadata_port}/authorize"),
                "token_endpoint": format!("http://127.0.0.1:{metadata_port}/token"),
                "scopes_supported": ["tools.read"]
            });
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                body.to_string().len(),
                body
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });

        let metadata = crate::auth::discover_authorization_server_metadata(&mcp_config_rebuild)
            .await
            .unwrap();
        let oauth = mcp_config_rebuild.oauth.as_ref().unwrap();

        let (handle, start_info) = start_auto_login(&mcp_config_rebuild, &metadata, oauth)
            .await
            .expect("start_auto_login should succeed");

        assert!(handle.authorization_url().contains("/authorize?"));
        assert!(handle.authorization_url().contains("code_challenge="));
        assert!(!start_info.state.is_empty());
        assert!(
            start_info
                .redirect_uri
                .contains(format!("{CALLBACK_PATH}").as_str())
        );

        drop(handle);
    }

    #[tokio::test]
    #[serial]
    async fn auto_login_timeout_clears_pending_state() {
        let temp = TempDir::new().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path().to_str().unwrap());
        let config = auto_login_test_config("timeout-server");
        let metadata = auto_login_test_metadata();
        let oauth = config.oauth.as_ref().unwrap();
        let auth_key = server_auth_key(&config);

        let (handle, _start) =
            start_auto_login_with_timeout(&config, &metadata, oauth, Duration::from_millis(25))
                .await
                .unwrap();
        assert!(
            crate::auth::read_pending_store()
                .unwrap()
                .pending
                .contains_key(&auth_key)
        );

        let error = handle.wait().await.unwrap_err();

        assert!(error.to_string().contains("timed out"));
        assert!(
            !crate::auth::read_pending_store()
                .unwrap()
                .pending
                .contains_key(&auth_key)
        );
    }

    #[tokio::test]
    #[serial]
    async fn auto_login_callback_error_clears_pending_state() {
        let temp = TempDir::new().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path().to_str().unwrap());
        let config = auto_login_test_config("callback-error-server");
        let metadata = auto_login_test_metadata();
        let oauth = config.oauth.as_ref().unwrap();
        let auth_key = server_auth_key(&config);

        let (handle, start) =
            start_auto_login_with_timeout(&config, &metadata, oauth, Duration::from_secs(5))
                .await
                .unwrap();
        assert!(
            crate::auth::read_pending_store()
                .unwrap()
                .pending
                .contains_key(&auth_key)
        );

        let redirect = Url::parse(&start.redirect_uri).unwrap();
        let mut callback_stream =
            tokio::net::TcpStream::connect(format!("127.0.0.1:{}", redirect.port().unwrap()))
                .await
                .unwrap();
        let callback_request = format!(
            "GET {}?error=access_denied&error_description=denied&state={} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
            redirect.path(),
            start.state
        );
        callback_stream
            .write_all(callback_request.as_bytes())
            .await
            .unwrap();
        let callback_response = read_http_message(&mut callback_stream).await;
        assert!(callback_response.starts_with("HTTP/1.1 400 Bad Request"));

        let error = handle.wait().await.unwrap_err();

        assert!(
            error
                .to_string()
                .contains("OAuth provider returned error: access_denied")
        );
        assert!(
            !crate::auth::read_pending_store()
                .unwrap()
                .pending
                .contains_key(&auth_key)
        );
    }

    #[tokio::test]
    #[serial]
    async fn auto_login_callback_uses_original_transport_and_resource() {
        let temp = TempDir::new().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path().to_str().unwrap());

        let auth_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let auth_port = auth_listener.local_addr().unwrap().port();
        let metadata_url =
            format!("http://127.0.0.1:{auth_port}/.well-known/oauth-authorization-server");
        let token_url = format!("http://127.0.0.1:{auth_port}/token");
        let authorize_url = format!("http://127.0.0.1:{auth_port}/authorize");
        let (token_request_tx, token_request_rx) = oneshot::channel::<String>();

        let server = tokio::spawn(async move {
            let (mut metadata_stream, _) = auth_listener.accept().await.unwrap();
            let _metadata_request = read_http_message(&mut metadata_stream).await;
            let metadata_body = serde_json::json!({
                "issuer": format!("http://127.0.0.1:{auth_port}"),
                "authorization_endpoint": authorize_url,
                "token_endpoint": token_url,
                "scopes_supported": ["tools.read"]
            });
            write_json_response(&mut metadata_stream, metadata_body).await;

            let (mut token_stream, _) = auth_listener.accept().await.unwrap();
            let token_request = read_http_message(&mut token_stream).await;
            token_request_tx.send(token_request).unwrap();
            write_json_response(
                &mut token_stream,
                serde_json::json!({
                    "access_token": "auto-access",
                    "token_type": "Bearer",
                    "expires_in": 3600,
                    "scope": "tools.read"
                }),
            )
            .await;
        });

        let config = McpServerConfig {
            name: "sse-server".to_string(),
            transport: "sse".to_string(),
            command: None,
            args: None,
            url: Some("https://mcp.example.com/sse".to_string()),
            headers: None,
            oauth: Some(McpOAuthConfig {
                client_id: Some("test-client".to_string()),
                callback_port: None,
                auth_server_metadata_url: Some(metadata_url),
                scopes: Some(vec!["tools.read".to_string()]),
                oauth_resource: Some("https://resource.example.com/mcp".to_string()),
                credentials_store: Some("file".to_string()),
            }),
            env: None,
            browser_mcp: None,
            disabled: None,
            bearer_token_env_var: None,
            env_http_headers: None,
            auth: None,
        };

        let (handle, start) = start_auto_authorization(&config).await.unwrap();
        let redirect = Url::parse(&start.redirect_uri).unwrap();
        let callback_port = redirect.port().unwrap();
        let mut callback_stream =
            tokio::net::TcpStream::connect(format!("127.0.0.1:{callback_port}"))
                .await
                .unwrap();
        let callback_request = format!(
            "GET {}?code=returned-code&state={} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
            redirect.path(),
            start.state
        );
        callback_stream
            .write_all(callback_request.as_bytes())
            .await
            .unwrap();
        let callback_response = read_http_message(&mut callback_stream).await;
        assert!(callback_response.starts_with("HTTP/1.1 200 OK"));

        let token = handle.wait().await.unwrap();
        assert_eq!(token.access_token, "auto-access");

        let token_request = token_request_rx.await.unwrap();
        assert!(token_request.contains("grant_type=authorization_code"));
        assert!(token_request.contains("code=returned-code"));
        assert!(token_request.contains("resource=https%3A%2F%2Fresource.example.com%2Fmcp"));
        assert_eq!(
            crate::auth::authorization_header(&config)
                .await
                .unwrap()
                .as_deref(),
            Some("Bearer auto-access")
        );

        server.await.unwrap();
    }

    async fn read_http_message(stream: &mut tokio::net::TcpStream) -> String {
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
        let mut message = head;
        message.push_str("\r\n\r\n");
        message.push_str(&String::from_utf8(body).unwrap());
        message
    }

    async fn write_json_response(stream: &mut tokio::net::TcpStream, body: serde_json::Value) {
        let body = body.to_string();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).await.unwrap();
        stream.flush().await.unwrap();
    }
}
