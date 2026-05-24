use std::collections::HashMap;
use std::path::Path;

use crate::ui::diff::diff_dialog::DiffSource;
use crate::ui::diff::{DiffData, DiffFile, DiffStats};
pub(crate) fn build_diff_sources(cwd: &Path) -> Result<Vec<DiffSource>, String> {
    let repo = git2::Repository::discover(cwd)
        .map_err(|error| format!("Not a git repository (or any parent): {error}"))?;
    let staged = staged_diff_data(&repo).map_err(|error| format!("Failed staged diff: {error}"))?;
    let unstaged =
        unstaged_diff_data(&repo).map_err(|error| format!("Failed unstaged diff: {error}"))?;

    let mut sources = Vec::new();
    if !diff_data_is_empty(&staged) {
        sources.push(DiffSource::with_label("Staged", staged));
    }
    if !diff_data_is_empty(&unstaged) {
        sources.push(DiffSource::with_label("Unstaged", unstaged));
    }
    if sources.is_empty() {
        sources.push(DiffSource::with_label("Current", DiffData::empty()));
    }
    Ok(sources)
}

pub(crate) fn staged_diff_data(repo: &git2::Repository) -> Result<DiffData, git2::Error> {
    let head_tree = match repo.head() {
        Ok(head) => head
            .peel_to_commit()
            .ok()
            .and_then(|commit| commit.tree().ok()),
        Err(_) => None,
    };
    let diff = repo.diff_tree_to_index(head_tree.as_ref(), None, None)?;
    diff_to_diff_data(&diff)
}

pub(crate) fn unstaged_diff_data(repo: &git2::Repository) -> Result<DiffData, git2::Error> {
    let diff = repo.diff_index_to_workdir(None, None)?;
    diff_to_diff_data(&diff)
}

pub(crate) fn diff_to_diff_data(diff: &git2::Diff<'_>) -> Result<DiffData, git2::Error> {
    let stats = diff
        .stats()
        .ok()
        .map(|stats| DiffStats::new(stats.files_changed(), stats.insertions(), stats.deletions()));
    let mut files = Vec::new();
    let mut indices = HashMap::new();
    for delta in diff.deltas() {
        ensure_diff_file(&mut files, &mut indices, &delta);
    }

    let mut hunks: HashMap<String, Vec<String>> = HashMap::new();
    diff.print(git2::DiffFormat::Patch, |delta, _hunk, line| {
        let path = ensure_diff_file(&mut files, &mut indices, &delta);
        if let Some(file) = files.get_mut(path.1) {
            match line.origin() {
                '+' => file.lines_added += 1,
                '-' => file.lines_removed += 1,
                _ => {}
            }
        }

        let mut rendered = String::new();
        if matches!(line.origin(), '+' | '-' | ' ') {
            rendered.push(line.origin());
        }
        if let Ok(content) = std::str::from_utf8(line.content()) {
            rendered.push_str(content.trim_end_matches('\n'));
        }
        if !rendered.trim().is_empty() {
            hunks.entry(path.0).or_default().push(rendered);
        }
        true
    })?;

    Ok(DiffData {
        stats,
        files,
        hunks,
        loading: false,
    })
}

pub(crate) fn ensure_diff_file(
    files: &mut Vec<DiffFile>,
    indices: &mut HashMap<String, usize>,
    delta: &git2::DiffDelta<'_>,
) -> (String, usize) {
    let path = diff_delta_path(delta);
    if let Some(idx) = indices.get(&path).copied() {
        return (path, idx);
    }
    let is_untracked = delta.status() == git2::Delta::Untracked;
    let idx = files.len();
    files.push(DiffFile::new(
        path.clone(),
        0,
        0,
        false,
        false,
        false,
        is_untracked,
    ));
    indices.insert(path.clone(), idx);
    (path, idx)
}

pub(crate) fn diff_delta_path(delta: &git2::DiffDelta<'_>) -> String {
    delta
        .new_file()
        .path()
        .or_else(|| delta.old_file().path())
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|| "(unknown)".to_string())
}

pub(crate) fn diff_data_is_empty(data: &DiffData) -> bool {
    data.files.is_empty()
        && data
            .stats
            .as_ref()
            .map(|stats| stats.files_count == 0)
            .unwrap_or(true)
}
