# OAuth Login Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement OAuth 2.0 PKCE login for cc-rust supporting both Claude.ai (Bearer token) and Console (API Key creation) modes.

**Architecture:** New `src/auth/oauth/` submodule with 4 files (mod, config, pkce, client). Manual mode only — print authorization URL, user pastes code. Extends existing `resolve_auth()` priority chain and `/login` command with method selection menu.

**Tech Stack:** `sha2` (PKCE), `rand` (random bytes), `base64` (encoding), `reqwest` (HTTP), `serde_json` (de/serialization), `chrono` (expiry), `keyring` (API key storage)

**Spec:** [`../specs/2026-04-11-oauth-login-design.md`](../specs/2026-04-11-oauth-login-design.md)

---

### Task 1: Add `rand` dependency and create `pkce.rs` with tests

**Files:**
- Modify: `Cargo.toml:88-94` (add rand dependency in encoding section)
- Create: `src/auth/oauth/pkce.rs`

- [ ] **Step 1: Add `rand` to Cargo.toml**

In `Cargo.toml`, add `rand` after the `hex` line (line 89):

```toml
sha2 = "0.10"
hex = "0.4"
rand = "0.8"
```

- [ ] **Step 2: Create `src/auth/oauth/pkce.rs` with tests first**

```rust
//! PKCE (Proof Key for Code Exchange) utilities for OAuth 2.0.

use base64::{engine::general_purpose::STANDARD, Engine};
use rand::RngCore;
use sha2::{Digest, Sha256};

/// Base64url-encode bytes (RFC 4648 §5, no padding).
fn base64url_encode(bytes: &[u8]) -> String {
    STANDARD
        .encode(bytes)
        .replace('+', "-")
        .replace('/', "_")
        .trim_end_matches('=')
        .to_string()
}

/// Generate a cryptographically random code verifier (43 chars, base64url).
pub fn generate_code_verifier() -> String {
    let mut buf = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut buf);
    base64url_encode(&buf)
}

/// Generate a code challenge from a verifier (SHA-256 → base64url).
pub fn generate_code_challenge(verifier: &str) -> String {
    let hash = Sha256::digest(verifier.as_bytes());
    base64url_encode(&hash)
}

/// Generate a random state parameter for CSRF protection.
pub fn generate_state() -> String {
    let mut buf = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut buf);
    base64url_encode(&buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base64url_no_special_chars() {
        let bytes = [0xFF, 0xFE, 0xFD, 0xFC, 0xFB, 0xFA];
        let encoded = base64url_encode(&bytes);
        assert!(!encoded.contains('+'), "must not contain +");
        assert!(!encoded.contains('/'), "must not contain /");
        assert!(!encoded.contains('='), "must not contain =");
    }

    #[test]
    fn test_verifier_length() {
        let verifier = generate_code_verifier();
        assert_eq!(verifier.len(), 43, "32 bytes → 43 base64url chars");
    }

    #[test]
    fn test_challenge_is_sha256_of_verifier() {
        let verifier = generate_code_verifier();
        let challenge = generate_code_challenge(&verifier);
        // Independently compute expected
        let hash = Sha256::digest(verifier.as_bytes());
        let expected = base64url_encode(&hash);
        assert_eq!(challenge, expected);
    }

    #[test]
    fn test_state_not_empty_and_unique() {
        let s1 = generate_state();
        let s2 = generate_state();
        assert!(!s1.is_empty());
        assert_ne!(s1, s2, "consecutive states must differ");
    }
}
```

- [ ] **Step 3: Create `src/auth/oauth/mod.rs` minimal stub to compile**

```rust
//! OAuth 2.0 Authorization Code + PKCE login flow.

pub mod pkce;
```

- [ ] **Step 4: Register `oauth` submodule in `src/auth/mod.rs`**

Add after `pub mod token;` (line 10):

```rust
pub mod oauth;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p cc-rust auth::oauth::pkce -- --nocapture`
Expected: 4 tests PASS

- [ ] **Step 6: Compile and check for warnings**

Run: `cargo build 2>&1 | grep warning`
Expected: no new warnings from `pkce.rs`

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml src/auth/oauth/pkce.rs src/auth/oauth/mod.rs src/auth/mod.rs
git commit -m "feat(auth): add PKCE utilities for OAuth login"
```

---

### Task 2: Create `config.rs` — OAuth constants and URL builder

**Files:**
- Create: `src/auth/oauth/config.rs`
- Modify: `src/auth/oauth/mod.rs` (add `pub mod config;`)

- [ ] **Step 1: Write tests for URL construction in `config.rs`**

```rust
//! OAuth endpoint configuration and authorization URL construction.

/// OAuth login method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OAuthMethod {
    /// Claude.ai subscriber (Bearer token mode)
    ClaudeAi,
    /// Console user (API Key creation mode)
    Console,
}

pub const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
pub const AUTH_URL: &str = "https://platform.claude.com/oauth/authorize";
pub const TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";
pub const CREATE_API_KEY_URL: &str =
    "https://api.anthropic.com/api/oauth/claude_cli/create_api_key";
pub const MANUAL_REDIRECT_URL: &str = "https://platform.claude.com/oauth/code/callback";

pub const CLAUDE_AI_SCOPES: &[&str] = &[
    "user:profile",
    "user:inference",
    "user:sessions:claude_code",
    "user:mcp_servers",
    "user:file_upload",
];

pub const CONSOLE_SCOPES: &[&str] = &["org:create_api_key", "user:profile"];

/// Return the scopes for the given login method.
pub fn scopes_for(method: OAuthMethod) -> &'static [&'static str] {
    match method {
        OAuthMethod::ClaudeAi => CLAUDE_AI_SCOPES,
        OAuthMethod::Console => CONSOLE_SCOPES,
    }
}

/// Build the full authorization URL with PKCE parameters.
pub fn authorization_url(
    method: OAuthMethod,
    code_challenge: &str,
    state: &str,
) -> String {
    let scopes = scopes_for(method).join(" ");
    format!(
        "{}?code=true&client_id={}&response_type=code&redirect_uri={}&scope={}&code_challenge={}&code_challenge_method=S256&state={}",
        AUTH_URL,
        CLIENT_ID,
        urlencoding::encode(MANUAL_REDIRECT_URL),
        urlencoding::encode(&scopes),
        code_challenge,
        state,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_claude_ai_scopes() {
        let scopes = scopes_for(OAuthMethod::ClaudeAi);
        assert!(scopes.contains(&"user:inference"));
        assert!(!scopes.contains(&"org:create_api_key"));
    }

    #[test]
    fn test_console_scopes() {
        let scopes = scopes_for(OAuthMethod::Console);
        assert!(scopes.contains(&"org:create_api_key"));
        assert!(!scopes.contains(&"user:inference"));
    }

    #[test]
    fn test_authorization_url_contains_required_params() {
        let url = authorization_url(OAuthMethod::ClaudeAi, "test_challenge", "test_state");
        assert!(url.starts_with(AUTH_URL));
        assert!(url.contains("client_id=9d1c250a"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("code_challenge=test_challenge"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("state=test_state"));
        assert!(url.contains("redirect_uri="));
    }
}
```

- [ ] **Step 2: Add `urlencoding` to Cargo.toml**

In the encoding section (after `url = "2"`, line 94):

```toml
url = "2"
urlencoding = "2"
```

- [ ] **Step 3: Register module in `src/auth/oauth/mod.rs`**

```rust
//! OAuth 2.0 Authorization Code + PKCE login flow.

pub mod config;
pub mod pkce;
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p cc-rust auth::oauth::config -- --nocapture`
Expected: 3 tests PASS

- [ ] **Step 5: Check for warnings**

Run: `cargo build 2>&1 | grep warning`
Expected: no new warnings

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml src/auth/oauth/config.rs src/auth/oauth/mod.rs
git commit -m "feat(auth): add OAuth config constants and URL builder"
```

---

### Task 3: Create `client.rs` — token exchange and API key creation

**Files:**
- Create: `src/auth/oauth/client.rs`
- Modify: `src/auth/oauth/mod.rs` (add `pub mod client;`)

- [ ] **Step 1: Create `client.rs` with request types and functions**

```rust
//! OAuth HTTP client — token exchange, refresh, and API key creation.

use anyhow::{Context, Result};
use serde::Deserialize;

use super::config;

/// Response from the token endpoint.
#[derive(Debug, Deserialize)]
pub struct OAuthTokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: u64,
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub account: Option<OAuthAccount>,
}

/// Account info embedded in token response.
#[derive(Debug, Deserialize)]
pub struct OAuthAccount {
    pub uuid: Option<String>,
    pub email_address: Option<String>,
}

/// Response from the create_api_key endpoint.
#[derive(Debug, Deserialize)]
struct CreateApiKeyResponse {
    raw_key: String,
}

/// Exchange an authorization code for tokens.
pub async fn exchange_code(
    code: &str,
    code_verifier: &str,
    state: &str,
    redirect_uri: &str,
) -> Result<OAuthTokenResponse> {
    let body = serde_json::json!({
        "grant_type": "authorization_code",
        "code": code,
        "redirect_uri": redirect_uri,
        "client_id": config::CLIENT_ID,
        "code_verifier": code_verifier,
        "state": state,
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(config::TOKEN_URL)
        .json(&body)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .context("Failed to reach token endpoint")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("Token exchange failed ({}): {}", status, text);
    }

    resp.json::<OAuthTokenResponse>()
        .await
        .context("Failed to parse token response")
}

/// Refresh an OAuth token.
pub async fn refresh_token(
    refresh_tok: &str,
    scopes: &[&str],
) -> Result<OAuthTokenResponse> {
    let scope_str = scopes.join(" ");
    let body = serde_json::json!({
        "grant_type": "refresh_token",
        "refresh_token": refresh_tok,
        "client_id": config::CLIENT_ID,
        "scope": scope_str,
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(config::TOKEN_URL)
        .json(&body)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .context("Failed to reach token endpoint for refresh")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("Token refresh failed ({}): {}", status, text);
    }

    resp.json::<OAuthTokenResponse>()
        .await
        .context("Failed to parse refresh response")
}

/// Create an API key using an OAuth access token (Console mode).
pub async fn create_api_key(access_token: &str) -> Result<String> {
    let client = reqwest::Client::new();
    let resp = client
        .post(config::CREATE_API_KEY_URL)
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .context("Failed to reach create_api_key endpoint")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("Create API key failed ({}): {}", status, text);
    }

    let parsed: CreateApiKeyResponse = resp
        .json()
        .await
        .context("Failed to parse create_api_key response")?;
    Ok(parsed.raw_key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Requires network — integration test
    async fn test_exchange_invalid_code_returns_error() {
        let result = exchange_code(
            "invalid_code",
            "fake_verifier",
            "fake_state",
            config::MANUAL_REDIRECT_URL,
        )
        .await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("401") || err.contains("400") || err.contains("failed"),
            "Expected HTTP error, got: {}",
            err
        );
    }

    #[tokio::test]
    #[ignore] // Requires network — integration test
    async fn test_refresh_invalid_token_returns_error() {
        let result =
            refresh_token("invalid_refresh_token", config::CLAUDE_AI_SCOPES).await;
        assert!(result.is_err());
    }
}
```

- [ ] **Step 2: Register module in `src/auth/oauth/mod.rs`**

```rust
//! OAuth 2.0 Authorization Code + PKCE login flow.

pub mod client;
pub mod config;
pub mod pkce;
```

- [ ] **Step 3: Compile and check**

Run: `cargo build 2>&1 | grep warning`
Expected: no new warnings

- [ ] **Step 4: Commit**

```bash
git add src/auth/oauth/client.rs src/auth/oauth/mod.rs
git commit -m "feat(auth): add OAuth HTTP client for token exchange and API key creation"
```

---

### Task 4: Extend `token.rs` — add `scopes` and `oauth_method` fields

**Files:**
- Modify: `src/auth/token.rs`

- [ ] **Step 1: Write the new test in `token.rs`**

Add to the bottom of `token.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_save_and_load_roundtrip() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("credentials.json");

        let token = StoredToken {
            access_token: "test-access".into(),
            refresh_token: Some("test-refresh".into()),
            expires_at: Some(1700000000),
            token_type: "bearer".into(),
            scopes: vec!["user:profile".into(), "user:inference".into()],
            oauth_method: Some("claude_ai".into()),
        };

        let content = serde_json::to_string_pretty(&token).unwrap();
        std::fs::write(&path, &content).unwrap();

        let loaded: StoredToken =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(loaded.access_token, "test-access");
        assert_eq!(loaded.refresh_token, Some("test-refresh".into()));
        assert_eq!(loaded.scopes.len(), 2);
        assert_eq!(loaded.oauth_method, Some("claude_ai".into()));
    }

    #[test]
    fn test_is_token_expired_with_buffer() {
        let future = chrono::Utc::now().timestamp() + 600; // 10 min from now
        let token = StoredToken {
            access_token: "t".into(),
            refresh_token: None,
            expires_at: Some(future),
            token_type: "bearer".into(),
            scopes: vec![],
            oauth_method: None,
        };
        assert!(!is_token_expired(&token));

        let past = chrono::Utc::now().timestamp() - 10;
        let expired = StoredToken {
            expires_at: Some(past),
            ..token.clone()
        };
        assert!(is_token_expired(&expired));
    }

    #[test]
    fn test_is_token_expired_none_means_not_expired() {
        let token = StoredToken {
            access_token: "t".into(),
            refresh_token: None,
            expires_at: None,
            token_type: "bearer".into(),
            scopes: vec![],
            oauth_method: None,
        };
        assert!(!is_token_expired(&token));
    }
}
```

- [ ] **Step 2: Update `StoredToken` struct and remove `#![allow(dead_code)]`**

Replace the entire `src/auth/token.rs` with:

```rust
//! OAuth token persistence.
//!
//! Stores OAuth tokens at `~/.cc-rust/credentials.json`.

use anyhow::Result;

/// Token storage file path: `~/.cc-rust/credentials.json`
pub fn token_file_path() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".cc-rust")
        .join("credentials.json")
}

/// Stored token data (OAuth).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StoredToken {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<i64>,
    pub token_type: String,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub oauth_method: Option<String>,
}

/// Load stored OAuth token from disk.
pub fn load_token() -> Result<Option<StoredToken>> {
    let path = token_file_path();
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)?;
    let token: StoredToken = serde_json::from_str(&content)?;
    Ok(Some(token))
}

/// Save OAuth token to disk.
pub fn save_token(token: &StoredToken) -> Result<()> {
    let path = token_file_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(token)?;
    std::fs::write(&path, content)?;
    Ok(())
}

/// Remove stored OAuth token from disk.
pub fn remove_token() -> Result<()> {
    let path = token_file_path();
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

/// Check if a stored token has expired (with 5-minute buffer).
pub fn is_token_expired(token: &StoredToken) -> bool {
    if let Some(expires_at) = token.expires_at {
        let now = chrono::Utc::now().timestamp();
        now >= expires_at - 300
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_save_and_load_roundtrip() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("credentials.json");

        let token = StoredToken {
            access_token: "test-access".into(),
            refresh_token: Some("test-refresh".into()),
            expires_at: Some(1700000000),
            token_type: "bearer".into(),
            scopes: vec!["user:profile".into(), "user:inference".into()],
            oauth_method: Some("claude_ai".into()),
        };

        let content = serde_json::to_string_pretty(&token).unwrap();
        std::fs::write(&path, &content).unwrap();

        let loaded: StoredToken =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(loaded.access_token, "test-access");
        assert_eq!(loaded.refresh_token, Some("test-refresh".into()));
        assert_eq!(loaded.scopes.len(), 2);
        assert_eq!(loaded.oauth_method, Some("claude_ai".into()));
    }

    #[test]
    fn test_is_token_expired_with_buffer() {
        let future = chrono::Utc::now().timestamp() + 600;
        let token = StoredToken {
            access_token: "t".into(),
            refresh_token: None,
            expires_at: Some(future),
            token_type: "bearer".into(),
            scopes: vec![],
            oauth_method: None,
        };
        assert!(!is_token_expired(&token));

        let past = chrono::Utc::now().timestamp() - 10;
        let expired = StoredToken {
            expires_at: Some(past),
            ..token.clone()
        };
        assert!(is_token_expired(&expired));
    }

    #[test]
    fn test_is_token_expired_none_means_not_expired() {
        let token = StoredToken {
            access_token: "t".into(),
            refresh_token: None,
            expires_at: None,
            token_type: "bearer".into(),
            scopes: vec![],
            oauth_method: None,
        };
        assert!(!is_token_expired(&token));
    }
}
```

- [ ] **Step 3: Check `tempfile` is in dev-dependencies, add if not**

Run: `cargo build --tests 2>&1 | head -5`

If `tempfile` not found, add to `[dev-dependencies]` in `Cargo.toml`:

```toml
tempfile = "3"
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p cc-rust auth::token -- --nocapture`
Expected: 3 tests PASS

- [ ] **Step 5: Check for warnings**

Run: `cargo build 2>&1 | grep warning`
Expected: no warnings from `token.rs` (dead_code removed)

- [ ] **Step 6: Commit**

```bash
git add src/auth/token.rs Cargo.toml
git commit -m "feat(auth): extend StoredToken with scopes and oauth_method fields"
```

---

### Task 5: Create `oauth/mod.rs` — auto-refresh and browser interface

**Files:**
- Modify: `src/auth/oauth/mod.rs`

- [ ] **Step 1: Write the full `oauth/mod.rs`**

Replace `src/auth/oauth/mod.rs` with:

```rust
//! OAuth 2.0 Authorization Code + PKCE login flow.
//!
//! Supports two modes:
//! - **Claude.ai**: Bearer token for Pro/Max subscribers
//! - **Console**: Creates an API key via OAuth
//!
//! The login flow is two-step (driven by `/login` and `/login-code` commands):
//! 1. `/login 2|3` → generates PKCE, prints auth URL
//! 2. `/login-code <code>` → exchanges code for tokens, stores them

pub mod client;
pub mod config;
pub mod pkce;

use anyhow::Result;
pub use config::OAuthMethod;

use crate::auth::token;

/// Open a browser to the given URL.
///
/// Reserved for future implementation (auto-open browser + localhost callback).
/// Currently a no-op — the manual flow prints the URL for the user to copy.
pub async fn open_browser(_url: &str) -> Result<()> {
    // Future: use `open` crate to launch system browser
    // + spawn localhost HTTP callback server
    Ok(())
}

/// Try to auto-refresh the stored OAuth token if it is about to expire.
///
/// Returns the (possibly refreshed) access token and method, or `None`.
pub async fn try_refresh_if_needed() -> Result<Option<(String, String)>> {
    let stored = match token::load_token()? {
        Some(t) => t,
        None => return Ok(None),
    };

    let method = stored.oauth_method.clone().unwrap_or_default();

    if !token::is_token_expired(&stored) {
        return Ok(Some((stored.access_token, method)));
    }

    // Token expired — try refresh
    let refresh_tok = match &stored.refresh_token {
        Some(t) => t.clone(),
        None => {
            // No refresh token — clear and bail
            let _ = token::remove_token();
            return Ok(None);
        }
    };

    let scopes: Vec<&str> = stored.scopes.iter().map(|s| s.as_str()).collect();
    let scopes_ref = if scopes.is_empty() {
        config::CLAUDE_AI_SCOPES
    } else {
        &scopes
    };

    match client::refresh_token(&refresh_tok, scopes_ref).await {
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
                    stored.scopes
                } else {
                    new_scopes
                },
                oauth_method: stored.oauth_method,
            };
            token::save_token(&updated)?;
            Ok(Some((resp.access_token, method)))
        }
        Err(e) => {
            tracing::warn!(error = %e, "OAuth token refresh failed, clearing credentials");
            let _ = token::remove_token();
            Ok(None)
        }
    }
}
```

- [ ] **Step 2: Compile and check**

Run: `cargo build 2>&1 | grep warning`
Expected: no new warnings

- [ ] **Step 3: Commit**

```bash
git add src/auth/oauth/mod.rs
git commit -m "feat(auth): implement OAuth flow orchestration and auto-refresh"
```

---

### Task 6: Wire `resolve_auth()` and `AuthMethod` to support OAuth

**Files:**
- Modify: `src/auth/mod.rs`

- [ ] **Step 1: Write test for OAuth priority in `mod.rs`**

Add to the `#[cfg(test)] mod tests` block at the bottom of `src/auth/mod.rs`:

```rust
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
```

- [ ] **Step 2: Run test — verify it fails**

Run: `cargo test -p cc-rust auth::tests::test_auth_method_oauth_token`
Expected: FAIL (no `OAuthToken` variant yet)

- [ ] **Step 3: Update `AuthMethod` enum and methods**

Replace the `AuthMethod` enum and impl block (lines 17-46) in `src/auth/mod.rs`:

```rust
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
        /// "claude_ai" or "console"
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
```

- [ ] **Step 4: Update `resolve_auth()` to include OAuth**

Replace the `resolve_auth()` function:

```rust
/// Resolve authentication from environment variables, OAuth tokens, and keychain.
///
/// Priority:
/// 1. `ANTHROPIC_API_KEY` env var
/// 2. `ANTHROPIC_AUTH_TOKEN` env var
/// 3. OAuth token from `~/.cc-rust/credentials.json` (if not expired)
/// 4. API key from system keychain
/// 5. `AuthMethod::None`
pub fn resolve_auth() -> AuthMethod {
    // 1. ANTHROPIC_API_KEY
    if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
        if !key.is_empty() && api_key::validate_api_key(&key) {
            return AuthMethod::ApiKey(key);
        }
    }

    // 2. ANTHROPIC_AUTH_TOKEN
    if let Ok(token) = std::env::var("ANTHROPIC_AUTH_TOKEN") {
        if !token.is_empty() {
            return AuthMethod::ExternalToken(token);
        }
    }

    // 3. OAuth token from disk
    if let Ok(Some(stored)) = token::load_token() {
        if !token::is_token_expired(&stored) {
            let method = stored.oauth_method.clone().unwrap_or_default();
            // Console mode: API key should be in keychain (created at login)
            // Claude.ai mode: use Bearer token directly
            if method == "console" {
                // Fall through to keychain check below
            } else {
                return AuthMethod::OAuthToken {
                    access_token: stored.access_token,
                    method,
                };
            }
        }
        // Note: expired tokens are handled by try_refresh_if_needed() at startup
    }

    // 4. Keychain
    if let Ok(Some(key)) = api_key::load_api_key() {
        if api_key::validate_api_key(&key) {
            return AuthMethod::ApiKey(key);
        }
    }

    AuthMethod::None
}
```

- [ ] **Step 5: Remove old OAuth stubs**

Delete the `OAuthConfig` struct, `OAuthTokens` struct, and the three stub functions (`oauth_login`, `oauth_refresh`, `oauth_logout`) from `src/auth/mod.rs` (lines 84-141). Replace with:

```rust
// ---------------------------------------------------------------------------
// OAuth convenience functions
// ---------------------------------------------------------------------------

/// Clear all OAuth state (tokens + keychain API key).
pub fn oauth_logout() -> anyhow::Result<()> {
    token::remove_token()?;
    let _ = api_key::remove_api_key();
    Ok(())
}
```

- [ ] **Step 6: Run all auth tests**

Run: `cargo test -p cc-rust auth -- --nocapture`
Expected: all tests PASS (including the new `test_auth_method_oauth_token`)

- [ ] **Step 7: Check for warnings**

Run: `cargo build 2>&1 | grep warning`
Expected: no warnings

- [ ] **Step 8: Commit**

```bash
git add src/auth/mod.rs
git commit -m "feat(auth): wire OAuth into resolve_auth() and remove old stubs"
```

---

### Task 7: Update `from_auth()` in `api/client.rs`

**Files:**
- Modify: `src/api/client.rs:238-261`

- [ ] **Step 1: Add the `OAuthToken` match arm**

In `src/api/client.rs`, in the `from_auth()` method, replace the match block (the `match crate::auth::resolve_auth()` at line 238) with:

```rust
        match crate::auth::resolve_auth() {
            crate::auth::AuthMethod::ApiKey(api_key) => {
                let base_url = std::env::var("ANTHROPIC_BASE_URL").ok();
                Some(Self::new(ApiClientConfig {
                    provider: ApiProvider::Anthropic { api_key, base_url },
                    default_model: "claude-sonnet-4-20250514".to_string(),
                    max_retries: 3,
                    timeout_secs: 120,
                }))
            }
            crate::auth::AuthMethod::ExternalToken(token) => {
                let base_url = std::env::var("ANTHROPIC_BASE_URL").ok();
                Some(Self::new(ApiClientConfig {
                    provider: ApiProvider::Anthropic {
                        api_key: token,
                        base_url,
                    },
                    default_model: "claude-sonnet-4-20250514".to_string(),
                    max_retries: 3,
                    timeout_secs: 120,
                }))
            }
            crate::auth::AuthMethod::OAuthToken { access_token, .. } => {
                let base_url = std::env::var("ANTHROPIC_BASE_URL").ok();
                Some(Self::new(ApiClientConfig {
                    provider: ApiProvider::Anthropic {
                        api_key: access_token,
                        base_url,
                    },
                    default_model: "claude-sonnet-4-20250514".to_string(),
                    max_retries: 3,
                    timeout_secs: 120,
                }))
            }
            crate::auth::AuthMethod::None => None,
        }
```

- [ ] **Step 2: Compile and check**

Run: `cargo build 2>&1 | grep warning`
Expected: no warnings

- [ ] **Step 3: Commit**

```bash
git add src/api/client.rs
git commit -m "feat(api): handle OAuthToken in from_auth() client construction"
```

---

### Task 8: Create `/login-code` command and update `/login` with menu

The `/login` command can't read stdin interactively (it returns `CommandResult`). So the OAuth flow is two-step: `/login 2` generates PKCE + prints URL, then `/login-code <code>` completes the exchange.

**Files:**
- Create: `src/commands/login_code.rs`
- Modify: `src/commands/login.rs`
- Modify: `src/commands/mod.rs` (register new command)

- [ ] **Step 1: Create `src/commands/login_code.rs`**

```rust
//! `/login-code` command — complete OAuth login by exchanging an authorization code.
//!
//! Usage:
//!   /login-code <authorization-code>
//!
//! This is the second step of the OAuth flow started by `/login 2` or `/login 3`.

use anyhow::Result;
use async_trait::async_trait;

use super::{CommandContext, CommandHandler, CommandResult};
use crate::auth::oauth::{client, config, pkce};
use crate::auth::{api_key, token};

/// Pending OAuth state (PKCE verifier, state, method).
static PENDING_OAUTH: parking_lot::Mutex<Option<PendingOAuth>> =
    parking_lot::Mutex::new(None);

struct PendingOAuth {
    method: config::OAuthMethod,
    verifier: String,
    state: String,
}

/// Start a pending OAuth flow. Called from `/login 2` or `/login 3`.
///
/// Generates PKCE params, stores them, and returns the message with the auth URL.
pub fn start_pending(method: config::OAuthMethod) -> String {
    let verifier = pkce::generate_code_verifier();
    let challenge = pkce::generate_code_challenge(&verifier);
    let state = pkce::generate_state();
    let url = config::authorization_url(method, &challenge, &state);

    *PENDING_OAUTH.lock() = Some(PendingOAuth {
        method,
        verifier,
        state,
    });

    let method_name = match method {
        config::OAuthMethod::ClaudeAi => "Claude.ai",
        config::OAuthMethod::Console => "Console",
    };

    format!(
        "Opening {} authorization...\n\n\
         Please visit this URL to authorize:\n\n  {}\n\n\
         After authorizing, paste the code:\n  /login-code <paste-code-here>",
        method_name, url
    )
}

pub struct LoginCodeHandler;

#[async_trait]
impl CommandHandler for LoginCodeHandler {
    async fn execute(&self, args: &str, _ctx: &mut CommandContext) -> Result<CommandResult> {
        let code = args.trim();
        if code.is_empty() {
            return Ok(CommandResult::Output(
                "Usage: /login-code <authorization-code>\n\
                 Start the OAuth flow first with /login 2 or /login 3"
                    .to_string(),
            ));
        }

        let pending = PENDING_OAUTH.lock().take();
        let pending = match pending {
            Some(p) => p,
            None => {
                return Ok(CommandResult::Output(
                    "No pending OAuth flow. Start one with /login 2 or /login 3".to_string(),
                ));
            }
        };

        // Exchange code for tokens
        let token_resp = match client::exchange_code(
            code,
            &pending.verifier,
            &pending.state,
            config::MANUAL_REDIRECT_URL,
        )
        .await
        {
            Ok(r) => r,
            Err(e) => {
                return Ok(CommandResult::Output(format!(
                    "Token exchange failed: {}\n\nPlease retry with /login 2 or /login 3",
                    e
                )));
            }
        };

        // Store tokens
        let expires_at = chrono::Utc::now().timestamp() + token_resp.expires_in as i64;
        let method_str = match pending.method {
            config::OAuthMethod::ClaudeAi => "claude_ai",
            config::OAuthMethod::Console => "console",
        };
        let scopes: Vec<String> = token_resp
            .scope
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();

        let stored = token::StoredToken {
            access_token: token_resp.access_token.clone(),
            refresh_token: token_resp.refresh_token.clone(),
            expires_at: Some(expires_at),
            token_type: "bearer".into(),
            scopes,
            oauth_method: Some(method_str.to_string()),
        };

        if let Err(e) = token::save_token(&stored) {
            return Ok(CommandResult::Output(format!(
                "Failed to save OAuth tokens: {}",
                e
            )));
        }

        // Console mode: create API key
        if pending.method == config::OAuthMethod::Console {
            match client::create_api_key(&token_resp.access_token).await {
                Ok(raw_key) => {
                    if let Err(e) = api_key::store_api_key(&raw_key) {
                        return Ok(CommandResult::Output(format!(
                            "OAuth tokens saved, but keychain storage failed: {}",
                            e
                        )));
                    }
                    return Ok(CommandResult::Output(
                        "Logged in successfully (Console). API key stored to keychain."
                            .to_string(),
                    ));
                }
                Err(e) => {
                    return Ok(CommandResult::Output(format!(
                        "OAuth tokens saved, but API key creation failed: {}\n\
                         You can retry with /login 3",
                        e
                    )));
                }
            }
        }

        Ok(CommandResult::Output(
            "Logged in successfully (Claude.ai).".to_string(),
        ))
    }
}
```

- [ ] **Step 2: Rewrite `src/commands/login.rs`**

```rust
//! `/login` command — authenticate with an LLM provider.
//!
//! Usage:
//!   /login              — interactive login (choose method)
//!   /login status       — show current auth status
//!   /login sk-ant-...   — store API key directly
//!   /login 1|2|3        — select login method

use anyhow::Result;
use async_trait::async_trait;

use super::login_code;
use super::{CommandContext, CommandHandler, CommandResult};
use crate::auth;
use crate::auth::oauth::config::OAuthMethod;

pub struct LoginHandler;

#[async_trait]
impl CommandHandler for LoginHandler {
    async fn execute(&self, args: &str, _ctx: &mut CommandContext) -> Result<CommandResult> {
        let args = args.trim();

        if args == "status" {
            return Ok(CommandResult::Output(auth_status_text()));
        }

        if args.starts_with("sk-ant-") {
            return Ok(CommandResult::Output(store_api_key(args)));
        }

        if args.is_empty() {
            return Ok(CommandResult::Output(login_menu()));
        }

        match args {
            "1" => Ok(CommandResult::Output(
                "Paste your API key:\n  /login sk-ant-api03-...".to_string(),
            )),
            "2" => Ok(CommandResult::Output(
                login_code::start_pending(OAuthMethod::ClaudeAi),
            )),
            "3" => Ok(CommandResult::Output(
                login_code::start_pending(OAuthMethod::Console),
            )),
            _ => Ok(CommandResult::Output(format!(
                "Unknown option: \"{}\"\n\n{}",
                args,
                login_menu()
            ))),
        }
    }
}

fn login_menu() -> String {
    "Select login method:\n\
     \n  [1] API Key (paste manually)\
     \n  [2] Claude.ai OAuth (Pro/Max subscription)\
     \n  [3] Console OAuth (API billing)\
     \n\nType /login 1, /login 2, or /login 3"
        .to_string()
}

fn auth_status_text() -> String {
    let current = auth::resolve_auth();
    match &current {
        auth::AuthMethod::ApiKey(key) => {
            let source = if std::env::var("ANTHROPIC_API_KEY")
                .map(|v| !v.is_empty())
                .unwrap_or(false)
            {
                "env ANTHROPIC_API_KEY"
            } else {
                "system keychain"
            };
            format!("Authenticated: API Key {} (source: {})", mask_key(key), source)
        }
        auth::AuthMethod::ExternalToken(_) => {
            "Authenticated: External Token (ANTHROPIC_AUTH_TOKEN)".to_string()
        }
        auth::AuthMethod::OAuthToken { method, .. } => {
            format!("Authenticated: OAuth ({})", method)
        }
        auth::AuthMethod::None => "Not authenticated".to_string(),
    }
}

fn store_api_key(key: &str) -> String {
    if !auth::api_key::validate_api_key(key) {
        return "Invalid API key format. Keys start with \"sk-ant-\" and are >20 chars.".into();
    }
    match auth::api_key::store_api_key(key) {
        Ok(()) => format!("API Key {} stored to keychain.", mask_key(key)),
        Err(e) => format!("Failed to store API key: {}", e),
    }
}

fn mask_key(key: &str) -> String {
    if key.len() > 12 {
        format!("{}...{}", &key[..7], &key[key.len() - 4..])
    } else {
        "sk-ant-****".to_string()
    }
}
```

- [ ] **Step 3: Register `/login-code` in `src/commands/mod.rs`**

Add `pub mod login_code;` near the other `pub mod` declarations, and add to the `get_all_commands()` vec:

```rust
        Command {
            name: "login-code".into(),
            aliases: vec![],
            description: "Complete OAuth login with authorization code".into(),
            handler: Box::new(login_code::LoginCodeHandler),
        },
```

- [ ] **Step 4: Compile and check for warnings**

Run: `cargo build 2>&1 | grep warning`
Expected: no warnings

- [ ] **Step 5: Commit**

```bash
git add src/commands/login.rs src/commands/login_code.rs src/commands/mod.rs
git commit -m "feat(commands): add /login-code for two-step OAuth flow"
```

---

### Task 9: Update `/logout` to clear OAuth state

**Files:**
- Modify: `src/commands/logout.rs`

- [ ] **Step 1: Update logout to use `oauth_logout()`**

Replace the `execute` method in `src/commands/logout.rs`:

```rust
    async fn execute(&self, _args: &str, _ctx: &mut CommandContext) -> Result<CommandResult> {
        let current_auth = auth::resolve_auth();

        if !current_auth.is_authenticated() {
            return Ok(CommandResult::Output(
                "Not currently authenticated — nothing to clear.".to_string(),
            ));
        }

        // Clear all auth state: keychain + credentials.json
        if let Err(e) = auth::oauth_logout() {
            tracing::warn!(error = %e, "error during logout cleanup");
        }

        Ok(CommandResult::Output(
            "Logged out successfully. Cleared keychain and stored OAuth tokens.\n\
             Note: environment variables (ANTHROPIC_API_KEY, ANTHROPIC_AUTH_TOKEN) \
             must be unset manually."
                .to_string(),
        ))
    }
```

- [ ] **Step 2: Compile and check**

Run: `cargo build 2>&1 | grep warning`
Expected: no warnings

- [ ] **Step 3: Commit**

```bash
git add src/commands/logout.rs
git commit -m "feat(commands): update /logout to clear OAuth state"
```

---

### Task 10: Final integration — build, test all, fix warnings

**Files:**
- All previously modified files

- [ ] **Step 1: Run full build**

Run: `cargo build 2>&1`
Expected: clean build, no errors

- [ ] **Step 2: Run all auth tests**

Run: `cargo test -p cc-rust auth -- --nocapture`
Expected: all tests PASS (pkce: 4, config: 3, token: 3, mod: 4 = 14 total)

- [ ] **Step 3: Run full test suite**

Run: `cargo test -p cc-rust -- --nocapture`
Expected: no regressions

- [ ] **Step 4: Grep for any remaining warnings**

Run: `cargo build 2>&1 | grep -i warning`
Expected: zero warnings from auth/ or commands/ modules

- [ ] **Step 5: Verify oauth/mod.rs `open_browser` has no warning**

The function has `_url` parameter prefix and a non-empty body (`Ok(())`), so no unused warnings should appear.

- [ ] **Step 6: Commit any final fixes**

```bash
git add -A
git commit -m "chore(auth): final cleanup — all tests pass, zero warnings"
```
