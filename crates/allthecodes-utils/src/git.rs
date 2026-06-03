//! Git repository utilities using the `git2` crate.
//!
//! Provides repository detection, status queries, diff/log wrappers,
//! and branch information — all without spawning `git` subprocesses.
//!
//! Reference: TypeScript `src/utils/git.ts` and `src/utils/git/` directory.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use git2::{BranchType, Delta, Diff, DiffOptions, Repository, StatusOptions};
use serde::Serialize;

// =============================================================================
// Repository detection
// =============================================================================

/// Check if the given directory (or an ancestor) is inside a git repository.
pub fn is_git_repo(path: &Path) -> bool {
    Repository::discover(path).is_ok()
}

/// Find the root directory of the git repository containing `start_path`.
///
/// Walks up from `start_path` looking for a `.git` directory or file
/// (worktrees/submodules use a `.git` file). Returns `None` if no
/// repository is found.
pub fn find_git_root(start_path: &Path) -> Option<PathBuf> {
    let repo = Repository::discover(start_path).ok()?;
    repo.workdir().map(|p| p.to_path_buf())
}

/// Open the git repository at or above `path`.
///
/// Returns an error if the path is not inside a git repository.
pub fn open_repo(path: &Path) -> Result<Repository> {
    Repository::discover(path).context("Not a git repository (or any parent)")
}

// =============================================================================
// Status
// =============================================================================

/// Summary of a file's status in the working tree / index.
#[derive(Debug, Clone)]
pub struct FileStatus {
    /// Path relative to the repository root.
    pub path: String,
    /// Status category.
    pub status: FileStatusKind,
}

/// High-level file status category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatusKind {
    /// Staged for commit (in the index).
    Staged,
    /// Modified in the working tree but not staged.
    Unstaged,
    /// Not tracked by git.
    Untracked,
    /// Staged and also modified in the working tree.
    StagedAndModified,
    /// Deleted.
    Deleted,
    /// Renamed.
    Renamed,
    /// Conflicted (merge conflict).
    Conflicted,
}

/// Full repository status: staged, unstaged, and untracked files.
#[derive(Debug, Clone, Default)]
pub struct RepoStatus {
    pub staged: Vec<FileStatus>,
    pub unstaged: Vec<FileStatus>,
    pub untracked: Vec<FileStatus>,
}

/// Get the full status of the repository at `path`.
///
/// Roughly equivalent to `git status --porcelain`.
pub fn get_status(path: &Path) -> Result<RepoStatus> {
    let repo = open_repo(path)?;

    let mut opts = StatusOptions::new();
    opts.include_untracked(true)
        .recurse_untracked_dirs(false)
        .include_ignored(false);

    let statuses = repo
        .statuses(Some(&mut opts))
        .context("Failed to get repository status")?;

    let mut result = RepoStatus::default();

    for entry in statuses.iter() {
        let path_str = entry.path().unwrap_or("").to_string();
        let status = entry.status();

        // Staged changes (index vs HEAD)
        if status.intersects(
            git2::Status::INDEX_NEW
                | git2::Status::INDEX_MODIFIED
                | git2::Status::INDEX_DELETED
                | git2::Status::INDEX_RENAMED
                | git2::Status::INDEX_TYPECHANGE,
        ) {
            let kind = if status.contains(git2::Status::INDEX_NEW) {
                FileStatusKind::Staged
            } else if status.contains(git2::Status::INDEX_DELETED) {
                FileStatusKind::Deleted
            } else if status.contains(git2::Status::INDEX_RENAMED) {
                FileStatusKind::Renamed
            } else {
                FileStatusKind::Staged
            };

            // Also modified in worktree?
            let final_kind =
                if status.intersects(git2::Status::WT_MODIFIED | git2::Status::WT_DELETED) {
                    FileStatusKind::StagedAndModified
                } else {
                    kind
                };

            result.staged.push(FileStatus {
                path: path_str.clone(),
                status: final_kind,
            });
        }

        // Unstaged changes (worktree vs index)
        if status.intersects(
            git2::Status::WT_MODIFIED
                | git2::Status::WT_DELETED
                | git2::Status::WT_RENAMED
                | git2::Status::WT_TYPECHANGE,
        ) && !status.intersects(
            git2::Status::INDEX_NEW
                | git2::Status::INDEX_MODIFIED
                | git2::Status::INDEX_DELETED
                | git2::Status::INDEX_RENAMED
                | git2::Status::INDEX_TYPECHANGE,
        ) {
            let kind = if status.contains(git2::Status::WT_DELETED) {
                FileStatusKind::Deleted
            } else if status.contains(git2::Status::WT_RENAMED) {
                FileStatusKind::Renamed
            } else {
                FileStatusKind::Unstaged
            };
            result.unstaged.push(FileStatus {
                path: path_str.clone(),
                status: kind,
            });
        }

        // Untracked
        if status.contains(git2::Status::WT_NEW) {
            result.untracked.push(FileStatus {
                path: path_str.clone(),
                status: FileStatusKind::Untracked,
            });
        }

        // Conflicted
        if status.contains(git2::Status::CONFLICTED) {
            result.staged.push(FileStatus {
                path: path_str.clone(),
                status: FileStatusKind::Conflicted,
            });
        }
    }

    Ok(result)
}

// =============================================================================
// Diff
// =============================================================================

/// A single diff entry for a file.
#[derive(Debug, Clone)]
pub struct DiffEntry {
    /// Path of the file (new path for renames).
    pub path: String,
    /// Old path for renames, `None` otherwise.
    pub old_path: Option<String>,
    /// Kind of change.
    pub delta: DeltaKind,
    /// Number of lines added.
    pub additions: usize,
    /// Number of lines removed.
    pub deletions: usize,
}

/// Kind of diff change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeltaKind {
    Added,
    Deleted,
    Modified,
    Renamed,
    Copied,
    Other,
}

impl DeltaKind {
    pub fn as_str(self) -> &'static str {
        match self {
            DeltaKind::Added => "added",
            DeltaKind::Deleted => "deleted",
            DeltaKind::Modified => "modified",
            DeltaKind::Renamed => "renamed",
            DeltaKind::Copied => "copied",
            DeltaKind::Other => "other",
        }
    }
}

impl From<Delta> for DeltaKind {
    fn from(d: Delta) -> Self {
        match d {
            Delta::Added => DeltaKind::Added,
            Delta::Deleted => DeltaKind::Deleted,
            Delta::Modified => DeltaKind::Modified,
            Delta::Renamed => DeltaKind::Renamed,
            Delta::Copied => DeltaKind::Copied,
            _ => DeltaKind::Other,
        }
    }
}

/// Line-level diff for a single file.
#[derive(Debug, Clone, Serialize)]
pub struct FileDiff {
    /// Path of the file (new path for renames).
    pub path: String,
    /// Old path for renames, `None` otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_path: Option<String>,
    /// Kind of change.
    pub delta: String,
    /// Number of lines added.
    pub additions: usize,
    /// Number of lines removed.
    pub deletions: usize,
    /// Line-level hunks.
    pub hunks: Vec<DiffHunk>,
}

/// One hunk inside a file diff.
#[derive(Debug, Clone, Serialize)]
pub struct DiffHunk {
    pub header: String,
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub lines: Vec<DiffLine>,
}

/// One line inside a diff hunk.
#[derive(Debug, Clone, Serialize)]
pub struct DiffLine {
    #[serde(rename = "type")]
    pub line_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_no: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_no: Option<u32>,
    pub content: String,
}

/// Add/delete stats for one commit.
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct CommitStats {
    pub additions: usize,
    pub deletions: usize,
}

/// Get the staged diff (index vs HEAD), similar to `git diff --cached`.
pub fn diff_staged(path: &Path) -> Result<Vec<DiffEntry>> {
    let repo = open_repo(path)?;
    let head_tree = repo.head().ok().and_then(|h| h.peel_to_tree().ok());

    let diff = repo
        .diff_tree_to_index(head_tree.as_ref(), None, None)
        .context("Failed to compute staged diff")?;

    collect_diff_entries(&diff)
}

/// Get the unstaged diff (worktree vs index), similar to `git diff`.
pub fn diff_unstaged(path: &Path) -> Result<Vec<DiffEntry>> {
    let repo = open_repo(path)?;

    let diff = repo
        .diff_index_to_workdir(None, None)
        .context("Failed to compute unstaged diff")?;

    collect_diff_entries(&diff)
}

/// Get the unstaged diff with line-level hunks, similar to `git diff`.
pub fn diff_unstaged_with_hunks(path: &Path, file: Option<&str>) -> Result<Vec<FileDiff>> {
    let repo = open_repo(path)?;
    let mut opts = diff_options_for_file(file);

    let diff = repo
        .diff_index_to_workdir(None, Some(&mut opts))
        .context("Failed to compute unstaged diff")?;

    collect_file_diffs(&diff)
}

/// Get the diff between two commits by their OID strings.
///
/// `from_ref` and `to_ref` can be commit SHAs, branch names, or other
/// revspec strings that resolve to commits.
pub fn diff_between(path: &Path, from_ref: &str, to_ref: &str) -> Result<Vec<DiffEntry>> {
    let repo = open_repo(path)?;

    let from_obj = repo
        .revparse_single(from_ref)
        .with_context(|| format!("Cannot resolve ref: {}", from_ref))?;
    let to_obj = repo
        .revparse_single(to_ref)
        .with_context(|| format!("Cannot resolve ref: {}", to_ref))?;

    let from_tree = from_obj
        .peel_to_tree()
        .with_context(|| format!("Cannot peel to tree: {}", from_ref))?;
    let to_tree = to_obj
        .peel_to_tree()
        .with_context(|| format!("Cannot peel to tree: {}", to_ref))?;

    let diff = repo
        .diff_tree_to_tree(Some(&from_tree), Some(&to_tree), None)
        .context("Failed to compute diff between commits")?;

    collect_diff_entries(&diff)
}

/// Get the line-level diff between two commits by their OID/revspec strings.
pub fn diff_between_with_hunks(
    path: &Path,
    from_ref: &str,
    to_ref: &str,
    file: Option<&str>,
) -> Result<Vec<FileDiff>> {
    let repo = open_repo(path)?;

    let from_obj = repo
        .revparse_single(from_ref)
        .with_context(|| format!("Cannot resolve ref: {}", from_ref))?;
    let to_obj = repo
        .revparse_single(to_ref)
        .with_context(|| format!("Cannot resolve ref: {}", to_ref))?;

    let from_tree = from_obj
        .peel_to_tree()
        .with_context(|| format!("Cannot peel to tree: {}", from_ref))?;
    let to_tree = to_obj
        .peel_to_tree()
        .with_context(|| format!("Cannot peel to tree: {}", to_ref))?;

    let mut opts = diff_options_for_file(file);
    let diff = repo
        .diff_tree_to_tree(Some(&from_tree), Some(&to_tree), Some(&mut opts))
        .context("Failed to compute diff between commits")?;

    collect_file_diffs(&diff)
}

/// Get the line-level diff introduced by `commit_ref`, relative to its first parent.
pub fn diff_commit_with_hunks(
    path: &Path,
    commit_ref: &str,
    file: Option<&str>,
) -> Result<Vec<FileDiff>> {
    let repo = open_repo(path)?;
    let commit = repo
        .revparse_single(commit_ref)
        .with_context(|| format!("Cannot resolve ref: {}", commit_ref))?
        .peel_to_commit()
        .with_context(|| format!("Cannot peel to commit: {}", commit_ref))?;

    let tree = commit.tree().context("Failed to read commit tree")?;
    let parent_tree = if commit.parent_count() > 0 {
        Some(
            commit
                .parent(0)
                .context("Failed to read commit parent")?
                .tree()
                .context("Failed to read parent tree")?,
        )
    } else {
        None
    };

    let mut opts = diff_options_for_file(file);
    let diff = repo
        .diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), Some(&mut opts))
        .context("Failed to compute commit diff")?;

    collect_file_diffs(&diff)
}

fn diff_options_for_file(file: Option<&str>) -> DiffOptions {
    let mut opts = DiffOptions::new();
    if let Some(file) = file {
        opts.pathspec(file);
    }
    opts
}

/// Collect `DiffEntry` items from a `git2::Diff`.
fn collect_diff_entries(diff: &Diff) -> Result<Vec<DiffEntry>> {
    let mut entries = Vec::new();

    for (i, delta) in diff.deltas().enumerate() {
        let new_file = delta.new_file();
        let old_file = delta.old_file();

        let path = new_file
            .path()
            .or_else(|| old_file.path())
            .and_then(|p| p.to_str())
            .unwrap_or("")
            .to_string();

        let old_path = if delta.status() == Delta::Renamed {
            old_file
                .path()
                .and_then(|p| p.to_str())
                .map(|s| s.to_string())
        } else {
            None
        };

        // Count additions/deletions per file via patch
        let mut additions = 0usize;
        let mut deletions = 0usize;
        if let Ok(Some(patch)) = git2::Patch::from_diff(diff, i) {
            let (_, adds, dels) = patch.line_stats().unwrap_or((0, 0, 0));
            additions = adds;
            deletions = dels;
        }

        entries.push(DiffEntry {
            path,
            old_path,
            delta: delta.status().into(),
            additions,
            deletions,
        });
    }

    Ok(entries)
}

fn collect_file_diffs(diff: &Diff) -> Result<Vec<FileDiff>> {
    let mut files = Vec::new();

    for (i, delta) in diff.deltas().enumerate() {
        let new_file = delta.new_file();
        let old_file = delta.old_file();

        let path = new_file
            .path()
            .or_else(|| old_file.path())
            .and_then(|p| p.to_str())
            .unwrap_or("")
            .to_string();

        let old_path = if delta.status() == Delta::Renamed {
            old_file
                .path()
                .and_then(|p| p.to_str())
                .map(|s| s.to_string())
        } else {
            None
        };

        let mut additions = 0usize;
        let mut deletions = 0usize;
        let mut hunks = Vec::new();

        if let Some(patch) = git2::Patch::from_diff(diff, i)
            .with_context(|| format!("Failed to read patch for {}", path))?
        {
            let (_, adds, dels) = patch.line_stats().unwrap_or((0, 0, 0));
            additions = adds;
            deletions = dels;

            for hunk_idx in 0..patch.num_hunks() {
                let (hunk, line_count) = patch
                    .hunk(hunk_idx)
                    .with_context(|| format!("Failed to read diff hunk for {}", path))?;
                let mut lines = Vec::with_capacity(line_count);

                for line_idx in 0..line_count {
                    let line = patch.line_in_hunk(hunk_idx, line_idx).with_context(|| {
                        format!("Failed to read diff line for {} hunk {}", path, hunk_idx)
                    })?;

                    let line_type = match line.origin() {
                        '+' => "addition",
                        '-' => "deletion",
                        _ => "context",
                    };

                    lines.push(DiffLine {
                        line_type: line_type.to_string(),
                        old_no: line.old_lineno(),
                        new_no: line.new_lineno(),
                        content: String::from_utf8_lossy(line.content()).into_owned(),
                    });
                }

                hunks.push(DiffHunk {
                    header: String::from_utf8_lossy(hunk.header())
                        .trim_end()
                        .to_string(),
                    old_start: hunk.old_start(),
                    old_lines: hunk.old_lines(),
                    new_start: hunk.new_start(),
                    new_lines: hunk.new_lines(),
                    lines,
                });
            }
        }

        files.push(FileDiff {
            path,
            old_path,
            delta: DeltaKind::from(delta.status()).as_str().to_string(),
            additions,
            deletions,
            hunks,
        });
    }

    Ok(files)
}

// =============================================================================
// Log
// =============================================================================

/// A single commit entry from the log.
#[derive(Debug, Clone)]
pub struct LogEntry {
    /// Full commit SHA.
    pub sha: String,
    /// Short (7-char) SHA.
    pub short_sha: String,
    /// Commit summary (first line of the message).
    pub summary: String,
    /// Full commit message.
    pub message: String,
    /// Author name.
    pub author_name: String,
    /// Author email.
    pub author_email: String,
    /// Commit timestamp (Unix seconds).
    pub timestamp: i64,
}

/// Get recent commits from the repository at `path`.
///
/// Returns up to `max_count` commits starting from HEAD, similar to
/// `git log -n <max_count> --format=...`.
pub fn get_log(path: &Path, max_count: usize) -> Result<Vec<LogEntry>> {
    let repo = open_repo(path)?;
    let mut revwalk = repo.revwalk().context("Failed to create revwalk")?;
    revwalk
        .push_head()
        .context("Failed to push HEAD to revwalk")?;
    revwalk.set_sorting(git2::Sort::TIME)?;

    let mut entries = Vec::with_capacity(max_count);

    for (i, oid_result) in revwalk.enumerate() {
        if i >= max_count {
            break;
        }
        let oid = oid_result.context("Failed to get commit OID")?;
        let commit = repo.find_commit(oid).context("Failed to find commit")?;

        let sha = oid.to_string();
        let short_sha = sha[..7.min(sha.len())].to_string();
        let message = commit.message().unwrap_or("").to_string();
        let summary = commit.summary().unwrap_or("").to_string();
        let author = commit.author();
        let author_name = author.name().unwrap_or("").to_string();
        let author_email = author.email().unwrap_or("").to_string();
        let timestamp = commit.time().seconds();

        entries.push(LogEntry {
            sha,
            short_sha,
            summary,
            message,
            author_name,
            author_email,
            timestamp,
        });
    }

    Ok(entries)
}

/// Return true when `commit_ref` changes `file` relative to its first parent.
pub fn commit_touches_file(path: &Path, commit_ref: &str, file: &str) -> Result<bool> {
    Ok(!diff_commit_with_hunks(path, commit_ref, Some(file))?.is_empty())
}

/// Compute aggregate additions/deletions introduced by `commit_ref`.
pub fn get_commit_stats(path: &Path, commit_ref: &str, file: Option<&str>) -> Result<CommitStats> {
    let files = diff_commit_with_hunks(path, commit_ref, file)?;
    Ok(files
        .iter()
        .fold(CommitStats::default(), |mut stats, file| {
            stats.additions += file.additions;
            stats.deletions += file.deletions;
            stats
        }))
}

// =============================================================================
// Branch information
// =============================================================================

/// Information about a git branch.
#[derive(Debug, Clone)]
pub struct BranchInfo {
    /// Branch name (e.g., "main", "feature/foo").
    pub name: String,
    /// Whether this is the currently checked-out branch.
    pub is_head: bool,
    /// Whether this is a remote-tracking branch.
    pub is_remote: bool,
}

/// Get the name of the currently checked-out branch.
///
/// Returns `"HEAD"` if in detached HEAD state.
pub fn current_branch(path: &Path) -> Result<String> {
    let repo = open_repo(path)?;

    if repo.head_detached().unwrap_or(false) {
        return Ok("HEAD".to_string());
    }

    let head = repo.head().context("Failed to read HEAD")?;
    let name = head.shorthand().unwrap_or("HEAD").to_string();

    Ok(name)
}

/// Get the current HEAD commit SHA.
///
/// Returns the full 40-character hex string, or an empty string if
/// there are no commits yet.
pub fn head_sha(path: &Path) -> Result<String> {
    let repo = open_repo(path)?;
    let head = match repo.head() {
        Ok(h) => h,
        Err(_) => return Ok(String::new()),
    };
    let oid = head
        .peel_to_commit()
        .context("Failed to peel HEAD to commit")?
        .id();
    Ok(oid.to_string())
}

/// List all local branches in the repository.
pub fn list_branches(path: &Path) -> Result<Vec<BranchInfo>> {
    let repo = open_repo(path)?;
    let head_ref = repo.head().ok();
    let head_name = head_ref.as_ref().and_then(|h| h.shorthand()).unwrap_or("");

    let branches = repo
        .branches(Some(BranchType::Local))
        .context("Failed to list branches")?;

    let mut result = Vec::new();
    for branch_result in branches {
        let (branch, _branch_type) = branch_result.context("Failed to read branch")?;
        let name = branch.name().ok().flatten().unwrap_or("").to_string();

        result.push(BranchInfo {
            is_head: name == head_name,
            name,
            is_remote: false,
        });
    }

    Ok(result)
}

/// Get the default branch name (e.g., "main" or "master").
///
/// Checks `refs/remotes/origin/HEAD` first, then falls back to
/// probing for `main` or `master` branches.
pub fn default_branch(path: &Path) -> Result<String> {
    let repo = open_repo(path)?;

    // Try refs/remotes/origin/HEAD
    if let Ok(reference) = repo.find_reference("refs/remotes/origin/HEAD") {
        if let Some(target) = reference.symbolic_target() {
            // target is like "refs/remotes/origin/main"
            if let Some(name) = target.strip_prefix("refs/remotes/origin/") {
                return Ok(name.to_string());
            }
        }
    }

    // Probe for common default branch names
    for candidate in &["main", "master"] {
        let refname = format!("refs/remotes/origin/{}", candidate);
        if repo.find_reference(&refname).is_ok() {
            return Ok(candidate.to_string());
        }
    }

    // Fall back to "main"
    Ok("main".to_string())
}

/// Check if the repository is a shallow clone.
pub fn is_shallow(path: &Path) -> Result<bool> {
    let repo = open_repo(path)?;
    Ok(repo.is_shallow())
}

// =============================================================================
// Remote URL helpers
// =============================================================================

/// Get the URL of the `origin` remote for the repository at `path`.
pub fn get_remote_url(path: &Path) -> Result<String> {
    let repo = open_repo(path)?;
    let remote = repo
        .find_remote("origin")
        .map_err(|e| anyhow::anyhow!("no origin remote: {}", e))?;
    remote
        .url()
        .map(|u| u.to_string())
        .ok_or_else(|| anyhow::anyhow!("origin remote has no URL"))
}

/// Parse a GitHub remote URL into `owner/repo` format.
///
/// Supports:
/// - HTTPS: `https://github.com/owner/repo.git`
/// - SSH: `git@github.com:owner/repo.git`
pub fn parse_github_repo(url: &str) -> Option<String> {
    let url = url.trim();

    // SSH: git@github.com:owner/repo.git
    if let Some(rest) = url.strip_prefix("git@github.com:") {
        let repo = rest.strip_suffix(".git").unwrap_or(rest);
        if repo.contains('/') {
            return Some(repo.to_string());
        }
    }

    // HTTPS: https://github.com/owner/repo.git
    if let Some(rest) = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))
    {
        let repo = rest.strip_suffix(".git").unwrap_or(rest);
        if repo.contains('/') {
            let parts: Vec<&str> = repo.splitn(3, '/').collect();
            if parts.len() >= 2 {
                return Some(format!("{}/{}", parts[0], parts[1]));
            }
        }
    }

    None
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Create a unique temporary directory for testing (no tempfile crate needed).
    fn make_temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("allthecodes_tests")
            .join(name)
            .join(format!("{}", std::process::id()));
        if dir.exists() {
            let _ = fs::remove_dir_all(&dir);
        }
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Create a temporary git repo for testing. Returns (dir_path, Repository).
    fn setup_test_repo(test_name: &str) -> (PathBuf, Repository) {
        let dir = make_temp_dir(test_name);
        let repo = Repository::init(&dir).unwrap();

        // Create an initial commit so HEAD exists
        {
            let sig = git2::Signature::now("Test", "test@test.com").unwrap();
            let tree_id = {
                let mut index = repo.index().unwrap();
                let file_path = dir.join("README.md");
                fs::write(&file_path, "# Test\n").unwrap();
                index.add_path(Path::new("README.md")).unwrap();
                index.write().unwrap();
                index.write_tree().unwrap()
            };
            let tree = repo.find_tree(tree_id).unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
                .unwrap();
        }

        (dir, repo)
    }

    /// Clean up a test directory.
    fn cleanup(dir: &Path) {
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_is_git_repo() {
        let (dir, _repo) = setup_test_repo("is_git_repo");
        assert!(is_git_repo(&dir));
        cleanup(&dir);
    }

    #[test]
    fn test_not_git_repo() {
        let dir = make_temp_dir("not_git_repo");
        assert!(!is_git_repo(&dir));
        cleanup(&dir);
    }

    #[test]
    fn test_find_git_root() {
        let (dir, _repo) = setup_test_repo("find_git_root");
        let root = find_git_root(&dir);
        assert!(root.is_some());
        assert_eq!(root.unwrap(), dir);
        cleanup(&dir);
    }

    #[test]
    fn test_current_branch() {
        let (dir, _repo) = setup_test_repo("current_branch");
        let branch = current_branch(&dir).unwrap();
        // Default branch for git init is either "main" or "master"
        assert!(!branch.is_empty());
        cleanup(&dir);
    }

    #[test]
    fn test_head_sha() {
        let (dir, _repo) = setup_test_repo("head_sha");
        let sha = head_sha(&dir).unwrap();
        assert_eq!(sha.len(), 40);
        assert!(sha.chars().all(|c| c.is_ascii_hexdigit()));
        cleanup(&dir);
    }

    #[test]
    fn test_get_status_clean() {
        let (dir, _repo) = setup_test_repo("status_clean");
        let status = get_status(&dir).unwrap();
        assert!(status.staged.is_empty());
        assert!(status.unstaged.is_empty());
        assert!(status.untracked.is_empty());
        cleanup(&dir);
    }

    #[test]
    fn test_get_status_untracked() {
        let (dir, _repo) = setup_test_repo("status_untracked");
        fs::write(dir.join("new_file.txt"), "hello").unwrap();
        let status = get_status(&dir).unwrap();
        assert_eq!(status.untracked.len(), 1);
        assert_eq!(status.untracked[0].path, "new_file.txt");
        cleanup(&dir);
    }

    #[test]
    fn test_get_status_modified() {
        let (dir, _repo) = setup_test_repo("status_modified");
        fs::write(dir.join("README.md"), "# Modified\n").unwrap();
        let status = get_status(&dir).unwrap();
        assert_eq!(status.unstaged.len(), 1);
        cleanup(&dir);
    }

    #[test]
    fn test_get_log() {
        let (dir, _repo) = setup_test_repo("get_log");
        let log = get_log(&dir, 10).unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].summary, "Initial commit");
        assert_eq!(log[0].author_name, "Test");
        cleanup(&dir);
    }

    #[test]
    fn test_diff_staged() {
        let (dir, repo) = setup_test_repo("diff_staged");

        let new_file = dir.join("staged.txt");
        fs::write(&new_file, "staged content\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("staged.txt")).unwrap();
        index.write().unwrap();

        let diff = diff_staged(&dir).unwrap();
        assert_eq!(diff.len(), 1);
        assert_eq!(diff[0].path, "staged.txt");
        assert_eq!(diff[0].delta, DeltaKind::Added);
        cleanup(&dir);
    }

    #[test]
    fn test_diff_unstaged() {
        let (dir, _repo) = setup_test_repo("diff_unstaged");

        fs::write(dir.join("README.md"), "# Changed\nNew line\n").unwrap();

        let diff = diff_unstaged(&dir).unwrap();
        assert_eq!(diff.len(), 1);
        assert_eq!(diff[0].path, "README.md");
        assert_eq!(diff[0].delta, DeltaKind::Modified);
        cleanup(&dir);
    }

    #[test]
    fn test_diff_unstaged_with_hunks() {
        let (dir, _repo) = setup_test_repo("diff_unstaged_with_hunks");

        fs::write(dir.join("README.md"), "# Changed\nNew line\n").unwrap();

        let diff = diff_unstaged_with_hunks(&dir, Some("README.md")).unwrap();
        assert_eq!(diff.len(), 1);
        assert_eq!(diff[0].path, "README.md");
        assert_eq!(diff[0].delta, "modified");
        assert_eq!(diff[0].additions, 2);
        assert_eq!(diff[0].deletions, 1);
        assert_eq!(diff[0].hunks.len(), 1);
        assert!(diff[0].hunks[0]
            .lines
            .iter()
            .any(|line| line.line_type == "addition" && line.content == "New line\n"));
        cleanup(&dir);
    }

    #[test]
    fn test_commit_stats_and_touches_file() {
        let (dir, repo) = setup_test_repo("commit_stats_and_touches_file");

        let oid = {
            let sig = git2::Signature::now("Test", "test@test.com").unwrap();
            let mut index = repo.index().unwrap();
            fs::write(dir.join("feature.txt"), "one\ntwo\n").unwrap();
            index.add_path(Path::new("feature.txt")).unwrap();
            index.write().unwrap();
            let tree_id = index.write_tree().unwrap();
            let tree = repo.find_tree(tree_id).unwrap();
            let parent = repo.head().unwrap().peel_to_commit().unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "Add feature", &tree, &[&parent])
                .unwrap()
        };

        let stats = get_commit_stats(&dir, &oid.to_string(), Some("feature.txt")).unwrap();
        assert_eq!(stats.additions, 2);
        assert_eq!(stats.deletions, 0);
        assert!(commit_touches_file(&dir, &oid.to_string(), "feature.txt").unwrap());
        assert!(!commit_touches_file(&dir, &oid.to_string(), "README.md").unwrap());
        cleanup(&dir);
    }

    #[test]
    fn test_list_branches() {
        let (dir, _repo) = setup_test_repo("list_branches");
        let branches = list_branches(&dir).unwrap();
        assert!(!branches.is_empty());
        assert!(branches.iter().any(|b| b.is_head));
        cleanup(&dir);
    }

    #[test]
    fn test_default_branch_fallback() {
        let (dir, _repo) = setup_test_repo("default_branch");
        let branch = default_branch(&dir).unwrap();
        // Without remotes, should fall back to "main"
        assert_eq!(branch, "main");
        cleanup(&dir);
    }

    #[test]
    fn parse_github_repo_https() {
        assert_eq!(
            parse_github_repo("https://github.com/owner/repo.git"),
            Some("owner/repo".to_string())
        );
    }

    #[test]
    fn parse_github_repo_https_no_git_suffix() {
        assert_eq!(
            parse_github_repo("https://github.com/owner/repo"),
            Some("owner/repo".to_string())
        );
    }

    #[test]
    fn parse_github_repo_ssh() {
        assert_eq!(
            parse_github_repo("git@github.com:owner/repo.git"),
            Some("owner/repo".to_string())
        );
    }

    #[test]
    fn parse_github_repo_ssh_no_suffix() {
        assert_eq!(
            parse_github_repo("git@github.com:owner/repo"),
            Some("owner/repo".to_string())
        );
    }

    #[test]
    fn parse_github_repo_non_github() {
        assert_eq!(parse_github_repo("https://gitlab.com/owner/repo.git"), None);
    }

    #[test]
    fn parse_github_repo_invalid() {
        assert_eq!(parse_github_repo("not-a-url"), None);
    }
}
