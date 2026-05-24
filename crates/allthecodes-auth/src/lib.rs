//! Authentication system
//!
//! Supports three active auth methods:
//! - API Key: via `ANTHROPIC_API_KEY` env var or system keychain
//! - External Auth Token: via `ANTHROPIC_AUTH_TOKEN` env var
//! - OAuth Token: from allthecodes' isolated credentials path (Claude.ai / Console / OpenAI Codex)

pub mod api_key;
pub mod codex_cli;
pub mod oauth;
pub mod token;

const OPENAI_CODEX_AUTH_TOKEN_ENV: &str = "OPENAI_CODEX_AUTH_TOKEN";

// ---------------------------------------------------------------------------
// Credentials path
// ---------------------------------------------------------------------------

/// Return allthecodes' isolated OAuth credentials path.
///
/// `cc-auth` depends on `cc-config` for path ownership, so binary startup no
/// longer installs this path as an adapter.
pub(crate) fn credentials_path() -> std::path::PathBuf {
    allthecodes_config::paths::credentials_path()
}

// ---------------------------------------------------------------------------
// Auth method enum
// ---------------------------------------------------------------------------

/// Authentication method resolved at startup.
#[derive(Debug, Clone)]
pub enum AuthMethod {
    /// Direct API key (`ANTHROPIC_API_KEY`)
    ApiKey(String),
    /// External auth token (`ANTHROPIC_AUTH_TOKEN`)
    ExternalToken(String),
    /// OAuth access token (Claude.ai or Console)
    OAuthToken {
        access_token: String,
        /// "claude_ai", "console", or "openai_codex"
        method: String,
    },
    /// No authentication configured
    None,
}

impl AuthMethod {
    pub fn is_authenticated(&self) -> bool {
        !matches!(self, Self::None)
    }

    pub fn api_key(&self) -> Option<&str> {
        match self {
            Self::ApiKey(key) => Some(key),
            _ => None,
        }
    }

    pub fn bearer_token(&self) -> Option<&str> {
        match self {
            Self::ExternalToken(token) => Some(token),
            Self::OAuthToken { access_token, .. } => Some(access_token),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Resolve auth from environment
// ---------------------------------------------------------------------------

/// Resolve authentication from environment variables, OAuth tokens, and keychain.
///
/// Priority:
/// 1. `ANTHROPIC_API_KEY` env var
/// 2. `ANTHROPIC_AUTH_TOKEN` env var
/// 3. OAuth token from `~/.allthecodes/credentials.json` (if not expired)
/// 4. API key from system keychain
/// 5. `AuthMethod::None`
pub fn resolve_auth() -> AuthMethod {
    match try_resolve_auth() {
        Ok(auth) => auth,
        Err(error) => {
            tracing::warn!(%error, "Auth resolution failed");
            AuthMethod::None
        }
    }
}

/// Resolve authentication, preserving diagnostics for present-but-invalid
/// credentials and runtime/infrastructure failures.
pub fn try_resolve_auth() -> anyhow::Result<AuthMethod> {
    // 1. ANTHROPIC_API_KEY
    if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
        if !key.is_empty() {
            if api_key::validate_api_key(&key) {
                return Ok(AuthMethod::ApiKey(key));
            }
            anyhow::bail!("ANTHROPIC_API_KEY is present but has an invalid API key format");
        }
    }

    // 2. ANTHROPIC_AUTH_TOKEN
    if let Ok(token) = std::env::var("ANTHROPIC_AUTH_TOKEN") {
        if !token.is_empty() {
            return Ok(AuthMethod::ExternalToken(token));
        }
    }

    // 3. OAuth token from disk (with auto-refresh if expired)
    if let Some((access_token, method)) = try_resolve_oauth()? {
        if method == "console" || method == "openai_codex" {
            // Console mode: API key is in keychain (created at login).
            // OpenAI Codex mode: handled by resolve_codex_auth_token().
            // Fall through to keychain check below.
        } else {
            return Ok(AuthMethod::OAuthToken {
                access_token,
                method,
            });
        }
    }

    // 4. Keychain
    if let Some(key) = api_key::load_api_key()? {
        if !api_key::validate_api_key(&key) {
            anyhow::bail!("system keychain contains an invalid API key format");
        }
        return Ok(AuthMethod::ApiKey(key));
    }

    Ok(AuthMethod::None)
}

/// Resolve OpenAI Codex auth token.
///
/// Priority:
/// 1. `OPENAI_CODEX_AUTH_TOKEN` environment variable
/// 2. OAuth token from `~/.allthecodes/credentials.json` when method is `openai_codex`
/// 3. Codex CLI credentials from `~/.codex/auth.json` (fallback)
pub fn resolve_codex_auth_token() -> Option<String> {
    match try_resolve_codex_auth_token() {
        Ok(token) => token,
        Err(error) => {
            tracing::warn!(%error, "OpenAI Codex auth resolution failed");
            None
        }
    }
}

/// Resolve OpenAI Codex auth token, preserving diagnostics for invalid stored
/// credentials and refresh infrastructure failures.
pub fn try_resolve_codex_auth_token() -> anyhow::Result<Option<String>> {
    // 1. Environment variable
    if let Ok(token) = std::env::var(OPENAI_CODEX_AUTH_TOKEN_ENV) {
        let trimmed = token.trim();
        if !trimmed.is_empty() {
            return Ok(Some(trimmed.to_string()));
        }
    }

    // 2. allthecodes credentials.json
    if let Some(token) = try_resolve_codex_from_credentials()? {
        return Ok(Some(token));
    }

    // 3. Codex CLI fallback (~/.codex/auth.json)
    try_resolve_codex_cli()
}

/// Resolve an OpenAI Platform API key from allthecodes' provider-scoped keychain.
///
/// `OPENAI_API_KEY` is intentionally not read here; environment variables are
/// handled by `cc-api` before provider-scoped keychain fallback runs.
pub fn try_resolve_openai_api_key() -> anyhow::Result<Option<String>> {
    let Some(key) = api_key::load_openai_api_key()? else {
        return Ok(None);
    };
    if !api_key::validate_openai_api_key(&key) {
        anyhow::bail!("system keychain contains an invalid OpenAI API key format");
    }
    Ok(Some(key.trim().to_string()))
}

/// Try to resolve Codex token from allthecodes' own `credentials.json`.
fn try_resolve_codex_from_credentials() -> anyhow::Result<Option<String>> {
    let stored = match token::load_token()? {
        Some(stored) => stored,
        None => return Ok(None),
    };
    let method = stored.oauth_method.clone().unwrap_or_default();
    if !method.eq_ignore_ascii_case("openai_codex") {
        return Ok(None);
    }

    if !token::is_token_expired(&stored) {
        return Ok(Some(stored.access_token));
    }

    let refresh_tok = match &stored.refresh_token {
        Some(t) if !t.trim().is_empty() => t.clone(),
        _ => anyhow::bail!("OpenAI Codex credentials are expired and have no refresh token"),
    };

    let scopes: Vec<String> = stored.scopes.clone();
    match try_refresh_sync(&refresh_tok, &scopes, &stored) {
        Ok(Some((access_token, refreshed_method))) if refreshed_method == "openai_codex" => {
            Ok(Some(access_token))
        }
        Ok(Some(_)) => {
            anyhow::bail!("OpenAI Codex refresh returned credentials for another OAuth method")
        }
        Ok(None) => Ok(None),
        Err(e) => {
            if is_revoked_or_invalid_grant(&e) {
                let _ = token::remove_token();
            }
            Err(e.context("OpenAI Codex OAuth auto-refresh failed"))
        }
    }
}

/// Try to resolve Codex token from Codex CLI's `~/.codex/auth.json`.
///
/// If the token is expired, attempt refresh using the Codex CLI client_id
/// and save the refreshed token to allthecodes' `credentials.json`.
fn try_resolve_codex_cli() -> anyhow::Result<Option<String>> {
    let cred = match codex_cli::read_codex_cli_credential()? {
        Some(cred) => cred,
        None => return Ok(None),
    };

    if !codex_cli::is_credential_expired(&cred) {
        return Ok(Some(cred.access_token));
    }

    // Token expired; try to refresh.
    let refresh_tok = match &cred.refresh_token {
        Some(t) if !t.trim().is_empty() => t.clone(),
        _ => anyhow::bail!("Codex CLI credentials are expired and have no refresh token"),
    };

    let handle = match tokio::runtime::Handle::try_current() {
        Ok(h) => h,
        Err(e) => anyhow::bail!("Codex CLI token refresh requires a Tokio runtime: {e}"),
    };

    let client_id = cred.client_id.clone();
    let token_url = oauth::config::token_url_for(oauth::config::OAuthMethod::OpenAiCodex);
    let scopes_owned: Vec<String> =
        oauth::config::resolved_scopes_for(oauth::config::OAuthMethod::OpenAiCodex);
    let refresh_tok_for_fallback = refresh_tok.clone();

    let result = std::thread::spawn(move || {
        handle.block_on(async {
            let scope_strs: Vec<&str> = scopes_owned.iter().map(|s| s.as_str()).collect();
            oauth::client::refresh_token_with_client_id(
                &client_id,
                &token_url,
                &refresh_tok,
                &scope_strs,
            )
            .await
        })
    })
    .join()
    .map_err(|_| anyhow::anyhow!("Codex CLI token refresh thread panicked"))??;

    // Save refreshed token to allthecodes' credentials.json
    let expires_at = chrono::Utc::now().timestamp() + result.expires_in as i64;
    let new_scopes: Vec<String> = result
        .scope
        .split_whitespace()
        .map(|s| s.to_string())
        .collect();
    let stored = token::StoredToken {
        access_token: result.access_token.clone(),
        refresh_token: result.refresh_token.or(Some(refresh_tok_for_fallback)),
        expires_at: Some(expires_at),
        token_type: "bearer".into(),
        scopes: if new_scopes.is_empty() {
            oauth::config::OPENAI_CODEX_SCOPES
                .iter()
                .map(|s| s.to_string())
                .collect()
        } else {
            new_scopes
        },
        oauth_method: Some("openai_codex".to_string()),
    };
    let _ = token::save_token(&stored);
    tracing::info!("Codex CLI token refreshed and saved to allthecodes credentials");
    Ok(Some(result.access_token))
}

// ---------------------------------------------------------------------------
// OAuth helpers
// ---------------------------------------------------------------------------

/// Try to load OAuth token, auto-refreshing if expired.
///
/// Returns `Ok(Some((access_token, method)))` or `Ok(None)`.
fn try_resolve_oauth() -> anyhow::Result<Option<(String, String)>> {
    let stored = match token::load_token()? {
        Some(t) => t,
        None => return Ok(None),
    };

    let method = stored.oauth_method.clone().unwrap_or_default();

    if !token::is_token_expired(&stored) {
        return Ok(Some((stored.access_token, method)));
    }

    // Token expired; try synchronous refresh via a blocking runtime.
    // If we're already inside a tokio runtime, spawn a blocking task;
    // otherwise create a temporary one.
    let refresh_tok = match &stored.refresh_token {
        Some(t) => t.clone(),
        None => anyhow::bail!("OAuth credentials are expired and have no refresh token"),
    };

    let scopes: Vec<String> = stored.scopes.clone();
    match try_refresh_sync(&refresh_tok, &scopes, &stored) {
        Ok(result) => Ok(result),
        Err(e) => {
            if is_revoked_or_invalid_grant(&e) {
                let _ = token::remove_token();
            }
            Err(e.context("OAuth auto-refresh failed"))
        }
    }
}

fn is_revoked_or_invalid_grant(error: &anyhow::Error) -> bool {
    let text = format!("{error:#}").to_ascii_lowercase();
    text.contains("invalid_grant") || text.contains("revoked")
}

/// Synchronous wrapper for token refresh (called from `resolve_auth()`).
fn try_refresh_sync(
    refresh_tok: &str,
    scopes: &[String],
    stored: &token::StoredToken,
) -> anyhow::Result<Option<(String, String)>> {
    let oauth_method = stored
        .oauth_method
        .as_deref()
        .and_then(oauth::config::method_from_storage_name)
        .unwrap_or(oauth::config::OAuthMethod::ClaudeAi);

    let scope_strs: Vec<&str> = scopes.iter().map(|s| s.as_str()).collect();
    let scopes_ref: &[&str] = if scope_strs.is_empty() {
        oauth::config::scopes_for(oauth_method)
    } else {
        &scope_strs
    };

    // Use tokio Handle if available, otherwise skip refresh
    let handle = match tokio::runtime::Handle::try_current() {
        Ok(h) => h,
        Err(e) => anyhow::bail!("OAuth token refresh requires a Tokio runtime: {e}"),
    };

    let refresh_tok = refresh_tok.to_string();
    let scopes_owned: Vec<String> = scopes_ref.iter().map(|s| s.to_string()).collect();
    let stored_clone = stored.clone();

    let result = std::thread::spawn(move || {
        handle.block_on(async {
            let scope_strs: Vec<&str> = scopes_owned.iter().map(|s| s.as_str()).collect();
            match oauth::client::refresh_token(oauth_method, &refresh_tok, &scope_strs).await {
                Ok(resp) => {
                    let expires_at = chrono::Utc::now().timestamp() + resp.expires_in as i64;
                    let new_scopes: Vec<String> = resp
                        .scope
                        .split_whitespace()
                        .map(|s| s.to_string())
                        .collect();

                    let updated = token::StoredToken {
                        access_token: resp.access_token.clone(),
                        refresh_token: resp.refresh_token.or(Some(refresh_tok)),
                        expires_at: Some(expires_at),
                        token_type: "bearer".into(),
                        scopes: if new_scopes.is_empty() {
                            stored_clone.scopes
                        } else {
                            new_scopes
                        },
                        oauth_method: stored_clone.oauth_method,
                    };
                    let method = updated.oauth_method.clone().unwrap_or_default();
                    let _ = token::save_token(&updated);
                    Ok(Some((resp.access_token, method)))
                }
                Err(e) => Err(e),
            }
        })
    })
    .join()
    .map_err(|_| anyhow::anyhow!("OAuth refresh thread panicked"))??;

    Ok(result)
}

/// Clear all OAuth state (tokens + keychain API key).
pub fn oauth_logout() -> anyhow::Result<()> {
    token::remove_token()?;
    let _ = api_key::remove_api_key();
    let _ = api_key::remove_openai_api_key();
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn test_auth_method_api_key() {
        let auth = AuthMethod::ApiKey("sk-ant-test-key-123456789".into());
        assert!(auth.is_authenticated());
        assert_eq!(auth.api_key(), Some("sk-ant-test-key-123456789"));
        assert_eq!(auth.bearer_token(), None);
    }

    #[test]
    fn test_auth_method_external_token() {
        let auth = AuthMethod::ExternalToken("ext-token-abc".into());
        assert!(auth.is_authenticated());
        assert_eq!(auth.api_key(), None);
        assert_eq!(auth.bearer_token(), Some("ext-token-abc"));
    }

    #[test]
    fn test_auth_method_none() {
        let auth = AuthMethod::None;
        assert!(!auth.is_authenticated());
        assert_eq!(auth.api_key(), None);
        assert_eq!(auth.bearer_token(), None);
    }

    #[test]
    fn test_auth_method_oauth_token() {
        let auth = AuthMethod::OAuthToken {
            access_token: "oauth-test-token".into(),
            method: "claude_ai".into(),
        };
        assert!(auth.is_authenticated());
        assert_eq!(auth.api_key(), None);
        assert_eq!(auth.bearer_token(), Some("oauth-test-token"));
    }

    #[test]
    fn corrupt_credentials_file_is_diagnostic_not_missing() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("credentials.json");
        std::fs::write(&path, "{not-json").unwrap();
        let _allthecodes_home = EnvGuard::set("ALLTHECODES_HOME", dir.path().to_str().unwrap());

        let err = try_resolve_oauth().expect_err("corrupt existing credentials must be diagnostic");

        assert!(
            err.to_string().contains("expected")
                || err.to_string().contains("key")
                || err.to_string().contains("JSON"),
            "unexpected error: {err:#}"
        );
        assert!(
            path.exists(),
            "diagnostic read failure must not clear credentials"
        );
    }

    #[test]
    fn expired_oauth_refresh_without_runtime_is_diagnostic_and_preserves_credentials() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("credentials.json");
        let _allthecodes_home = EnvGuard::set("ALLTHECODES_HOME", dir.path().to_str().unwrap());
        token::save_token(&expired_token(Some("refresh-token"), "claude_ai")).unwrap();

        let err =
            try_resolve_oauth().expect_err("refresh infrastructure failure must not be Ok(None)");

        let diagnostic = format!("{err:#}");
        assert!(
            diagnostic.contains("Tokio"),
            "unexpected error: {diagnostic}"
        );
        assert!(
            path.exists(),
            "infrastructure failure must not clear credentials"
        );
    }

    #[test]
    fn expired_oauth_without_refresh_token_is_diagnostic_and_preserves_credentials() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("credentials.json");
        let _allthecodes_home = EnvGuard::set("ALLTHECODES_HOME", dir.path().to_str().unwrap());
        token::save_token(&expired_token(None, "claude_ai")).unwrap();

        let err = try_resolve_oauth().expect_err("expired present credentials are invalid");

        assert!(
            err.to_string().contains("no refresh token"),
            "unexpected error: {err:#}"
        );
        assert!(
            path.exists(),
            "invalid present credentials are diagnostic, not auto-cleared"
        );
    }

    #[test]
    fn expired_codex_credentials_refresh_without_runtime_is_diagnostic_and_preserves_credentials() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("credentials.json");
        let _allthecodes_home = EnvGuard::set("ALLTHECODES_HOME", dir.path().to_str().unwrap());
        token::save_token(&expired_token(Some("refresh-token"), "openai_codex")).unwrap();

        let err = try_resolve_codex_from_credentials()
            .expect_err("Codex refresh infrastructure failure must not be Ok(None)");

        assert!(
            err.to_string().contains("OAuth auto-refresh failed"),
            "unexpected error: {err:#}"
        );
        assert!(
            path.exists(),
            "Codex refresh infrastructure failure must not clear credentials"
        );
    }

    #[test]
    fn invalid_present_env_api_key_is_diagnostic() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _api_key = EnvGuard::set("ANTHROPIC_API_KEY", "not-a-valid-key");
        let _auth_token = EnvGuard::remove("ANTHROPIC_AUTH_TOKEN");
        let dir = tempfile::TempDir::new().unwrap();
        let _allthecodes_home = EnvGuard::set("ALLTHECODES_HOME", dir.path().to_str().unwrap());

        let err = try_resolve_auth().expect_err("invalid present env API key must be diagnostic");

        assert!(
            err.to_string().contains("ANTHROPIC_API_KEY"),
            "unexpected error: {err:#}"
        );
    }

    #[test]
    fn credentials_path_is_owned_by_allthecodes_config_path_isolation() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::TempDir::new().unwrap();
        let _allthecodes_home = EnvGuard::set("ALLTHECODES_HOME", dir.path().to_str().unwrap());
        let stored = token::StoredToken {
            access_token: "isolated-access".into(),
            refresh_token: None,
            expires_at: None,
            token_type: "bearer".into(),
            scopes: vec![],
            oauth_method: Some("claude_ai".into()),
        };

        assert_eq!(credentials_path(), dir.path().join("credentials.json"));
        assert_eq!(
            token::token_file_path(),
            dir.path().join("credentials.json")
        );

        token::save_token(&stored).unwrap();
        let loaded = token::load_token().unwrap().expect("stored token");
        assert_eq!(loaded.access_token, "isolated-access");
        assert!(dir.path().join("credentials.json").is_file());
        assert!(!dir.path().join(".Codex").exists());
        assert!(!dir.path().join(".codex").exists());
    }

    fn expired_token(refresh_token: Option<&str>, method: &str) -> token::StoredToken {
        token::StoredToken {
            access_token: "expired-access".into(),
            refresh_token: refresh_token.map(str::to_string),
            expires_at: Some(chrono::Utc::now().timestamp() - 60),
            token_type: "bearer".into(),
            scopes: vec!["user:profile".into()],
            oauth_method: Some(method.to_string()),
        }
    }

    struct EnvGuard {
        key: &'static str,
        prev: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let prev = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, prev }
        }

        fn remove(key: &'static str) -> Self {
            let prev = std::env::var(key).ok();
            std::env::remove_var(key);
            Self { key, prev }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.prev {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }
}
