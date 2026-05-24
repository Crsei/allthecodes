//! Worktree hook integration shared by user tools and Agent isolation.
//!
//! `WorktreeCreate` hooks may replace the default `git worktree add` path by
//! returning `updated_input.worktree_path` and, optionally,
//! `updated_input.branch_name` or `updated_input.branch`.
//! `WorktreeRemove` hooks replace default git cleanup only when they explicitly
//! return `updated_input.handled = true`, `updated_input.removed = true`, or a
//! `decision` of `handled` / `removed` / `skip_git`.

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use allthecodes_types::hooks::{HookOutput, HookRunner, HooksMap};
use anyhow::{bail, Context, Result};
use serde_json::json;

pub const WORKTREE_CREATE_EVENT: &str = "WorktreeCreate";
pub const WORKTREE_REMOVE_EVENT: &str = "WorktreeRemove";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeCreateHookResult {
    pub worktree_path: PathBuf,
    pub branch_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorktreeRemoveHookOutcome {
    NoHook,
    Handled,
    Unhandled,
}

pub fn default_user_worktree_path(slug: &str) -> PathBuf {
    crate::config::paths::worktrees_dir().join(format!("cc-worktree-{}", slug))
}

pub fn default_agent_worktree_path(short_id: &str) -> PathBuf {
    crate::config::paths::worktrees_dir().join(format!("agent-worktree-{}", short_id))
}

pub fn ensure_worktree_parent(worktree_path: &Path) -> Result<()> {
    validate_allowed_worktree_path(worktree_path)?;
    let Some(parent) = worktree_path.parent() else {
        bail!("worktree path has no parent: {}", worktree_path.display());
    };
    std::fs::create_dir_all(parent)
        .with_context(|| format!("failed to create worktree parent {}", parent.display()))
}

pub fn validate_allowed_worktree_path(worktree_path: &Path) -> Result<()> {
    validate_allowed_worktree_path_inner(worktree_path)
}

pub fn is_allowed_worktree_path(worktree_path: &Path) -> bool {
    validate_allowed_worktree_path(worktree_path).is_ok()
}

fn validate_allowed_worktree_path_inner(worktree_path: &Path) -> Result<()> {
    if has_parent_component(worktree_path) {
        bail!(
            "worktree path {} contains parent directory components",
            worktree_path.display()
        );
    }

    let path = absolute_path(worktree_path);
    let root = absolute_path(&crate::config::paths::worktrees_dir());
    if !path.starts_with(&root) {
        bail!(
            "worktree path {} is outside {}",
            worktree_path.display(),
            root.display()
        );
    }

    let resolved_root = resolve_existing_boundary(&root)
        .with_context(|| format!("failed to resolve worktree root {}", root.display()))?;
    let resolved_path = resolve_existing_boundary(&path)
        .with_context(|| format!("failed to resolve worktree path {}", path.display()))?;
    if !resolved_path.starts_with(&resolved_root) {
        bail!(
            "worktree path {} resolves outside {}",
            worktree_path.display(),
            root.display()
        );
    }

    Ok(())
}

pub fn parse_worktree_create_hook_output(
    output: &HookOutput,
    default_branch_name: &str,
) -> Result<Option<WorktreeCreateHookResult>> {
    if !output.should_continue {
        return Ok(None);
    }

    let Some(updated_input) = output.updated_input.as_ref() else {
        return Ok(None);
    };

    let Some(path_str) = string_field(updated_input, "worktree_path")
        .or_else(|| string_field(updated_input, "path"))
    else {
        return Ok(None);
    };

    let worktree_path = PathBuf::from(path_str);
    validate_allowed_worktree_path(&worktree_path)?;

    let branch_name = string_field(updated_input, "branch_name")
        .or_else(|| string_field(updated_input, "branch"))
        .unwrap_or(default_branch_name)
        .to_string();

    Ok(Some(WorktreeCreateHookResult {
        worktree_path,
        branch_name,
    }))
}

pub fn parse_worktree_remove_hook_output(output: &HookOutput) -> WorktreeRemoveHookOutcome {
    if !output.should_continue {
        return WorktreeRemoveHookOutcome::Unhandled;
    }

    if matches!(
        output.decision.as_deref(),
        Some("handled" | "removed" | "skip_git")
    ) {
        return WorktreeRemoveHookOutcome::Handled;
    }

    let Some(updated_input) = output.updated_input.as_ref() else {
        return WorktreeRemoveHookOutcome::Unhandled;
    };

    for key in ["handled", "removed", "skip_git"] {
        if updated_input
            .get(key)
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
        {
            return WorktreeRemoveHookOutcome::Handled;
        }
    }

    WorktreeRemoveHookOutcome::Unhandled
}

#[allow(clippy::too_many_arguments)]
pub async fn run_worktree_create_hook(
    hook_runner: &Arc<dyn HookRunner>,
    hooks: &HooksMap,
    source: &str,
    git_root: &Path,
    proposed_worktree_path: &Path,
    branch_name: &str,
    slug: &str,
    agent_id: Option<&str>,
) -> Result<Option<WorktreeCreateHookResult>> {
    let configs = hook_runner.load_hook_configs(hooks, WORKTREE_CREATE_EVENT);
    if configs.is_empty() {
        return Ok(None);
    }

    let payload = json!({
        "event": WORKTREE_CREATE_EVENT,
        "source": source,
        "slug": slug,
        "agent_id": agent_id,
        "git_root": git_root.display().to_string(),
        "proposed_worktree_path": proposed_worktree_path.display().to_string(),
        "branch_name": branch_name,
        "output_schema": {
            "updated_input": {
                "worktree_path": "absolute path under ALLTHECODES_HOME/worktrees",
                "branch_name": "optional branch name override"
            }
        }
    });

    let output = hook_runner
        .run_event_hooks(WORKTREE_CREATE_EVENT, &payload, &configs)
        .await?;
    parse_worktree_create_hook_output(&output, branch_name)
}

#[allow(clippy::too_many_arguments)]
pub async fn run_worktree_remove_hook(
    hook_runner: &Arc<dyn HookRunner>,
    hooks: &HooksMap,
    source: &str,
    git_root: &Path,
    worktree_path: &Path,
    branch_name: &str,
    agent_id: Option<&str>,
) -> Result<WorktreeRemoveHookOutcome> {
    let configs = hook_runner.load_hook_configs(hooks, WORKTREE_REMOVE_EVENT);
    if configs.is_empty() {
        return Ok(WorktreeRemoveHookOutcome::NoHook);
    }

    let payload = json!({
        "event": WORKTREE_REMOVE_EVENT,
        "source": source,
        "agent_id": agent_id,
        "git_root": git_root.display().to_string(),
        "worktree_path": worktree_path.display().to_string(),
        "branch_name": branch_name,
        "output_schema": {
            "updated_input": {
                "handled": "true when the hook removed the worktree",
                "removed": "alias for handled",
                "skip_git": "alias for handled"
            },
            "decision": "handled | removed | skip_git"
        }
    });

    let output = hook_runner
        .run_event_hooks(WORKTREE_REMOVE_EVENT, &payload, &configs)
        .await?;
    Ok(parse_worktree_remove_hook_output(&output))
}

fn string_field<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
}

fn absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

fn resolve_existing_boundary(path: &Path) -> Result<PathBuf> {
    let mut cursor = path;
    let mut suffix = Vec::<OsString>::new();

    loop {
        if cursor.exists() {
            let mut resolved = std::fs::canonicalize(cursor)
                .with_context(|| format!("failed to canonicalize {}", cursor.display()))?;
            for component in suffix.iter().rev() {
                resolved.push(component);
            }
            return Ok(resolved);
        }

        let Some(parent) = cursor.parent() else {
            bail!("path has no existing ancestor: {}", path.display());
        };
        let Some(file_name) = cursor.file_name() else {
            bail!("path has no existing ancestor: {}", path.display());
        };
        suffix.push(file_name.to_os_string());
        cursor = parent;
    }
}

fn has_parent_component(path: &Path) -> bool {
    path.components()
        .any(|component| matches!(component, Component::ParentDir))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    #[test]
    #[serial_test::serial]
    fn parse_create_output_accepts_allowed_path() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path().to_str().unwrap());
        let path = crate::config::paths::worktrees_dir().join("agent-worktree-test");
        let output = HookOutput {
            updated_input: Some(json!({
                "worktree_path": path.display().to_string(),
                "branch": "hook-branch",
            })),
            ..HookOutput::default()
        };

        let parsed = parse_worktree_create_hook_output(&output, "default")
            .unwrap()
            .unwrap();
        assert_eq!(parsed.worktree_path, path);
        assert_eq!(parsed.branch_name, "hook-branch");
    }

    #[test]
    #[serial_test::serial]
    fn parse_create_output_rejects_out_of_bounds_path() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path().to_str().unwrap());
        let output = HookOutput {
            updated_input: Some(json!({
                "worktree_path": std::env::temp_dir().join("outside-worktree").display().to_string(),
            })),
            ..HookOutput::default()
        };

        assert!(parse_worktree_create_hook_output(&output, "default").is_err());
    }

    #[test]
    #[serial_test::serial]
    fn allowed_path_accepts_nonexistent_child_under_worktrees_root() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path().to_str().unwrap());
        let path = crate::config::paths::worktrees_dir()
            .join("new-parent")
            .join("new-worktree");

        validate_allowed_worktree_path(&path).unwrap();
    }

    #[cfg(unix)]
    #[test]
    #[serial_test::serial]
    fn allowed_path_rejects_symlink_escape() {
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path().to_str().unwrap());
        let root = crate::config::paths::worktrees_dir();
        std::fs::create_dir_all(&root).unwrap();
        std::os::unix::fs::symlink(outside.path(), root.join("escape")).unwrap();

        let escaped = root.join("escape").join("victim");
        assert!(validate_allowed_worktree_path(&escaped).is_err());
    }

    #[cfg(windows)]
    #[test]
    #[serial_test::serial]
    fn allowed_path_rejects_junction_escape() {
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path().to_str().unwrap());
        let root = crate::config::paths::worktrees_dir();
        std::fs::create_dir_all(&root).unwrap();
        let junction = root.join("escape");
        let status = std::process::Command::new("cmd")
            .arg("/C")
            .arg("mklink")
            .arg("/J")
            .arg(&junction)
            .arg(outside.path())
            .status()
            .unwrap();
        assert!(status.success());

        let escaped = junction.join("victim");
        assert!(validate_allowed_worktree_path(&escaped).is_err());
    }

    #[test]
    fn parse_remove_output_accepts_handled_decision() {
        let output = HookOutput {
            decision: Some("handled".to_string()),
            ..HookOutput::default()
        };
        assert_eq!(
            parse_worktree_remove_hook_output(&output),
            WorktreeRemoveHookOutcome::Handled
        );
    }

    #[test]
    fn parse_remove_output_accepts_removed_flag() {
        let output = HookOutput {
            updated_input: Some(json!({ "removed": true })),
            ..HookOutput::default()
        };
        assert_eq!(
            parse_worktree_remove_hook_output(&output),
            WorktreeRemoveHookOutcome::Handled
        );
    }

    #[test]
    fn parse_remove_output_requires_explicit_handled_signal() {
        assert_eq!(
            parse_worktree_remove_hook_output(&HookOutput::default()),
            WorktreeRemoveHookOutcome::Unhandled
        );
    }
}
