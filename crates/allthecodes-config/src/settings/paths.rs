use std::path::{Path, PathBuf};

use anyhow::Result;

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

/// Global allthecodes data directory. Never fails — falls back to a temp dir.
pub fn global_claude_dir() -> Result<PathBuf> {
    Ok(crate::paths::data_root())
}

/// Path to the user-level settings file.
pub fn user_settings_path() -> PathBuf {
    crate::paths::data_root().join("settings.json")
}

/// Path to the effective project-level settings file for `cwd`.
///
/// If `cwd` is inside an existing `.allthecodes/` project hierarchy, this
/// resolves to the nearest ancestor settings file location. Otherwise it
/// points at `cwd/.allthecodes/settings.json`, which is also where a new file
/// would be created.
pub fn project_settings_path(cwd: &Path) -> PathBuf {
    find_project_dir(cwd)
        .unwrap_or_else(|| cwd.join(".allthecodes"))
        .join("settings.json")
}

/// Path to the effective local override settings file for `cwd`.
///
/// Mirrors [`project_settings_path`] but targets `settings.local.json`.
pub fn local_settings_path(cwd: &Path) -> PathBuf {
    find_project_dir(cwd)
        .unwrap_or_else(|| cwd.join(".allthecodes"))
        .join("settings.local.json")
}

/// Path to the managed/policy settings file, if one is configured.
///
/// Resolution:
///   1. `ALLTHECODES_MANAGED_SETTINGS` env (if non-empty).
///   2. Windows: `%ProgramData%\allthecodes\settings.json` (or
///      `C:\ProgramData\allthecodes\settings.json` if the env var is absent).
///   3. Other: `/etc/allthecodes/managed-settings.json`.
pub fn managed_settings_path() -> PathBuf {
    if let Ok(p) = std::env::var("ALLTHECODES_MANAGED_SETTINGS") {
        if !p.trim().is_empty() {
            return PathBuf::from(p);
        }
    }
    #[cfg(windows)]
    {
        let base = std::env::var_os("ProgramData")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("C:\\ProgramData"));
        base.join("allthecodes").join("settings.json")
    }
    #[cfg(not(windows))]
    {
        PathBuf::from("/etc/allthecodes/managed-settings.json")
    }
}

/// Return the path to the nearest ancestor project settings directory
/// (`.allthecodes/`, with a legacy project directory as a read-compatible fallback) or
/// `None`.
fn find_project_dir(cwd: &Path) -> Option<PathBuf> {
    let mut dir = cwd.to_path_buf();
    loop {
        let candidate = dir.join(".allthecodes");
        if candidate.is_dir() {
            return Some(candidate);
        }
        let legacy = dir.join(".cc-rust");
        if legacy.is_dir() {
            return Some(legacy);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Search `cwd` and its ancestors for `.allthecodes/settings.json`, then
/// legacy project settings.
pub(crate) fn find_project_config(cwd: &Path) -> Option<PathBuf> {
    let mut dir = cwd.to_path_buf();
    loop {
        let candidate = dir.join(".allthecodes").join("settings.json");
        if candidate.exists() {
            return Some(candidate);
        }
        let legacy = dir.join(".cc-rust").join("settings.json");
        if legacy.exists() {
            return Some(legacy);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Search for `.allthecodes/settings.local.json`, then legacy local project settings.
pub(crate) fn find_local_config(cwd: &Path) -> Option<PathBuf> {
    let mut dir = cwd.to_path_buf();
    loop {
        let candidate = dir.join(".allthecodes").join("settings.local.json");
        if candidate.exists() {
            return Some(candidate);
        }
        let legacy = dir.join(".cc-rust").join("settings.local.json");
        if legacy.exists() {
            return Some(legacy);
        }
        if !dir.pop() {
            return None;
        }
    }
}
