// test infrastructure — history cell rendering not wired to production transcript view
//! Typed chat history cells.

use ratatui::style::Style;
use ratatui::text::{Line, Span};

use super::theme::Theme;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryCell {
    User(String),
    Assistant(String),
    System(String),
    Tool {
        name: String,
        summary: String,
    },
    Diff {
        path: String,
        added: usize,
        removed: usize,
    },
    Status(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryRenderMode {
    Prompt,
    Transcript,
}

impl HistoryCell {
    pub fn role(&self) -> &'static str {
        match self {
            HistoryCell::User(_) => "user",
            HistoryCell::Assistant(_) => "assistant",
            HistoryCell::System(_) => "system",
            HistoryCell::Tool { .. } => "tool",
            HistoryCell::Diff { .. } => "diff",
            HistoryCell::Status(_) => "status",
        }
    }

    pub fn render(&self, mode: HistoryRenderMode) -> String {
        match (self, mode) {
            (HistoryCell::User(text), _) => format!("> {text}"),
            (HistoryCell::Assistant(text), HistoryRenderMode::Prompt) => text.clone(),
            (HistoryCell::Assistant(text), HistoryRenderMode::Transcript) => {
                format!("assistant: {text}")
            }
            (HistoryCell::System(text), HistoryRenderMode::Prompt) => format!("sys: {text}"),
            (HistoryCell::System(text), HistoryRenderMode::Transcript) => {
                format!("system: {text}")
            }
            (HistoryCell::Tool { name, summary }, HistoryRenderMode::Prompt) => {
                format!("tool {name}: {summary}")
            }
            (HistoryCell::Tool { name, summary }, HistoryRenderMode::Transcript) => {
                format!("tool-call {name}\n  {summary}")
            }
            (
                HistoryCell::Diff {
                    path,
                    added,
                    removed,
                },
                _,
            ) => {
                format!("diff {path} +{added} -{removed}")
            }
            (HistoryCell::Status(text), _) => format!("status: {text}"),
        }
    }

    /// Render as a styled ratatui Line with theme colors.
    pub fn render_styled(&self, mode: HistoryRenderMode, theme: &Theme) -> Line<'static> {
        match (self, mode) {
            (HistoryCell::User(text), _) => Line::from(vec![
                Span::styled("> ", theme.user_name),
                Span::styled(text.clone(), Style::default()),
            ]),
            (HistoryCell::Assistant(text), _) => {
                Line::from(Span::styled(text.clone(), Style::default()))
            }
            (HistoryCell::System(text), _) => Line::from(vec![
                Span::styled("system: ", theme.system_name),
                Span::styled(text.clone(), theme.dim),
            ]),
            (HistoryCell::Tool { name, summary }, _) => Line::from(vec![
                Span::styled(format!("tool {name}: "), theme.tool_name),
                Span::styled(summary.clone(), theme.tool_result),
            ]),
            (
                HistoryCell::Diff {
                    path,
                    added,
                    removed,
                },
                _,
            ) => Line::from(vec![
                Span::styled("diff ", theme.dim),
                Span::styled(path.clone(), theme.diff_header),
                Span::styled(format!(" +{added}"), theme.diff_add),
                Span::styled(format!(" -{removed}"), theme.diff_remove),
            ]),
            (HistoryCell::Status(text), _) => {
                Line::from(Span::styled(format!("status: {text}"), theme.dim))
            }
        }
    }
}

pub fn render_history(cells: &[HistoryCell], mode: HistoryRenderMode) -> String {
    cells
        .iter()
        .map(|cell| cell.render(mode))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Render history cells as styled ratatui lines.
pub fn render_styled_history(
    cells: &[HistoryCell],
    mode: HistoryRenderMode,
    theme: &Theme,
) -> Vec<Line<'static>> {
    cells
        .iter()
        .map(|cell| cell.render_styled(mode, theme))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_and_styled_history_cover_all_cell_kinds() {
        let cells = vec![
            HistoryCell::User("hi".into()),
            HistoryCell::Assistant("hello".into()),
            HistoryCell::System("boot".into()),
            HistoryCell::Tool {
                name: "Read".into(),
                summary: "src/main.rs".into(),
            },
            HistoryCell::Diff {
                path: "src/main.rs".into(),
                added: 2,
                removed: 1,
            },
            HistoryCell::Status("done".into()),
        ];

        assert_eq!(cells[2].role(), "system");
        assert!(render_history(&cells, HistoryRenderMode::Prompt).contains("sys: boot"));
        assert_eq!(
            render_styled_history(&cells, HistoryRenderMode::Transcript, &Theme::default()).len(),
            cells.len()
        );
    }
}
