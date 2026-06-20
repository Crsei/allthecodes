//! Central registry of allthecodes runtime persistence paths.
//!
//! All runtime data — sessions, logs, credentials, transcripts — is resolved
//! through this module. The resolution chain is:
//!
//! 1. `ALLTHECODES_HOME` env var (trim non-empty) → use as-is.
//! 2. `dirs::home_dir().join(".allthecodes")`.
//! 3. `std::env::temp_dir().join("allthecodes")` (non-persistent, warns once).
//!
//! Functions return `PathBuf` unconditionally; they never fail. Creation of
//! the directory is the caller's responsibility.

use chrono::{DateTime, Local};
use std::path::Path;
use std::path::PathBuf;
use std::sync::Once;

static TEMP_FALLBACK_WARN: Once = Once::new();

/// Return the allthecodes data root directory.
///
/// See module-level docs for the resolution chain.
pub fn data_root() -> PathBuf {
    if let Ok(override_dir) = std::env::var("ALLTHECODES_HOME") {
        if !override_dir.trim().is_empty() {
            return PathBuf::from(override_dir);
        }
    }

    if let Some(home) = dirs::home_dir() {
        return home.join(".allthecodes");
    }

    let tmp = std::env::temp_dir().join("allthecodes");
    TEMP_FALLBACK_WARN.call_once(|| {
        tracing::warn!(
            path = %tmp.display(),
            "unable to resolve home directory; allthecodes data will be written to a \
             non-persistent temp location. Set ALLTHECODES_HOME to override."
        );
    });
    tmp
}

/// Create the data root directory if it does not exist.
///
/// Also creates the `logs/` subdirectory so that `init_tracing` has a
/// writable location from the first startup cycle.
///
/// Returns `Ok(true)` if the directory was newly created, `Ok(false)` if
/// it already existed.
pub fn ensure_data_root() -> std::io::Result<bool> {
    let root = data_root();
    if root.exists() {
        return Ok(false);
    }
    std::fs::create_dir_all(&root)?;
    let _ = std::fs::create_dir_all(root.join("logs"));
    Ok(true)
}

// ----- Global paths (under data_root) --------------------------------------

pub fn sessions_dir() -> PathBuf {
    data_root().join("sessions")
}

pub fn logs_dir() -> PathBuf {
    data_root().join("logs")
}

pub fn daemon_dir() -> PathBuf {
    data_root().join("daemon")
}

pub fn gateway_dir() -> PathBuf {
    data_root().join("gateway")
}

pub fn gateway_runs_dir() -> PathBuf {
    gateway_dir().join("runs")
}

pub fn gateway_adapters_dir() -> PathBuf {
    gateway_dir().join("adapters")
}

pub fn gateway_webhooks_dir() -> PathBuf {
    gateway_dir().join("webhooks")
}

/// `{data_root}/logs/YYYY/MM/YYYY-MM-DD.md` — daemon daily log layout.
pub fn daily_log_path(now: DateTime<Local>) -> PathBuf {
    logs_dir()
        .join(now.format("%Y").to_string())
        .join(now.format("%m").to_string())
        .join(now.format("%Y-%m-%d.md").to_string())
}

pub fn credentials_path() -> PathBuf {
    data_root().join("credentials.json")
}

/// `{data_root}/keybindings.json` — user keybinding overrides (issue #10).
///
/// Location matches the Claude Code spec convention of sitting next to
/// `settings.json`; on allthecodes that's inside `{data_root}` rather than
/// `~/.claude/` to preserve path isolation.
pub fn keybindings_path() -> PathBuf {
    data_root().join("keybindings.json")
}

pub fn runs_dir(session_id: &str) -> PathBuf {
    data_root().join("runs").join(session_id)
}

pub fn exports_dir() -> PathBuf {
    data_root().join("exports")
}

pub fn audits_dir() -> PathBuf {
    data_root().join("audits")
}

pub fn transcripts_dir() -> PathBuf {
    data_root().join("transcripts")
}

pub fn memory_dir_global() -> PathBuf {
    data_root().join("memory")
}

/// `{data_root}/auto_memory/` — auto-captured memories (issue #45).
///
/// Separated from the primary `memory/` directory so users can inspect
/// or purge auto-captured notes independently of curated entries.
/// Creation is deferred to `memdir::ensure_memory_dir` on first write.
pub fn auto_memory_dir() -> PathBuf {
    data_root().join("auto_memory")
}

pub fn session_insights_dir() -> PathBuf {
    data_root().join("session-insights")
}

pub fn plugins_dir() -> PathBuf {
    data_root().join("plugins")
}

pub fn skills_dir_global() -> PathBuf {
    data_root().join("skills")
}

pub fn skill_usage_path() -> PathBuf {
    data_root().join("skill-usage.json")
}

pub fn teams_dir() -> PathBuf {
    data_root().join("teams")
}

pub fn tasks_dir() -> PathBuf {
    data_root().join("tasks")
}

pub fn goals_dir() -> PathBuf {
    data_root().join("goals")
}

pub fn goal_file_path(session_id: &str) -> PathBuf {
    goals_dir().join(format!("{}.json", sanitize_path_segment(session_id)))
}

pub fn workflows_dir() -> PathBuf {
    data_root().join("workflows")
}

pub fn workflow_file_path(workflow_id: &str) -> PathBuf {
    workflows_dir().join(format!("{}.json", sanitize_path_segment(workflow_id)))
}

pub fn vault_dir() -> PathBuf {
    data_root().join("vault")
}

pub fn notifications_dir() -> PathBuf {
    data_root().join("notifications")
}

pub fn worktrees_dir() -> PathBuf {
    data_root().join("worktrees")
}

pub fn pr_activity_subscriptions_path() -> PathBuf {
    data_root().join("pr-activity-subscriptions.json")
}

/// `{data_root}/projects/{sanitized_cwd}/memory/team/`
pub fn team_memory_dir(cwd: &Path) -> PathBuf {
    let sanitized: String = cwd
        .to_string_lossy()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    data_root()
        .join("projects")
        .join(sanitized)
        .join("memory")
        .join("team")
}

// ----- Project-local paths (under cwd) -------------------------------------

/// `{cwd}/.allthecodes/` — project-level settings / memory / skills root.
///
pub fn project_allthecodes_dir(cwd: &Path) -> PathBuf {
    cwd.join(".allthecodes")
}

/// `{cwd}/.allthecodes/skills/` — project-local skill packages.
pub fn project_skills_dir(cwd: &Path) -> PathBuf {
    project_allthecodes_dir(cwd).join("skills")
}

// ----- Plan file (issue #46) -----------------------------------------------

/// `{cwd}/.allthecodes/plan.md` — project-scoped plan file.
pub fn plan_file_path_project(cwd: &Path) -> PathBuf {
    project_allthecodes_dir(cwd).join("plan.md")
}

/// `{data_root}/plan.md` — fallback global plan file used outside a project.
pub fn plan_file_path_global() -> PathBuf {
    data_root().join("plan.md")
}

/// `{cwd}/.allthecodes/plan-workflow.json` — project-scoped durable plan workflow.
pub fn plan_workflow_file_path_project(cwd: &Path) -> PathBuf {
    project_allthecodes_dir(cwd).join("plan-workflow.json")
}

/// `{data_root}/plan-workflow.json` — fallback global durable plan workflow.
pub fn plan_workflow_file_path_global() -> PathBuf {
    data_root().join("plan-workflow.json")
}

/// Resolve the plan file the current session should read/write.
///
/// Priority:
///   1. If `cwd` or an ancestor has a project `.allthecodes/`, `AGENTS.md`,
///      use that workspace's plan file.
///   2. Else use global `{data_root}/plan.md`.
pub fn current_plan_file_path(cwd: &Path) -> PathBuf {
    if let Some(root) = find_plan_project_root(cwd) {
        plan_file_path_project(&root)
    } else {
        plan_file_path_global()
    }
}

/// Resolve the workflow record path matching [`current_plan_file_path`].
pub fn current_plan_workflow_file_path(cwd: &Path) -> PathBuf {
    if let Some(root) = find_plan_project_root(cwd) {
        plan_workflow_file_path_project(&root)
    } else {
        plan_workflow_file_path_global()
    }
}

fn find_plan_project_root(cwd: &Path) -> Option<PathBuf> {
    cwd.ancestors()
        .find(|candidate| has_project_plan_marker(candidate))
        .map(Path::to_path_buf)
}

fn has_project_plan_marker(candidate: &Path) -> bool {
    if candidate.join("AGENTS.md").is_file() {
        return true;
    }
    if candidate.join("CLAUDE.md").is_file() {
        return true;
    }

    let marker = candidate.join(".allthecodes");
    marker.is_dir() && !is_global_data_dir(&marker)
}

fn is_global_data_dir(path: &Path) -> bool {
    path == data_root()
        || dirs::home_dir()
            .map(|home| path == home.join(".allthecodes"))
            .unwrap_or(false)
}

fn sanitize_path_segment(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if sanitized.trim_matches('_').is_empty() {
        "default".to_string()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use serial_test::serial;

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

        fn unset(key: &'static str) -> Self {
            let previous = std::env::var(key).ok();
            std::env::remove_var(key);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }

    #[test]
    #[serial]
    fn data_root_prefers_allthecodes_home_env() {
        let _g = EnvGuard::set("ALLTHECODES_HOME", "/tmp/allthecodes-test-override");
        assert_eq!(data_root(), PathBuf::from("/tmp/allthecodes-test-override"));
    }

    #[test]
    #[serial]
    fn data_root_ignores_empty_env() {
        let _g = EnvGuard::set("ALLTHECODES_HOME", "");
        let root = data_root();
        assert!(
            root.ends_with(".allthecodes")
                || root
                    .file_name()
                    .map(|n| n == "allthecodes")
                    .unwrap_or(false),
            "expected home fallback or temp fallback, got {}",
            root.display()
        );
    }

    #[test]
    #[serial]
    fn data_root_ignores_whitespace_env() {
        let _g = EnvGuard::set("ALLTHECODES_HOME", "   ");
        let root = data_root();
        assert!(
            root.ends_with(".allthecodes")
                || root
                    .file_name()
                    .map(|n| n == "allthecodes")
                    .unwrap_or(false),
            "expected home fallback or temp fallback, got {}",
            root.display()
        );
    }

    #[test]
    #[serial]
    fn data_root_respects_env_with_whitespace_padding() {
        let _g = EnvGuard::set("ALLTHECODES_HOME", " /tmp/padded ");
        assert_eq!(data_root(), PathBuf::from(" /tmp/padded "));
    }

    #[test]
    #[serial]
    fn data_root_ignores_legacy_cc_rust_home_env() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let legacy = tmp.path().join("legacy");
        let _allthecodes_home = EnvGuard::unset("ALLTHECODES_HOME");
        let _cc_rust_home = EnvGuard::set("CC_RUST_HOME", legacy.to_str().unwrap());
        let _home = EnvGuard::set("HOME", home.to_str().unwrap());

        assert_eq!(data_root(), home.join(".allthecodes"));
    }

    #[test]
    #[serial]
    fn data_root_temp_fallback_best_effort() {
        let _g = EnvGuard::unset("ALLTHECODES_HOME");
        if dirs::home_dir().is_some() {
            eprintln!("skipping: dirs::home_dir() still resolvable via OS APIs");
            return;
        }
        let root = data_root();
        assert!(
            root.starts_with(std::env::temp_dir()),
            "expected temp fallback, got {}",
            root.display()
        );
    }

    #[test]
    #[serial]
    fn ensure_data_root_creates_and_reports() {
        let tmp = tempfile::tempdir().unwrap();
        let fresh = tmp.path().join("fresh_root");
        let _g = EnvGuard::set("ALLTHECODES_HOME", fresh.to_str().unwrap());
        assert!(!data_root().exists());
        assert!(ensure_data_root().unwrap());
        assert!(data_root().exists());
        assert!(data_root().join("logs").exists());
        // Second call is idempotent
        assert!(!ensure_data_root().unwrap());
    }

    #[test]
    #[serial]
    fn partition_functions_all_root_under_data_root() {
        let _g = EnvGuard::set("ALLTHECODES_HOME", "/tmp/allthecodes-partition-test");
        let base = PathBuf::from("/tmp/allthecodes-partition-test");
        assert_eq!(sessions_dir(), base.join("sessions"));
        assert_eq!(logs_dir(), base.join("logs"));
        assert_eq!(credentials_path(), base.join("credentials.json"));
        assert_eq!(runs_dir("abc"), base.join("runs").join("abc"));
        assert_eq!(exports_dir(), base.join("exports"));
        assert_eq!(audits_dir(), base.join("audits"));
        assert_eq!(transcripts_dir(), base.join("transcripts"));
        assert_eq!(memory_dir_global(), base.join("memory"));
        assert_eq!(auto_memory_dir(), base.join("auto_memory"));
        assert_eq!(session_insights_dir(), base.join("session-insights"));
        assert_eq!(plugins_dir(), base.join("plugins"));
        assert_eq!(skills_dir_global(), base.join("skills"));
        assert_eq!(teams_dir(), base.join("teams"));
        assert_eq!(tasks_dir(), base.join("tasks"));
        assert_eq!(goals_dir(), base.join("goals"));
        assert_eq!(
            goal_file_path("session/1"),
            base.join("goals").join("session_1.json")
        );
        assert_eq!(workflows_dir(), base.join("workflows"));
        assert_eq!(
            workflow_file_path("workflow:1"),
            base.join("workflows").join("workflow_1.json")
        );
        assert_eq!(vault_dir(), base.join("vault"));
        assert_eq!(notifications_dir(), base.join("notifications"));
        assert_eq!(worktrees_dir(), base.join("worktrees"));
        assert_eq!(
            pr_activity_subscriptions_path(),
            base.join("pr-activity-subscriptions.json")
        );
        assert_eq!(daemon_dir(), base.join("daemon"));
        assert_eq!(gateway_dir(), base.join("gateway"));
        assert_eq!(gateway_runs_dir(), base.join("gateway").join("runs"));
        assert_eq!(
            gateway_adapters_dir(),
            base.join("gateway").join("adapters")
        );
        assert_eq!(
            gateway_webhooks_dir(),
            base.join("gateway").join("webhooks")
        );
    }

    #[test]
    #[serial]
    fn gateway_paths_are_isolated_under_allthecodes_home() {
        let _g = EnvGuard::set("ALLTHECODES_HOME", "/tmp/allthecodes-gateway-paths");
        assert_eq!(
            gateway_dir(),
            PathBuf::from("/tmp/allthecodes-gateway-paths/gateway")
        );
        assert_eq!(
            gateway_runs_dir(),
            PathBuf::from("/tmp/allthecodes-gateway-paths/gateway/runs")
        );
        assert_eq!(
            gateway_adapters_dir(),
            PathBuf::from("/tmp/allthecodes-gateway-paths/gateway/adapters")
        );
        assert_eq!(
            gateway_webhooks_dir(),
            PathBuf::from("/tmp/allthecodes-gateway-paths/gateway/webhooks")
        );
    }

    #[test]
    #[serial]
    fn daily_log_path_builds_yyyy_mm_dd_layout() {
        let _g = EnvGuard::set("ALLTHECODES_HOME", "/tmp/allthecodes-dlp");
        let dt = Local.with_ymd_and_hms(2026, 4, 18, 10, 30, 0).unwrap();
        let p = daily_log_path(dt);
        let s = p.to_string_lossy().replace('\\', "/");
        assert!(s.ends_with("logs/2026/04/2026-04-18.md"), "got {}", s);
    }

    #[test]
    #[serial]
    fn team_memory_dir_sanitizes_cwd() {
        let _g = EnvGuard::set("ALLTHECODES_HOME", "/tmp/allthecodes-tmd");
        let cwd = Path::new("/home/user/My Project/sub");
        let p = team_memory_dir(cwd);
        let s = p.to_string_lossy().replace('\\', "/");
        assert!(
            s.ends_with("_home_user_My_Project_sub/memory/team")
                || s.ends_with("/_home_user_My_Project_sub/memory/team"),
            "unexpected path: {}",
            s
        );
    }

    #[test]
    #[serial]
    fn project_allthecodes_dir_defaults_to_new_path() {
        let _g = EnvGuard::set("ALLTHECODES_HOME", "/tmp/ignored");
        assert_eq!(
            project_allthecodes_dir(Path::new("/foo/bar")),
            PathBuf::from("/foo/bar/.allthecodes")
        );
    }

    #[test]
    #[serial]
    fn project_allthecodes_dir_prefers_new_dir() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".allthecodes")).unwrap();
        assert_eq!(
            project_allthecodes_dir(tmp.path()),
            tmp.path().join(".allthecodes")
        );
    }

    #[test]
    #[serial]
    fn phase3_required_paths_are_allthecodes_isolated() {
        let _g = EnvGuard::set("ALLTHECODES_HOME", "/tmp/phase3-allthecodes-home");
        let cwd = Path::new("/repo/worktree");
        let data = PathBuf::from("/tmp/phase3-allthecodes-home");

        assert_eq!(data_root(), data);
        assert_eq!(
            crate::settings::user_settings_path(),
            PathBuf::from("/tmp/phase3-allthecodes-home/settings.json")
        );
        assert_eq!(
            crate::settings::project_settings_path(cwd),
            PathBuf::from("/repo/worktree/.allthecodes/settings.json")
        );
        assert_eq!(
            project_skills_dir(cwd),
            PathBuf::from("/repo/worktree/.allthecodes/skills")
        );
        assert_eq!(credentials_path(), data.join("credentials.json"));
        assert_eq!(daemon_dir(), data.join("daemon"));
    }

    // Plan file path helpers (issue #46) ----------------------------------

    #[test]
    #[serial]
    fn plan_file_path_project_defaults_to_allthecodes() {
        let _g = EnvGuard::set("ALLTHECODES_HOME", "/tmp/ignored");
        assert_eq!(
            plan_file_path_project(Path::new("/foo/bar")),
            PathBuf::from("/foo/bar/.allthecodes/plan.md")
        );
    }

    #[test]
    #[serial]
    fn plan_file_path_global_is_under_data_root() {
        let _g = EnvGuard::set("ALLTHECODES_HOME", "/tmp/allthecodes-plan-global");
        assert_eq!(
            plan_file_path_global(),
            PathBuf::from("/tmp/allthecodes-plan-global/plan.md")
        );
    }

    #[test]
    #[serial]
    fn plan_workflow_path_follows_current_plan_scope_with_allthecodes() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".allthecodes")).unwrap();
        let _g = EnvGuard::set("ALLTHECODES_HOME", "/tmp/ignored");
        assert_eq!(
            current_plan_workflow_file_path(tmp.path()),
            tmp.path().join(".allthecodes").join("plan-workflow.json")
        );
    }

    #[test]
    #[serial]
    fn current_plan_prefers_project_when_markers_present() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".allthecodes")).unwrap();
        let _g = EnvGuard::set("ALLTHECODES_HOME", "/tmp/should-not-be-used");
        assert_eq!(
            current_plan_file_path(tmp.path()),
            tmp.path().join(".allthecodes").join("plan.md")
        );
    }

    #[test]
    #[serial]
    fn current_plan_uses_workspace_root_from_nested_cwd() {
        let tmp = tempfile::tempdir().unwrap();
        let nested = tmp.path().join("a").join("b");
        std::fs::create_dir_all(tmp.path().join(".allthecodes")).unwrap();
        std::fs::create_dir_all(&nested).unwrap();
        let _g = EnvGuard::set("ALLTHECODES_HOME", "/tmp/should-not-be-used");
        assert_eq!(
            current_plan_file_path(&nested),
            tmp.path().join(".allthecodes").join("plan.md")
        );
        assert_eq!(
            current_plan_workflow_file_path(&nested),
            tmp.path().join(".allthecodes").join("plan-workflow.json")
        );
    }

    #[test]
    #[serial]
    fn current_plan_uses_agents_md_ancestor_as_workspace_root() {
        let tmp = tempfile::tempdir().unwrap();
        let nested = tmp.path().join("src").join("module");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(tmp.path().join("AGENTS.md"), "# instructions\n").unwrap();
        let _g = EnvGuard::set("ALLTHECODES_HOME", "/tmp/should-not-be-used");
        assert_eq!(
            current_plan_file_path(&nested),
            tmp.path().join(".allthecodes").join("plan.md")
        );
    }

    #[test]
    #[serial]
    fn current_plan_falls_back_to_global_without_markers() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let _g = EnvGuard::set("ALLTHECODES_HOME", home.path().to_str().unwrap());
        assert_eq!(
            current_plan_file_path(tmp.path()),
            home.path().join("plan.md")
        );
    }

    #[test]
    #[serial]
    fn current_plan_is_idempotent_once_plan_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let plan = tmp.path().join(".allthecodes").join("plan.md");
        std::fs::create_dir_all(plan.parent().unwrap()).unwrap();
        std::fs::write(&plan, "# Plan\n").unwrap();
        // Even without other markers, an existing plan.md sticks.
        let _g = EnvGuard::set("ALLTHECODES_HOME", "/tmp/unused");
        assert_eq!(current_plan_file_path(tmp.path()), plan);
    }
}
