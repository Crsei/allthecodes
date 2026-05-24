use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::paths::{local_settings_path, project_settings_path, user_settings_path};
use super::raw::RawSettings;

// ---------------------------------------------------------------------------
// Write + backup
// ---------------------------------------------------------------------------

/// Maximum number of backup copies to retain per settings file.
pub const MAX_SETTINGS_BACKUPS: usize = 5;

/// Serialise `raw` to JSON (pretty) with atomic write + rotating backup.
///
/// - Creates parent directories as needed.
/// - If `path` already exists, it's copied to `{path}.{timestamp}.bak`.
/// - Old backups beyond [`MAX_SETTINGS_BACKUPS`] are pruned.
pub fn write_settings_file(path: &Path, raw: &RawSettings) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory {}", parent.display()))?;
    }

    if path.exists() {
        let ts = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let bak = path.with_extension(format!("json.{}.bak", ts));
        if let Err(e) = std::fs::copy(path, &bak) {
            tracing::warn!(
                source = %path.display(),
                target = %bak.display(),
                error = %e,
                "failed to copy settings backup"
            );
        }
        prune_backups(path, MAX_SETTINGS_BACKUPS);
    }

    let pretty =
        serde_json::to_string_pretty(raw).context("Failed to serialize settings to JSON")?;

    // Atomic-ish: write to a unique tmp sibling, then rename. A fixed
    // `settings.json.tmp` name races when config tests mutate isolated homes in
    // parallel on Windows.
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let tmp = path.with_extension(format!("json.{}.{}.tmp", std::process::id(), nonce));
    std::fs::write(&tmp, pretty).with_context(|| format!("Failed to write {}", tmp.display()))?;
    std::fs::rename(&tmp, path)
        .with_context(|| format!("Failed to rename {} -> {}", tmp.display(), path.display()))?;

    Ok(())
}

/// Write to the user-level settings file.
pub fn write_user_settings(raw: &RawSettings) -> Result<PathBuf> {
    let path = user_settings_path();
    write_settings_file(&path, raw)?;
    Ok(path)
}

/// Write to `cwd/.allthecodes/settings.json`, creating the directory if needed.
pub fn write_project_settings(cwd: &Path, raw: &RawSettings) -> Result<PathBuf> {
    let path = project_settings_path(cwd);
    write_settings_file(&path, raw)?;
    Ok(path)
}

/// Write to `cwd/.allthecodes/settings.local.json`.
pub fn write_local_settings(cwd: &Path, raw: &RawSettings) -> Result<PathBuf> {
    let path = local_settings_path(cwd);
    write_settings_file(&path, raw)?;
    Ok(path)
}

fn prune_backups(path: &Path, keep: usize) {
    let Some(parent) = path.parent() else { return };
    let Some(stem) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
        return;
    };
    let prefix = format!("{}.", stem);

    let mut backups: Vec<PathBuf> = Vec::new();
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(&prefix) && name.ends_with(".bak") {
            backups.push(entry.path());
        }
    }
    // Sort newest-first by filename (our timestamp format is sortable).
    backups.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    for old in backups.into_iter().skip(keep) {
        let _ = std::fs::remove_file(old);
    }
}
