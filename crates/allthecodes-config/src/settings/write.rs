use std::path::{Path, PathBuf};

use allthecodes_types::kairos::{KairosConfigScope, KairosFeatureProfilePatch};
use anyhow::{Context, Result};

use super::paths::{local_settings_path, project_settings_path, user_settings_path};
use super::raw::RawSettings;
use super::types::KairosSettings;

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

/// Update only the KAIROS subtree in the requested settings layer.
///
/// Existing typed fields and unknown forward-compatible keys are retained.
pub fn update_kairos_settings(
    cwd: &Path,
    scope: KairosConfigScope,
    patch: &KairosFeatureProfilePatch,
) -> Result<PathBuf> {
    let path = match scope {
        KairosConfigScope::User => user_settings_path(),
        KairosConfigScope::Project => project_settings_path(cwd),
        KairosConfigScope::Local => local_settings_path(cwd),
    };
    let mut raw = if path.exists() {
        let contents = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        serde_json::from_str::<RawSettings>(&contents)
            .with_context(|| format!("Failed to parse {}", path.display()))?
    } else {
        RawSettings::default()
    };
    let mut kairos = raw.kairos.take().unwrap_or_else(KairosSettings::default);
    kairos.apply_patch(patch);
    raw.kairos = Some(kairos);
    write_settings_file(&path, &raw)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn kairos_update_preserves_existing_and_unknown_settings() {
        let root = tempfile::tempdir().unwrap();
        let cwd = root.path().join("workspace");
        std::fs::create_dir_all(cwd.join(".allthecodes")).unwrap();
        let path = local_settings_path(&cwd);
        std::fs::write(
            &path,
            serde_json::to_vec_pretty(&json!({
                "model": "existing-model",
                "futureSetting": { "enabled": true },
                "kairos": {
                    "enabled": true,
                    "brief": true,
                    "futureGate": "keep"
                }
            }))
            .unwrap(),
        )
        .unwrap();

        update_kairos_settings(
            &cwd,
            KairosConfigScope::Local,
            &KairosFeatureProfilePatch {
                brief: Some(false),
                channels: Some(true),
                ..KairosFeatureProfilePatch::default()
            },
        )
        .unwrap();

        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(value["model"], "existing-model");
        assert_eq!(value["futureSetting"]["enabled"], true);
        assert_eq!(value["kairos"]["enabled"], true);
        assert_eq!(value["kairos"]["brief"], false);
        assert_eq!(value["kairos"]["channels"], true);
        assert_eq!(value["kairos"]["futureGate"], "keep");
    }
}
