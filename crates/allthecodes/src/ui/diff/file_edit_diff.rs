//! Shared file-edit diff preview helpers.

use similar::TextDiff;

use super::structured_diff::{
    parse_structured_hunks, render_structured_diff_hunks, StructuredDiffHunk,
    StructuredDiffLineKind,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileEditDiffStats {
    pub additions: usize,
    pub removals: usize,
}

pub fn unified_hunk_lines_from_edit(path: &str, old: &str, new: &str) -> Vec<String> {
    TextDiff::from_lines(old, new)
        .unified_diff()
        .context_radius(3)
        .header(&format!("a/{path}"), &format!("b/{path}"))
        .to_string()
        .lines()
        .map(str::to_string)
        .collect()
}

pub fn structured_file_edit_hunks(hunk_lines: &[String]) -> Vec<StructuredDiffHunk> {
    parse_structured_hunks(hunk_lines)
}

pub fn file_edit_diff_stats(hunks: &[StructuredDiffHunk]) -> FileEditDiffStats {
    let mut stats = FileEditDiffStats {
        additions: 0,
        removals: 0,
    };
    for line in hunks.iter().flat_map(|hunk| hunk.lines.iter()) {
        match line.kind {
            StructuredDiffLineKind::Add => stats.additions += 1,
            StructuredDiffLineKind::Remove => stats.removals += 1,
            StructuredDiffLineKind::Context | StructuredDiffLineKind::NoNewline => {}
        }
    }
    stats
}

pub fn format_file_edit_summary(stats: FileEditDiffStats) -> String {
    match (stats.additions, stats.removals) {
        (0, 0) => "No line changes".to_string(),
        (additions, 0) => format!(
            "Added {additions} {}",
            if additions == 1 { "line" } else { "lines" }
        ),
        (0, removals) => format!(
            "Removed {removals} {}",
            if removals == 1 { "line" } else { "lines" }
        ),
        (additions, removals) => format!(
            "Added {additions} {}, removed {removals} {}",
            if additions == 1 { "line" } else { "lines" },
            if removals == 1 { "line" } else { "lines" }
        ),
    }
}

pub fn render_file_edit_diff_preview(
    path: &str,
    hunk_lines: &[String],
    width: usize,
    max_lines: usize,
) -> Vec<String> {
    let hunks = structured_file_edit_hunks(hunk_lines);
    let stats = file_edit_diff_stats(&hunks);
    let mut lines = vec![
        format!("file: {path}"),
        format!("summary: {}", format_file_edit_summary(stats)),
    ];
    let body_lines = max_lines.saturating_sub(lines.len());
    let rendered = render_structured_diff_hunks(&hunks, width, body_lines);
    if rendered.is_empty() {
        lines.push("No structured diff available".to_string());
    } else {
        lines.extend(rendered);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::{
        file_edit_diff_stats, format_file_edit_summary, render_file_edit_diff_preview,
        structured_file_edit_hunks, unified_hunk_lines_from_edit,
    };

    #[test]
    fn builds_structured_preview_from_tool_inputs() {
        let hunk_lines = unified_hunk_lines_from_edit(
            "src/lib.rs",
            "fn main() {\n    old_call();\n}\n",
            "fn main() {\n    new_call();\n    extra_call();\n}\n",
        );
        let hunks = structured_file_edit_hunks(&hunk_lines);
        let stats = file_edit_diff_stats(&hunks);

        assert_eq!(
            format_file_edit_summary(stats),
            "Added 2 lines, removed 1 line"
        );
        assert!(
            render_file_edit_diff_preview("src/lib.rs", &hunk_lines, 80, 20)
                .join("\n")
                .contains("@@ -1,3 +1,4 @@")
        );
    }
}
