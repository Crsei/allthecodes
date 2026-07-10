//! JSON payload schema for the scriptable status line.
//!
//! Corresponds to `docs/claude-code-configuration/customize-status-line.md`.
//! The runtime assembles a [`StatusLinePayload`] once per refresh and pipes
//! it to the user's `statusLine.command` on stdin.
//!
//! The shape is intentionally stable / documented so shell/python scripts
//! can rely on it. Every nested struct uses `serde(skip_serializing_if)` for
//! `Option`s and empty collections so absent fields really are absent from
//! the JSON (scripts can `.get()` with confidence).

use std::path::{Path, PathBuf};

use allthecodes_compact::auto_compact::get_context_window_size;
use allthecodes_types::models::ModelSetting;
use git2::Repository;

pub use allthecodes_types::status_line::{
    ContextWindowStatus, CostStatus, ModelInfo, StatusLinePayload, VimStatus, WorkspaceStatus,
    WorktreeStatus,
};

/// Inputs needed to assemble a concrete status-line payload snapshot.
///
/// `resolved_output_style_name` and `worktree` are pre-computed by callers
/// because resolving them requires root-crate modules (`engine::output_style`
/// and `tools::worktree`) that cc-engine cannot reach. Callers that don't
/// need either field pass `None`.
pub struct StatusLineSnapshot<'a> {
    pub session_id: Option<String>,
    pub model_id: &'a str,
    pub backend: Option<&'a str>,
    pub cwd: &'a Path,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub total_cost_usd: f64,
    pub api_calls: u64,
    pub unknown_pricing_count: Option<u64>,
    pub backfilled_count: Option<u64>,
    pub session_duration_secs: Option<u64>,
    /// Pre-resolved canonical output-style name. Callers compute this by
    /// calling `allthecodes_engine::output_style::resolve(style, cwd).name()`.
    pub resolved_output_style_name: Option<String>,
    pub editor_mode: Option<&'a str>,
    /// Pre-built worktree status. Callers compute this from
    /// the root worktree session tracker.
    pub worktree: Option<WorktreeStatus>,
    pub streaming: bool,
    pub message_count: usize,
}

/// Build a payload from the runtime snapshot available to the caller.
pub fn build_payload_from_snapshot(snapshot: StatusLineSnapshot<'_>) -> StatusLinePayload {
    let mut payload = StatusLinePayload::new();
    payload.session_id = snapshot.session_id;
    payload.model = model_info_from_runtime(snapshot.model_id, snapshot.backend);
    payload.workspace = workspace_status_from_path(snapshot.cwd);
    payload.context = Some(context_status_from_usage(
        snapshot.model_id,
        snapshot.input_tokens,
        snapshot.output_tokens,
        snapshot.cache_read_tokens,
        snapshot.cache_creation_tokens,
    ));
    payload.cost = Some(CostStatus {
        total_usd: snapshot.total_cost_usd,
        api_calls: snapshot.api_calls,
        unknown_pricing_count: snapshot.unknown_pricing_count,
        backfilled_count: snapshot.backfilled_count,
        session_duration_secs: snapshot.session_duration_secs,
    });
    payload.output_style = snapshot.resolved_output_style_name;
    payload.vim = vim_status_from_editor_mode(snapshot.editor_mode);
    payload.worktree = snapshot.worktree;
    payload.streaming = snapshot.streaming;
    payload.message_count = snapshot.message_count;
    payload
}

pub fn model_info_from_runtime(model_id: &str, backend: Option<&str>) -> Option<ModelInfo> {
    let model_id = model_id.trim();
    if model_id.is_empty() {
        return None;
    }

    Some(ModelInfo {
        id: model_id.to_string(),
        display_name: Some(ModelSetting::from_model_id(model_id).display_name),
        backend: backend
            .map(str::trim)
            .filter(|backend| !backend.is_empty())
            .map(ToOwned::to_owned),
    })
}

// `resolve_output_style_name` used to live here but touched
// `allthecodes_engine::output_style::resolve`. Callers now
// pre-resolve the canonical name and pass it via
// `StatusLineSnapshot::resolved_output_style_name`.

pub fn vim_status_from_editor_mode(editor_mode: Option<&str>) -> Option<VimStatus> {
    match editor_mode.map(str::trim) {
        Some("vim") => Some(VimStatus {
            mode: "NORMAL".to_string(),
        }),
        _ => None,
    }
}

pub fn context_status_from_usage(
    model_id: &str,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_creation_tokens: u64,
) -> ContextWindowStatus {
    let max_tokens = get_context_window_size(model_id);
    let used_tokens = input_tokens
        .saturating_add(output_tokens)
        .saturating_add(cache_read_tokens)
        .saturating_add(cache_creation_tokens);
    let used_fraction = if max_tokens == 0 {
        None
    } else {
        Some((used_tokens as f64 / max_tokens as f64 * 10_000.0).round() / 10_000.0)
    };

    ContextWindowStatus {
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_creation_tokens,
        max_tokens: Some(max_tokens),
        used_fraction,
    }
}

pub fn workspace_status_from_path(cwd: &Path) -> Option<WorkspaceStatus> {
    let cwd = absolutize_for_statusline(cwd);
    let repo = Repository::discover(&cwd).ok();
    let git_root = repo
        .as_ref()
        .and_then(|repo| repo.workdir().map(Path::to_path_buf));
    let git_branch = repo
        .as_ref()
        .and_then(|repo| repo.head().ok())
        .and_then(|head| head.shorthand().ok().map(ToOwned::to_owned));
    let git_worktree = repo
        .as_ref()
        .and_then(|repo| git_worktree_name_from_repo(repo.path()));
    let project_dir = project_dir_for_statusline(&cwd, git_root.as_deref());

    Some(WorkspaceStatus {
        cwd: cwd.display().to_string(),
        project_dir: project_dir.map(|path| path.display().to_string()),
        git_branch,
        is_worktree: git_worktree.as_ref().map(|_| true),
        git_worktree,
    })
}

// `current_worktree_status` used to live here but read the root worktree
// session tracker.
// Callers now build a `WorktreeStatus` themselves and pass it via
// `StatusLineSnapshot::worktree`.

fn project_dir_for_statusline(cwd: &Path, git_root: Option<&Path>) -> Option<PathBuf> {
    let configured = allthecodes_bootstrap::state::project_root();
    if !configured.as_os_str().is_empty() {
        return Some(configured);
    }

    git_root
        .map(Path::to_path_buf)
        .or_else(|| Some(cwd.to_path_buf()))
}

fn absolutize_for_statusline(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

fn git_worktree_name_from_repo(git_dir: &Path) -> Option<String> {
    let parts: Vec<String> = git_dir
        .components()
        .map(|component| component.as_os_str().to_string_lossy().to_string())
        .collect();
    let worktrees_idx = parts
        .iter()
        .position(|component| component == "worktrees")?;
    parts.get(worktrees_idx + 1).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use git2::{Repository, Signature};
    use tempfile::tempdir;

    #[test]
    fn payload_serializes_minimum_required_keys() {
        let payload = StatusLinePayload::new();
        let v = serde_json::to_value(&payload).unwrap();
        assert_eq!(
            v.get("hookEventName").and_then(|x| x.as_str()),
            Some("StatusLine")
        );
        assert_eq!(v.get("version").and_then(|x| x.as_u64()), Some(1));
        assert_eq!(v.get("streaming").and_then(|x| x.as_bool()), Some(false));
        // Absent-optional fields really are absent from the JSON.
        assert!(v.get("sessionId").is_none());
        assert!(v.get("model").is_none());
        assert!(v.get("workspace").is_none());
    }

    #[test]
    fn payload_roundtrips_with_nested_structs() {
        let p = StatusLinePayload {
            hook_event_name: "StatusLine".to_string(),
            version: 1,
            session_id: Some("abc-123".into()),
            model: Some(ModelInfo {
                id: "claude-sonnet-4-20250514".into(),
                display_name: Some("sonnet-4".into()),
                backend: Some("native".into()),
            }),
            workspace: Some(WorkspaceStatus {
                cwd: "/tmp/x".into(),
                project_dir: Some("/tmp/x".into()),
                git_branch: Some("main".into()),
                is_worktree: Some(false),
                git_worktree: Some("feature-x".into()),
            }),
            context: Some(ContextWindowStatus {
                input_tokens: 1000,
                output_tokens: 500,
                cache_read_tokens: 2000,
                cache_creation_tokens: 0,
                max_tokens: Some(200_000),
                used_fraction: Some(0.005),
            }),
            cost: Some(CostStatus {
                total_usd: 0.0123,
                api_calls: 3,
                unknown_pricing_count: None,
                backfilled_count: None,
                session_duration_secs: Some(42),
            }),
            output_style: Some("default".into()),
            vim: Some(VimStatus {
                mode: "normal".into(),
            }),
            worktree: Some(WorktreeStatus {
                name: "feature-x".into(),
                path: "/tmp/worktree".into(),
                branch: Some("feature-x".into()),
                original_cwd: "/tmp/project".into(),
                original_branch: Some("main".into()),
            }),
            streaming: true,
            message_count: 7,
        };
        let s = serde_json::to_string(&p).unwrap();
        let parsed: StatusLinePayload = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed.session_id.as_deref(), Some("abc-123"));
        assert_eq!(
            parsed.model.as_ref().unwrap().id,
            "claude-sonnet-4-20250514"
        );
        assert_eq!(parsed.context.as_ref().unwrap().input_tokens, 1000);
        assert!(parsed.streaming);
        assert_eq!(parsed.message_count, 7);
    }

    #[test]
    fn new_fills_event_name_and_version() {
        let p = StatusLinePayload::new();
        assert_eq!(p.hook_event_name, "StatusLine");
        assert_eq!(p.version, StatusLinePayload::VERSION);
        assert!(!p.streaming);
        assert_eq!(p.message_count, 0);
    }

    #[test]
    fn build_snapshot_populates_output_style_vim_and_context_metadata() {
        let dir = tempdir().unwrap();
        let payload = build_payload_from_snapshot(StatusLineSnapshot {
            session_id: Some("sess-123".into()),
            model_id: "claude-sonnet-4-20250514",
            backend: Some("native"),
            cwd: dir.path(),
            input_tokens: 2_000,
            output_tokens: 1_000,
            cache_read_tokens: 500,
            cache_creation_tokens: 0,
            total_cost_usd: 0.42,
            api_calls: 3,
            unknown_pricing_count: Some(1),
            backfilled_count: Some(2),
            session_duration_secs: Some(9),
            resolved_output_style_name: Some("default".into()),
            editor_mode: Some("vim"),
            worktree: None,
            streaming: true,
            message_count: 5,
        });

        assert_eq!(payload.session_id.as_deref(), Some("sess-123"));
        assert_eq!(payload.output_style.as_deref(), Some("default"));
        assert_eq!(
            payload.vim.as_ref().map(|vim| vim.mode.as_str()),
            Some("NORMAL")
        );
        assert_eq!(payload.cost.as_ref().map(|cost| cost.api_calls), Some(3));
        assert_eq!(
            payload
                .cost
                .as_ref()
                .and_then(|cost| cost.unknown_pricing_count),
            Some(1)
        );
        assert_eq!(
            payload.cost.as_ref().and_then(|cost| cost.backfilled_count),
            Some(2)
        );
        assert_eq!(
            payload
                .context
                .as_ref()
                .and_then(|context| context.max_tokens),
            Some(200_000)
        );
        assert_eq!(payload.message_count, 5);
        assert!(payload.streaming);
    }

    #[test]
    fn workspace_status_detects_git_metadata() {
        let dir = tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        std::fs::write(dir.path().join("tracked.txt"), "hello\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("tracked.txt")).unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let sig = Signature::now("Status Line", "statusline@example.com").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();

        let workspace = workspace_status_from_path(dir.path()).unwrap();
        assert_eq!(workspace.cwd, dir.path().display().to_string());
        assert!(
            workspace
                .project_dir
                .as_deref()
                .map(|path| !path.is_empty())
                .unwrap_or(false),
            "expected a project_dir"
        );
        assert!(
            workspace
                .git_branch
                .as_deref()
                .map(|branch| !branch.is_empty())
                .unwrap_or(false),
            "expected a branch name"
        );
        assert_eq!(workspace.is_worktree, None);
    }
}
