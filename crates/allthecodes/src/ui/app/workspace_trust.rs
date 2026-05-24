use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent};
use serde::{Deserialize, Serialize};

use super::{App, AppAction};
const TRUSTED_WORKSPACES_FILE: &str = "trusted-workspaces.json";

#[derive(Debug, Default, Deserialize, Serialize)]
struct TrustedWorkspaces {
    #[serde(default)]
    workspaces: BTreeSet<String>,
}

pub(super) fn trusted_workspaces_path() -> PathBuf {
    allthecodes_config::paths::data_root().join(TRUSTED_WORKSPACES_FILE)
}

fn normalize_workspace_path(path: &Path) -> String {
    let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut normalized = resolved.to_string_lossy().replace('\\', "/");
    while normalized.len() > 1 && normalized.ends_with('/') && !normalized.ends_with(":/") {
        normalized.pop();
    }
    if cfg!(windows) {
        normalized.make_ascii_lowercase();
    }
    normalized
}

fn load_trusted_workspaces() -> TrustedWorkspaces {
    let path = trusted_workspaces_path();
    let Ok(bytes) = std::fs::read(path) else {
        return TrustedWorkspaces::default();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

pub(super) fn is_workspace_trusted(cwd: &str) -> bool {
    if cwd.trim().is_empty() {
        return false;
    }
    let normalized = normalize_workspace_path(Path::new(cwd));
    load_trusted_workspaces().workspaces.contains(&normalized)
}

fn remember_trusted_workspace(cwd: &str) {
    if cwd.trim().is_empty() {
        return;
    }

    let mut trusted = load_trusted_workspaces();
    if !trusted
        .workspaces
        .insert(normalize_workspace_path(Path::new(cwd)))
    {
        return;
    }

    let path = trusted_workspaces_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(&trusted) {
        let _ = std::fs::write(path, bytes);
    }
}

impl App {
    pub(super) fn handle_workspace_trust_key(&mut self, key: KeyEvent) -> AppAction {
        match key.code {
            KeyCode::Char('1') => {
                self.accept_workspace_trust();
            }
            KeyCode::Char('2') | KeyCode::Esc => {
                self.should_quit = true;
                return AppAction::Quit;
            }
            KeyCode::Up => {
                self.workspace_trust_selection = 0;
            }
            KeyCode::Down | KeyCode::Tab => {
                self.workspace_trust_selection = 1;
            }
            KeyCode::Enter => {
                if self.workspace_trust_selection == 0 {
                    self.accept_workspace_trust();
                } else {
                    self.should_quit = true;
                    return AppAction::Quit;
                }
            }
            _ => {}
        }
        AppAction::None
    }

    pub(super) fn accept_workspace_trust(&mut self) {
        remember_trusted_workspace(&self.cwd);
        self.workspace_trust_pending = false;
        self.dirty = true;
    }
}
