//! Persistent UI metadata for workspace rows in the web sidebar.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct WorkspaceUiMetadata {
    pub workspace_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub updated_at: i64,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct WorkspaceUiMetadataPatch {
    #[serde(default)]
    pub display_name: Option<Option<String>>,
    #[serde(default)]
    pub pinned: Option<bool>,
    #[serde(default)]
    pub hidden: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct WorkspaceMetadataFile {
    #[serde(default)]
    workspaces: HashMap<String, WorkspaceUiMetadata>,
}

pub fn metadata_path() -> PathBuf {
    allthecodes_config::paths::data_root()
        .join("web")
        .join("workspaces.json")
}

pub fn load_metadata() -> Result<HashMap<String, WorkspaceUiMetadata>> {
    let path = metadata_path();
    if !path.exists() {
        return Ok(HashMap::new());
    }

    let contents = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read workspace metadata {}", path.display()))?;
    if contents.trim().is_empty() {
        return Ok(HashMap::new());
    }

    let file: WorkspaceMetadataFile = serde_json::from_str(&contents)
        .with_context(|| format!("Failed to parse workspace metadata {}", path.display()))?;
    Ok(file.workspaces)
}

pub fn update_metadata(
    workspace_key: &str,
    patch: WorkspaceUiMetadataPatch,
) -> Result<WorkspaceUiMetadata> {
    let mut all = load_metadata()?;
    let entry = all
        .entry(workspace_key.to_string())
        .or_insert_with(|| WorkspaceUiMetadata {
            workspace_key: workspace_key.to_string(),
            ..WorkspaceUiMetadata::default()
        });

    if let Some(display_name) = patch.display_name {
        entry.display_name = display_name.and_then(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        });
    }
    if let Some(pinned) = patch.pinned {
        entry.pinned = pinned;
    }
    if let Some(hidden) = patch.hidden {
        entry.hidden = hidden;
    }
    entry.updated_at = Utc::now().timestamp();

    let updated = entry.clone();
    write_metadata(&all)?;
    Ok(updated)
}

fn write_metadata(workspaces: &HashMap<String, WorkspaceUiMetadata>) -> Result<()> {
    let path = metadata_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory {}", parent.display()))?;
    }

    let file = WorkspaceMetadataFile {
        workspaces: workspaces.clone(),
    };
    let pretty =
        serde_json::to_string_pretty(&file).context("Failed to serialize workspace metadata")?;
    let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    std::fs::write(&tmp, pretty).with_context(|| format!("Failed to write {}", tmp.display()))?;
    std::fs::rename(&tmp, &path)
        .with_context(|| format!("Failed to rename {} -> {}", tmp.display(), path.display()))?;
    Ok(())
}
