//! MCP OAuth credential store abstraction.
//!
//! Provides three store backends:
//!
//! * **File**: classic JSON file at `{data_root}/mcp-oauth.json` with Unix 0600 perms.
//! * **Keyring**: system keychain via the `keyring` crate.
//! * **Auto**: try keyring first, fall back to file on failure.
//!
//! The file store is the canonical backend used by existing code. The keyring
//! backend stores each server's token under the allthecodes keychain service
//! with an account name derived from the server auth key (SHA-256,
//! base64url-encoded).

use anyhow::{Context, Result};
use base64::Engine;
use tracing::warn;

use crate::auth::{read_oauth_store, write_oauth_store, StoredMcpOAuthToken};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Credential store backend selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpCredentialsStoreMode {
    /// Try keyring; fall back to file on any error.
    Auto,
    /// Classic file-based store at `{data_root}/mcp-oauth.json`.
    File,
    /// System keychain only (`keyring` crate). Fails if unavailable.
    Keyring,
}

impl McpCredentialsStoreMode {
    /// Resolve from the user-facing config string.
    ///
    /// * `None` or `"auto"` → [`Auto`]
    /// * `"file"` → [`File`]
    /// * `"keyring"` → [`Keyring`]
    /// * anything else → [`Auto`] with a warning.
    pub fn from_config(value: Option<&str>) -> Self {
        match value {
            None | Some("auto") => Self::Auto,
            Some("file") => Self::File,
            Some("keyring") => Self::Keyring,
            Some(other) => {
                warn!(value = %other, "unknown credentials_store mode, falling back to auto");
                Self::Auto
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Public store operations
// ---------------------------------------------------------------------------

pub mod store_ops {
    use super::*;

    /// Read stored token for `auth_key`, selecting the backend per `mode`.
    pub fn load_token(
        mode: McpCredentialsStoreMode,
        auth_key: &str,
    ) -> Result<Option<StoredMcpOAuthToken>> {
        match mode {
            McpCredentialsStoreMode::Auto => load_token_auto(auth_key),
            McpCredentialsStoreMode::File => load_token_file(auth_key),
            McpCredentialsStoreMode::Keyring => load_token_keyring(auth_key),
        }
    }

    /// Write `token` for `auth_key`, selecting the backend per `mode`.
    pub fn save_token(
        mode: McpCredentialsStoreMode,
        auth_key: &str,
        token: &StoredMcpOAuthToken,
    ) -> Result<()> {
        match mode {
            McpCredentialsStoreMode::Auto => save_token_auto(auth_key, token),
            McpCredentialsStoreMode::File => save_token_file(auth_key, token),
            McpCredentialsStoreMode::Keyring => save_token_keyring(auth_key, token),
        }
    }

    /// Delete stored token for `auth_key`, selecting the backend per `mode`.
    /// Returns `true` if a token was actually removed.
    pub fn delete_token(mode: McpCredentialsStoreMode, auth_key: &str) -> Result<bool> {
        match mode {
            McpCredentialsStoreMode::Auto => delete_token_auto(auth_key),
            McpCredentialsStoreMode::File => delete_token_file(auth_key),
            McpCredentialsStoreMode::Keyring => delete_token_keyring(auth_key),
        }
    }
}

// ---------------------------------------------------------------------------
// File backend
// ---------------------------------------------------------------------------

fn load_token_file(auth_key: &str) -> Result<Option<StoredMcpOAuthToken>> {
    let store = read_oauth_store()?;
    Ok(store.servers.get(auth_key).cloned())
}

fn save_token_file(auth_key: &str, token: &StoredMcpOAuthToken) -> Result<()> {
    let mut store = read_oauth_store()?;
    store.servers.insert(auth_key.to_string(), token.clone());
    write_oauth_store(&store)?;
    Ok(())
}

fn delete_token_file(auth_key: &str) -> Result<bool> {
    let mut store = read_oauth_store()?;
    let removed = store.servers.remove(auth_key).is_some();
    if removed {
        write_oauth_store(&store)?;
    }
    Ok(removed)
}

// ---------------------------------------------------------------------------
// Keyring backend
// ---------------------------------------------------------------------------

/// Keychain service name for MCP OAuth credentials.
/// Deliberately matches allthecodes' isolated keychain service and never uses
/// Codex/Claude service names.
const KEYRING_SERVICE_NAME: &str = "allthecodes";

/// Derive a safe keychain account name from the server auth key.
/// Uses SHA-256 then base64url-encode to avoid key-length limits and special
/// characters that some keychain backends reject.
fn keyring_account(auth_key: &str) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(auth_key.as_bytes());
    format!(
        "mcp-oauth:{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
    )
}

fn save_token_keyring(auth_key: &str, token: &StoredMcpOAuthToken) -> Result<()> {
    let account = keyring_account(auth_key);
    let entry = keyring::Entry::new(KEYRING_SERVICE_NAME, &account)?;
    let json = serde_json::to_string(token).context("failed to serialize OAuth token")?;
    entry.set_password(&json)?;
    Ok(())
}

fn load_token_keyring(auth_key: &str) -> Result<Option<StoredMcpOAuthToken>> {
    let account = keyring_account(auth_key);
    let entry = keyring::Entry::new(KEYRING_SERVICE_NAME, &account)?;
    match entry.get_password() {
        Ok(json) => serde_json::from_str(&json)
            .map(Some)
            .context("failed to deserialize OAuth token from keyring"),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(err.into()),
    }
}

fn delete_token_keyring(auth_key: &str) -> Result<bool> {
    let account = keyring_account(auth_key);
    let entry = keyring::Entry::new(KEYRING_SERVICE_NAME, &account)?;
    match entry.delete_credential() {
        Ok(()) => Ok(true),
        Err(keyring::Error::NoEntry) => Ok(false),
        Err(err) => Err(err.into()),
    }
}

// ---------------------------------------------------------------------------
// Auto backend — try keyring, fall back to file
// ---------------------------------------------------------------------------

fn load_token_auto(auth_key: &str) -> Result<Option<StoredMcpOAuthToken>> {
    match load_token_keyring(auth_key) {
        Ok(Some(token)) => return Ok(Some(token)),
        Ok(None) => { /* fall through to file */ }
        Err(err) => {
            warn!(error = %err, "keyring read failed, falling back to file store");
        }
    }
    load_token_file(auth_key)
}

fn save_token_auto(auth_key: &str, token: &StoredMcpOAuthToken) -> Result<()> {
    match save_token_keyring(auth_key, token) {
        Ok(()) => Ok(()),
        Err(err) => {
            warn!(error = %err, "keyring write failed, falling back to file store");
            save_token_file(auth_key, token)
        }
    }
}

fn delete_token_auto(auth_key: &str) -> Result<bool> {
    let keyring_deleted = match delete_token_keyring(auth_key) {
        Ok(deleted) => deleted,
        Err(err) => {
            warn!(error = %err, "keyring delete failed, proceeding with file store");
            false
        }
    };
    let file_deleted = delete_token_file(auth_key).unwrap_or(false);
    Ok(keyring_deleted || file_deleted)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::token_store_path;
    use serial_test::serial;
    use std::sync::{Mutex, OnceLock};

    use crate::auth::{now_timestamp, server_auth_key};
    use crate::McpOAuthConfig;
    use crate::McpServerConfig;

    /// Static lock to serialize keyring tests (shared in-memory keychain).
    static KEYRING_LOCK: Mutex<()> = Mutex::new(());
    /// In-memory backing for test keychain entries.
    type TestKeychainEntries = Vec<(String, String, Vec<u8>)>;
    static TEST_KEYCHAIN: OnceLock<Mutex<TestKeychainEntries>> = OnceLock::new();

    /// A credential that stores data in a static `HashMap` rather than the
    /// real system keychain.
    #[derive(Debug)]
    struct PersistentTestCredential {
        service: String,
        user: String,
    }

    impl keyring::credential::CredentialApi for PersistentTestCredential {
        fn set_secret(&self, secret: &[u8]) -> keyring::Result<()> {
            let chain = TEST_KEYCHAIN.get_or_init(Default::default);
            let mut guard = chain.lock().expect("test keychain poisoned");
            // Remove existing entry for same (service, user)
            guard.retain(|(s, u, _)| s != &self.service || u != &self.user);
            guard.push((self.service.clone(), self.user.clone(), secret.to_vec()));
            Ok(())
        }

        fn get_secret(&self) -> keyring::Result<Vec<u8>> {
            let chain = TEST_KEYCHAIN.get_or_init(Default::default);
            let guard = chain.lock().expect("test keychain poisoned");
            guard
                .iter()
                .find(|(s, u, _)| s == &self.service && u == &self.user)
                .map(|(_, _, secret)| secret.clone())
                .ok_or(keyring::Error::NoEntry)
        }

        fn delete_credential(&self) -> keyring::Result<()> {
            let chain = TEST_KEYCHAIN.get_or_init(Default::default);
            let mut guard = chain.lock().expect("test keychain poisoned");
            let idx = guard
                .iter()
                .position(|(s, u, _)| s == &self.service && u == &self.user);
            match idx {
                Some(i) => {
                    guard.remove(i);
                    Ok(())
                }
                None => Err(keyring::Error::NoEntry),
            }
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    struct PersistentTestCredentialBuilder;

    impl keyring::credential::CredentialBuilderApi for PersistentTestCredentialBuilder {
        fn build(
            &self,
            _target: Option<&str>,
            service: &str,
            user: &str,
        ) -> keyring::Result<Box<keyring::Credential>> {
            Ok(Box::new(PersistentTestCredential {
                service: service.to_string(),
                user: user.to_string(),
            }))
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn persistence(&self) -> keyring::credential::CredentialPersistence {
            keyring::credential::CredentialPersistence::ProcessOnly
        }
    }

    fn use_persistent_test_keyring() {
        TEST_KEYCHAIN
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .clear();
        keyring::set_default_credential_builder(Box::new(PersistentTestCredentialBuilder));
    }

    fn sample_token() -> StoredMcpOAuthToken {
        StoredMcpOAuthToken {
            access_token: "test-access-token".to_string(),
            refresh_token: Some("test-refresh-token".to_string()),
            token_type: "Bearer".to_string(),
            expires_at: Some(now_timestamp() + 3600),
            scopes: vec!["tools.read".to_string(), "tools.write".to_string()],
            authorization_server: "https://auth.example.com".to_string(),
            token_endpoint: "https://auth.example.com/token".to_string(),
            client_id: "test-client".to_string(),
        }
    }

    fn sample_auth_key() -> String {
        // Build a minimal McpServerConfig just to derive a stable auth key.
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
                scopes: None,
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
        server_auth_key(&config)
    }

    // -----------------------------------------------------------------------
    // Store mode from config
    // -----------------------------------------------------------------------

    #[test]
    fn store_mode_from_config_defaults_to_auto() {
        assert_eq!(
            McpCredentialsStoreMode::from_config(None),
            McpCredentialsStoreMode::Auto
        );
        assert_eq!(
            McpCredentialsStoreMode::from_config(Some("auto")),
            McpCredentialsStoreMode::Auto
        );
    }

    #[test]
    fn store_mode_from_config_file() {
        assert_eq!(
            McpCredentialsStoreMode::from_config(Some("file")),
            McpCredentialsStoreMode::File
        );
    }

    #[test]
    fn store_mode_from_config_keyring() {
        assert_eq!(
            McpCredentialsStoreMode::from_config(Some("keyring")),
            McpCredentialsStoreMode::Keyring
        );
    }

    #[test]
    fn store_mode_from_config_unknown_falls_back_to_auto() {
        assert_eq!(
            McpCredentialsStoreMode::from_config(Some("invalid")),
            McpCredentialsStoreMode::Auto
        );
    }

    // -----------------------------------------------------------------------
    // File backend
    // -----------------------------------------------------------------------

    #[test]
    #[serial]
    fn save_and_load_token_file_round_trip() {
        let dir = tempfile::TempDir::new().unwrap();
        std::env::set_var("ALLTHECODES_HOME", dir.path());
        let key = sample_auth_key();
        let token = sample_token();

        save_token_file(&key, &token).unwrap();
        let loaded = load_token_file(&key).unwrap().expect("token should exist");
        assert_eq!(loaded.access_token, "test-access-token");
        assert_eq!(loaded.refresh_token, Some("test-refresh-token".to_string()));
        assert_eq!(loaded.scopes, vec!["tools.read", "tools.write"]);
    }

    #[test]
    #[serial]
    fn save_token_file_sets_0600_permissions() {
        let dir = tempfile::TempDir::new().unwrap();
        std::env::set_var("ALLTHECODES_HOME", dir.path());
        let key = sample_auth_key();
        let token = sample_token();

        save_token_file(&key, &token).unwrap();
        let path = token_store_path();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = std::fs::metadata(&path).unwrap();
            let mode = meta.permissions().mode();
            // Check that the file has 0600 (owner read+write only)
            assert_eq!(
                mode & 0o777,
                0o600,
                "expected 0600 permissions, got {:o}",
                mode
            );
        }
    }

    #[test]
    #[serial]
    fn delete_token_file_returns_true_when_exists() {
        let dir = tempfile::TempDir::new().unwrap();
        std::env::set_var("ALLTHECODES_HOME", dir.path());
        let key = sample_auth_key();
        let token = sample_token();

        save_token_file(&key, &token).unwrap();
        assert!(delete_token_file(&key).unwrap());
        assert!(load_token_file(&key).unwrap().is_none());
    }

    #[test]
    #[serial]
    fn delete_token_file_returns_false_when_missing() {
        let dir = tempfile::TempDir::new().unwrap();
        std::env::set_var("ALLTHECODES_HOME", dir.path());
        assert!(!delete_token_file("nonexistent|test|url").unwrap());
    }

    // -----------------------------------------------------------------------
    // Keyring backend
    // -----------------------------------------------------------------------

    #[test]
    #[serial]
    fn save_and_load_token_keyring_round_trip() {
        let _lock = KEYRING_LOCK.lock().unwrap();
        use_persistent_test_keyring();
        let key = sample_auth_key();
        let token = sample_token();

        save_token_keyring(&key, &token).unwrap();
        let loaded = load_token_keyring(&key)
            .unwrap()
            .expect("token should exist");
        assert_eq!(loaded.access_token, "test-access-token");
        assert_eq!(loaded.refresh_token, Some("test-refresh-token".to_string()));
        assert_eq!(loaded.scopes, vec!["tools.read", "tools.write"]);
    }

    #[test]
    #[serial]
    fn delete_token_keyring_returns_true_when_exists() {
        let _lock = KEYRING_LOCK.lock().unwrap();
        use_persistent_test_keyring();
        let key = sample_auth_key();
        let token = sample_token();

        save_token_keyring(&key, &token).unwrap();
        assert!(delete_token_keyring(&key).unwrap());
        assert!(load_token_keyring(&key).unwrap().is_none());
    }

    #[test]
    #[serial]
    fn delete_token_keyring_returns_false_when_missing() {
        let _lock = KEYRING_LOCK.lock().unwrap();
        use_persistent_test_keyring();
        assert!(!delete_token_keyring("nonexistent|test|url").unwrap());
    }

    #[test]
    fn keyring_account_is_stable() {
        let key = "my-server|sse|https://mcp.example.com/sse";
        let account1 = keyring_account(key);
        let account2 = keyring_account(key);
        assert_eq!(account1, account2);
        assert!(account1.starts_with("mcp-oauth:"));
        assert!(account1.len() > 20); // SHA-256 → base64url ≈ 43 chars
    }

    #[test]
    fn keyring_service_name_is_path_isolated() {
        assert_eq!(KEYRING_SERVICE_NAME, "allthecodes");
        assert_ne!(KEYRING_SERVICE_NAME, "Codex");
    }

    #[test]
    fn keyring_account_differs_for_different_keys() {
        let a = keyring_account("server-a");
        let b = keyring_account("server-b");
        assert_ne!(a, b);
    }

    // -----------------------------------------------------------------------
    // Auto backend
    // -----------------------------------------------------------------------

    #[test]
    #[serial]
    fn auto_mode_reads_keyring_first() {
        let _lock = KEYRING_LOCK.lock().unwrap();
        use_persistent_test_keyring();
        let dir = tempfile::TempDir::new().unwrap();
        std::env::set_var("ALLTHECODES_HOME", dir.path());

        let key = sample_auth_key();

        // Write different tokens to keyring and file
        let keyring_token = StoredMcpOAuthToken {
            access_token: "from-keyring".to_string(),
            ..sample_token()
        };
        let file_token = StoredMcpOAuthToken {
            access_token: "from-file".to_string(),
            ..sample_token()
        };

        save_token_keyring(&key, &keyring_token).unwrap();
        save_token_file(&key, &file_token).unwrap();

        // Auto mode should return keyring value
        let loaded = load_token_auto(&key).unwrap().expect("token should exist");
        assert_eq!(loaded.access_token, "from-keyring");
    }

    #[test]
    #[serial]
    fn auto_mode_falls_back_to_file_when_keyring_empty() {
        let _lock = KEYRING_LOCK.lock().unwrap();
        use_persistent_test_keyring();
        let dir = tempfile::TempDir::new().unwrap();
        std::env::set_var("ALLTHECODES_HOME", dir.path());

        let key = sample_auth_key();
        let token = sample_token();
        save_token_file(&key, &token).unwrap();

        // Keyring has no entry for this key → should read from file
        let loaded = load_token_auto(&key).unwrap().expect("token should exist");
        assert_eq!(loaded.access_token, "test-access-token");
    }

    #[test]
    #[serial]
    fn auto_mode_saves_to_keyring_without_file_copy() {
        let _lock = KEYRING_LOCK.lock().unwrap();
        use_persistent_test_keyring();
        let dir = tempfile::TempDir::new().unwrap();
        std::env::set_var("ALLTHECODES_HOME", dir.path());

        let key = sample_auth_key();
        let token = sample_token();

        save_token_auto(&key, &token).unwrap();

        let from_keyring = load_token_keyring(&key).unwrap();
        let from_file = load_token_file(&key).unwrap();
        assert!(from_keyring.is_some(), "should persist to keyring");
        assert!(
            from_file.is_none(),
            "auto mode should only write file when keyring fails"
        );
    }

    #[test]
    #[serial]
    fn auto_delete_removes_from_both() {
        let _lock = KEYRING_LOCK.lock().unwrap();
        use_persistent_test_keyring();
        let dir = tempfile::TempDir::new().unwrap();
        std::env::set_var("ALLTHECODES_HOME", dir.path());

        let key = sample_auth_key();
        let token = sample_token();
        save_token_auto(&key, &token).unwrap();

        assert!(delete_token_auto(&key).unwrap());
        assert!(load_token_auto(&key).unwrap().is_none());
    }
}
