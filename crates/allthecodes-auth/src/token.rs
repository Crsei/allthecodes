//! OAuth token persistence.
//!
//! Stores OAuth tokens at allthecodes' path-isolated credentials file, resolved
//! through `cc-config`.

use anyhow::Result;

/// Token storage file path.
pub fn token_file_path() -> std::path::PathBuf {
    crate::credentials_path()
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
