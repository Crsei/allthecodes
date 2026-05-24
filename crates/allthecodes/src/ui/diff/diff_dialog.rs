use super::diff_detail_view::render_diff_detail_view_lines;
use super::diff_file_list::render_diff_file_list_lines;
use super::DiffData;
use super::{DiffFile, DiffStats};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffDialogMode {
    List,
    Detail,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffSource {
    pub label: String,
    pub data: DiffData,
}

impl DiffSource {
    pub fn current() -> Self {
        Self {
            label: "Current".to_string(),
            data: DiffData::empty(),
        }
    }

    pub fn with_label(label: impl Into<String>, data: DiffData) -> Self {
        Self {
            label: label.into(),
            data,
        }
    }
}

fn pluralize(count: usize, singular: &str) -> &str {
    if count == 1 {
        singular
    } else {
        "files"
    }
}

fn stats_line(stats: Option<&DiffStats>) -> String {
    match stats {
        Some(stats) => {
            let mut line = format!(
                "{} {}/line changed",
                stats.files_count,
                pluralize(stats.files_count, "file")
            );
            if stats.lines_added > 0 {
                line.push_str(&format!(" +{}", stats.lines_added));
            }
            if stats.lines_removed > 0 {
                line.push_str(&format!(" -{}", stats.lines_removed));
            }
            line
        }
        None => String::new(),
    }
}

fn source_selector(sources: &[DiffSource], source_index: usize) -> Vec<String> {
    if sources.len() <= 1 {
        return Vec::new();
    }

    let labels = sources
        .iter()
        .enumerate()
        .map(|(index, source)| {
            let marker = if index == source_index { "*" } else { " " };
            format!("{marker}{}", source.label)
        })
        .collect::<Vec<_>>();

    if labels.is_empty() {
        Vec::new()
    } else {
        vec![format!("Sources: {}", labels.join(" "))]
    }
}

fn empty_message(data: &DiffData) -> &str {
    if data.loading {
        "Loading diff..."
    } else if data.files.is_empty() {
        if data
            .stats
            .as_ref()
            .is_some_and(|stats| stats.files_count > 0)
        {
            "Too many files to display details"
        } else {
            "Working tree is clean"
        }
    } else {
        ""
    }
}

fn list_mode_lines(data: &DiffData, selected_index: usize, width: usize) -> Vec<String> {
    if data.files.is_empty() {
        return vec![empty_message(data).to_string()];
    }
    render_diff_file_list_lines(&data.files, selected_index, width)
}

fn detail_mode_lines(
    selected_file: Option<&DiffFile>,
    data: &DiffData,
    width: usize,
) -> Vec<String> {
    let Some(file) = selected_file else {
        return vec![empty_message(data).to_string()];
    };
    let hunks = data.hunks_for_path(&file.path);
    render_diff_detail_view_lines(file, hunks, width)
}

fn help_line(view_mode: DiffDialogMode, has_sources: bool) -> String {
    match view_mode {
        DiffDialogMode::List => {
            if has_sources {
                "source select Enter view [esc] close".to_string()
            } else {
                "Enter view [esc] close".to_string()
            }
        }
        DiffDialogMode::Detail => "[esc] close".to_string(),
    }
}

fn clamp_index(idx: usize, files: &[DiffFile]) -> usize {
    if files.is_empty() {
        0
    } else {
        idx.min(files.len() - 1)
    }
}

/// Render a full dialog-like text snapshot for diff list/detail composition.
pub fn render_diff_dialog_lines(
    title: &str,
    subtitle: Option<&str>,
    sources: &[DiffSource],
    source_index: usize,
    selected_index: usize,
    mode: DiffDialogMode,
    width: usize,
) -> Vec<String> {
    let safe_source_index = if sources.is_empty() {
        0
    } else {
        source_index.min(sources.len() - 1)
    };
    let source = sources.get(safe_source_index);
    let empty_data = DiffData::empty();
    let data = if let Some(source_data) = source {
        &source_data.data
    } else {
        &empty_data
    };

    let mut lines = Vec::new();
    let mut title_line = title.to_string();
    if let Some(subtitle) = subtitle.filter(|s| !s.is_empty()) {
        title_line.push(' ');
        title_line.push('(');
        title_line.push_str(subtitle);
        title_line.push(')');
    }
    lines.push(title_line);

    if let Some(stats) = data.stats.as_ref() {
        lines.push(stats_line(Some(stats)));
    }

    lines.extend(source_selector(sources, safe_source_index));

    let safe_selected = clamp_index(selected_index, &data.files);
    let body = match mode {
        DiffDialogMode::List => list_mode_lines(data, safe_selected, width),
        DiffDialogMode::Detail => {
            let selected_file = data.files.get(safe_selected);
            detail_mode_lines(selected_file, data, width)
        }
    };
    lines.push(String::new());
    lines.extend(body);

    lines.push(String::new());
    lines.push(help_line(mode, sources.len() > 1));
    lines
}

#[cfg(test)]
mod tests {
    use super::super::{DiffData, DiffFile, DiffStats};
    use super::{render_diff_dialog_lines, DiffDialogMode, DiffSource};
    use insta::assert_snapshot;
    use std::collections::HashMap;

    #[test]
    fn snapshot_diff_dialog_list_mode() {
        let data = DiffData {
            stats: Some(DiffStats::new(1, 3, 1)),
            files: vec![DiffFile::new(
                "src/main.rs",
                3,
                1,
                false,
                false,
                false,
                false,
            )],
            hunks: Default::default(),
            loading: false,
        };
        let source = DiffSource::with_label("Current", data);
        let lines = render_diff_dialog_lines(
            "Uncommitted changes",
            None,
            &[source],
            0,
            0,
            DiffDialogMode::List,
            80,
        );
        assert_snapshot!(lines.join("\n"));
    }

    #[test]
    fn snapshot_diff_dialog_detail_mode_with_sources() {
        let current_data = DiffData {
            stats: Some(DiffStats::new(2, 1, 2)),
            files: vec![
                DiffFile::new("cmd.rs", 1, 0, false, false, false, false),
                DiffFile::new("README.md", 0, 2, false, false, false, true),
            ],
            hunks: HashMap::from([(
                "cmd.rs".to_string(),
                vec!["@@ -1,1 +1,1 @@".to_string(), "+fn main() {}".to_string()],
            )]),
            loading: false,
        };
        let source = DiffSource::with_label("Current", current_data);
        let turn_data = DiffData {
            stats: Some(DiffStats::new(2, 10, 3)),
            files: vec![
                DiffFile::new("src/lib.rs", 10, 3, false, false, false, false),
                DiffFile::new("src/main.rs", 0, 0, true, false, false, false),
            ],
            hunks: Default::default(),
            loading: false,
        };
        let sources = vec![source, DiffSource::with_label("Turn 1", turn_data)];
        let lines = render_diff_dialog_lines(
            "Diff details",
            Some("Current"),
            &sources,
            1,
            0,
            DiffDialogMode::Detail,
            80,
        );
        assert_snapshot!(lines.join("\n"));
    }
}
