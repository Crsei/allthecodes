//! Startup helpers — env loading, tracing setup, fast-path handlers,
//! runtime-config assembly, and non-interactive output modes.
//!
//! Entry point is [`main.rs`]; this module owns the pieces main would
//! otherwise inline. Split out of the monolithic `main.rs` per issue #22
//! to keep the entry point focused on orchestration.

pub mod engine_runtime;
pub mod fast_paths;
pub mod logging;
pub mod modes;
pub mod runtime_config;
pub mod tool_registry;

use allthecodes_config::settings;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct EnvLoadDiagnostic {
    pub path: PathBuf,
    pub error: String,
}

/// Load `.env` files in priority order (later loads do NOT override earlier):
///   1. `~/.allthecodes/.env`    (global user config)
///   2. `<exe-dir>/.env`         (portable, next to the binary)
///   3. `<cwd>/.env`             (project-local)
pub fn load_env_files() {
    for diagnostic in load_env_files_collect_diagnostics() {
        eprintln!(
            "warning: failed to load existing .env {}: {}",
            diagnostic.path.display(),
            diagnostic.error
        );
    }
}

fn load_existing_env(path: &Path) -> Option<EnvLoadDiagnostic> {
    if !path.exists() {
        return None;
    }
    dotenvy::from_path(path)
        .err()
        .map(|error| EnvLoadDiagnostic {
            path: path.to_path_buf(),
            error: error.to_string(),
        })
}

pub fn load_env_files_collect_diagnostics() -> Vec<EnvLoadDiagnostic> {
    let mut diagnostics = Vec::new();
    if let Ok(global_dir) = settings::global_claude_dir() {
        let global_env = global_dir.join(".env");
        if let Some(diagnostic) = load_existing_env(&global_env) {
            diagnostics.push(diagnostic);
        }
    }
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let exe_env = exe_dir.join(".env");
            if let Some(diagnostic) = load_existing_env(&exe_env) {
                diagnostics.push(diagnostic);
            }
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        let cwd_env = cwd.join(".env");
        if let Some(diagnostic) = load_existing_env(&cwd_env) {
            diagnostics.push(diagnostic);
        }
    }
    diagnostics
}

#[cfg(test)]
mod tests {
    use super::load_existing_env;

    #[test]
    fn startup_existing_invalid_env_returns_diagnostic() {
        let tmp = tempfile::TempDir::new().unwrap();
        let env_path = tmp.path().join(".env");
        std::fs::write(&env_path, "BROKEN=\"unterminated\n").unwrap();

        let diagnostic = load_existing_env(&env_path).expect("expected .env diagnostic");
        assert_eq!(diagnostic.path, env_path);
        assert!(!diagnostic.error.is_empty());
    }

    #[test]
    fn startup_absent_env_is_not_diagnostic() {
        let tmp = tempfile::TempDir::new().unwrap();
        assert!(load_existing_env(&tmp.path().join(".env")).is_none());
    }
}
