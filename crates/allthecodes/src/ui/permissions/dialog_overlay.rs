use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::ui::approval_overlay::ApprovalKind;
use crate::ui::better_view_panel::NAV_WIDTH;
use crate::ui::panel_layout::PanelSizePreset;
use crate::ui::permissions::permission_request_router::{
    PermissionDialogRequest, PermissionRequestRouter,
};
use crate::ui::theme::Theme;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// The user's response to a permission prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionDecisionChoice {
    /// Allow this single invocation.
    Allow,
    /// Deny this single invocation.
    Deny,
    /// Always allow this tool (add a permanent rule).
    AlwaysAllow,
    /// Ask the automatic approval reviewer to decide this request.
    AutoReview,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionChoice {
    pub decision: PermissionDecisionChoice,
    pub feedback: String,
}

impl PermissionChoice {
    fn from_decision(decision: PermissionDecisionChoice) -> Self {
        Self {
            decision,
            feedback: String::new(),
        }
    }

    fn with_feedback(decision: PermissionDecisionChoice, feedback: &str) -> Self {
        Self {
            decision,
            feedback: feedback.trim().to_string(),
        }
    }
}

const DEFAULT_OPTIONS: [&str; 4] = ["Allow", "Deny", "Always Allow", "Auto Review"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PermissionDialogMode {
    Selecting,
    TypingFeedback { target: PermissionDecisionChoice },
}

/// An overlay dialog that asks the user whether to permit a tool invocation.
pub struct PermissionDialog {
    /// Complete permission request payload used by the dispatcher.
    pub request: PermissionDialogRequest,
    /// Current permission category inferred from the tool name.
    kind: ApprovalKind,
    /// Button labels shown at the bottom of the dialog.
    options: Vec<String>,
    /// Currently highlighted choice.
    pub selected: usize,
    mode: PermissionDialogMode,
    accept_feedback: String,
    reject_feedback: String,
    show_raw_details: bool,
}

impl PermissionDialog {
    pub fn new(tool_name: &str, input: &str, message: &str) -> Self {
        Self::from_request(PermissionDialogRequest::legacy(tool_name, input, message))
    }

    pub fn from_request(request: PermissionDialogRequest) -> Self {
        let routed = PermissionRequestRouter::route(&request, 0);
        let input_summary = request.input_summary();
        let kind = approval_kind(&request.tool_name, &input_summary, &request.message);
        let options = if routed.options.is_empty() {
            default_options()
        } else {
            routed.options
        };
        Self {
            request,
            kind,
            options,
            selected: 0,
            mode: PermissionDialogMode::Selecting,
            accept_feedback: String::new(),
            reject_feedback: String::new(),
            show_raw_details: false,
        }
    }

    /// Handle a key event. Returns `Some(choice)` when the user confirms a
    /// selection with Enter, or makes a direct choice via a keyboard shortcut.
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<PermissionChoice> {
        if let PermissionDialogMode::TypingFeedback { target } = self.mode {
            return self.handle_feedback_key(key, target);
        }

        let choice_count = self.normalized_options().len().max(1);

        match (key.modifiers, key.code) {
            // Navigation
            (_, KeyCode::Left) | (_, KeyCode::Char('h')) => {
                self.selected = self.selected.saturating_sub(1);
            }
            (_, KeyCode::Right) | (_, KeyCode::Char('l')) => {
                if self.selected + 1 < choice_count {
                    self.selected += 1;
                }
            }
            (_, KeyCode::Tab) => {
                if let Some(target) = self.feedback_target_for_selected() {
                    self.mode = PermissionDialogMode::TypingFeedback { target };
                } else {
                    self.selected = (self.selected + 1) % choice_count;
                }
            }
            (KeyModifiers::SHIFT, KeyCode::BackTab) => {
                self.selected = if self.selected == 0 {
                    choice_count - 1
                } else {
                    self.selected - 1
                };
            }

            // Confirm
            (_, KeyCode::Enter) => {
                return Some(self.choice_for_selected());
            }

            // Quick keys
            (_, KeyCode::Char('y')) | (_, KeyCode::Char('Y')) => {
                return Some(PermissionChoice::from_decision(
                    PermissionDecisionChoice::Allow,
                ));
            }
            (_, KeyCode::Char('n')) | (_, KeyCode::Char('N')) => {
                return Some(PermissionChoice::from_decision(
                    PermissionDecisionChoice::Deny,
                ));
            }
            (_, KeyCode::Char('a')) | (_, KeyCode::Char('A')) => {
                return Some(PermissionChoice::from_decision(
                    PermissionDecisionChoice::AlwaysAllow,
                ));
            }
            (_, KeyCode::Char('r')) | (_, KeyCode::Char('R')) => {
                return Some(PermissionChoice::from_decision(
                    PermissionDecisionChoice::AutoReview,
                ));
            }
            (_, KeyCode::Char('e')) | (_, KeyCode::Char('E')) => {
                self.show_raw_details = !self.show_raw_details;
            }
            (_, KeyCode::Esc) => {
                return Some(PermissionChoice::from_decision(
                    PermissionDecisionChoice::Deny,
                ));
            }

            _ => {}
        }
        None
    }

    /// Render the permission dialog as a centered or prompt-adjacent overlay.
    pub fn render(&self, area: Rect, prompt_area: Option<Rect>, buf: &mut Buffer, theme: &Theme) {
        let spec = PanelSizePreset::PermissionDialog.spec();
        let dialog_width = spec
            .resolve_prompt_or_centered_rect(area, prompt_area, spec.min_height)
            .map(|rect| rect.width)
            .unwrap_or(area.width);
        let labels = self.normalized_options();
        let estimated_footer_width = dialog_width.saturating_sub(4) as usize;
        let estimated_button_rows =
            button_lines_for_width(&labels, self.selected, estimated_footer_width, theme).len();
        let footer_height = (estimated_button_rows as u16)
            .saturating_add(if self.is_typing_feedback() { 2 } else { 1 })
            .max(if self.is_typing_feedback() { 4 } else { 3 });
        let preferred_height =
            footer_height.saturating_add(if self.is_typing_feedback() { 14 } else { 12 });
        let dialog_area = spec
            .resolve_prompt_or_centered_rect(area, prompt_area, preferred_height)
            .unwrap_or(Rect::new(area.x, area.y, area.width, area.height));

        Clear.render_ref(dialog_area, buf);

        let block = Block::default()
            .title(format!(" Permission Required - {} ", self.kind.title()))
            .borders(Borders::ALL)
            .border_style(theme.warning)
            .style(Style::default());
        let inner = block.inner(dialog_area);
        block.render_ref(dialog_area, buf);

        if inner.height == 0 || inner.width == 0 {
            return;
        }

        let chunks = Layout::vertical([
            Constraint::Length(2),             // tool info
            Constraint::Min(2),                // request body
            Constraint::Length(footer_height), // buttons + feedback/hint
        ])
        .split(inner);

        let tool_info = vec![
            Line::from(vec![
                Span::styled("Tool: ", theme.dim),
                Span::styled(self.request.tool_name.clone(), theme.tool_name),
            ]),
            Line::from(vec![
                Span::styled("Kind: ", theme.dim),
                Span::styled(
                    self.kind.title().trim_end_matches('?').to_string(),
                    theme.info,
                ),
            ]),
        ];
        Paragraph::new(tool_info).render_ref(chunks[0], buf);

        let request_block = Block::default()
            .borders(Borders::LEFT)
            .border_style(theme.warning);
        let request_inner = request_block.inner(chunks[1]);
        request_block.render_ref(chunks[1], buf);
        if request_inner.height > 0 && request_inner.width > 0 {
            let body_lines =
                self.body_lines(request_inner.width as usize, request_inner.height, theme);

            Paragraph::new(body_lines)
                .wrap(Wrap { trim: true })
                .render_ref(request_inner, buf);
        }

        let footer_width = chunks[2].width.saturating_sub(2) as usize;
        let button_lines = button_lines_for_width(&labels, self.selected, footer_width, theme);
        let reserved_footer_rows = 1 + u16::from(self.is_typing_feedback());
        let max_button_rows = chunks[2].height.saturating_sub(reserved_footer_rows).max(1) as usize;
        for (idx, button_line) in button_lines.iter().take(max_button_rows).enumerate() {
            buf.set_line(
                chunks[2].x + 1,
                chunks[2].y + idx as u16,
                button_line,
                footer_width as u16,
            );
        }

        if self.is_typing_feedback() && chunks[2].height >= 2 {
            let feedback = match self.mode {
                PermissionDialogMode::TypingFeedback { target } => self.feedback_for(target),
                PermissionDialogMode::Selecting => "",
            };
            let input = Line::from(vec![
                Span::styled("Feedback: ", theme.dim),
                Span::styled(
                    truncate_str(feedback, footer_width.saturating_sub(10)),
                    theme.info,
                ),
            ]);
            buf.set_line(
                chunks[2].x + 1,
                chunks[2]
                    .y
                    .saturating_add(button_lines.len().min(max_button_rows) as u16),
                &input,
                footer_width as u16,
            );
        }

        let (full_hint, compact_hint) = match self.feedback_target_for_selected() {
            Some(PermissionDecisionChoice::Allow) => (
                "Arrows/hotkeys. Enter confirms. R auto review. E details. Esc denies. Tab: tell model what to do differently.",
                "Tab: tell model what to do differently.",
            ),
            Some(PermissionDecisionChoice::Deny) => (
                "Arrows/hotkeys. Enter confirms. R auto review. E details. Esc denies. Tab: tell model what to do differently.",
                "Tab: tell model what to do differently.",
            ),
            _ => (
                "Arrows/hotkeys. Enter confirms. R auto review. E details. Esc denies.",
                "R auto review. E details. Esc denies.",
            ),
        };
        let hint_text = if self.is_typing_feedback() {
            "Enter submits with feedback. Esc cancels input. Tab closes input."
        } else if full_hint.chars().count() <= footer_width {
            full_hint
        } else {
            compact_hint
        };
        let hint = Line::from(vec![Span::styled(
            truncate_str(hint_text, footer_width),
            theme.dim,
        )]);
        buf.set_line(
            chunks[2].x + 1,
            chunks[2].y + chunks[2].height.saturating_sub(1),
            &hint,
            footer_width as u16,
        );
    }

    fn body_text(&self) -> String {
        let summary = self.request.input_summary();
        if summary != "(no details supplied)" {
            summary
        } else if !self.request.message.trim().is_empty() {
            self.request.message.clone()
        } else {
            "(no details supplied)".to_string()
        }
    }

    fn body_lines(&self, max_width: usize, max_height: u16, theme: &Theme) -> Vec<Line<'static>> {
        let routed = PermissionRequestRouter::route(&self.request, self.selected);
        let max_detail_lines = max_height.saturating_sub(1) as usize;
        let mut detail_texts = routed_detail_lines(&routed.rendered);
        if self.show_raw_details {
            let raw_lines = self.raw_detail_lines();
            if !raw_lines.is_empty() {
                let raw_reserved_lines = raw_lines.len().saturating_add(1);
                let routed_budget = max_detail_lines.saturating_sub(raw_reserved_lines);
                detail_texts.truncate(routed_budget);
                detail_texts.push("raw details".to_string());
                detail_texts.extend(raw_lines);
            }
        }
        let routed_lines = detail_texts
            .into_iter()
            .take(max_detail_lines)
            .map(|line| {
                Line::from(vec![Span::styled(
                    truncate_str(&line, max_width),
                    theme_style_for_routed_line(&line, theme),
                )])
            })
            .collect::<Vec<_>>();

        if !routed_lines.is_empty() {
            return routed_lines;
        }

        let body_label = body_label_for_kind(&self.kind);
        let body_text = self.body_text();
        let mut body_lines = vec![Line::from(vec![
            Span::styled(format!("{body_label}: "), theme.dim),
            Span::styled(truncate_str(&body_text, max_width), theme.warning),
        ])];

        if !self.request.message.trim().is_empty()
            && self.request.message.trim() != body_text.trim()
        {
            body_lines.push(Line::from(vec![Span::styled(
                truncate_str(&self.request.message, max_width),
                theme.dim,
            )]));
        }
        if self.show_raw_details {
            body_lines.extend(self.raw_detail_lines().into_iter().map(|line| {
                Line::from(vec![Span::styled(
                    truncate_str(&line, max_width),
                    theme.dim,
                )])
            }));
        }
        body_lines
    }

    fn raw_detail_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if !self.request.message.trim().is_empty() {
            lines.push(format!("message: {}", self.request.message.trim()));
        }
        if !matches!(self.request.tool_input, serde_json::Value::Null) {
            lines.push("input:".to_string());
            let input = serde_json::to_string_pretty(&self.request.tool_input)
                .unwrap_or_else(|_| self.request.tool_input.to_string());
            lines.extend(input.lines().map(|line| format!("  {line}")));
        }
        if lines.is_empty() {
            lines.push("raw: (empty)".to_string());
        }
        lines
    }

    fn normalized_options(&self) -> Vec<String> {
        if self.options.is_empty() {
            default_options()
        } else {
            self.options.clone()
        }
    }

    fn choice_for_selected(&self) -> PermissionChoice {
        let labels = self.normalized_options();
        labels
            .get(self.selected.min(labels.len().saturating_sub(1)))
            .map(|label| {
                PermissionChoice::from_decision(
                    choice_for_label(label).unwrap_or(PermissionDecisionChoice::Deny),
                )
            })
            .unwrap_or_else(|| PermissionChoice::from_decision(PermissionDecisionChoice::Deny))
    }

    fn handle_feedback_key(
        &mut self,
        key: KeyEvent,
        target: PermissionDecisionChoice,
    ) -> Option<PermissionChoice> {
        match (key.modifiers, key.code) {
            (_, KeyCode::Enter) => {
                let feedback = self.feedback_for(target).to_string();
                return Some(PermissionChoice::with_feedback(target, &feedback));
            }
            (_, KeyCode::Esc) | (_, KeyCode::Tab) => {
                self.mode = PermissionDialogMode::Selecting;
            }
            (KeyModifiers::CONTROL, KeyCode::Char('u')) => {
                self.feedback_for_mut(target).clear();
            }
            (_, KeyCode::Backspace) => {
                let feedback = self.feedback_for_mut(target);
                if let Some((start, _)) = feedback.grapheme_indices(true).next_back() {
                    feedback.truncate(start);
                }
            }
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(ch)) => {
                self.feedback_for_mut(target).push(ch);
            }
            _ => {}
        }
        None
    }

    fn feedback_target_for_selected(&self) -> Option<PermissionDecisionChoice> {
        let labels = self.normalized_options();
        labels
            .get(self.selected.min(labels.len().saturating_sub(1)))
            .and_then(|label| match choice_for_label(label) {
                Some(PermissionDecisionChoice::Allow) => Some(PermissionDecisionChoice::Allow),
                Some(PermissionDecisionChoice::Deny) => Some(PermissionDecisionChoice::Deny),
                _ => None,
            })
    }

    fn feedback_for(&self, target: PermissionDecisionChoice) -> &str {
        match target {
            PermissionDecisionChoice::Allow => &self.accept_feedback,
            PermissionDecisionChoice::Deny => &self.reject_feedback,
            PermissionDecisionChoice::AlwaysAllow => "",
            PermissionDecisionChoice::AutoReview => "",
        }
    }

    fn feedback_for_mut(&mut self, target: PermissionDecisionChoice) -> &mut String {
        match target {
            PermissionDecisionChoice::Allow => &mut self.accept_feedback,
            PermissionDecisionChoice::Deny => &mut self.reject_feedback,
            PermissionDecisionChoice::AlwaysAllow => &mut self.accept_feedback,
            PermissionDecisionChoice::AutoReview => &mut self.reject_feedback,
        }
    }

    pub fn is_typing_feedback(&self) -> bool {
        matches!(self.mode, PermissionDialogMode::TypingFeedback { .. })
    }

    /// Return the physical cursor cell for the feedback field. Selection
    /// mode is deliberately read-only and therefore returns no placement.
    pub fn cursor_position(
        &self,
        area: Rect,
        prompt_area: Option<Rect>,
        theme: &Theme,
    ) -> Option<Position> {
        let PermissionDialogMode::TypingFeedback { target } = self.mode else {
            return None;
        };
        let spec = PanelSizePreset::PermissionDialog.spec();
        let dialog_width = spec
            .resolve_prompt_or_centered_rect(area, prompt_area, spec.min_height)
            .map(|rect| rect.width)
            .unwrap_or(area.width);
        let labels = self.normalized_options();
        let estimated_footer_width = dialog_width.saturating_sub(4) as usize;
        let estimated_button_rows =
            button_lines_for_width(&labels, self.selected, estimated_footer_width, theme).len();
        let footer_height = (estimated_button_rows as u16).saturating_add(2).max(4);
        let preferred_height = footer_height.saturating_add(14);
        let dialog_area = spec
            .resolve_prompt_or_centered_rect(area, prompt_area, preferred_height)
            .or_else(|| (area.width > 0 && area.height > 0).then_some(area))?;
        let block = Block::default().borders(Borders::ALL);
        let inner = block.inner(dialog_area);
        if inner.width == 0 || inner.height == 0 {
            return None;
        }
        let chunks = Layout::vertical([
            Constraint::Length(2),
            Constraint::Min(2),
            Constraint::Length(footer_height),
        ])
        .split(inner);
        let footer_width = chunks[2].width.saturating_sub(2) as usize;
        let button_lines = button_lines_for_width(&labels, self.selected, footer_width, theme);
        let reserved_footer_rows = 1 + usize::from(self.is_typing_feedback());
        let max_button_rows = usize::from(chunks[2].height)
            .saturating_sub(reserved_footer_rows)
            .max(1);
        let row = chunks[2]
            .y
            .saturating_add(button_lines.len().min(max_button_rows) as u16);
        let feedback = truncate_str(
            self.feedback_for(target),
            footer_width.saturating_sub("Feedback: ".chars().count()),
        );
        let x = chunks[2]
            .x
            .saturating_add(1)
            .saturating_add(UnicodeWidthStr::width("Feedback: ") as u16)
            .saturating_add(UnicodeWidthStr::width(feedback.as_str()) as u16);
        let right = chunks[2].x.saturating_add(chunks[2].width);
        (row < chunks[2].y.saturating_add(chunks[2].height) && x < right)
            .then_some(Position { x, y: row })
    }
}

fn choice_for_label(label: &str) -> Option<PermissionDecisionChoice> {
    let normalized = label.trim().to_ascii_lowercase();
    if normalized.contains("always") {
        return Some(PermissionDecisionChoice::AlwaysAllow);
    }
    if normalized.contains("auto")
        || normalized.contains("review")
        || normalized.contains("escalate")
    {
        return Some(PermissionDecisionChoice::AutoReview);
    }
    if normalized.contains("deny") || normalized.contains("reject") || normalized == "no" {
        return Some(PermissionDecisionChoice::Deny);
    }
    if normalized.contains("allow") || normalized == "yes" {
        return Some(PermissionDecisionChoice::Allow);
    }
    match normalized.as_str() {
        "always_allow" | "always path" | "always exact" => {
            Some(PermissionDecisionChoice::AlwaysAllow)
        }
        "ask lead" => Some(PermissionDecisionChoice::AutoReview),
        _ => None,
    }
}

fn theme_style_for_routed_line(line: &str, theme: &Theme) -> Style {
    if line.contains("risk:") || line.contains("network access") || line.contains("unknown tool") {
        theme.warning.add_modifier(Modifier::BOLD)
    } else {
        theme.warning
    }
}

fn routed_detail_lines(rendered: &str) -> Vec<String> {
    rendered
        .lines()
        .map(panel_line_to_text)
        .filter(|line| !line.is_empty())
        .filter(|line| !is_router_chrome_line(line))
        .take(8)
        .collect()
}

fn panel_line_to_text(line: &str) -> String {
    let trimmed = line.trim();
    if trimmed.starts_with('+')
        || trimmed.starts_with("|---")
        || trimmed.starts_with('┌')
        || trimmed.starts_with('└')
        || trimmed.starts_with("│─")
    {
        return String::new();
    }
    if trimmed.starts_with('|') || trimmed.starts_with('│') {
        let Some(border) = trimmed.chars().next() else {
            return String::new();
        };
        let inner = trimmed
            .strip_prefix(border)
            .unwrap_or(trimmed)
            .strip_suffix(border)
            .unwrap_or_else(|| trimmed.strip_prefix(border).unwrap_or(trimmed));
        let chars = inner.chars().collect::<Vec<_>>();
        let nav_start = 1;
        let nav_end = nav_start + NAV_WIDTH;
        let right_column_start = nav_end + 1;
        if chars.len() > right_column_start {
            let left = chars[nav_start..nav_end.min(chars.len())]
                .iter()
                .collect::<String>();
            if !is_panel_nav_cell(&left) {
                return String::new();
            }
            return chars[right_column_start..]
                .iter()
                .collect::<String>()
                .trim()
                .to_string();
        }
        return String::new();
    }
    trimmed.to_string()
}

fn is_panel_nav_cell(cell: &str) -> bool {
    let label = cell.trim().trim_start_matches('>').trim();
    label.is_empty() || matches!(label, "Request" | "Context" | "Decisions")
}

fn is_router_chrome_line(line: &str) -> bool {
    line.eq_ignore_ascii_case("Request")
        || line.eq_ignore_ascii_case("Context")
        || line.eq_ignore_ascii_case("Decision")
        || line.eq_ignore_ascii_case("Decisions")
        || line.contains("Request") && line.contains("Decision")
        || line.starts_with("tool=")
        || line.starts_with("Up/Down decision")
        || line.contains("scope=")
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::{
        routed_detail_lines, PermissionChoice, PermissionDecisionChoice, PermissionDialog,
    };
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use insta::assert_snapshot;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    use crate::ui::permissions::permission_request_router::PermissionDialogRequest;
    use crate::ui::permissions::web_fetch_permission_request::web_fetch_permission_request::render_web_fetch_permission_request;
    use crate::ui::theme::Theme;

    #[test]
    fn routed_detail_lines_keep_detail_column_without_panel_chrome() {
        let rendered = render_web_fetch_permission_request("https://example.com/docs", "GET", 0);
        let lines = routed_detail_lines(&rendered);

        assert!(lines
            .iter()
            .any(|line| line.contains("https://example.com/docs")));
        assert!(lines.iter().any(|line| line.contains("method: GET")));
        assert!(!lines
            .iter()
            .any(|line| line.contains("Web fetch permission")));
        assert!(!lines
            .iter()
            .any(|line| line.contains("Request") && line.contains("Decision")));
        assert!(!lines.iter().any(|line| line == "Decision"));
        assert!(!lines.iter().any(|line| line == "work access"));
        assert!(!lines.iter().any(|line| line.starts_with("tool=")));
        assert!(!lines.iter().any(|line| line.starts_with("risk=")));
        assert!(!lines.iter().any(|line| line.contains("scope=")));
        assert!(!lines.iter().any(|line| line.contains("Up/Down")));
    }

    #[test]
    fn rendered_dialog_omits_routed_panel_chrome() {
        let dialog = PermissionDialog::new("WebFetch", r#"{"url":"https://example.com/docs"}"#, "");
        let rendered = render_dialog_text(&dialog);

        assert!(rendered.contains("Permission Required"));
        assert!(rendered.contains("https://example.com/docs"));
        assert!(rendered.contains("method: GET"));
        assert!(!rendered.contains("Web fetch permission"));
        assert!(!rendered.contains("tool=web_fetch"));
        assert!(!rendered.contains("risk=network access"));
        assert!(!rendered.contains("scope="));
        assert!(!rendered.contains("Up/Down decision"));
        assert!(!rendered.lines().any(|line| line.trim() == "Decision"));
        assert!(!rendered.lines().any(|line| line.trim() == "work access"));
    }

    #[test]
    fn rendered_dialog_extracts_structured_bash_json() {
        let dialog = PermissionDialog::new("Bash", r#"{"command":"cargo test"}"#, "");
        let rendered = render_dialog_text(&dialog);

        assert!(rendered.contains("command: cargo test"));
        assert!(!rendered.contains(r#"{"command":"#));
    }

    #[test]
    fn bash_dialog_shows_and_selects_always_exact() {
        let mut dialog = PermissionDialog::new("Bash", r#"{"command":"cargo test"}"#, "");
        let rendered = render_dialog_text(&dialog);

        assert!(rendered.contains("Always exact"));
        assert_eq!(
            dialog.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)),
            None
        );
        assert_eq!(
            dialog.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)),
            None
        );
        assert_eq!(
            dialog.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Some(PermissionChoice::from_decision(
                PermissionDecisionChoice::AlwaysAllow,
            ))
        );
    }

    #[test]
    fn rendered_dialog_extracts_structured_file_json() {
        let dialog = PermissionDialog::new("Edit", r#"{"file_path":"src/lib.rs"}"#, "");
        let rendered = render_dialog_text(&dialog);

        assert!(rendered.contains("src/lib.rs"));
        assert!(!rendered.contains(r#"{"file_path":"#));
    }

    #[test]
    fn file_permission_labels_navigate_to_choices() {
        let mut dialog = PermissionDialog::new("Edit", r#"{"file_path":"src/lib.rs"}"#, "");
        let rendered = render_dialog_text(&dialog);

        assert!(rendered.contains("Allow edit"));
        assert!(rendered.contains("Deny edit"));
        assert!(rendered.contains("Always allow path"));

        assert_eq!(
            dialog.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Some(PermissionChoice::from_decision(
                PermissionDecisionChoice::Allow,
            ))
        );

        let mut dialog = PermissionDialog::new("Edit", r#"{"file_path":"src/lib.rs"}"#, "");
        assert_eq!(
            dialog.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)),
            None
        );
        assert_eq!(
            dialog.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Some(PermissionChoice::from_decision(
                PermissionDecisionChoice::Deny,
            ))
        );

        let mut dialog = PermissionDialog::new("Edit", r#"{"file_path":"src/lib.rs"}"#, "");
        assert_eq!(
            dialog.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)),
            None
        );
        assert_eq!(
            dialog.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)),
            None
        );
        assert_eq!(
            dialog.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Some(PermissionChoice::from_decision(
                PermissionDecisionChoice::AlwaysAllow,
            ))
        );
    }

    #[test]
    fn unknown_permission_option_fails_closed() {
        let mut dialog = PermissionDialog::from_request(PermissionDialogRequest {
            tool_use_id: "toolu_1".to_string(),
            tool_name: "Bash".to_string(),
            tool_input: serde_json::json!({ "command": "cargo test" }),
            message: "Allow command?".to_string(),
            options: vec!["Proceed".to_string()],
            operation: None,
            security: None,
        });

        assert_eq!(
            dialog.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Some(PermissionChoice::from_decision(
                PermissionDecisionChoice::Deny,
            ))
        );
    }

    #[test]
    fn tab_opens_allow_feedback_and_enter_submits_it() {
        let mut dialog = PermissionDialog::new("Bash", r#"{"command":"cargo test"}"#, "");

        assert_eq!(
            dialog.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
            None
        );
        for ch in "run tests first".chars() {
            assert_eq!(
                dialog.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE)),
                None
            );
        }

        assert_eq!(
            dialog.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Some(PermissionChoice::with_feedback(
                PermissionDecisionChoice::Allow,
                "run tests first",
            ))
        );
    }

    #[test]
    fn feedback_cursor_uses_display_columns_without_rendering_a_pipe() {
        let mut dialog = PermissionDialog::new("Bash", r#"{"command":"cargo test"}"#, "");
        assert_eq!(
            dialog.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
            None
        );
        for ch in "你e\u{301}".chars() {
            assert_eq!(
                dialog.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE)),
                None
            );
        }

        let area = Rect::new(0, 0, 120, 30);
        let prompt_area = Rect::new(0, 24, 120, 3);
        let position = dialog
            .cursor_position(area, Some(prompt_area), &Theme::default())
            .expect("feedback cursor");
        assert!(position.x < area.x + area.width);

        let rendered = render_dialog_text_in_area(&dialog, area);
        assert!(rendered.contains("Feedback: 你e\u{301}"));
        assert!(!rendered.contains("Feedback: 你e\u{301}|"));
    }

    #[test]
    fn escape_cancels_feedback_before_denial() {
        let mut dialog = PermissionDialog::new("Bash", r#"{"command":"cargo test"}"#, "");

        assert_eq!(
            dialog.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
            None
        );
        assert_eq!(
            dialog.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)),
            None
        );
        assert_eq!(
            dialog.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            None
        );
        assert_eq!(
            dialog.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            Some(PermissionChoice::from_decision(
                PermissionDecisionChoice::Deny,
            ))
        );
    }

    #[test]
    fn auto_review_shortcut_and_expand_details_are_distinct() {
        let mut dialog =
            PermissionDialog::new("Bash", r#"{"command":"cargo test"}"#, "Allow command?");

        let collapsed = render_dialog_text(&dialog);
        assert!(!collapsed.contains("raw details"));

        assert_eq!(
            dialog.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE)),
            None
        );
        let expanded = render_dialog_text(&dialog);
        assert!(expanded.contains("raw details"));
        assert!(expanded.contains("\"command\": \"cargo test\""));

        assert_eq!(
            dialog.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE)),
            Some(PermissionChoice::from_decision(
                PermissionDecisionChoice::AutoReview,
            ))
        );
    }

    #[test]
    fn exit_plan_mode_denial_shows_feedback_path() {
        let mut dialog = PermissionDialog::new(
            "ExitPlanMode",
            r#"{"plan":"1. edit files\n2. run tests","tests":["cargo test"]}"#,
            "",
        );
        dialog.selected = 1;

        let rendered = render_dialog_text(&dialog);

        assert!(rendered.contains("tell model what to do differently"));
    }

    #[test]
    fn snapshot_permission_dialog_expanded_area_shows_more_long_request_text() {
        let dialog = PermissionDialog::new(
            "Bash",
            r#"{"command":"python scripts/run_extremely_long_validation_command.py --workspace /tmp/very/long/path/that/should/be/abbreviated --include snapshots --include truncation --include permissions --include overlays"}"#,
            "This approval request includes a long explanatory message that should be abbreviated inside the request body instead of overflowing the dialog.",
        );
        let rendered = render_dialog_text_in_area(&dialog, Rect::new(0, 0, 152, 24));

        assert_snapshot!("permission_dialog_expanded_long_request_152x24", rendered);
    }

    #[test]
    fn permission_dialog_can_render_above_prompt() {
        let dialog = PermissionDialog::new("Bash", r#"{"command":"cargo test"}"#, "");
        let area = Rect::new(0, 0, 120, 30);
        let prompt_area = Rect::new(0, 24, 120, 3);
        let mut buffer = Buffer::empty(area);

        dialog.render(area, Some(prompt_area), &mut buffer, &Theme::default());

        let rendered = buffer_text(&buffer, area);
        let title_row = rendered
            .lines()
            .position(|line| line.contains("Permission Required"))
            .expect("title row");
        assert!(title_row < prompt_area.y as usize);
    }

    fn render_dialog_text(dialog: &PermissionDialog) -> String {
        render_dialog_text_in_area(dialog, Rect::new(0, 0, 120, 24))
    }

    fn render_dialog_text_in_area(dialog: &PermissionDialog, area: Rect) -> String {
        let mut buffer = Buffer::empty(area);
        dialog.render(area, None, &mut buffer, &Theme::default());
        buffer_text(&buffer, area)
    }

    fn buffer_text(buffer: &Buffer, area: Rect) -> String {
        let mut lines = Vec::new();
        for y in area.y..area.y + area.height {
            let mut line = String::new();
            for x in area.x..area.x + area.width {
                line.push_str(buffer[(x, y)].symbol());
            }
            lines.push(line.trim_end().to_string());
        }
        lines.join("\n")
    }
}

fn approval_kind(tool_name: &str, input: &str, message: &str) -> ApprovalKind {
    let joined = format!("{tool_name} {input} {message}").to_ascii_lowercase();
    let subject = if !input.trim().is_empty() {
        input.trim().to_string()
    } else if !message.trim().is_empty() {
        message.trim().to_string()
    } else {
        tool_name.to_string()
    };

    if joined.contains("bash")
        || joined.contains("shell")
        || joined.contains("terminal")
        || joined.contains("powershell")
    {
        ApprovalKind::Bash { command: subject }
    } else if joined.contains("write") || joined.contains("edit") || joined.contains("patch") {
        ApprovalKind::FileEdit { path: subject }
    } else if joined.contains("fetch") || joined.contains("web") || joined.contains("url") {
        ApprovalKind::WebFetch { url: subject }
    } else if joined.contains("mcp") {
        ApprovalKind::Mcp {
            server: tool_name.to_string(),
            action: subject,
        }
    } else if joined.contains("ask") || joined.contains("question") {
        ApprovalKind::UserInput { prompt: subject }
    } else {
        ApprovalKind::Fallback { summary: subject }
    }
}

fn body_label_for_kind(kind: &ApprovalKind) -> &'static str {
    match kind {
        ApprovalKind::Bash { .. } => "Command",
        ApprovalKind::FileEdit { .. } => "File",
        ApprovalKind::WebFetch { .. } => "URL",
        ApprovalKind::Mcp { .. } => "Action",
        ApprovalKind::UserInput { .. } => "Prompt",
        ApprovalKind::Fallback { .. } => "Details",
    }
}

fn default_options() -> Vec<String> {
    DEFAULT_OPTIONS
        .iter()
        .map(|value| (*value).to_string())
        .collect()
}

fn shortcut_for_label(label: &str) -> Option<&'static str> {
    match choice_for_label(label) {
        Some(PermissionDecisionChoice::Allow) => Some("y"),
        Some(PermissionDecisionChoice::Deny) => Some("n"),
        Some(PermissionDecisionChoice::AlwaysAllow) => Some("a"),
        Some(PermissionDecisionChoice::AutoReview) => Some("r"),
        None => None,
    }
}

fn button_lines_for_width(
    labels: &[String],
    selected: usize,
    max_width: usize,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let single_line = button_spans(labels, selected, max_width, theme, true);
    if spans_width(&single_line) <= max_width {
        return vec![Line::from(single_line)];
    }

    let mut rows: Vec<Vec<Span<'static>>> = Vec::new();
    let mut current: Vec<Span<'static>> = Vec::new();
    let mut current_width = 0usize;

    for (idx, label) in labels.iter().enumerate() {
        let mut button = button_span(label, idx == selected, theme, true);
        let mut button_width = spans_width(&button);
        if button_width > max_width {
            button = vec![Span::styled(
                truncate_str(&spans_text(&button), max_width),
                if idx == selected {
                    theme.selected
                } else {
                    theme.unselected
                },
            )];
            button_width = spans_width(&button);
        }

        let separator_width = usize::from(!current.is_empty()) * 2;
        if !current.is_empty() && current_width + separator_width + button_width > max_width {
            rows.push(current);
            current = Vec::new();
            current_width = 0;
        }

        if !current.is_empty() {
            current.push(Span::raw("  "));
            current_width += 2;
        }
        current.extend(button);
        current_width += button_width;
    }

    if !current.is_empty() {
        rows.push(current);
    }

    if rows.is_empty() {
        rows.push(vec![Span::styled("".to_string(), theme.unselected)]);
    }

    rows.into_iter().map(Line::from).collect()
}

fn button_spans(
    labels: &[String],
    selected: usize,
    _max_width: usize,
    theme: &Theme,
    padded: bool,
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for (idx, label) in labels.iter().enumerate() {
        spans.extend(button_span(label, idx == selected, theme, padded));
        if idx + 1 < labels.len() {
            spans.push(Span::raw(if padded { "  " } else { " " }));
        }
    }
    spans
}

fn button_span(label: &str, selected: bool, theme: &Theme, padded: bool) -> Vec<Span<'static>> {
    let style = if selected {
        theme.selected
    } else {
        theme.unselected
    };
    let label = button_bar_label(label);
    let shortcut = shortcut_for_label(&label);
    let text = match (padded, shortcut) {
        (true, Some(shortcut)) => format!(" {label} ({shortcut}) "),
        (true, None) => format!(" {label} "),
        (false, Some(shortcut)) => format!("{label}({shortcut})"),
        (false, None) => label,
    };
    vec![Span::styled(text, style)]
}

fn button_bar_label(label: &str) -> String {
    let lower = label.to_ascii_lowercase();
    if lower.contains("allow edit") {
        "Allow edit".to_string()
    } else if lower.contains("deny edit") {
        "Deny edit".to_string()
    } else if lower.contains("always") && lower.contains("path") {
        "Always allow path".to_string()
    } else if lower.contains("always") && lower.contains("exact") {
        "Always exact".to_string()
    } else if lower == "always allow" {
        "Always".to_string()
    } else if lower.contains("auto") || lower.contains("review") || lower.contains("escalate") {
        "Auto Review".to_string()
    } else if lower.contains("allow") {
        "Allow".to_string()
    } else if lower.contains("deny") || lower.contains("reject") || lower == "no" {
        "Deny".to_string()
    } else if lower == "yes" {
        "Allow".to_string()
    } else {
        "Select".to_string()
    }
}

fn spans_width(spans: &[Span<'_>]) -> usize {
    spans.iter().map(|span| span.content.chars().count()).sum()
}

fn spans_text(spans: &[Span<'_>]) -> String {
    spans.iter().map(|span| span.content.as_ref()).collect()
}

// ── Helper rendering traits ───────────────────────────────────────────────

trait RenderRef {
    fn render_ref(&self, area: Rect, buf: &mut Buffer);
}

impl RenderRef for Clear {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        for y in area.y..area.y + area.height {
            for x in area.x..area.x + area.width {
                if let Some(cell) = buf.cell_mut((x, y)) {
                    cell.reset();
                }
            }
        }
    }
}

impl RenderRef for Block<'_> {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        ratatui::widgets::Widget::render(self.clone(), area, buf);
    }
}

impl RenderRef for Paragraph<'_> {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        ratatui::widgets::Widget::render(self.clone(), area, buf);
    }
}

/// Truncate a string to a terminal display width, appending "..." when it
/// does not fit. The feedback renderer and its physical cursor both consume
/// this helper, so a wide Unicode character cannot make the cursor drift past
/// the text that was actually drawn.
fn truncate_str(s: &str, max_width: usize) -> String {
    let total_width = s
        .chars()
        .map(|ch| UnicodeWidthChar::width(ch).unwrap_or(0))
        .sum::<usize>();
    if total_width <= max_width {
        return s.to_string();
    }

    let (content_width, suffix) = if max_width > 3 {
        (max_width - 3, "...")
    } else {
        (max_width, "")
    };
    let mut width = 0usize;
    let mut truncated = String::new();
    for grapheme in s.graphemes(true) {
        let ch_width = UnicodeWidthStr::width(grapheme);
        if width.saturating_add(ch_width) > content_width {
            break;
        }
        truncated.push_str(grapheme);
        width = width.saturating_add(ch_width);
    }
    truncated.push_str(suffix);
    truncated
}
