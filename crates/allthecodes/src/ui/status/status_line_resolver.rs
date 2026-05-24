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
    let session = allthecodes_worktree::get_current_worktree_session()?;
    let name = session
        .worktree_path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("worktree")
        .to_string();

    Some(WorktreeStatus {
        name,
        path: session.worktree_path.display().to_string(),
        branch: Some(session.branch_name),
        original_cwd: session.original_cwd.display().to_string(),
        original_branch: None,
    })
}
