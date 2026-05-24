//! Rust-side helper for bash output messages.

#[cfg(test)]
use crate::ui::theme::Theme;
use unicode_width::UnicodeWidthChar;

const DEFAULT_WIDTH: usize = 80;
const DEFAULT_MAX_LINES: usize = 5;
const MAX_JSON_FORMAT_LENGTH: usize = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShellOutputRenderOptions {
    pub width: usize,
    pub max_lines: usize,
    pub expanded: bool,
    pub elapsed_ms: Option<u64>,
    pub timeout_ms: Option<u64>,
    pub total_lines: Option<usize>,
    pub total_bytes: Option<usize>,
}

impl Default for ShellOutputRenderOptions {
    fn default() -> Self {
        Self {
            width: DEFAULT_WIDTH,
            max_lines: DEFAULT_MAX_LINES,
            expanded: false,
            elapsed_ms: None,
            timeout_ms: None,
            total_lines: None,
            total_bytes: None,
        }
    }
}

#[cfg(test)]
pub fn render_user_bash_output_message(command: &str, output: &str, _theme: &Theme) -> String {
    let options = ShellOutputRenderOptions::default();
    render_user_bash_output_message_with_options(command, output, options)
}

pub fn render_user_bash_output_message_with_options(
    command: &str,
    output: &str,
    options: ShellOutputRenderOptions,
) -> String {
    let output = output.trim();
    if output.is_empty() {
        return format!("{command} -> (no output)");
    }

    let formatted = format_shell_output(output);
    let all_lines = formatted
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();

    if all_lines.is_empty() {
        return format!("{command} -> (no output)");
    }

    let max_lines = options.max_lines.max(1);
    let shown_start = if options.expanded || all_lines.len() <= max_lines {
        0
    } else {
        all_lines.len() - max_lines
    };
    let shown = all_lines[shown_start..]
        .iter()
        .map(|line| truncate_to_width(line, options.width))
        .collect::<Vec<_>>();

    let footer = render_footer(all_lines.len(), options);
    if shown.len() == 1 && footer.is_empty() {
        return format!("{command} -> {}", shown[0]);
    }

    let mut lines = vec![format!("$ {command}")];
    lines.extend(shown);
    if !footer.is_empty() {
        lines.push(footer);
    }
    lines.join("\n")
}

fn format_shell_output(output: &str) -> String {
    let stripped = strip_ansi(output);
    if stripped.len() > MAX_JSON_FORMAT_LENGTH {
        return stripped;
    }

    stripped
        .lines()
        .map(try_format_json_line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn try_format_json_line(line: &str) -> String {
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(line) else {
        return line.to_string();
    };
    serde_json::to_string_pretty(&parsed).unwrap_or_else(|_| line.to_string())
}

fn strip_ansi(output: &str) -> String {
    let bytes = strip_ansi_escapes::strip(output.as_bytes());
    String::from_utf8_lossy(&bytes).into_owned()
}

fn render_footer(actual_lines: usize, options: ShellOutputRenderOptions) -> String {
    let mut parts = Vec::new();
    let total_lines = options.total_lines.unwrap_or(actual_lines);
    if !options.expanded && total_lines > options.max_lines.max(1) {
        parts.push(format!("+{} lines", total_lines - options.max_lines.max(1)));
    }
    if let Some(time) = render_shell_time(options.elapsed_ms, options.timeout_ms) {
        parts.push(time);
    }
    if let Some(bytes) = options.total_bytes {
        parts.push(format_file_size(bytes));
    }
    parts.join(" ")
}

fn render_shell_time(elapsed_ms: Option<u64>, timeout_ms: Option<u64>) -> Option<String> {
    let timeout = timeout_ms.map(format_duration);
    match (elapsed_ms.map(format_duration), timeout) {
        (None, None) => None,
        (None, Some(timeout)) => Some(format!("(timeout {timeout})")),
        (Some(elapsed), None) => Some(format!("({elapsed})")),
        (Some(elapsed), Some(timeout)) => Some(format!("({elapsed} · timeout {timeout})")),
    }
}

fn format_duration(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms}ms")
    } else if ms.is_multiple_of(1000) {
        format!("{}s", ms / 1000)
    } else {
        format!("{:.1}s", ms as f64 / 1000.0)
    }
}

fn format_file_size(bytes: usize) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    let bytes = bytes as f64;
    if bytes < KB {
        format!("{}B", bytes as usize)
    } else if bytes < MB {
        format!("{:.1}KB", bytes / KB)
    } else {
        format!("{:.1}MB", bytes / MB)
    }
}

fn truncate_to_width(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }

    let mut width = 0usize;
    let mut out = String::new();
    for ch in text.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + ch_width > max_width {
            break;
        }
        out.push(ch);
        width += ch_width;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{render_user_bash_output_message_with_options, ShellOutputRenderOptions};

    #[test]
    fn collapses_to_recent_lines_with_footer() {
        let rendered = render_user_bash_output_message_with_options(
            "cargo test",
            "one\ntwo\nthree\nfour\nfive\nsix",
            ShellOutputRenderOptions {
                width: 20,
                elapsed_ms: Some(12_400),
                total_bytes: Some(2048),
                ..ShellOutputRenderOptions::default()
            },
        );

        assert_eq!(
            rendered,
            "$ cargo test\ntwo\nthree\nfour\nfive\nsix\n+1 lines (12.4s) 2.0KB"
        );
    }

    #[test]
    fn expanded_output_keeps_all_lines_without_count_footer() {
        let rendered = render_user_bash_output_message_with_options(
            "printf",
            "one\ntwo\nthree\nfour\nfive\nsix",
            ShellOutputRenderOptions {
                expanded: true,
                elapsed_ms: Some(1000),
                ..ShellOutputRenderOptions::default()
            },
        );

        assert_eq!(rendered, "$ printf\none\ntwo\nthree\nfour\nfive\nsix\n(1s)");
    }

    #[test]
    fn strips_ansi_formats_small_json_and_truncates_width() {
        let rendered = render_user_bash_output_message_with_options(
            "node",
            "\u{1b}[31m{\"status\":\"ok\"}\u{1b}[0m",
            ShellOutputRenderOptions {
                width: 40,
                expanded: true,
                ..ShellOutputRenderOptions::default()
            },
        );

        assert_eq!(rendered, "$ node\n{\n  \"status\": \"ok\"\n}");
    }
}
