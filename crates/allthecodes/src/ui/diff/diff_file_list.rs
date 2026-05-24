use super::truncate_start_to_width;
use super::DiffFile;
use super::MAX_VISIBLE_FILES;

const PATH_PADDING: usize = 20;
const POINTER_WIDTH: usize = 2;

fn pluralize(count: usize, singular: &str) -> &str {
    if count == 1 {
        singular
    } else {
        match singular {
            "file" => "files",
            _ => "items",
        }
    }
}

fn file_stats(file: &DiffFile, _is_selected: bool) -> String {
    if file.is_untracked {
        return "untracked".to_string();
    }

    if file.is_binary {
        return "Binary file".to_string();
    }

    if file.is_large_file {
        return "Large file modified".to_string();
    }

    let mut parts = Vec::new();
    if file.lines_added > 0 {
        parts.push(format!("+{}", file.lines_added));
    }
    if file.lines_removed > 0 {
        parts.push(format!("-{}", file.lines_removed));
    }

    let mut stats = parts.join(" ");
    if file.is_truncated {
        if !stats.is_empty() {
            stats.push(' ');
        }
        stats.push_str("(truncated)");
    }

    stats
}

fn pagination_range(total: usize, selected_index: usize) -> (usize, usize) {
    if total == 0 || total <= MAX_VISIBLE_FILES {
        return (0, total);
    }

    let mut start = selected_index.saturating_sub(MAX_VISIBLE_FILES / 2);
    let mut end = start + MAX_VISIBLE_FILES;
    if end > total {
        end = total;
        start = end.saturating_sub(MAX_VISIBLE_FILES);
    }
    (start, end)
}

fn line_with_stats(file: &DiffFile, is_selected: bool, max_path_width: usize) -> String {
    let pointer = if is_selected { "> " } else { "  " };
    let path_width = max_path_width.max(1);
    let display_path = truncate_start_to_width(&file.path, path_width);
    let mut line = format!(
        "{pointer}{}",
        format_args!("{:width$}", display_path, width = path_width)
    );
    let stats = file_stats(file, is_selected);
    if !stats.is_empty() {
        line.push(' ');
        line.push_str(&stats);
    }
    line
}

fn pagination_label(showing: usize, _total: usize, label: &str) -> String {
    format!(
        "... {showing} more {label}",
        showing = showing,
        label = pluralize(showing, label)
    )
}

/// Render the file list surface for the diff dialog.
pub fn render_diff_file_list_lines(
    files: &[DiffFile],
    selected_index: usize,
    width: usize,
) -> Vec<String> {
    if files.is_empty() {
        return vec!["No changed files".to_string()];
    }

    let (start_index, end_index) =
        pagination_range(files.len(), selected_index.min(files.len() - 1));
    let has_more_above = start_index > 0;
    let has_more_below = end_index < files.len();
    let needs_pagination = files.len() > MAX_VISIBLE_FILES;

    let max_path_width = (width.saturating_sub(POINTER_WIDTH + PATH_PADDING + 4)).max(PATH_PADDING);
    let visible_files = &files[start_index..end_index];

    let mut lines = Vec::new();
    if has_more_above && needs_pagination {
        lines.push(pagination_label(start_index, files.len(), "file"));
    }

    lines.extend(visible_files.iter().enumerate().map(|(idx, file)| {
        let actual_index = start_index + idx;
        let is_selected = actual_index == selected_index;
        line_with_stats(file, is_selected, max_path_width)
    }));

    if has_more_below && needs_pagination {
        lines.push(pagination_label(
            files.len() - end_index,
            files.len(),
            "file",
        ));
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::super::DiffFile;
    use super::render_diff_file_list_lines;
    use insta::assert_snapshot;

    #[test]
    fn snapshot_diff_file_list_with_pagination() {
        let files = vec![
            DiffFile::new("src/main.rs", 12, 4, false, false, false, false),
            DiffFile::new("src/lib.rs", 3, 1, false, false, false, false),
            DiffFile::new("README.md", 0, 0, true, false, false, false),
            DiffFile::new("CHANGELOG.md", 8, 2, false, true, false, false),
            DiffFile::new("old/new/file.rs", 0, 4, false, false, false, false),
            DiffFile::new("src/utils/render.rs", 6, 1, false, false, true, false),
            DiffFile::new("docs/guide.md", 0, 0, false, false, false, true),
        ];

        let output = render_diff_file_list_lines(&files, 4, 80);
        assert_snapshot!(output.join("\n"));
    }
}
