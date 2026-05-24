use std::path::{Path, PathBuf};

use super::CommandItem;

#[derive(Debug, Clone)]
pub(crate) struct EditTarget {
    pub(super) label: String,
    pub(super) display: String,
    pub(super) insert: String,
}

impl EditTarget {
    pub(super) fn new(label: &str, path: PathBuf, cwd: &Path, insert: &str) -> Self {
        let display = display_path(&path, cwd);
        Self {
            label: label.to_string(),
            display,
            insert: insert.to_string(),
        }
    }
}

pub(super) fn has_edit_target_picker(item: &CommandItem) -> bool {
    item.edit_targets
        .iter()
        .any(|target| !target.insert.is_empty())
}

pub(super) fn file_uri(path: &Path) -> String {
    let normalized = path.to_string_lossy().replace('\\', "/");
    let encoded = normalized.replace(' ', "%20");

    if encoded.starts_with('/') {
        format!("file://{encoded}")
    } else {
        format!("file:///{encoded}")
    }
}

pub(super) fn display_path(path: &Path, cwd: &Path) -> String {
    if !cwd.as_os_str().is_empty() && cwd != Path::new(".") {
        if let Ok(relative) = path.strip_prefix(cwd) {
            return format!("./{}", normalize_path(relative));
        }
    }

    if let Ok(current_dir) = std::env::current_dir() {
        if let Ok(relative) = path.strip_prefix(&current_dir) {
            return format!("./{}", normalize_path(relative));
        }
    }

    let data_root = allthecodes_config::paths::data_root();
    if let Ok(relative) = path.strip_prefix(&data_root) {
        let prefix = if std::env::var_os("ALLTHECODES_HOME").is_some() {
            "$ALLTHECODES_HOME"
        } else {
            "~/.allthecodes"
        };
        return format!("{}/{}", prefix, normalize_path(relative));
    }

    file_uri(path)
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
