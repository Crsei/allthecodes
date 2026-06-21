use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TextLine {
    pub(crate) body: String,
    pub(crate) ending: String,
}

impl TextLine {
    pub(crate) fn full_text(&self) -> String {
        format!("{}{}", self.body, self.ending)
    }
}

pub(crate) fn file_hash(content: &str) -> String {
    short_hash(content.as_bytes(), 16)
}

pub(crate) fn line_hash(line: &str) -> String {
    short_hash(line.as_bytes(), 12)
}

pub(crate) fn parse_lines(content: &str) -> Vec<TextLine> {
    let mut lines = Vec::new();
    let bytes = content.as_bytes();
    let mut start = 0;

    while start < bytes.len() {
        let Some(relative_newline) = bytes[start..].iter().position(|byte| *byte == b'\n') else {
            lines.push(TextLine {
                body: content[start..].to_string(),
                ending: String::new(),
            });
            break;
        };

        let newline = start + relative_newline;
        let body_end = if newline > start && bytes[newline - 1] == b'\r' {
            newline - 1
        } else {
            newline
        };
        let ending = if body_end == newline { "\n" } else { "\r\n" };
        lines.push(TextLine {
            body: content[start..body_end].to_string(),
            ending: ending.to_string(),
        });
        start = newline + 1;
    }

    lines
}

pub(crate) fn dominant_line_ending(lines: &[TextLine]) -> &'static str {
    let crlf = lines.iter().filter(|line| line.ending == "\r\n").count();
    let lf = lines.iter().filter(|line| line.ending == "\n").count();
    if crlf > lf {
        "\r\n"
    } else {
        "\n"
    }
}

pub(crate) fn format_window_with_hashlines(
    content: &str,
    offset: usize,
    limit: Option<usize>,
    default_limit: usize,
) -> (String, usize, usize, usize, Option<usize>, bool) {
    let lines = parse_lines(content);
    let total_lines = lines.len();
    let start = if offset > 0 { offset - 1 } else { 0 };

    if start >= total_lines {
        return (String::new(), total_lines, offset, 0, None, false);
    }

    let window_limit = limit.unwrap_or(default_limit);
    let end = start.saturating_add(window_limit).min(total_lines);
    let mut output = format!("base_file_hash\t{}\n", file_hash(content));

    for (i, line) in lines[start..end].iter().enumerate() {
        let line_num = start + i + 1;
        output.push_str(&format!(
            "{}#{}\t{}\n",
            line_num,
            line_hash(&line.body),
            line.body
        ));
    }

    let next_offset = if end < total_lines {
        Some(end + 1)
    } else {
        None
    };

    (
        output,
        total_lines,
        start + 1,
        end,
        next_offset,
        next_offset.is_some(),
    )
}

fn short_hash(bytes: &[u8], chars: usize) -> String {
    let digest = Sha256::digest(bytes);
    let encoded = hex::encode(digest);
    encoded.chars().take(chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lf_crlf_and_final_line_without_newline() {
        let lines = parse_lines("a\nb\r\nc");
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].body, "a");
        assert_eq!(lines[0].ending, "\n");
        assert_eq!(lines[1].body, "b");
        assert_eq!(lines[1].ending, "\r\n");
        assert_eq!(lines[2].body, "c");
        assert_eq!(lines[2].ending, "");
    }

    #[test]
    fn hashline_window_includes_file_and_line_hashes() {
        let (output, total, start, end, next, limited) =
            format_window_with_hashlines("alpha\nbeta\n", 0, None, 2000);
        assert_eq!(total, 2);
        assert_eq!(start, 1);
        assert_eq!(end, 2);
        assert_eq!(next, None);
        assert!(!limited);
        assert!(output.starts_with("base_file_hash\t"));
        assert!(output.contains(&format!("1#{}\talpha\n", line_hash("alpha"))));
        assert!(output.contains(&format!("2#{}\tbeta\n", line_hash("beta"))));
    }
}
