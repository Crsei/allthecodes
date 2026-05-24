//! Codex CLI credential fallback — reads `~/.codex/auth.json` for token reuse.
//!
//! When a user has already logged into the OpenAI Codex CLI, allthecodes can
//! transparently reuse those credentials instead of requiring `/login 4`.

use std::path::PathBuf;

use anyhow::Context as _;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::Deserialize;

/// Codex CLI built-in OAuth client_id (extracted from codex.exe binary).
pub const CODEX_CLI_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";

/// Codex CLI config directory name.
const CODEX_CLI_DIR_NAME: &str = ".codex";

/// Auth file name inside the Codex CLI config directory.
const CODEX_CLI_AUTH_FILE: &str = "auth.json";

/// Environment variable that overrides the Codex CLI config directory.
const CODEX_HOME_ENV: &str = "CODEX_HOME";

/// Expiry buffer in seconds (same as `token::is_token_expired`).
const EXPIRY_BUFFER_SECS: i64 = 300;

// ---------------------------------------------------------------------------
// Deserialization types (private — match ~/.codex/auth.json structure)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct CodexCliAuthFile {
    auth_mode: String,
    tokens: Option<CodexCliTokens>,
}

#[derive(Deserialize)]
struct CodexCliTokens {
    access_token: String,
    refresh_token: Option<String>,
    #[serde(rename = "account_id")]
    _account_id: Option<String>,
    #[serde(rename = "id_token")]
    _id_token: Option<String>,
}

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Structured credential parsed from Codex CLI's `auth.json`.
#[derive(Debug, Clone)]
pub struct CodexCliCredential {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub client_id: String,
    pub expires_at: Option<i64>,
}

// ---------------------------------------------------------------------------
// Public functions
// ---------------------------------------------------------------------------

/// Return the path to Codex CLI's `auth.json` if it exists on disk.
///
/// Resolution: `$CODEX_HOME/auth.json` > `~/.codex/auth.json`.
pub fn codex_cli_auth_path() -> Option<PathBuf> {
    let dir = if let Ok(home) = std::env::var(CODEX_HOME_ENV) {
        let p = PathBuf::from(home.trim());
        if p.as_os_str().is_empty() {
            default_codex_dir()?
        } else {
            p
        }
    } else {
        default_codex_dir()?
    };

    let path = dir.join(CODEX_CLI_AUTH_FILE);
    if path.is_file() {
        Some(path)
    } else {
        None
    }
}

/// Read and parse a [`CodexCliCredential`] from Codex CLI's `auth.json`.
///
/// Returns `Ok(None)` when:
/// - The file does not exist
/// - `auth_mode` is not `"chatgpt"`
/// - `tokens` is absent or `access_token` is empty
///
/// Returns `Err` when an existing file cannot be read or parsed.
pub fn read_codex_cli_credential() -> anyhow::Result<Option<CodexCliCredential>> {
    let path = match codex_cli_auth_path() {
        Some(path) => path,
        None => return Ok(None),
    };
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("cannot read Codex CLI auth.json at {}", path.display()))?;

    let auth_file: CodexCliAuthFile = serde_json::from_str(&content)
        .with_context(|| format!("cannot parse Codex CLI auth.json at {}", path.display()))?;

    if !auth_file.auth_mode.eq_ignore_ascii_case("chatgpt") {
        tracing::debug!(auth_mode = %auth_file.auth_mode, "Codex CLI auth_mode is not chatgpt");
        return Ok(None);
    }

    let tokens = match auth_file.tokens {
        Some(tokens) => tokens,
        None => return Ok(None),
    };
    if tokens.access_token.trim().is_empty() {
        tracing::debug!("Codex CLI access_token is empty");
        return Ok(None);
    }

    let expires_at = decode_jwt_exp(&tokens.access_token);

    Ok(Some(CodexCliCredential {
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token,
        client_id: CODEX_CLI_CLIENT_ID.to_string(),
        expires_at,
    }))
}

/// Check whether a [`CodexCliCredential`] has expired (with 5-minute buffer).
pub fn is_credential_expired(cred: &CodexCliCredential) -> bool {
    if let Some(exp) = cred.expires_at {
        let now = chrono::Utc::now().timestamp();
        now >= exp - EXPIRY_BUFFER_SECS
    } else {
        // No expiry information — assume valid, let the server decide.
        false
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Default Codex CLI config directory: `~/.codex/`.
fn default_codex_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(CODEX_CLI_DIR_NAME))
}

/// Decode the `exp` claim from a JWT payload without signature verification.
///
/// Returns `None` on any parse failure (not a JWT, malformed base64, missing
/// `exp` field). Callers treat `None` as "not expired" and let the server
/// reject the token with 401 if it actually is.
fn decode_jwt_exp(token: &str) -> Option<i64> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let payload_bytes = URL_SAFE_NO_PAD.decode(parts[1]).ok()?;
    let payload: serde_json::Value = serde_json::from_slice(&payload_bytes).ok()?;
    payload.get("exp")?.as_i64()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_jwt(exp: i64) -> String {
        let header = URL_SAFE_NO_PAD.encode(b"{}");
        let payload_json = format!(r#"{{"exp":{}}}"#, exp);
        let payload = URL_SAFE_NO_PAD.encode(payload_json.as_bytes());
        let sig = URL_SAFE_NO_PAD.encode(b"sig");
        format!("{}.{}.{}", header, payload, sig)
    }

    fn make_auth_json(auth_mode: &str, access_token: &str, refresh_token: Option<&str>) -> String {
        let refresh = match refresh_token {
            Some(r) => format!(r#""refresh_token": "{}","#, r),
            None => String::new(),
        };
        format!(
            r#"{{
                "auth_mode": "{}",
                "tokens": {{
                    "access_token": "{}",
                    {}
                    "account_id": "test-account"
                }}
            }}"#,
            auth_mode, access_token, refresh
        )
    }

    // ---- decode_jwt_exp ----

    #[test]
    fn test_decode_jwt_exp_valid() {
        let exp = 1700000000i64;
        let token = make_jwt(exp);
        assert_eq!(decode_jwt_exp(&token), Some(exp));
    }

    #[test]
    fn test_decode_jwt_exp_invalid_not_jwt() {
        assert_eq!(decode_jwt_exp("not-a-jwt"), None);
    }

    #[test]
    fn test_decode_jwt_exp_missing_exp_field() {
        let header = URL_SAFE_NO_PAD.encode(b"{}");
        let payload = URL_SAFE_NO_PAD.encode(br#"{"sub":"user"}"#);
        let sig = URL_SAFE_NO_PAD.encode(b"sig");
        let token = format!("{}.{}.{}", header, payload, sig);
        assert_eq!(decode_jwt_exp(&token), None);
    }

    #[test]
    fn test_decode_jwt_exp_bad_base64() {
        assert_eq!(decode_jwt_exp("a.!!!.c"), None);
    }

    // ---- is_credential_expired ----

    #[test]
    fn test_is_credential_expired_future() {
        let cred = CodexCliCredential {
            access_token: "t".into(),
            refresh_token: None,
            client_id: CODEX_CLI_CLIENT_ID.into(),
            expires_at: Some(chrono::Utc::now().timestamp() + 600),
        };
        assert!(!is_credential_expired(&cred));
    }

    #[test]
    fn test_is_credential_expired_past() {
        let cred = CodexCliCredential {
            access_token: "t".into(),
            refresh_token: None,
            client_id: CODEX_CLI_CLIENT_ID.into(),
            expires_at: Some(chrono::Utc::now().timestamp() - 10),
        };
        assert!(is_credential_expired(&cred));
    }

    #[test]
    fn test_is_credential_expired_none_means_valid() {
        let cred = CodexCliCredential {
            access_token: "t".into(),
            refresh_token: None,
            client_id: CODEX_CLI_CLIENT_ID.into(),
            expires_at: None,
        };
        assert!(!is_credential_expired(&cred));
    }

    // ---- read_codex_cli_credential (file-based) ----

    #[test]
    fn test_parse_valid_auth_json() {
        let dir = tempfile::TempDir::new().unwrap();
        let auth_path = dir.path().join(CODEX_CLI_AUTH_FILE);
        let token = make_jwt(chrono::Utc::now().timestamp() + 3600);
        let json = make_auth_json("chatgpt", &token, Some("refresh-tok"));
        std::fs::write(&auth_path, json).unwrap();

        let cred = with_codex_home(dir.path(), || {
            read_codex_cli_credential()
                .unwrap()
                .expect("should parse valid auth.json")
        });
        assert_eq!(cred.access_token, token);
        assert_eq!(cred.refresh_token.as_deref(), Some("refresh-tok"));
        assert_eq!(cred.client_id, CODEX_CLI_CLIENT_ID);
        assert!(cred.expires_at.is_some());
    }

    #[test]
    fn test_parse_api_key_mode_returns_none() {
        let dir = tempfile::TempDir::new().unwrap();
        let auth_path = dir.path().join(CODEX_CLI_AUTH_FILE);
        let json = make_auth_json("api_key", "sk-test", None);
        std::fs::write(&auth_path, json).unwrap();

        with_codex_home(dir.path(), || {
            assert!(read_codex_cli_credential().unwrap().is_none());
        });
    }

    #[test]
    fn test_parse_missing_tokens_returns_none() {
        let dir = tempfile::TempDir::new().unwrap();
        let auth_path = dir.path().join(CODEX_CLI_AUTH_FILE);
        std::fs::write(&auth_path, r#"{"auth_mode":"chatgpt"}"#).unwrap();

        with_codex_home(dir.path(), || {
            assert!(read_codex_cli_credential().unwrap().is_none());
        });
    }

    #[test]
    fn test_parse_empty_access_token_returns_none() {
        let dir = tempfile::TempDir::new().unwrap();
        let auth_path = dir.path().join(CODEX_CLI_AUTH_FILE);
        let json = make_auth_json("chatgpt", "", None);
        std::fs::write(&auth_path, json).unwrap();

        with_codex_home(dir.path(), || {
            assert!(read_codex_cli_credential().unwrap().is_none());
        });
    }

    #[test]
    fn test_parse_malformed_auth_json_returns_error() {
        let dir = tempfile::TempDir::new().unwrap();
        let auth_path = dir.path().join(CODEX_CLI_AUTH_FILE);
        std::fs::write(&auth_path, "{not-json").unwrap();

        let err = with_codex_home(dir.path(), || {
            read_codex_cli_credential().expect_err("malformed auth.json must be diagnostic")
        });
        assert!(
            err.to_string().contains("cannot parse Codex CLI auth.json"),
            "unexpected error: {err:#}"
        );
    }

    #[test]
    fn test_read_codex_cli_credential_leaves_fallback_file_unchanged() {
        let dir = tempfile::TempDir::new().unwrap();
        let auth_path = dir.path().join(CODEX_CLI_AUTH_FILE);
        let token = make_jwt(chrono::Utc::now().timestamp() + 3600);
        let json = make_auth_json("chatgpt", &token, Some("refresh-tok"));
        std::fs::write(&auth_path, &json).unwrap();

        let cred = with_codex_home(dir.path(), || {
            read_codex_cli_credential()
                .unwrap()
                .expect("should parse valid auth.json")
        });

        assert_eq!(cred.access_token, token);
        assert_eq!(std::fs::read_to_string(&auth_path).unwrap(), json);
    }

    // ---- codex_cli_auth_path ----

    #[test]
    fn test_codex_cli_auth_path_with_env() {
        let dir = tempfile::TempDir::new().unwrap();
        let auth_path = dir.path().join(CODEX_CLI_AUTH_FILE);
        std::fs::write(&auth_path, "{}").unwrap();

        let resolved = with_codex_home(dir.path(), codex_cli_auth_path);
        assert_eq!(resolved, Some(auth_path));
    }

    #[test]
    fn test_codex_cli_auth_path_missing_file() {
        let dir = tempfile::TempDir::new().unwrap();
        with_codex_home(dir.path(), || {
            assert!(codex_cli_auth_path().is_none());
        });
    }

    // ---- Test helper: RAII env var guard ----

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn with_codex_home<T>(path: &std::path::Path, f: impl FnOnce() -> T) -> T {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _guard = EnvGuard::set(CODEX_HOME_ENV, path.to_str().unwrap());
        f()
    }

    struct EnvGuard {
        key: String,
        prev: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &str, value: &str) -> Self {
            let prev = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self {
                key: key.to_string(),
                prev,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.prev {
                Some(v) => std::env::set_var(&self.key, v),
                None => std::env::remove_var(&self.key),
            }
        }
    }
}
