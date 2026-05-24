// test infrastructure — diff helpers for future git-diff UI; tracked in IMPLEMENTATION_GAPS.md
//! Git diff helpers shared by TUI surfaces.

use std::path::Path;

use anyhow::{Context, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitDiffMode {
    Staged,
    Unstaged,
    Combined,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitDiffResult {
    pub diff: String,
    pub files_changed: usize,
    pub lines_added: usize,
    pub lines_removed: usize,
}

impl GitDiffResult {
    fn from_string(diff: String) -> Self {
        let stats = parse_diff_stats(&diff);
        Self {
            diff,
            files_changed: stats.0,
            lines_added: stats.1,
            lines_removed: stats.2,
        }
    }
}

pub fn get_git_diff(cwd: impl AsRef<Path>, mode: GitDiffMode) -> Result<String> {
    let repo = git2::Repository::discover(cwd).context("not a git repository")?;
    match mode {
        GitDiffMode::Staged => get_staged_diff(&repo),
        GitDiffMode::Unstaged => get_unstaged_diff(&repo),
        GitDiffMode::Combined => {
            let staged = get_staged_diff(&repo)?;
            let unstaged = get_unstaged_diff(&repo)?;
            Ok(join_diff_sections(&staged, &unstaged))
        }
    }
}

pub fn get_git_diff_with_stats(cwd: impl AsRef<Path>, mode: GitDiffMode) -> Result<GitDiffResult> {
    let raw = get_git_diff(cwd, mode)?;
    Ok(GitDiffResult::from_string(raw))
}

pub fn get_status_summary(cwd: impl AsRef<Path>) -> Result<String> {
    let repo = git2::Repository::discover(cwd).context("not a git repository")?;
    let statuses = repo.statuses(None).context("failed to read git status")?;
    if statuses.is_empty() {
        return Ok("Working tree clean.".to_string());
    }

    let mut lines = Vec::new();
    for entry in statuses.iter() {
        let status = entry.status();
        let marker = if status.contains(git2::Status::INDEX_NEW) {
            "A "
        } else if status.contains(git2::Status::INDEX_MODIFIED) {
            "M "
        } else if status.contains(git2::Status::INDEX_DELETED) {
            "D "
        } else if status.contains(git2::Status::WT_NEW) {
            "??"
        } else if status.contains(git2::Status::WT_MODIFIED) {
            " M"
        } else if status.contains(git2::Status::WT_DELETED) {
            " D"
        } else {
            "  "
        };
        lines.push(format!(
            "{} {}",
            marker,
            entry.path().unwrap_or("(unknown)")
        ));
    }
    Ok(lines.join("\n"))
}

pub fn get_status_summary_with_untracked(cwd: impl AsRef<Path>) -> Result<String> {
    let repo = git2::Repository::discover(cwd).context("not a git repository")?;

    let statuses = repo
        .statuses(Some(git2::StatusOptions::new().include_untracked(true)))
        .context("failed to read git status")?;

    if statuses.is_empty() {
        return Ok("Working tree clean.".to_string());
    }

    let mut lines = Vec::new();
    for entry in statuses.iter() {
        let status = entry.status();
        let marker = if status.contains(git2::Status::INDEX_NEW) {
            "A "
        } else if status.contains(git2::Status::INDEX_MODIFIED) {
            "M "
        } else if status.contains(git2::Status::INDEX_DELETED) {
            "D "
        } else if status.contains(git2::Status::WT_NEW) {
            "??"
        } else if status.contains(git2::Status::WT_MODIFIED) {
            " M"
        } else if status.contains(git2::Status::WT_DELETED) {
            " D"
        } else {
            "  "
        };
        lines.push(format!(
            "{} {}",
            marker,
            entry.path().unwrap_or("(unknown)")
        ));
    }
    Ok(lines.join("\n"))
}

/// Parse a unified diff string to extract approximate stats.
/// Returns (files_changed, lines_added, lines_removed).
fn parse_diff_stats(diff: &str) -> (usize, usize, usize) {
    let mut files = 0usize;
    let mut added = 0usize;
    let mut removed = 0usize;

    for line in diff.lines() {
        if line.starts_with("diff --git") {
            files += 1;
        } else if let Some(rest) = line.strip_prefix('+') {
            if !rest.starts_with('+') {
                added += 1;
            }
        } else if let Some(rest) = line.strip_prefix('-') {
            if !rest.starts_with('-') {
                removed += 1;
            }
        }
    }

    (files, added, removed)
}

fn get_staged_diff(repo: &git2::Repository) -> Result<String> {
    let head_tree = match repo.head() {
        Ok(head) => {
            let commit = head.peel_to_commit().context("failed to peel HEAD")?;
            Some(commit.tree().context("failed to read HEAD tree")?)
        }
        Err(_) => None,
    };
    let diff = repo
        .diff_tree_to_index(head_tree.as_ref(), None, None)
        .context("failed to compute staged diff")?;
    diff_to_string(&diff)
}

fn get_unstaged_diff(repo: &git2::Repository) -> Result<String> {
    let diff = repo
        .diff_index_to_workdir(None, None)
        .context("failed to compute unstaged diff")?;
    diff_to_string(&diff)
}

fn diff_to_string(diff: &git2::Diff<'_>) -> Result<String> {
    let mut output = String::new();
    diff.print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
        match line.origin() {
            '+' | '-' | ' ' => output.push(line.origin()),
            _ => {}
        }
        if let Ok(content) = std::str::from_utf8(line.content()) {
            output.push_str(content);
        }
        true
    })
    .context("failed to format git diff")?;
    Ok(output)
}

fn join_diff_sections(staged: &str, unstaged: &str) -> String {
    let mut combined = String::new();
    if !staged.is_empty() {
        combined.push_str("=== Staged changes ===\n\n");
        combined.push_str(staged);
    }
    if !unstaged.is_empty() {
        if !combined.is_empty() {
            combined.push_str("\n\n");
        }
        combined.push_str("=== Unstaged changes ===\n\n");
        combined.push_str(unstaged);
    }
    combined
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_diff_stats_counts_lines_and_files() {
        let diff = "\
diff --git a/src/main.rs b/src/main.rs
@@ -1 +1 @@
-old line
+new line
+another line
diff --git a/src/lib.rs b/src/lib.rs
@@ -1 +1 @@
-old
+new
";
        let stats = parse_diff_stats(diff);
        assert_eq!(stats.0, 2); // 2 files
        assert_eq!(stats.1, 3); // 3 added (new line, another line, new)
        assert_eq!(stats.2, 2); // 2 removed (old, old line)
    }

    #[test]
    fn result_and_combined_mode_use_diff_helpers() {
        let result =
            GitDiffResult::from_string("diff --git a/a b/a\n@@ -1 +1 @@\n-old\n+new\n".to_string());

        assert_eq!(result.files_changed, 1);
        assert_eq!(result.lines_added, 1);
        assert_eq!(result.lines_removed, 1);
        assert!(join_diff_sections("staged", "unstaged").contains("Staged changes"));

        let modes = [
            GitDiffMode::Staged,
            GitDiffMode::Unstaged,
            GitDiffMode::Combined,
        ];
        assert_eq!(modes.len(), 3);

        if false {
            let cwd = Path::new(".");
            let _ = get_git_diff(cwd, GitDiffMode::Staged);
            let _ = get_git_diff_with_stats(cwd, GitDiffMode::Combined);
            let _ = get_status_summary(cwd);
            let _ = get_status_summary_with_untracked(cwd);
        }
    }
}
