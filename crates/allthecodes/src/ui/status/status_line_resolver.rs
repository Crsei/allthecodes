//! Root-crate helpers for pre-resolving fields that `allthecodes_engine::status_line`
//! cannot resolve itself.
//!
//! The `StatusLineSnapshot` struct in cc-engine takes
//! `resolved_output_style_name: Option<String>` and `worktree: Option<WorktreeStatus>`
//! as already-resolved values, because the original in-crate helpers touched
//! `allthecodes_engine::output_style` and `allthecodes_worktree` — modules that
//! haven't moved out of the root crate yet. These small helpers perform that
//! resolution at each snapshot-building call site.

use std::path::Path;

use allthecodes_engine::status_line::payload::WorktreeStatus;
use allthecodes_worktree::tool::WorktreeSession;
use tracing::warn;

/// Resolve an output-style name via the engine's output-style registry.
///
/// Returns `None` when the input is missing or empty.
pub fn resolve_output_style_name(output_style: Option<&str>, cwd: &Path) -> Option<String> {
    output_style
        .map(str::trim)
        .filter(|style| !style.is_empty())
        .map(|style| {
            allthecodes_engine::output_style::resolve(style, cwd)
                .name()
                .to_string()
        })
}

/// Build a `WorktreeStatus` from the current worktree session (if any).
pub fn current_worktree_status() -> Option<WorktreeStatus> {
    current_worktree_status_for_session(None)
}

/// Build a `WorktreeStatus` from process state, then persisted session state.
pub fn current_worktree_status_for_session(session_id: Option<&str>) -> Option<WorktreeStatus> {
    if let Some(session) = allthecodes_worktree::get_current_worktree_session() {
        return Some(worktree_status_from_runtime_session(session));
    }

    let session_id = session_id?;
    match allthecodes_session::worktree_sessions::get_active_worktree_session(session_id) {
        Ok(Some(record)) => Some(worktree_status_from_record(record)),
        Ok(None) => None,
        Err(error) => {
            warn!(session_id, %error, "failed to load persisted worktree status");
            None
        }
    }
}

fn worktree_status_from_runtime_session(session: WorktreeSession) -> WorktreeStatus {
    let name = session
        .worktree_path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("worktree")
        .to_string();

    WorktreeStatus {
        name,
        path: session.worktree_path.display().to_string(),
        branch: Some(session.branch_name),
        original_cwd: session.original_cwd.display().to_string(),
        original_branch: None,
    }
}

fn worktree_status_from_record(
    record: allthecodes_session::worktree_sessions::WorktreeSessionRecord,
) -> WorktreeStatus {
    let name = record
        .worktree_path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("worktree")
        .to_string();
    let original_cwd = record
        .original_cwd
        .as_deref()
        .unwrap_or(record.git_root.as_path())
        .display()
        .to_string();

    WorktreeStatus {
        name,
        path: record.worktree_path.display().to_string(),
        branch: Some(record.branch),
        original_cwd,
        original_branch: None,
    }
}
