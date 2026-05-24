use super::structured_diff::{parse_structured_hunks, render_structured_diff_hunks};
use super::{truncate_by_width, DiffFile};

const MAX_CONTEXT_LINES: usize = 400;
const LARGE_FILE_LIMIT_NOTE: &str = "Large file - diff exceeds 1 MB limit";
const BINARY_NOTE: &str = "Binary file - cannot display diff";
const UNTRACKED_NOTE: &str = "New file not yet staged.";
const UNTRACKED_HINT: &str = "Run `git add {file}` to see line counts.";
const NO_DIFF_NOTE: &str = "No diff content";
const TRUNCATED_NOTE: &str = "(diff truncated (exceeded 400 line limit))";

fn split_hunks(lines: &[String]) -> Vec<String> {
    let mut output = Vec::new();
    for chunk in lines {
        output.extend(chunk.split('\n').map(str::to_string));
    }
    output
}

fn render_plain_lines(lines: Vec<String>, width: usize) -> Vec<String> {
    lines
        .into_iter()
        .map(|line| truncate_by_width(&line, width))
        .collect()
}

fn render_diff_body(hunks: &[String], width: usize) -> Vec<String> {
    let structured_hunks = parse_structured_hunks(hunks);
    if !structured_hunks.is_empty() {
        return render_structured_diff_hunks(&structured_hunks, width, MAX_CONTEXT_LINES);
    }

    let mut body = split_hunks(hunks);
    if body.len() > MAX_CONTEXT_LINES {
        body.truncate(MAX_CONTEXT_LINES);
    }
    render_plain_lines(body, width)
}

/// Render the detail view for a selected file.
pub fn render_diff_detail_view_lines(
    file: &DiffFile,
    hunks: &[String],
    width: usize,
) -> Vec<String> {
    if file.is_untracked {
        return vec![
            file.path.clone(),
            UNTRACKED_NOTE.to_string(),
            UNTRACKED_HINT.replace("{file}", &file.path),
        ];
    }

    if file.is_binary {
        return vec![file.path.clone(), BINARY_NOTE.to_string()];
    }

    if file.is_large_file {
        return vec![file.path.clone(), LARGE_FILE_LIMIT_NOTE.to_string()];
    }

    let mut lines = vec![file.path.clone()];
    let mut body = render_diff_body(hunks, width);
    if body.is_empty() {
        body.push(NO_DIFF_NOTE.to_string());
    }

    lines.extend(body);
    if file.is_truncated {
        lines.push(TRUNCATED_NOTE.to_string());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::{render_diff_detail_view_lines, DiffFile};
    use insta::assert_snapshot;

    #[test]
    fn snapshot_diff_detail_view_for_untracked() {
        let file = DiffFile::new("foo.rs", 0, 0, false, false, false, true);
        let output = render_diff_detail_view_lines(&file, &[], 80);
        assert_snapshot!(output.join("\n"));
    }

    #[test]
    fn snapshot_diff_detail_view_for_binary() {
        let file = DiffFile::new("asset.bin", 0, 0, true, false, false, false);
        let output = render_diff_detail_view_lines(&file, &[], 80);
        assert_snapshot!(output.join("\n"));
    }

    #[test]
    fn snapshot_diff_detail_view_for_structured_hunks() {
        let file = DiffFile::new("src/main.rs", 2, 1, false, false, false, false);
        let hunks = vec![
            "diff --git a/src/main.rs b/src/main.rs".to_string(),
            "index 1111111..2222222 100644".to_string(),
            "--- a/src/main.rs".to_string(),
            "+++ b/src/main.rs".to_string(),
            "@@ -1,3 +1,4 @@ fn main".to_string(),
            " fn main() {".to_string(),
            "-    old_call();".to_string(),
            "+    new_call();".to_string(),
            "+    extra_call();".to_string(),
            " }".to_string(),
        ];

        let output = render_diff_detail_view_lines(&file, &hunks, 80);

        assert_snapshot!(output.join("\n"));
    }

    #[test]
    fn snapshot_diff_detail_view_for_large_file() {
        let file = DiffFile::new("large.rs", 0, 0, false, true, false, false);
        let output = render_diff_detail_view_lines(&file, &[], 80);
        assert_snapshot!(output.join("\n"));
    }

    #[test]
    fn snapshot_diff_detail_view_for_truncated_structured_hunks() {
        let file = DiffFile::new("src/main.rs", 1, 1, false, false, true, false);
        let hunks = vec![
            "@@ -1,1 +1,1 @@".to_string(),
            "-old".to_string(),
            "+new".to_string(),
        ];

        let output = render_diff_detail_view_lines(&file, &hunks, 80);

        assert_snapshot!(output.join("\n"));
    }

    #[test]
    fn snapshot_diff_detail_view_truncates_long_plain_lines_at_narrow_width() {
        let file = DiffFile::new(
            "src/very_long_file_name.rs",
            1,
            1,
            false,
            false,
            false,
            false,
        );
        let hunks = vec![
            "+let value = \"this added line is intentionally much longer than the available diff detail width\";".to_string(),
            "-let previous = \"this removed line is also intentionally too long for the narrow panel\";".to_string(),
        ];

        let output = render_diff_detail_view_lines(&file, &hunks, 32);

        assert_snapshot!(
            "diff_detail_view_truncates_long_plain_lines_32w",
            output.join("\n")
        );
    }
}
