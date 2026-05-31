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

/// Apply `settings.env` before tracing is initialized.
///
/// Langfuse is initialized as part of tracing setup, so environment values
/// that should affect Langfuse must be seeded before `init_tracing()`.
pub fn apply_settings_env_before_tracing(
    cwd: &Path,
) -> anyhow::Result<settings::RuntimeEnvApplyReport> {
    let loaded = settings::load_effective(cwd)?;
    settings::apply_runtime_env(&loaded.effective.env)
}

#[cfg(test)]
mod tests {
    use serial_test::serial;

    use super::{apply_settings_env_before_tracing, load_existing_env};

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set_value(key: &'static str, value: impl AsRef<str>) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value.as_ref());
            Self { key, previous }
        }

        fn unset(key: &'static str) -> Self {
            let previous = std::env::var(key).ok();
            std::env::remove_var(key);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match self.previous.as_ref() {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

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

    #[test]
    #[serial]
    fn settings_env_before_tracing_applies_missing_langfuse_env() {
        const KEY: &str = "LANGFUSE_PUBLIC_KEY";
        let tmp = tempfile::TempDir::new().unwrap();
        let _home = EnvGuard::set_value("ALLTHECODES_HOME", tmp.path().to_string_lossy());
        let _managed = EnvGuard::set_value(
            "ALLTHECODES_MANAGED_SETTINGS",
            tmp.path().join("missing-managed.json").to_string_lossy(),
        );
        let _key = EnvGuard::unset(KEY);
        std::fs::write(
            tmp.path().join("settings.json"),
            r#"{"env":{"LANGFUSE_PUBLIC_KEY":"pk-from-settings"}}"#,
        )
        .unwrap();

        let report = apply_settings_env_before_tracing(tmp.path()).unwrap();

        assert_eq!(report.applied, 1);
        assert_eq!(report.skipped, 0);
        assert_eq!(std::env::var(KEY).as_deref(), Ok("pk-from-settings"));
    }

    #[test]
    #[serial]
    fn settings_env_before_tracing_does_not_overwrite_process_env() {
        const KEY: &str = "LANGFUSE_SECRET_KEY";
        let tmp = tempfile::TempDir::new().unwrap();
        let _home = EnvGuard::set_value("ALLTHECODES_HOME", tmp.path().to_string_lossy());
        let _managed = EnvGuard::set_value(
            "ALLTHECODES_MANAGED_SETTINGS",
            tmp.path().join("missing-managed.json").to_string_lossy(),
        );
        let _key = EnvGuard::set_value(KEY, "sk-from-process");
        std::fs::write(
            tmp.path().join("settings.json"),
            r#"{"env":{"LANGFUSE_SECRET_KEY":"sk-from-settings"}}"#,
        )
        .unwrap();

        let report = apply_settings_env_before_tracing(tmp.path()).unwrap();

        assert_eq!(report.applied, 0);
        assert_eq!(report.skipped, 1);
        assert_eq!(std::env::var(KEY).as_deref(), Ok("sk-from-process"));
    }

    #[test]
    #[serial]
    fn settings_env_before_tracing_loads_project_settings_for_cwd() {
        const KEY: &str = "LANGFUSE_BASE_URL";
        let home = tempfile::TempDir::new().unwrap();
        let project = tempfile::TempDir::new().unwrap();
        std::fs::create_dir(project.path().join(".allthecodes")).unwrap();
        let _home = EnvGuard::set_value("ALLTHECODES_HOME", home.path().to_string_lossy());
        let _managed = EnvGuard::set_value(
            "ALLTHECODES_MANAGED_SETTINGS",
            home.path().join("missing-managed.json").to_string_lossy(),
        );
        let _key = EnvGuard::unset(KEY);
        std::fs::write(
            project.path().join(".allthecodes").join("settings.json"),
            r#"{"env":{"LANGFUSE_BASE_URL":"https://langfuse.example.com"}}"#,
        )
        .unwrap();

        let report = apply_settings_env_before_tracing(project.path()).unwrap();

        assert_eq!(report.applied, 1);
        assert_eq!(
            std::env::var(KEY).as_deref(),
            Ok("https://langfuse.example.com")
        );
    }

    #[test]
    #[serial]
    fn settings_env_before_tracing_reports_invalid_settings() {
        let tmp = tempfile::TempDir::new().unwrap();
        let _home = EnvGuard::set_value("ALLTHECODES_HOME", tmp.path().to_string_lossy());
        let _managed = EnvGuard::set_value(
            "ALLTHECODES_MANAGED_SETTINGS",
            tmp.path().join("missing-managed.json").to_string_lossy(),
        );
        std::fs::write(tmp.path().join("settings.json"), "{").unwrap();

        let error = apply_settings_env_before_tracing(tmp.path()).unwrap_err();

        assert!(error.to_string().contains("Failed to parse"));
    }
}
