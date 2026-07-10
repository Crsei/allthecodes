//! OAuth token persistence.
//!
//! Stores OAuth tokens at allthecodes' path-isolated credentials file, resolved
//! through `cc-config`.

use anyhow::Result;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

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
    allthecodes_config::paths::ensure_data_root()?;
    save_token_to_path(&path, token)
}

fn save_token_to_path(path: &Path, token: &StoredToken) -> Result<()> {
    let content = serde_json::to_string_pretty(token)?;
    atomic_write(path, content.as_bytes(), replace_file)
}

#[cfg(not(windows))]
fn replace_file(temporary: &Path, destination: &Path) -> std::io::Result<()> {
    std::fs::rename(temporary, destination)
}

#[cfg(windows)]
fn replace_file(temporary: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    }
    let existing = temporary
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let new = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        MoveFileExW(
            existing.as_ptr(),
            new.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn atomic_write<F>(path: &Path, content: &[u8], replace: F) -> Result<()>
where
    F: FnOnce(&Path, &Path) -> std::io::Result<()>,
{
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("credentials path has no parent"))?;
    let sequence = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(
        ".credentials.json.tmp.{}.{}",
        std::process::id(),
        sequence
    ));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| -> Result<()> {
        let mut file = options.open(&temporary)?;
        file.write_all(content)?;
        file.sync_all()?;
        drop(file);
        replace(&temporary, path)?;
        allthecodes_config::paths::set_private_file_permissions(path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
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

    #[cfg(unix)]
    #[test]
    fn save_token_to_path_enforces_private_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("credentials.json");
        std::fs::write(&path, "old credentials").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();

        let token = StoredToken {
            access_token: "test-access".into(),
            refresh_token: Some("test-refresh".into()),
            expires_at: None,
            token_type: "bearer".into(),
            scopes: vec![],
            oauth_method: Some("claude_ai".into()),
        };

        save_token_to_path(&path, &token).unwrap();

        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let loaded: StoredToken =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(loaded.access_token, "test-access");
    }

    #[test]
    fn failed_atomic_replace_preserves_existing_credentials() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("credentials.json");
        std::fs::write(&path, b"old credentials").unwrap();

        let error = atomic_write(&path, b"new credentials", |_, _| {
            Err(std::io::Error::other("injected replace failure"))
        })
        .unwrap_err();

        assert!(error.to_string().contains("injected replace failure"));
        assert_eq!(std::fs::read(&path).unwrap(), b"old credentials");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

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
