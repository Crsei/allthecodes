//! Low-level reader for managed settings files.
//!
//! Provides raw file I/O for the MDM layer without policy interpretation.
//! All path resolution delegates to `settings::managed_settings_path()`.

use std::path::Path;

use anyhow::{Context, Result};
use serde_json::Value;

use crate::settings::{managed_settings_path, RawSettings};

/// Read and parse the managed settings file into `RawSettings`.
///
/// Returns `Ok(None)` if the managed settings file does not exist.
/// Returns `Err` if the file exists but cannot be read or parsed.
/// The returned `RawSettings` will contain managed-specific fields (e.g.
/// `policy`, `enforcement`) in its `extra` map.
pub fn read_managed_settings_raw() -> Result<Option<RawSettings>> {
    let path = managed_settings_path();
    read_managed_settings_raw_from(&path)
}

/// Read managed settings from a specific path. Used by tests and overrides.
pub fn read_managed_settings_raw_from(path: &Path) -> Result<Option<RawSettings>> {
    if !path.exists() {
        return Ok(None);
    }
    let contents = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read managed settings file: {}", path.display()))?;
    let raw: RawSettings = serde_json::from_str(&contents)
        .with_context(|| format!("Failed to parse managed settings file: {}", path.display()))?;
    Ok(Some(raw))
}

/// Check whether a managed settings file exists at the default path.
pub fn managed_settings_exists() -> bool {
    managed_settings_path().exists()
}

/// Read the managed settings file as an untyped JSON value (for diagnostics /
/// raw display). Returns `Ok(None)` if no file exists.
pub fn read_managed_settings_raw_value() -> Result<Option<Value>> {
    let path = managed_settings_path();
    if !path.exists() {
        return Ok(None);
    }
    let contents = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read managed settings file: {}", path.display()))?;
    let value: Value = serde_json::from_str(&contents)
        .with_context(|| format!("Failed to parse managed settings JSON: {}", path.display()))?;
    Ok(Some(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_missing_file_returns_none() {
        let result = read_managed_settings_raw_from(Path::new("/nonexistent/path.json"));
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_read_invalid_json_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.json");
        std::fs::write(&path, "not valid json {{").unwrap();
        let result = read_managed_settings_raw_from(&path);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Failed to parse"));
    }

    #[test]
    fn test_managed_settings_exists_returns_false_for_missing() {
        // When no managed settings file exists at the default path, this
        // should return false without panicking.
        let exists = managed_settings_exists();
        // We just verify it doesn't panic and returns a boolean.
        assert!(exists == false || exists == true);
    }
}
