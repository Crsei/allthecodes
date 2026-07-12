//! Worktree tools — EnterWorktree / ExitWorktree.
//!
//! Corresponds to TypeScript:
//!   src/tools/EnterWorktreeTool/EnterWorktreeTool.ts
//!   src/tools/ExitWorktreeTool/ExitWorktreeTool.ts
//!   src/utils/worktree.ts
//!
//! Creates an isolated git worktree for the agent to make changes in without
//! affecting the main working tree.  On exit the worktree can be kept
//! (branch + directory remain) or removed (cleaned up).
//!
//! Safety invariants:
//! - Cannot nest: only one worktree session at a time
//! - Fail-closed: if git status cannot be determined, refuse to remove
//! - Change detection: counts uncommitted files + new commits before removal
//! - Requires explicit `discard_changes: true` to remove with unsaved work

use parking_lot::Mutex;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use anyhow::{bail, Result};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::{debug, info, warn};

use allthecodes_engine::types::tool::*;
use allthecodes_engine::worktree_hooks::{
    default_user_worktree_path, ensure_worktree_parent, run_worktree_create_hook,
    run_worktree_remove_hook, validate_allowed_worktree_path, WorktreeRemoveHookOutcome,
};
use allthecodes_session::storage::workspace_key;
use allthecodes_session::worktree_sessions::{
    mark_worktree_session_cleanup_failed, mark_worktree_session_kept,
    mark_worktree_session_removed, upsert_worktree_session, WorktreeSessionCreator,
    WorktreeSessionRecord, WorktreeSessionSource,
};
use allthecodes_types::message::AssistantMessage;

// ---------------------------------------------------------------------------
// Worktree session state (process-global, single session)
// ---------------------------------------------------------------------------

/// Tracks the current worktree session.
#[derive(Debug, Clone)]
pub struct WorktreeSession {
    /// The path to the worktree directory.
    pub worktree_path: PathBuf,
    /// The branch name created for this worktree.
    pub branch_name: String,
    /// The original working directory before entering the worktree.
    pub original_cwd: PathBuf,
    /// The repository root resolved when the worktree was created.
    pub git_root: PathBuf,
    /// The HEAD commit SHA when the worktree was created.
    pub original_head_commit: Option<String>,
}

static CURRENT_SESSION: LazyLock<Mutex<Option<WorktreeSession>>> =
    LazyLock::new(|| Mutex::new(None));

/// Get the current worktree session (if any).
pub fn get_current_worktree_session() -> Option<WorktreeSession> {
    CURRENT_SESSION.lock().clone()
}

/// Set the current worktree session.
fn set_worktree_session(session: Option<WorktreeSession>) {
    *CURRENT_SESSION.lock() = session;
}

fn persist_worktree_session_record(
    ctx: &ToolUseContext,
    session: &WorktreeSession,
    source: WorktreeSessionSource,
) {
    let repo_id = workspace_key(&session.git_root);
    let record = WorktreeSessionRecord::new_active(
        ctx.session_id.clone(),
        repo_id,
        session.worktree_path.clone(),
        session.branch_name.clone(),
        session.original_head_commit.clone(),
        active_goal_id_for_session_best_effort(&ctx.session_id),
        WorktreeSessionCreator::Human,
        session.git_root.clone(),
        Some(session.original_cwd.clone()),
        source,
    );
    if let Err(err) = upsert_worktree_session(&record) {
        warn!(
            session_id = %ctx.session_id,
            worktree_path = %session.worktree_path.display(),
            error = %err,
            "failed to persist worktree session record"
        );
    }
}

fn active_goal_id_for_session_best_effort(session_id: &str) -> Option<String> {
    match allthecodes_tools::goals::active_goal_id_for_session(session_id) {
        Ok(goal_id) => goal_id,
        Err(err) => {
            warn!(
                session_id,
                error = %err,
                "failed to load active goal for worktree session"
            );
            None
        }
    }
}

fn mark_worktree_session_kept_best_effort(ctx: &ToolUseContext, worktree_path: &Path) {
    if let Err(err) = mark_worktree_session_kept(&ctx.session_id, worktree_path) {
        warn!(
            session_id = %ctx.session_id,
            worktree_path = %worktree_path.display(),
            error = %err,
            "failed to mark worktree session kept"
        );
    }
}

fn mark_worktree_session_removed_best_effort(ctx: &ToolUseContext, worktree_path: &Path) {
    if let Err(err) = mark_worktree_session_removed(&ctx.session_id, worktree_path) {
        warn!(
            session_id = %ctx.session_id,
            worktree_path = %worktree_path.display(),
            error = %err,
            "failed to mark worktree session removed"
        );
    }
}

fn mark_worktree_session_cleanup_failed_best_effort(ctx: &ToolUseContext, worktree_path: &Path) {
    if let Err(err) = mark_worktree_session_cleanup_failed(&ctx.session_id, worktree_path) {
        warn!(
            session_id = %ctx.session_id,
            worktree_path = %worktree_path.display(),
            error = %err,
            "failed to mark worktree session cleanup failed"
        );
    }
}

// ---------------------------------------------------------------------------
// Git helpers
// ---------------------------------------------------------------------------

/// Count uncommitted file changes and new commits in a worktree.
async fn count_worktree_changes(
    worktree_path: &Path,
    original_head: Option<&str>,
) -> Option<(usize, usize)> {
    let status = tokio::process::Command::new("git")
        .args([
            "-C",
            &worktree_path.to_string_lossy(),
            "status",
            "--porcelain",
        ])
        .output()
        .await
        .ok()?;

    if !status.status.success() {
        return None;
    }

    let changed_files = String::from_utf8_lossy(&status.stdout)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .count();

    let Some(orig_head) = original_head else {
        return Some((changed_files, 0));
    };

    let rev_list = tokio::process::Command::new("git")
        .args([
            "-C",
            &worktree_path.to_string_lossy(),
            "rev-list",
            "--count",
            &format!("{}..HEAD", orig_head),
        ])
        .output()
        .await
        .ok()?;

    if !rev_list.status.success() {
        return None;
    }

    let commits = String::from_utf8_lossy(&rev_list.stdout)
        .trim()
        .parse::<usize>()
        .unwrap_or(0);

    Some((changed_files, commits))
}

/// Get the current HEAD sha of the repository at `cwd`.
async fn get_head_sha(cwd: &Path) -> Option<String> {
    let output = tokio::process::Command::new("git")
        .args(["-C", &cwd.to_string_lossy(), "rev-parse", "HEAD"])
        .output()
        .await
        .ok()?;

    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

/// Find the canonical git root from a path.
async fn find_git_root(cwd: &Path) -> Option<PathBuf> {
    let output = tokio::process::Command::new("git")
        .args(["-C", &cwd.to_string_lossy(), "rev-parse", "--show-toplevel"])
        .output()
        .await
        .ok()?;

    if output.status.success() {
        let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Some(PathBuf::from(root))
    } else {
        None
    }
}

/// Validate a slug for path safety.
fn validate_slug(slug: &str) -> Result<()> {
    if slug.is_empty() {
        bail!("Worktree name cannot be empty");
    }
    if slug.contains("..") || slug.contains('/') || slug.contains('\\') {
        bail!("Worktree name cannot contain path separators or '..'");
    }
    if slug.len() > 64 {
        bail!("Worktree name too long (max 64 chars)");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// EnterWorktree
// ---------------------------------------------------------------------------

/// EnterWorktree — create a temporary git worktree for isolated changes.
pub struct EnterWorktreeTool;

#[derive(Deserialize)]
struct EnterWorktreeInput {
    /// Optional name/slug for the worktree and branch.
    name: Option<String>,
}

#[async_trait]
impl Tool for EnterWorktreeTool {
    fn name(&self) -> &str {
        "EnterWorktree"
    }

    async fn description(&self, _input: &Value) -> String {
        "Create a temporary git worktree for isolated changes.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Optional name for the worktree (used as branch suffix)"
                }
            },
            "additionalProperties": false
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        false // modifies global cwd state
    }

    async fn validate_input(&self, _input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        if get_current_worktree_session().is_some() {
            return ValidationResult::Error {
                message: "Already in a worktree session. Exit the current one first.".to_string(),
                error_code: 1,
            };
        }
        ValidationResult::Ok
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let params: EnterWorktreeInput = serde_json::from_value(input)?;

        let slug = params
            .name
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()[..8].to_string());
        validate_slug(&slug)?;

        let cwd = std::env::current_dir()?;
        let git_root = find_git_root(&cwd).await.unwrap_or_else(|| cwd.clone());
        let original_head = get_head_sha(&git_root).await;

        let worktree_path = default_user_worktree_path(&slug);
        let branch_name = format!("cc-worktree-{}", slug);

        info!(
            worktree_path = %worktree_path.display(),
            branch = %branch_name,
            "creating git worktree"
        );

        let app_state = (ctx.get_app_state)();
        let hook_created = match run_worktree_create_hook(
            &ctx.hook_runner,
            &app_state.hooks,
            "EnterWorktree",
            &git_root,
            &worktree_path,
            &branch_name,
            &slug,
            ctx.agent_id.as_deref(),
        )
        .await
        {
            Ok(Some(created)) if created.worktree_path.is_dir() => Some(created),
            Ok(Some(created)) => {
                warn!(
                    worktree_path = %created.worktree_path.display(),
                    "WorktreeCreate hook returned a missing directory; falling back to git"
                );
                None
            }
            Ok(None) => None,
            Err(err) => {
                warn!(
                    error = %err,
                    "WorktreeCreate hook failed; falling back to git"
                );
                None
            }
        };

        let (worktree_path, branch_name, created_by, record_source) =
            if let Some(created) = hook_created {
                (
                    created.worktree_path,
                    created.branch_name,
                    "WorktreeCreate hook",
                    WorktreeSessionSource::WorktreeCreateHook,
                )
            } else {
                ensure_worktree_parent(&worktree_path)?;

                let output = tokio::process::Command::new("git")
                    .args([
                        "-C",
                        &git_root.to_string_lossy(),
                        "worktree",
                        "add",
                        "-B",
                        &branch_name,
                        &worktree_path.to_string_lossy(),
                    ])
                    .output()
                    .await?;

                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    bail!("Failed to create worktree: {}", stderr);
                }

                (
                    worktree_path,
                    branch_name,
                    "git worktree",
                    WorktreeSessionSource::EnterWorktree,
                )
            };

        let session = WorktreeSession {
            worktree_path: worktree_path.clone(),
            branch_name: branch_name.clone(),
            original_cwd: cwd,
            git_root,
            original_head_commit: original_head,
        };
        persist_worktree_session_record(ctx, &session, record_source);
        set_worktree_session(Some(session));

        info!(
            worktree_path = %worktree_path.display(),
            branch = %branch_name,
            "worktree created successfully"
        );

        Ok(ToolResult {
            data: json!({
                "worktree_path": worktree_path.display().to_string(),
                "branch": branch_name,
                "created_by": created_by,
                "message": format!(
                    "Created worktree at {} on branch {}. \
                     Changes made here are isolated from the main working tree. \
                     Use ExitWorktree to keep or remove when done.",
                    worktree_path.display(),
                    branch_name,
                ),
            }),
            new_messages: vec![],
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        concat!(
            "Create an isolated git worktree to make changes without affecting ",
            "the main working tree. Optionally provide a name for the worktree. ",
            "Use ExitWorktree with action 'keep' or 'remove' when done.",
        )
        .to_string()
    }
}

// ---------------------------------------------------------------------------
// ExitWorktree
// ---------------------------------------------------------------------------

/// ExitWorktree — leave and optionally clean up a git worktree.
pub struct ExitWorktreeTool;

#[derive(Deserialize)]
struct ExitWorktreeInput {
    /// "keep" to leave worktree intact, "remove" to delete it.
    action: String,
    /// If true, force removal even with uncommitted changes.
    /// Checked in `validate_input` via raw JSON; kept here for schema completeness.
    #[serde(default, rename = "discard_changes")]
    _discard_changes: bool,
}

#[async_trait]
impl Tool for ExitWorktreeTool {
    fn name(&self) -> &str {
        "ExitWorktree"
    }

    async fn description(&self, _input: &Value) -> String {
        "Leave and clean up a git worktree.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["keep", "remove"],
                    "description": "'keep' to preserve the worktree, 'remove' to delete it"
                },
                "discard_changes": {
                    "type": "boolean",
                    "default": false,
                    "description": "If true, force removal even with uncommitted changes"
                }
            },
            "required": ["action"],
            "additionalProperties": false
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        false
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        let session = get_current_worktree_session();
        let Some(session) = session else {
            return ValidationResult::Error {
                message: "No active worktree session to exit.".to_string(),
                error_code: 1,
            };
        };

        let action = input.get("action").and_then(|v| v.as_str()).unwrap_or("");

        if action != "keep" && action != "remove" {
            return ValidationResult::Error {
                message: "action must be 'keep' or 'remove'.".to_string(),
                error_code: 3,
            };
        }

        // If removing, check for changes (fail-closed safety)
        let discard = input
            .get("discard_changes")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if action == "remove" && !discard {
            let changes = count_worktree_changes(
                &session.worktree_path,
                session.original_head_commit.as_deref(),
            )
            .await;

            match changes {
                None => {
                    return ValidationResult::Error {
                        message: concat!(
                            "Could not verify worktree state. ",
                            "Re-invoke with discard_changes: true to force removal, ",
                            "or use action: 'keep' to preserve the worktree.",
                        )
                        .to_string(),
                        error_code: 4,
                    };
                }
                Some((changed_files, commits)) if changed_files > 0 || commits > 0 => {
                    let mut parts = Vec::new();
                    if changed_files > 0 {
                        parts.push(format!("{} uncommitted file change(s)", changed_files));
                    }
                    if commits > 0 {
                        parts.push(format!("{} new commit(s)", commits));
                    }
                    return ValidationResult::Error {
                        message: format!(
                            "Worktree has {}. Removing will discard this work permanently. \
                             Confirm with the user, then re-invoke with discard_changes: true, \
                             or use action: 'keep' to preserve the worktree.",
                            parts.join(" and "),
                        ),
                        error_code: 2,
                    };
                }
                _ => {} // No changes — safe to remove
            }
        }

        ValidationResult::Ok
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let params: ExitWorktreeInput = serde_json::from_value(input)?;

        let session = get_current_worktree_session()
            .ok_or_else(|| anyhow::anyhow!("No active worktree session"))?;

        let worktree_path = session.worktree_path.clone();
        let branch_name = session.branch_name.clone();
        let git_root = session.git_root.clone();

        match params.action.as_str() {
            "keep" => {
                info!(
                    worktree_path = %worktree_path.display(),
                    branch = %branch_name,
                    "keeping worktree"
                );

                mark_worktree_session_kept_best_effort(ctx, &worktree_path);
                set_worktree_session(None);

                Ok(ToolResult {
                    data: json!({
                        "action": "keep",
                        "worktree_path": worktree_path.display().to_string(),
                        "branch": branch_name,
                        "message": format!(
                            "Worktree kept at {} on branch {}. \
                             You can return to it later or merge the branch.",
                            worktree_path.display(),
                            branch_name,
                        ),
                    }),
                    new_messages: vec![],
                    ..Default::default()
                })
            }
            "remove" => {
                info!(
                    worktree_path = %worktree_path.display(),
                    branch = %branch_name,
                    "removing worktree"
                );

                let mut warnings = Vec::new();

                if let Err(err) = validate_allowed_worktree_path(&worktree_path) {
                    warnings.push(format!(
                        "worktree removal refused: {}. Worktree session remains active.",
                        err
                    ));
                    mark_worktree_session_cleanup_failed_best_effort(ctx, &worktree_path);
                    return Ok(ToolResult {
                        data: json!({
                            "action": "remove",
                            "removed": false,
                            "worktree_path": worktree_path.display().to_string(),
                            "branch": branch_name,
                            "message": "Worktree was not removed because its path is outside the cc-rust worktree root.",
                            "warnings": warnings,
                        }),
                        new_messages: vec![],
                        ..Default::default()
                    });
                }

                let app_state = (ctx.get_app_state)();
                let hook_outcome = match run_worktree_remove_hook(
                    &ctx.hook_runner,
                    &app_state.hooks,
                    "ExitWorktree",
                    &git_root,
                    &worktree_path,
                    &branch_name,
                    ctx.agent_id.as_deref(),
                )
                .await
                {
                    Ok(outcome) => outcome,
                    Err(err) => {
                        warnings.push(format!(
                            "WorktreeRemove hook failed: {}. Worktree session remains active.",
                            err
                        ));
                        WorktreeRemoveHookOutcome::Unhandled
                    }
                };

                let mut removed_by = None;

                match hook_outcome {
                    WorktreeRemoveHookOutcome::NoHook => {
                        let remove_result = tokio::process::Command::new("git")
                            .args([
                                "-C",
                                &git_root.to_string_lossy(),
                                "worktree",
                                "remove",
                                "--force",
                                &worktree_path.to_string_lossy(),
                            ])
                            .output()
                            .await;

                        match remove_result {
                            Ok(o) if o.status.success() => {
                                debug!("worktree directory removed");
                                removed_by = Some("git worktree");
                            }
                            Ok(o) => {
                                let stderr = String::from_utf8_lossy(&o.stderr);
                                warn!("git worktree remove warning: {}", stderr);
                                warnings
                                    .push(format!("worktree remove warning: {}", stderr.trim()));
                            }
                            Err(e) => {
                                warn!("git worktree remove failed: {}", e);
                                warnings.push(format!("worktree remove failed: {}", e));
                            }
                        }

                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

                        if !worktree_path.exists() {
                            let branch_result = tokio::process::Command::new("git")
                                .args([
                                    "-C",
                                    &git_root.to_string_lossy(),
                                    "branch",
                                    "-D",
                                    &branch_name,
                                ])
                                .output()
                                .await;

                            match branch_result {
                                Ok(o) if o.status.success() => {
                                    debug!("worktree branch deleted");
                                }
                                Ok(o) => {
                                    let stderr = String::from_utf8_lossy(&o.stderr);
                                    warn!("branch delete warning: {}", stderr);
                                    warnings
                                        .push(format!("branch delete warning: {}", stderr.trim()));
                                }
                                Err(e) => {
                                    warn!("branch delete failed: {}", e);
                                    warnings.push(format!("branch delete failed: {}", e));
                                }
                            }
                        }
                    }
                    WorktreeRemoveHookOutcome::Handled => {
                        removed_by = Some("WorktreeRemove hook");
                    }
                    WorktreeRemoveHookOutcome::Unhandled => {
                        if warnings.is_empty() {
                            warnings.push(
                                "WorktreeRemove hook did not explicitly report removal. \
                                 Worktree session remains active."
                                    .to_string(),
                            );
                        }
                    }
                }

                if worktree_path.exists() {
                    warnings.push(
                        "Worktree removal could not be verified; session remains active."
                            .to_string(),
                    );
                    mark_worktree_session_cleanup_failed_best_effort(ctx, &worktree_path);
                    return Ok(ToolResult {
                        data: json!({
                            "action": "remove",
                            "removed": false,
                            "worktree_path": worktree_path.display().to_string(),
                            "branch": branch_name,
                            "message": "Worktree kept because removal could not be verified.",
                            "warnings": warnings,
                        }),
                        new_messages: vec![],
                        ..Default::default()
                    });
                }

                let mut result = json!({
                    "action": "remove",
                    "removed": true,
                    "removed_by": removed_by.unwrap_or("unknown"),
                    "message": "Worktree removed.",
                });
                if !warnings.is_empty() {
                    result["warnings"] = json!(warnings);
                }

                mark_worktree_session_removed_best_effort(ctx, &worktree_path);
                set_worktree_session(None);

                Ok(ToolResult {
                    data: result,
                    new_messages: vec![],
                    ..Default::default()
                })
            }
            other => bail!("Unknown action: {}. Use 'keep' or 'remove'.", other),
        }
    }

    async fn prompt(&self) -> String {
        concat!(
            "Exit an active git worktree session. Use action 'keep' to preserve ",
            "the worktree and branch, or 'remove' to clean them up. ",
            "If removing with unsaved changes, you must set discard_changes: true.",
        )
        .to_string()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_engine::types::tool::ToolAppState;
    use allthecodes_engine::worktree_hooks::{WORKTREE_CREATE_EVENT, WORKTREE_REMOVE_EVENT};
    use allthecodes_session::storage::workspace_key;
    use allthecodes_session::worktree_sessions::{
        get_active_worktree_session, list_worktree_sessions_for_repo, WorktreeSessionSource,
        WorktreeSessionStatus,
    };
    use allthecodes_types::hooks::{
        HookEventConfig, HookOutput, HookRunner, HooksMap, NoopHookRunner,
    };
    use async_trait::async_trait;
    use parking_lot::RwLock;
    use serde_json::{json, Value};
    use serial_test::serial;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::Arc;
    use tempfile::TempDir;

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

    struct CurrentDirGuard {
        previous: PathBuf,
    }

    impl CurrentDirGuard {
        fn set(path: &Path) -> Self {
            let previous = std::env::current_dir().expect("read current dir");
            std::env::set_current_dir(path).expect("set current dir");
            Self { previous }
        }
    }

    impl Drop for CurrentDirGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.previous);
        }
    }

    struct WorktreeSessionGuard;

    impl Drop for WorktreeSessionGuard {
        fn drop(&mut self) {
            set_worktree_session(None);
        }
    }

    #[derive(Default)]
    struct Phase6HookRunner;

    #[async_trait]
    impl HookRunner for Phase6HookRunner {
        fn load_hook_configs(
            &self,
            hooks_value: &HooksMap,
            event_name: &str,
        ) -> Vec<HookEventConfig> {
            hooks_value
                .get(event_name)
                .and_then(|value| serde_json::from_value(value.clone()).ok())
                .unwrap_or_default()
        }

        async fn run_pre_tool_hooks(
            &self,
            _tool_name: &str,
            _input: &Value,
            _hook_configs: &[HookEventConfig],
        ) -> anyhow::Result<allthecodes_types::hooks::PreToolHookResult> {
            Ok(allthecodes_types::hooks::PreToolHookResult::Continue {
                updated_input: None,
                permission_override: None,
            })
        }

        async fn run_post_tool_hooks(
            &self,
            _tool_name: &str,
            _input: &Value,
            _tool_result_data: &Value,
            _hook_configs: &[HookEventConfig],
        ) -> anyhow::Result<allthecodes_types::hooks::PostToolHookResult> {
            Ok(allthecodes_types::hooks::PostToolHookResult::Continue)
        }

        async fn run_post_tool_failure_hooks(
            &self,
            _tool_name: &str,
            _input: &Value,
            _error: &str,
            _hook_configs: &[HookEventConfig],
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn run_event_hooks(
            &self,
            event_name: &str,
            payload: &Value,
            _hook_configs: &[HookEventConfig],
        ) -> anyhow::Result<HookOutput> {
            match event_name {
                WORKTREE_CREATE_EVENT => {
                    let worktree_path = PathBuf::from(
                        payload
                            .get("proposed_worktree_path")
                            .and_then(Value::as_str)
                            .unwrap_or(""),
                    );
                    fs::create_dir_all(&worktree_path)?;
                    Ok(HookOutput {
                        updated_input: Some(json!({
                            "worktree_path": worktree_path.display().to_string(),
                            "branch_name": "hook-branch",
                        })),
                        ..HookOutput::default()
                    })
                }
                WORKTREE_REMOVE_EVENT => {
                    let worktree_path = PathBuf::from(
                        payload
                            .get("worktree_path")
                            .and_then(Value::as_str)
                            .unwrap_or(""),
                    );
                    if worktree_path.exists() {
                        fs::remove_dir_all(&worktree_path)?;
                    }
                    Ok(HookOutput {
                        decision: Some("handled".to_string()),
                        updated_input: Some(json!({
                            "handled": true,
                            "removed": true,
                        })),
                        ..HookOutput::default()
                    })
                }
                _ => Ok(HookOutput::default()),
            }
        }

        async fn run_stop_hooks(
            &self,
            _hook_configs: &[HookEventConfig],
        ) -> anyhow::Result<allthecodes_types::hooks::PostToolHookResult> {
            Ok(allthecodes_types::hooks::PostToolHookResult::Continue)
        }
    }

    fn make_ctx() -> ToolUseContext {
        make_ctx_with(ToolAppState::default(), Arc::new(NoopHookRunner::new()))
    }

    fn make_ctx_with(app_state: ToolAppState, hook_runner: Arc<dyn HookRunner>) -> ToolUseContext {
        let state = Arc::new(RwLock::new(ToolAppState::default()));
        let state_r = Arc::clone(&state);
        let state_w = Arc::clone(&state);
        *state.write() = app_state;

        ToolUseContext {
            cwd: ".".to_string(),
            options: ToolUseOptions {
                debug: false,
                main_loop_model: "test".to_string(),
                verbose: false,
                is_non_interactive_session: false,
                custom_system_prompt: None,
                append_system_prompt: None,
                max_budget_usd: None,
            },
            abort_signal: tokio::sync::watch::channel(false).1,
            read_file_state: FileStateCache::default(),
            get_app_state: Arc::new(move || state_r.read().clone()),
            set_app_state: Arc::new(move |f: Box<dyn FnOnce(ToolAppState) -> ToolAppState>| {
                let mut s = state_w.write();
                let old = s.clone();
                *s = f(old);
            }),
            session_id: "test-session".to_string(),
            langfuse_session_id: "test-session".to_string(),
            messages: vec![],
            agent_id: None,
            agent_type: None,
            query_tracking: None,
            permission_callback: None,
            permission_event_callback: None,
            ask_user_callback: None,
            bg_agent_tx: None,
            hook_runner,
            command_dispatcher: Arc::new(allthecodes_types::commands::NoopCommandDispatcher::new()),
            available_tools: vec![],
            execute_deferred_tool: None,
            taint_context: Default::default(),
        }
    }

    fn parent_message() -> AssistantMessage {
        AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 0,
            role: "assistant".to_string(),
            content: vec![],
            usage: None,
            stop_reason: None,
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        }
    }

    fn run_git(repo: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .expect("run git command");
        assert!(
            output.status.success(),
            "git command failed: {:?}\nstderr: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn init_git_repo(repo: &Path) {
        fs::create_dir_all(repo).expect("create repo dir");
        run_git(repo, &["init"]);
        run_git(repo, &["config", "user.email", "phase6@example.com"]);
        run_git(repo, &["config", "user.name", "Phase Six"]);
        fs::write(repo.join("README.md"), "phase 6\n").expect("write repo file");
        run_git(repo, &["add", "README.md"]);
        run_git(repo, &["commit", "-m", "initial commit"]);
    }

    #[test]
    fn test_enter_worktree_name() {
        let tool = EnterWorktreeTool;
        assert_eq!(tool.name(), "EnterWorktree");
    }

    #[test]
    fn test_exit_worktree_name() {
        let tool = ExitWorktreeTool;
        assert_eq!(tool.name(), "ExitWorktree");
    }

    #[test]
    fn test_validate_slug() {
        assert!(validate_slug("my-feature").is_ok());
        assert!(validate_slug("fix_123").is_ok());
        assert!(validate_slug("").is_err());
        assert!(validate_slug("../escape").is_err());
        assert!(validate_slug("path/traversal").is_err());
        assert!(validate_slug("back\\slash").is_err());
        assert!(validate_slug(&"x".repeat(65)).is_err());
    }

    #[test]
    fn test_enter_worktree_schema() {
        let tool = EnterWorktreeTool;
        let schema = tool.input_json_schema();
        assert!(schema["properties"].get("name").is_some());
    }

    #[test]
    fn test_exit_worktree_schema() {
        let tool = ExitWorktreeTool;
        let schema = tool.input_json_schema();
        let props = schema["properties"].as_object().unwrap();
        assert!(props.contains_key("action"));
        assert!(props.contains_key("discard_changes"));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_exit_worktree_no_session() {
        set_worktree_session(None);

        let tool = ExitWorktreeTool;
        let ctx = make_ctx();
        let result = tool.validate_input(&json!({"action": "keep"}), &ctx).await;
        assert!(matches!(
            result,
            ValidationResult::Error { error_code: 1, .. }
        ));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_exit_worktree_invalid_action() {
        set_worktree_session(Some(WorktreeSession {
            worktree_path: PathBuf::from("/tmp/test"),
            branch_name: "test-branch".to_string(),
            original_cwd: PathBuf::from("/tmp"),
            git_root: PathBuf::from("/tmp"),
            original_head_commit: None,
        }));

        let tool = ExitWorktreeTool;
        let ctx = make_ctx();
        let result = tool
            .validate_input(&json!({"action": "invalid"}), &ctx)
            .await;
        assert!(matches!(
            result,
            ValidationResult::Error { error_code: 3, .. }
        ));

        set_worktree_session(None);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_exit_worktree_remove_fails_closed_when_state_cannot_be_verified() {
        let tmp = tempfile::tempdir().unwrap();
        set_worktree_session(Some(WorktreeSession {
            worktree_path: tmp.path().join("missing-worktree"),
            branch_name: "test-branch".to_string(),
            original_cwd: tmp.path().to_path_buf(),
            git_root: tmp.path().to_path_buf(),
            original_head_commit: Some("abc123".to_string()),
        }));

        let tool = ExitWorktreeTool;
        let ctx = make_ctx();
        let result = tool
            .validate_input(&json!({"action": "remove"}), &ctx)
            .await;
        match result {
            ValidationResult::Error {
                message,
                error_code,
            } => {
                assert_eq!(error_code, 4);
                assert!(message.contains("Could not verify worktree state"));
                assert!(message.contains("discard_changes: true"));
            }
            other => panic!("expected fail-closed validation error, got {other:?}"),
        }

        set_worktree_session(None);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_exit_worktree_remove_refuses_out_of_bounds_path() {
        let tmp = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", tmp.path().to_str().unwrap());
        let ctx = make_ctx();
        let session = WorktreeSession {
            worktree_path: tmp.path().join("outside-worktree"),
            branch_name: "test-branch".to_string(),
            original_cwd: tmp.path().to_path_buf(),
            git_root: tmp.path().to_path_buf(),
            original_head_commit: None,
        };
        persist_worktree_session_record(&ctx, &session, WorktreeSessionSource::EnterWorktree);
        set_worktree_session(Some(session));

        let parent = AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 0,
            role: "assistant".to_string(),
            content: vec![],
            usage: None,
            stop_reason: None,
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        };
        let tool = ExitWorktreeTool;
        let result = tool
            .call(
                json!({"action": "remove", "discard_changes": true}),
                &ctx,
                &parent,
                None,
            )
            .await
            .unwrap();

        assert_eq!(result.data["removed"], false);
        assert!(result.data["message"]
            .as_str()
            .unwrap()
            .contains("outside the cc-rust worktree root"));
        assert!(get_current_worktree_session().is_some());
        let active = get_active_worktree_session("test-session")
            .unwrap()
            .expect("cleanup_failed remains active");
        assert_eq!(active.status, WorktreeSessionStatus::CleanupFailed);

        set_worktree_session(None);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_exit_worktree_keep_marks_session_kept() {
        let tmp = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", tmp.path().to_str().unwrap());
        let ctx = make_ctx();
        let session = WorktreeSession {
            worktree_path: tmp.path().join("kept-worktree"),
            branch_name: "test-branch".to_string(),
            original_cwd: tmp.path().to_path_buf(),
            git_root: tmp.path().to_path_buf(),
            original_head_commit: None,
        };
        persist_worktree_session_record(&ctx, &session, WorktreeSessionSource::EnterWorktree);
        set_worktree_session(Some(session));

        let tool = ExitWorktreeTool;
        let result = tool
            .call(json!({"action": "keep"}), &ctx, &parent_message(), None)
            .await
            .unwrap();

        assert_eq!(result.data["action"], "keep");
        assert!(get_current_worktree_session().is_none());
        assert!(get_active_worktree_session("test-session")
            .unwrap()
            .is_none());
        let records = list_worktree_sessions_for_repo(&workspace_key(tmp.path())).unwrap();
        assert_eq!(records[0].status, WorktreeSessionStatus::Kept);
    }

    #[test]
    #[serial_test::serial]
    fn test_worktree_session_lifecycle() {
        set_worktree_session(None);
        assert!(get_current_worktree_session().is_none());

        let session = WorktreeSession {
            worktree_path: PathBuf::from("/tmp/wt"),
            branch_name: "wt-branch".to_string(),
            original_cwd: PathBuf::from("/project"),
            git_root: PathBuf::from("/project"),
            original_head_commit: Some("abc123".to_string()),
        };
        set_worktree_session(Some(session));
        let current = get_current_worktree_session().unwrap();
        assert_eq!(current.branch_name, "wt-branch");
        assert_eq!(current.git_root, PathBuf::from("/project"));
        assert_eq!(current.original_head_commit.as_deref(), Some("abc123"));

        set_worktree_session(None);
        assert!(get_current_worktree_session().is_none());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_enter_worktree_blocks_nesting() {
        set_worktree_session(Some(WorktreeSession {
            worktree_path: PathBuf::from("/tmp/existing"),
            branch_name: "existing".to_string(),
            original_cwd: PathBuf::from("/tmp"),
            git_root: PathBuf::from("/tmp"),
            original_head_commit: None,
        }));

        let tool = EnterWorktreeTool;
        let ctx = make_ctx();
        let result = tool.validate_input(&json!({}), &ctx).await;
        assert!(matches!(
            result,
            ValidationResult::Error { error_code: 1, .. }
        ));

        set_worktree_session(None);
    }

    #[tokio::test]
    #[serial]
    async fn test_worktree_hooks_create_and_remove_path() {
        let _session_guard = WorktreeSessionGuard;
        set_worktree_session(None);

        let home = TempDir::new().unwrap();
        let cwd = TempDir::new().unwrap();
        let _allthecodes_home = EnvGuard::set("ALLTHECODES_HOME", home.path().to_str().unwrap());
        let _home = EnvGuard::set("CC_RUST_HOME", home.path().to_str().unwrap());
        let _cwd = CurrentDirGuard::set(cwd.path());

        let mut hooks = ToolAppState::default();
        hooks.hooks.insert(
            WORKTREE_CREATE_EVENT.to_string(),
            json!([{ "hooks": [{ "type": "command", "command": "create" }] }]),
        );
        hooks.hooks.insert(
            WORKTREE_REMOVE_EVENT.to_string(),
            json!([{ "hooks": [{ "type": "command", "command": "remove" }] }]),
        );

        let ctx = make_ctx_with(hooks, Arc::new(Phase6HookRunner));
        let parent = parent_message();

        let enter = EnterWorktreeTool
            .call(json!({"name": "hooked"}), &ctx, &parent, None)
            .await
            .unwrap();

        let worktree_path = PathBuf::from(enter.data["worktree_path"].as_str().unwrap());
        assert_eq!(enter.data["created_by"], "WorktreeCreate hook");
        assert_eq!(enter.data["branch"], "hook-branch");
        assert!(worktree_path.exists(), "hook should create worktree path");
        assert!(get_current_worktree_session().is_some());
        let active = get_active_worktree_session("test-session")
            .unwrap()
            .expect("active hook worktree session");
        assert_eq!(active.source, WorktreeSessionSource::WorktreeCreateHook);

        let exit = ExitWorktreeTool
            .call(
                json!({"action": "remove", "discard_changes": true}),
                &ctx,
                &parent,
                None,
            )
            .await
            .unwrap();

        assert_eq!(exit.data["action"], "remove");
        assert_eq!(exit.data["removed"], true);
        assert_eq!(exit.data["removed_by"], "WorktreeRemove hook");
        assert!(
            !worktree_path.exists(),
            "hook remove should delete the worktree path"
        );
        assert!(get_current_worktree_session().is_none());
        assert!(get_active_worktree_session("test-session")
            .unwrap()
            .is_none());
        let records = list_worktree_sessions_for_repo(&workspace_key(cwd.path())).unwrap();
        assert_eq!(records[0].status, WorktreeSessionStatus::Removed);
    }

    #[tokio::test]
    #[serial]
    async fn test_worktree_git_fallback_creates_and_removes_real_worktree() {
        let _session_guard = WorktreeSessionGuard;
        set_worktree_session(None);

        let home = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        let _allthecodes_home = EnvGuard::set("ALLTHECODES_HOME", home.path().to_str().unwrap());
        let _home = EnvGuard::set("CC_RUST_HOME", home.path().to_str().unwrap());
        init_git_repo(repo.path());
        let _cwd = CurrentDirGuard::set(repo.path());

        let ctx = make_ctx();
        let parent = parent_message();

        let enter = EnterWorktreeTool
            .call(json!({"name": "fallback-git"}), &ctx, &parent, None)
            .await
            .unwrap();

        let worktree_path = PathBuf::from(enter.data["worktree_path"].as_str().unwrap());
        assert_eq!(enter.data["created_by"], "git worktree");
        assert!(
            worktree_path.exists(),
            "git fallback should create worktree"
        );
        assert!(get_current_worktree_session().is_some());
        let active = get_active_worktree_session("test-session")
            .unwrap()
            .expect("active git worktree session");
        assert_eq!(active.source, WorktreeSessionSource::EnterWorktree);

        let exit = ExitWorktreeTool
            .call(json!({"action": "remove"}), &ctx, &parent, None)
            .await
            .unwrap();

        assert_eq!(exit.data["action"], "remove");
        assert_eq!(exit.data["removed"], true);
        assert_eq!(exit.data["removed_by"], "git worktree");
        assert!(
            !worktree_path.exists(),
            "git fallback removal should delete the worktree"
        );
        assert!(get_current_worktree_session().is_none());
        assert!(get_active_worktree_session("test-session")
            .unwrap()
            .is_none());
        let records = list_worktree_sessions_for_repo(&workspace_key(repo.path())).unwrap();
        assert_eq!(records[0].status, WorktreeSessionStatus::Removed);
    }

    #[tokio::test]
    #[serial]
    async fn test_enter_from_subdir_records_git_root_for_cleanup() {
        let _session_guard = WorktreeSessionGuard;
        set_worktree_session(None);

        let home = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        let _allthecodes_home = EnvGuard::set("ALLTHECODES_HOME", home.path().to_str().unwrap());
        let _home = EnvGuard::set("CC_RUST_HOME", home.path().to_str().unwrap());
        init_git_repo(repo.path());
        let subdir = repo.path().join("nested").join("child");
        fs::create_dir_all(&subdir).unwrap();
        let _cwd = CurrentDirGuard::set(&subdir);

        let ctx = make_ctx();
        let parent = parent_message();

        let enter = EnterWorktreeTool
            .call(json!({"name": "subdir-root"}), &ctx, &parent, None)
            .await
            .unwrap();

        let worktree_path = PathBuf::from(enter.data["worktree_path"].as_str().unwrap());
        let session = get_current_worktree_session().expect("session exists");
        assert_eq!(session.original_cwd, subdir);
        assert_eq!(session.git_root, repo.path());

        let exit = ExitWorktreeTool
            .call(json!({"action": "remove"}), &ctx, &parent, None)
            .await
            .unwrap();

        assert_eq!(exit.data["removed"], true);
        assert!(
            !worktree_path.exists(),
            "cleanup should use the recorded git root"
        );
        assert!(get_current_worktree_session().is_none());
    }
}
