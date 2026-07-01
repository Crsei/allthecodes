//! Git log and diff endpoints used by the web right sidebar.

use std::path::{Component, Path, PathBuf};

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

use allthecodes_protocol::v1::git::{
    CommitStats as ProtocolCommitStats, GitDiffParams, GitLogCommit, GitLogParams,
    GitMetadataParams, GitMetadataResponse, GitWorktreeSummary, GitWorktreesParams,
    GitWorktreesResponse,
};
use allthecodes_utils::git::{
    commit_touches_file, current_branch, diff_between_with_hunks, diff_commit_with_hunks,
    diff_unstaged_with_hunks, find_git_root, get_commit_stats, get_log, get_status, head_sha,
    is_git_repo, open_repo,
};

use crate::state::WebState;

const DEFAULT_LOG_LIMIT: usize = 50;
const MAX_LOG_LIMIT: usize = 200;

/// GET /api/git/log -- Return recent commits, optionally filtered to one file.
pub async fn git_log_handler(
    State(state): State<WebState>,
    Query(params): Query<GitLogParams>,
) -> Response {
    let cwd = resolve_repo_path(&state, params.path.as_deref());
    let file = match normalize_repo_file(params.file.as_deref()) {
        Ok(file) => file,
        Err(message) => return api_error(StatusCode::BAD_REQUEST, "invalid_params", message),
    };

    if !is_git_repo(&cwd) {
        return api_error(
            StatusCode::BAD_REQUEST,
            "not_a_git_repo",
            "Not a git repository",
        );
    }

    if let Some(file) = file.as_deref() {
        if !repo_file_exists(&cwd, file) {
            return api_error(
                StatusCode::NOT_FOUND,
                "file_not_found",
                "File not found in repository",
            );
        }
    }

    let max_count = params
        .max_count
        .unwrap_or(DEFAULT_LOG_LIMIT)
        .min(MAX_LOG_LIMIT);

    let entries = match get_log(&cwd, max_count) {
        Ok(entries) => entries,
        Err(err) => return git_error(err),
    };

    let mut commits = Vec::new();
    for entry in entries {
        if let Some(file) = file.as_deref() {
            match commit_touches_file(&cwd, &entry.sha, file) {
                Ok(true) => {}
                Ok(false) => continue,
                Err(_) => continue,
            }
        }

        let stats = get_commit_stats(&cwd, &entry.sha, file.as_deref()).unwrap_or_default();
        commits.push(GitLogCommit {
            sha: entry.sha,
            short_sha: entry.short_sha,
            summary: entry.summary,
            message: entry.message,
            author_name: entry.author_name,
            author_email: entry.author_email,
            timestamp: entry.timestamp,
            stats: ProtocolCommitStats {
                additions: stats.additions,
                deletions: stats.deletions,
            },
        });
    }

    let branch = current_branch(&cwd).unwrap_or_default();
    let git_root = find_git_root(&cwd).map(|path| path.display().to_string());

    Json(json!({
        "ok": true,
        "commits": commits,
        "branch": branch,
        "git_root": git_root,
    }))
    .into_response()
}

/// GET /api/git/diff -- Return unstaged, commit, or commit-range diff details.
pub async fn git_diff_handler(
    State(state): State<WebState>,
    Query(params): Query<GitDiffParams>,
) -> Response {
    let cwd = resolve_repo_path(&state, params.path.as_deref());
    let file = match normalize_repo_file(params.file.as_deref()) {
        Ok(file) => file,
        Err(message) => return api_error(StatusCode::BAD_REQUEST, "invalid_params", message),
    };

    if !is_git_repo(&cwd) {
        return api_error(
            StatusCode::BAD_REQUEST,
            "not_a_git_repo",
            "Not a git repository",
        );
    }

    let files = match (
        params.commit.as_deref(),
        params.from.as_deref(),
        params.to.as_deref(),
    ) {
        (None, None, None) => diff_unstaged_with_hunks(&cwd, file.as_deref()),
        (Some(commit), None, None) => diff_commit_with_hunks(&cwd, commit, file.as_deref()),
        (None, Some(from), Some(to)) => diff_between_with_hunks(&cwd, from, to, file.as_deref()),
        _ => {
            return api_error(
                StatusCode::BAD_REQUEST,
                "invalid_params",
                "Provide commit= or from= and to=, or omit all three for unstaged diff",
            );
        }
    };

    let files = match files {
        Ok(files) => files,
        Err(err) => return git_error(err),
    };

    let branch = current_branch(&cwd).unwrap_or_default();
    Json(json!({
        "ok": true,
        "files": files,
        "branch": branch,
    }))
    .into_response()
}

/// GET /api/git/metadata -- Return repository metadata for the workspace/path.
pub async fn git_metadata_handler(
    State(state): State<WebState>,
    Query(params): Query<GitMetadataParams>,
) -> Response {
    let cwd = resolve_repo_path(&state, params.path.as_deref());
    Json(git_metadata_response(&cwd)).into_response()
}

/// GET /api/git/worktrees -- Return linked worktrees for the repository.
pub async fn git_worktrees_handler(
    State(state): State<WebState>,
    Query(params): Query<GitWorktreesParams>,
) -> Response {
    let cwd = resolve_repo_path(&state, params.path.as_deref());
    Json(git_worktrees_response(&cwd)).into_response()
}

fn resolve_repo_path(state: &WebState, path: Option<&str>) -> PathBuf {
    path.map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(state.engine().cwd()))
}

fn normalize_repo_file(file: Option<&str>) -> Result<Option<String>, &'static str> {
    let Some(file) = file.map(str::trim).filter(|file| !file.is_empty()) else {
        return Ok(None);
    };

    let path = Path::new(file);
    if path.is_absolute() {
        return Err("file must be relative to the repository root");
    }

    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err("file must not contain parent directory components");
    }

    Ok(Some(file.replace('\\', "/")))
}

fn repo_file_exists(cwd: &Path, file: &str) -> bool {
    let Some(root) = find_git_root(cwd) else {
        return false;
    };

    if root.join(file).exists() {
        return true;
    }

    let Ok(repo) = allthecodes_utils::git::open_repo(cwd) else {
        return false;
    };

    if repo
        .index()
        .ok()
        .and_then(|index| index.get_path(Path::new(file), 0).map(|_| ()))
        .is_some()
    {
        return true;
    }

    repo.head()
        .ok()
        .and_then(|head| head.peel_to_tree().ok())
        .and_then(|tree| tree.get_path(Path::new(file)).ok())
        .is_some()
}

fn git_metadata_response(cwd: &Path) -> GitMetadataResponse {
    let path = cwd.to_string_lossy().to_string();
    let Some(git_root) = find_git_root(cwd) else {
        return GitMetadataResponse {
            ok: true,
            path,
            is_repo: false,
            git_root: None,
            branch: None,
            head_sha: None,
            is_dirty: false,
            staged_count: 0,
            unstaged_count: 0,
            untracked_count: 0,
            error: None,
        };
    };

    let status = get_status(cwd).ok();
    let staged_count = status
        .as_ref()
        .map(|status| status.staged.len())
        .unwrap_or(0);
    let unstaged_count = status
        .as_ref()
        .map(|status| status.unstaged.len())
        .unwrap_or(0);
    let untracked_count = status
        .as_ref()
        .map(|status| status.untracked.len())
        .unwrap_or(0);
    let error = if status.is_none() {
        Some("failed to read git status".to_string())
    } else {
        None
    };

    GitMetadataResponse {
        ok: true,
        path,
        is_repo: true,
        git_root: Some(git_root.to_string_lossy().to_string()),
        branch: current_branch(cwd).ok(),
        head_sha: head_sha(cwd).ok().filter(|sha| !sha.is_empty()),
        is_dirty: staged_count + unstaged_count + untracked_count > 0,
        staged_count,
        unstaged_count,
        untracked_count,
        error,
    }
}

fn git_worktrees_response(cwd: &Path) -> GitWorktreesResponse {
    let path = cwd.to_string_lossy().to_string();
    let Some(git_root) = find_git_root(cwd) else {
        return GitWorktreesResponse {
            ok: true,
            path,
            is_repo: false,
            git_root: None,
            worktrees: Vec::new(),
            error: None,
        };
    };

    let repo = match open_repo(cwd) {
        Ok(repo) => repo,
        Err(err) => {
            return GitWorktreesResponse {
                ok: false,
                path,
                is_repo: false,
                git_root: Some(git_root.to_string_lossy().to_string()),
                worktrees: Vec::new(),
                error: Some(err.to_string()),
            };
        }
    };

    let mut worktrees = vec![worktree_summary("main", &git_root, true, false, false)];
    match repo.worktrees() {
        Ok(names) => {
            for name in names.iter().flatten() {
                let Ok(worktree) = repo.find_worktree(name) else {
                    continue;
                };
                let path = worktree.path().to_path_buf();
                let is_locked = worktree
                    .is_locked()
                    .map(|status| matches!(status, git2::WorktreeLockStatus::Locked(_)))
                    .unwrap_or(false);
                let is_prunable = worktree.is_prunable(None).unwrap_or(false);
                if path != git_root {
                    worktrees.push(worktree_summary(name, &path, false, is_locked, is_prunable));
                }
            }
            GitWorktreesResponse {
                ok: true,
                path,
                is_repo: true,
                git_root: Some(git_root.to_string_lossy().to_string()),
                worktrees,
                error: None,
            }
        }
        Err(err) => GitWorktreesResponse {
            ok: false,
            path,
            is_repo: true,
            git_root: Some(git_root.to_string_lossy().to_string()),
            worktrees,
            error: Some(err.to_string()),
        },
    }
}

fn worktree_summary(
    name: &str,
    path: &Path,
    is_main: bool,
    is_locked: bool,
    is_prunable: bool,
) -> GitWorktreeSummary {
    let status = get_status(path).ok();
    let is_dirty = status.as_ref().map(|status| {
        !(status.staged.is_empty() && status.unstaged.is_empty() && status.untracked.is_empty())
    });
    GitWorktreeSummary {
        name: name.to_string(),
        path: path.to_string_lossy().to_string(),
        is_main,
        is_locked,
        is_prunable,
        branch: current_branch(path).ok(),
        head_sha: head_sha(path).ok().filter(|sha| !sha.is_empty()),
        is_dirty,
    }
}

fn git_error(error: anyhow::Error) -> Response {
    api_error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "git_error",
        format!("Git operation failed: {error}"),
    )
}

fn api_error(status: StatusCode, code: &'static str, message: impl Into<String>) -> Response {
    (
        status,
        Json(json!({
            "ok": false,
            "error": message.into(),
            "code": code,
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::test_support::*;
    use axum::extract::{Query, State};
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use serde_json::json;
    use serial_test::serial;

    #[tokio::test]
    #[serial]
    async fn git_metadata_reports_repo_status() {
        let project = tempfile::tempdir().expect("project");
        git2::Repository::init(project.path()).expect("init repo");
        std::fs::write(project.path().join("note.txt"), b"dirty").expect("seed file");
        let state = make_web_state_with_cwd(project.path());

        let response = git_metadata_handler(State(state), Query(GitMetadataParams { path: None }))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["ok"], json!(true));
        assert_eq!(body["is_repo"], json!(true));
        assert_eq!(body["is_dirty"], json!(true));
        assert_eq!(body["staged_count"], json!(0));
        assert_eq!(body["untracked_count"], json!(1));
        assert_eq!(
            Path::new(body["git_root"].as_str().expect("git root")),
            project.path()
        );
    }

    #[tokio::test]
    #[serial]
    async fn git_worktrees_includes_main_worktree() {
        let project = tempfile::tempdir().expect("project");
        git2::Repository::init(project.path()).expect("init repo");
        let state = make_web_state_with_cwd(project.path());

        let response =
            git_worktrees_handler(State(state), Query(GitWorktreesParams { path: None }))
                .await
                .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["ok"], json!(true));
        assert_eq!(body["is_repo"], json!(true));
        assert_eq!(
            Path::new(body["git_root"].as_str().expect("git root")),
            project.path()
        );
        let worktrees = body["worktrees"].as_array().expect("worktrees");
        assert_eq!(worktrees.len(), 1);
        assert_eq!(worktrees[0]["name"], json!("main"));
        assert_eq!(worktrees[0]["is_main"], json!(true));
        assert_eq!(
            Path::new(worktrees[0]["path"].as_str().expect("worktree path")),
            project.path()
        );
    }
}
