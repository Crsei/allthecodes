use std::path::{Path, PathBuf};

use allthecodes_tools::tool::{AdditionalWorkingDirectory, ToolPermissionContext};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScratchpadContext {
    pub path: PathBuf,
    pub prompt: String,
}

pub fn scratchpad_context(team_name: &str, session_id: &str) -> Option<ScratchpadContext> {
    if !scratchpad_enabled() {
        return None;
    }

    let name = format!(
        "{}-{}",
        sanitize_path_segment(team_name, "team"),
        sanitize_path_segment(session_id, "session")
    );
    let path = allthecodes_config::paths::data_root()
        .join("scratchpads")
        .join(name);
    if let Err(err) = std::fs::create_dir_all(&path) {
        tracing::warn!(
            path = %path.display(),
            error = %err,
            "failed to create coordinator scratchpad directory"
        );
        return None;
    }

    let prompt = format!(
        "Scratchpad path: {}\n\
         Use this directory only for durable cross-worker findings that another worker may need. \
         Do not use Scratchpad as a substitute for final synthesis.",
        path.display()
    );

    Some(ScratchpadContext { path, prompt })
}

pub fn apply_scratchpad_permissions(context: &mut ToolPermissionContext, path: &Path) {
    let path = path.to_string_lossy().to_string();
    context.additional_working_directories.insert(
        path.clone(),
        AdditionalWorkingDirectory {
            path,
            read_only: false,
        },
    );
}

fn scratchpad_enabled() -> bool {
    [
        "ALLTHECODES_COORDINATOR_SCRATCHPAD",
        "TENGU_SCRATCH",
        "CLAUDE_CODE_TENGU_SCRATCH",
    ]
    .iter()
    .any(|key| std::env::var(key).ok().is_some_and(|value| truthy(&value)))
}

fn truthy(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn sanitize_path_segment(value: &str, fallback: &str) -> String {
    let mut out = String::new();
    let mut last_was_separator = false;

    for ch in value.chars() {
        let next = if ch.is_ascii_alphanumeric() {
            last_was_separator = false;
            Some(ch.to_ascii_lowercase())
        } else if ch == '-' || ch == '_' {
            last_was_separator = true;
            Some(ch)
        } else if !last_was_separator {
            last_was_separator = true;
            Some('-')
        } else {
            None
        };

        if let Some(ch) = next {
            out.push(ch);
        }
    }

    let trimmed = out.trim_matches(['-', '_']).to_string();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var(key).ok();
            unsafe {
                std::env::set_var(key, value);
            }
            Self { key, previous }
        }

        fn set_path(key: &'static str, value: &std::path::Path) -> Self {
            Self::set(key, value.to_str().expect("utf-8 temp path"))
        }

        fn remove(key: &'static str) -> Self {
            let previous = std::env::var(key).ok();
            unsafe {
                std::env::remove_var(key);
            }
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            unsafe {
                match &self.previous {
                    Some(value) => std::env::set_var(self.key, value),
                    None => std::env::remove_var(self.key),
                }
            }
        }
    }

    #[test]
    #[serial_test::serial]
    fn scratchpad_disabled_by_default() {
        let _gate = EnvGuard::remove("ALLTHECODES_COORDINATOR_SCRATCHPAD");
        let _tengu = EnvGuard::remove("TENGU_SCRATCH");
        let _claude = EnvGuard::remove("CLAUDE_CODE_TENGU_SCRATCH");

        assert!(scratchpad_context("team-a", "session-a").is_none());
    }

    #[test]
    #[serial_test::serial]
    fn scratchpad_path_is_under_allthecodes_home() {
        let temp = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set_path("ALLTHECODES_HOME", temp.path());
        let _gate = EnvGuard::set("ALLTHECODES_COORDINATOR_SCRATCHPAD", "1");
        let _tengu = EnvGuard::remove("TENGU_SCRATCH");
        let _claude = EnvGuard::remove("CLAUDE_CODE_TENGU_SCRATCH");

        let ctx = scratchpad_context("Team A", "session-123").expect("scratchpad enabled");

        assert!(ctx.path.starts_with(temp.path()));
        assert!(ctx.path.ends_with("scratchpads/team-a-session-123"));
        assert!(ctx.path.is_dir());
        assert!(ctx.prompt.contains("Scratchpad"));
        assert!(ctx.prompt.contains(ctx.path.to_string_lossy().as_ref()));
    }

    #[test]
    #[serial_test::serial]
    fn scratchpad_accepts_legacy_gate_alias() {
        let temp = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set_path("ALLTHECODES_HOME", temp.path());
        let _gate = EnvGuard::remove("ALLTHECODES_COORDINATOR_SCRATCHPAD");
        let _tengu = EnvGuard::set("TENGU_SCRATCH", "true");
        let _claude = EnvGuard::remove("CLAUDE_CODE_TENGU_SCRATCH");

        assert!(scratchpad_context("team", "session").is_some());
    }

    #[test]
    #[serial_test::serial]
    fn scratchpad_accepts_claude_gate_alias() {
        let temp = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set_path("ALLTHECODES_HOME", temp.path());
        let _gate = EnvGuard::remove("ALLTHECODES_COORDINATOR_SCRATCHPAD");
        let _tengu = EnvGuard::remove("TENGU_SCRATCH");
        let _claude = EnvGuard::set("CLAUDE_CODE_TENGU_SCRATCH", "1");

        assert!(scratchpad_context("team", "session").is_some());
    }

    #[test]
    fn scratchpad_permission_adds_writable_additional_directory() {
        let mut context = allthecodes_tools::tool::ToolAppState::default().tool_permission_context;
        let path = std::path::Path::new("/tmp/allthecodes-scratchpad-test");

        apply_scratchpad_permissions(&mut context, path);

        let key = path.to_string_lossy().to_string();
        let dir = context
            .additional_working_directories
            .get(&key)
            .expect("scratchpad grant");
        assert_eq!(dir.path, key);
        assert!(!dir.read_only);
    }
}
