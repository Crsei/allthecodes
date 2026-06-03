//! Git log and diff endpoints used by the web right sidebar.

use std::path::{Component, Path, PathBuf};

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;

use allthecodes_utils::git::{
    commit_touches_file, current_branch, diff_between_with_hunks, diff_commit_with_hunks,
    diff_unstaged_with_hunks, find_git_root, get_commit_stats, get_log, is_git_repo, CommitStats,
};

use crate::state::WebState;

const DEFAULT_LOG_LIMIT: usize = 50;
const MAX_LOG_LIMIT: usize = 200;

#[derive(Debug, Deserialize)]
pub struct GitLogParams {
    pub file: Option<String>,
    pub max_count: Option<usize>,
    pub path: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GitDiffParams {
    pub file: Option<String>,
    pub commit: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub path: Option<String>,
}

#[derive(Debug, Serialize)]
struct GitLogCommit {
    sha: String,
    short_sha: String,
    summary: String,
    message: String,
    author_name: String,
    author_email: String,
    timestamp: i64,
    stats: CommitStats,
}

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
            stats,
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
