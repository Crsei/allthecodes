#[cfg(test)]
use std::path::Path;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget};

use crate::ui::keyboard_shortcut::{render_shortcut_hints, ShortcutHint};
use crate::ui::selection_surface::SelectionSurface;
use crate::ui::theme::Theme;

use super::edit_targets::{has_edit_target_picker, EditTarget};
#[cfg(test)]
use super::filter::command_from_argument_input;
use super::filter::CommandGroup;
use super::CommandItem;
use super::{
    CommandPalette, MAX_EDIT_ROWS, MAX_EDIT_TARGET_ROWS, MAX_ROWS, RESERVED_NON_COMMAND_ROWS,
};

const COMMAND_COLUMN_WIDTH: usize = 34;

impl CommandPalette {
    pub fn render(&self, area: Rect, buf: &mut Buffer, theme: &Theme) {
        if area.height == 0 || area.width == 0 || !self.active {
            return;
        }

        Clear.render(area, buf);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(if self.edit_target_picker.is_some() {
                " Commands - Edit targets "
            } else {
                " Commands "
            })
            .border_style(theme.dim);
        let inner = block.inner(area);
        block.render(area, buf);

        if self.filtered.is_empty() {
            let line = Line::from(Span::styled("No matching commands", theme.dim));
            buf.set_line(inner.x, inner.y, &line, inner.width);
            return;
        }

        let selected_item = self.filtered.get(self.selected);
        let reserved_rows = RESERVED_NON_COMMAND_ROWS
            + self
                .edit_target_picker
                .as_ref()
                .map(|picker| picker.items.len().min(MAX_EDIT_TARGET_ROWS) + 3)
                .unwrap_or(0);
        let visible_rows = (inner.height as usize)
            .saturating_sub(reserved_rows)
            .clamp(1, MAX_ROWS);
        let mut lines = Vec::new();
        lines.push(Line::from(vec![
            Span::styled(format!("{:<COMMAND_COLUMN_WIDTH$}", "Commands"), theme.dim),
            Span::styled("Command details", theme.dim),
        ]));
        let visible_start = visible_window_start(self.filtered.len(), self.selected, visible_rows);

        // Track group transitions for group headers
        let mut last_group: Option<CommandGroup> = None;

        for (idx, item) in self
            .filtered
            .iter()
            .enumerate()
            .skip(visible_start)
            .take(visible_rows)
        {
            let selected = idx == self.selected;
            let style = if selected {
                theme.selected
            } else {
                theme.unselected
            };
            let command_label = command_label(item);
            let detail = truncate(&item.description, 46);

            // Group header on group transitions (only for non-empty query)
            if self.query.is_empty() {
                // Infer group from command name priority
                // For simplicity, we use a heuristic: inline commands are Builtin,
                // plugin/skill entries from dynamic registry would have source_group set
                let current_group = item.source_group.unwrap_or("Builtin");
                let last_group_str = last_group.map(|g| g.label()).unwrap_or("");
                if current_group != last_group_str {
                    let group_label = format!("  \u{2500} {} \u{2500}", current_group);
                    lines.push(Line::from(Span::styled(
                        group_label,
                        Style::default()
                            .fg(Color::DarkGray)
                            .add_modifier(Modifier::DIM),
                    )));
                }
                last_group = Some(CommandGroup::Builtin);
            }

            lines.push(Line::from(vec![
                Span::styled(
                    format!(
                        "{:<COMMAND_COLUMN_WIDTH$}",
                        truncate(&command_label, COMMAND_COLUMN_WIDTH)
                    ),
                    style,
                ),
                Span::styled(detail, theme.unselected),
            ]));
        }

        if let Some(selected) = selected_item {
            lines.push(Line::default());
            lines.push(Line::from(Span::styled("Behavior", theme.dim)));
            lines.push(Line::from(vec![
                Span::styled("  Enter runs: ", theme.dim),
                Span::styled(format!("/{}", selected.name), theme.info),
            ]));
            // Ghost suffix hint
            let ghost = self.selected_ghost_suffix();
            if let Some(ghost_str) = ghost.filter(|s| !s.is_empty()) {
                lines.push(Line::from(vec![
                    Span::styled("  ghost: ", theme.dim),
                    Span::styled(ghost_str, theme.dim),
                ]));
            }
            lines.push(Line::from(vec![
                Span::styled("  usage: ", theme.dim),
                Span::styled(selected.usage.clone(), theme.info),
            ]));
            if let Some(example) = selected.examples.first() {
                lines.push(Line::from(vec![
                    Span::styled("  example: ", theme.dim),
                    Span::styled(example.clone(), theme.unselected),
                ]));
            }
            if !selected.edit_targets.is_empty() {
                for (idx, text) in
                    edit_target_lines(&selected.edit_targets, inner.width as usize, MAX_EDIT_ROWS)
                        .into_iter()
                        .enumerate()
                {
                    let label = if idx == 0 { "  edit: " } else { "        " };
                    lines.push(Line::from(vec![
                        Span::styled(label, theme.dim),
                        Span::styled(text, theme.info),
                    ]));
                }
                if has_edit_target_picker(selected) {
                    lines.push(Line::from(vec![
                        Span::styled("  target: ", theme.dim),
                        Span::styled(command_target_hint(), theme.info),
                    ]));
                }
            }
            lines.push(Line::from(Span::styled(command_palette_hint(), theme.dim)));
        }

        if let Some(picker) = &self.edit_target_picker {
            lines.push(Line::default());
            lines.push(Line::from(vec![
                Span::styled("Targets               Target details", theme.dim),
                Span::styled(command_target_picker_hint(), theme.dim),
            ]));
            lines.extend(render_picker_lines(picker, MAX_EDIT_TARGET_ROWS, theme));
        }

        Paragraph::new(lines).render(inner, buf);
    }

    #[cfg(test)]
    pub fn render_argument_help(
        input: &str,
        cwd: &Path,
        area: Rect,
        buf: &mut Buffer,
        theme: &Theme,
    ) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        let Some(item) = command_from_argument_input(input, cwd) else {
            return;
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" /{} arguments ", item.name))
            .border_style(theme.dim);
        let inner = block.inner(area);
        block.render(area, buf);

        let usage = Line::from(vec![
            Span::styled("Usage: ", theme.dim),
            Span::styled(truncate(&item.usage, inner.width as usize), theme.info),
        ]);
        set_inner_line(buf, inner, 0, &usage);

        let example = item
            .examples
            .first()
            .map(|example| format!("Example: {example}"))
            .unwrap_or_else(|| "Type arguments after the command, then press Enter.".to_string());
        let example_line = Line::from(Span::styled(
            truncate(&example, inner.width as usize),
            theme.unselected,
        ));
        set_inner_line(buf, inner, 1, &example_line);

        let about_line = Line::from(vec![
            Span::styled("About: ", theme.dim),
            Span::styled(
                truncate(&item.description, inner.width as usize),
                theme.unselected,
            ),
        ]);
        set_inner_line(buf, inner, 2, &about_line);

        if !item.edit_targets.is_empty() {
            for (idx, text) in
                edit_target_lines(&item.edit_targets, inner.width as usize, MAX_EDIT_ROWS)
                    .into_iter()
                    .enumerate()
            {
                let row_idx = 3 + idx;
                if row_idx as u16 >= inner.height {
                    break;
                }
                let label = if idx == 0 { "Edit: " } else { "      " };
                let edit_line = Line::from(vec![
                    Span::styled(label, theme.dim),
                    Span::styled(text, theme.link),
                ]);
                set_inner_line(buf, inner, row_idx as u16, &edit_line);
            }
            if has_edit_target_picker(&item) {
                let hint = Line::from(vec![
                    Span::styled("Hint: ", theme.dim),
                    Span::styled(command_target_hint(), theme.info),
                ]);
                let row_idx = 3 + item.edit_targets.len().min(MAX_EDIT_ROWS);
                if (row_idx as u16) < inner.height {
                    set_inner_line(buf, inner, row_idx as u16, &hint);
                }
            }
        }
    }
}

fn command_label(item: &CommandItem) -> String {
    if item.aliases.is_empty() {
        format!("/{}", item.name)
    } else {
        format!("/{}({})", item.name, item.aliases.join(", "))
    }
}

pub(super) fn visible_window_start(total: usize, selected: usize, max_rows: usize) -> usize {
    if max_rows == 0 || total <= max_rows {
        return 0;
    }

    selected.saturating_add(1).saturating_sub(max_rows)
}

#[cfg(test)]
fn set_inner_line(buf: &mut Buffer, inner: Rect, row: u16, line: &Line<'_>) {
    if row < inner.height {
        buf.set_line(inner.x, inner.y + row, line, inner.width);
    }
}

#[cfg(test)]
pub(super) fn argument_edit_row_count(item: &CommandItem) -> usize {
    if item.edit_targets.is_empty() {
        0
    } else {
        item.edit_targets.len().min(MAX_EDIT_ROWS)
    }
}

fn truncate(s: &str, max_width: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_width {
        s.to_string()
    } else if max_width <= 3 {
        ".".repeat(max_width)
    } else {
        format!("{}...", chars[..max_width - 3].iter().collect::<String>())
    }
}

fn edit_target_lines(targets: &[EditTarget], max_width: usize, max_lines: usize) -> Vec<String> {
    if targets.is_empty() || max_width == 0 || max_lines == 0 {
        return Vec::new();
    }

    let fragments = targets
        .iter()
        .map(|target| format!("{}: {}", target.label, target.display))
        .collect::<Vec<_>>();
    let mut lines = Vec::new();
    let mut current = String::new();

    for fragment in fragments {
        let prefix_width = if lines.is_empty() {
            "Edit: ".len()
        } else {
            "      ".len()
        };
        let available = max_width.saturating_sub(prefix_width).max(1);
        let separator = if current.is_empty() { "" } else { " | " };
        let candidate_width = current.len() + separator.len() + fragment.len();

        if candidate_width <= available {
            if !current.is_empty() {
                current.push_str(separator);
            }
            current.push_str(&fragment);
            continue;
        }

        if !current.is_empty() {
            lines.push(current);
            if lines.len() == max_lines {
                return lines;
            }
        }

        let available = max_width.saturating_sub("      ".len()).max(1);
        current = truncate(&fragment, available);
    }

    if !current.is_empty() && lines.len() < max_lines {
        lines.push(current);
    }

    lines
}

fn render_picker_lines(
    picker: &SelectionSurface,
    max_rows: usize,
    theme: &Theme,
) -> Vec<Line<'static>> {
    picker
        .render_lines(max_rows)
        .into_iter()
        .enumerate()
        .map(|(idx, text)| {
            let style = if idx == 0 {
                theme.dim
            } else if text.starts_with('>') {
                theme.selected
            } else {
                theme.unselected
            };
            Line::from(Span::styled(text, style))
        })
        .collect()
}

fn command_palette_hint() -> String {
    render_shortcut_hints(&[
        ShortcutHint::new("Enter", "run"),
        ShortcutHint::new("Space", "add args"),
        ShortcutHint::new("Esc", "close"),
        ShortcutHint::new("type", "filter"),
    ])
}

fn command_target_hint() -> String {
    render_shortcut_hints(&[ShortcutHint::new("Ctrl+E", "pick target")])
}

fn command_target_picker_hint() -> String {
    render_shortcut_hints(&[
        ShortcutHint::new("Enter", "insert"),
        ShortcutHint::new("Ctrl+E", "close"),
    ])
}
