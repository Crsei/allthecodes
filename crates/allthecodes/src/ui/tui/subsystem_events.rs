use super::engine_events::now_ts;
use crate::ui::app::App;
use crate::ui::command_surface::CommandSurface;
use crate::ui::context_layer::{ContextLayerItem, ContextLayerKey, ContextTone};
use allthecodes_ipc_protocol::subsystem_events::{LspCommand, LspEvent, SubsystemEvent};
use allthecodes_ipc_protocol::BackendMessage;
use allthecodes_types::message::{InfoLevel, Message, SystemMessage, SystemSubtype};
pub(super) fn handle_subsystem_event(app: &mut App, event: SubsystemEvent) {
    match event {
        SubsystemEvent::Lsp(LspEvent::RecommendationRequest { payload }) => {
            app.open_command_surface(CommandSurface::lsp_recommendation(payload));
        }
        SubsystemEvent::Lsp(LspEvent::CommandError { message, .. }) => {
            add_system_error(app, &message);
        }
        _ => {}
    }
}

pub(super) fn handle_lsp_recommendation_response(
    app: &mut App,
    request_id: String,
    plugin_name: String,
    decision: String,
) {
    crate::app_runtime_adapters::ensure_installed();
    let messages = allthecodes_ipc::subsystem_handlers::handle_lsp_command(
        LspCommand::RecommendationResponse {
            request_id,
            plugin_name,
            decision,
        },
    );
    handle_backend_messages(app, messages);
}

fn handle_backend_messages(app: &mut App, messages: Vec<BackendMessage>) {
    for message in messages {
        if let BackendMessage::SystemInfo { text, level } = message {
            add_system_message(app, &text, info_level_from_str(&level));
        }
    }
}

fn info_level_from_str(level: &str) -> InfoLevel {
    match level {
        "error" => InfoLevel::Error,
        "warning" => InfoLevel::Warning,
        _ => InfoLevel::Info,
    }
}

/// Add an informational system message to the app.
pub(super) fn add_system_info(app: &mut App, text: &str) {
    add_system_message(app, text, InfoLevel::Info);
}

/// Add a warning system message to the app.
pub(super) fn add_system_warning(app: &mut App, text: &str) {
    add_system_message(app, text, InfoLevel::Warning);
}

fn add_system_message(app: &mut App, text: &str, level: InfoLevel) {
    // State-prefix notices (branch / repo / cwd / model / context usage) ride
    // the well-known sticky slots; they are not duplicated into the Notices
    // catch-all slot, nor into the transcript, nor surfaced as a transient
    // In-App Notification.
    if let Some((key, label, value)) = parse_current_state_info(text) {
        app.upsert_context_layer_item(ContextLayerItem::keyed(
            key,
            tone_for_level(&level),
            label,
            value,
        ));
        return;
    }

    // Everything else: keep the rolling transcript entry (so the user can
    // scroll back to read it) and also surface it as a sticky context-layer
    // notice. Routes both paths regardless of level — no transient
    // In-App Notification is emitted.
    app.add_message(Message::System(SystemMessage {
        uuid: uuid::Uuid::new_v4(),
        timestamp: now_ts(),
        subtype: SystemSubtype::Informational {
            level: level.clone(),
        },
        content: text.to_string(),
    }));
    let trimmed = text.trim();
    if !trimmed.is_empty() {
        app.upsert_context_layer_item(ContextLayerItem::keyed(
            ContextLayerKey::Notices,
            tone_for_level(&level),
            label_for_level(&level),
            trimmed,
        ));
    }
}

/// Add an error system message to the app.
pub(super) fn add_system_error(app: &mut App, text: &str) {
    add_system_message(app, text, InfoLevel::Error);
}

fn tone_for_level(level: &InfoLevel) -> ContextTone {
    match level {
        InfoLevel::Info => ContextTone::Info,
        InfoLevel::Warning => ContextTone::Warning,
        InfoLevel::Error => ContextTone::Error,
    }
}

fn label_for_level(level: &InfoLevel) -> &'static str {
    match level {
        InfoLevel::Info => "notice",
        InfoLevel::Warning => "warning",
        InfoLevel::Error => "error",
    }
}

fn parse_current_state_info(text: &str) -> Option<(ContextLayerKey, &'static str, String)> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    for (prefix, key, label) in [
        ("current branch:", ContextLayerKey::Branch, "branch"),
        ("branch:", ContextLayerKey::Branch, "branch"),
        ("current repo:", ContextLayerKey::Repo, "repo"),
        ("repo:", ContextLayerKey::Repo, "repo"),
        ("cwd:", ContextLayerKey::Repo, "repo"),
        ("current model:", ContextLayerKey::Model, "model"),
        ("model:", ContextLayerKey::Model, "model"),
        ("context usage:", ContextLayerKey::ContextUsage, "ctx"),
    ] {
        if lower.starts_with(prefix) {
            let value = trimmed[prefix.len()..].trim();
            if !value.is_empty() {
                return Some((key, label, value.to_string()));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::context_layer::ContextLayerKey;
    use crate::ui::theme::Theme;
    use ratatui::backend::TestBackend;
    use ratatui::style::{Color, Style};
    use ratatui::Terminal;

    #[test]
    fn current_state_system_info_routes_to_context_without_notification() {
        let mut app = App::new();

        add_system_info(&mut app, "branch: feature/sticky-context");
        let mut terminal = Terminal::new(TestBackend::new(100, 16)).expect("terminal");
        terminal.draw(|frame| app.render(frame)).expect("draw");

        assert!(app.current_notification().is_none());
        let rendered = buffer_to_lines(terminal.backend().buffer(), 100, 16).join("\n");
        assert!(
            rendered.contains("branch: feature/sticky-context"),
            "rendered frame should include sticky branch context\n{rendered}"
        );
    }

    #[test]
    fn model_system_info_routes_to_context_without_notification() {
        let mut app = App::new();

        add_system_info(&mut app, "model: codex-high");
        let mut terminal = Terminal::new(TestBackend::new(100, 16)).expect("terminal");
        terminal.draw(|frame| app.render(frame)).expect("draw");

        assert!(app.current_notification().is_none());
        let rendered = buffer_to_lines(terminal.backend().buffer(), 100, 16).join("\n");
        assert!(
            rendered.contains("model: codex-high"),
            "rendered frame should include sticky model context\n{rendered}"
        );
    }

    #[test]
    fn plain_context_prefix_routes_to_sticky_context_without_notification() {
        let mut app = App::new();

        add_system_info(&mut app, "context: ordinary note");
        let item = app
            .context_layer_item(&ContextLayerKey::Notices)
            .expect("sticky context item");
        let mut terminal = Terminal::new(TestBackend::new(100, 16)).expect("terminal");
        terminal.draw(|frame| app.render(frame)).expect("draw");

        assert!(app.current_notification().is_none());
        assert_eq!(item.tone, ContextTone::Info);
        assert_eq!(item.label, "notice");
        assert_eq!(item.value, "context: ordinary note");
        let rendered = buffer_to_lines(terminal.backend().buffer(), 100, 16).join("\n");
        assert!(rendered.contains("notice: context: ordinary note"));
    }

    #[test]
    fn warning_notice_routes_to_sticky_context_with_warning_tone() {
        let mut app = App::new();

        add_system_warning(&mut app, "disk space low");

        assert!(app.current_notification().is_none());

        let notice = app
            .context_layer_item(&ContextLayerKey::Notices)
            .expect("warning should sit in the Notices slot");
        assert_eq!(notice.tone, ContextTone::Warning);
        assert_eq!(notice.label, "warning");
        assert_eq!(notice.value, "disk space low");

        let mut terminal = Terminal::new(TestBackend::new(100, 16)).expect("terminal");
        terminal.draw(|frame| app.render(frame)).expect("draw");
        let rendered = buffer_to_lines(terminal.backend().buffer(), 100, 16).join("\n");
        assert!(
            rendered.contains("warning: disk space low"),
            "rendered frame should include the sticky warning notice\n{rendered}"
        );
        let expected_fg = Theme::default().context_warning.fg;
        assert_style_fg_contains(
            terminal.backend().buffer(),
            100,
            16,
            "warning: disk space low",
            expected_fg,
            "warning notice should be rendered with the yellow context_warning color",
        );
    }

    #[test]
    fn error_notice_routes_to_sticky_context_with_error_tone() {
        let mut app = App::new();

        add_system_error(&mut app, "command failed");

        assert!(app.current_notification().is_none());

        let notice = app
            .context_layer_item(&ContextLayerKey::Notices)
            .expect("error should sit in the Notices slot");
        assert_eq!(notice.tone, ContextTone::Error);
        assert_eq!(notice.label, "error");
        assert_eq!(notice.value, "command failed");

        let mut terminal = Terminal::new(TestBackend::new(100, 16)).expect("terminal");
        terminal.draw(|frame| app.render(frame)).expect("draw");
        let rendered = buffer_to_lines(terminal.backend().buffer(), 100, 16).join("\n");
        assert!(
            rendered.contains("error: command failed"),
            "rendered frame should include the sticky error notice\n{rendered}"
        );
        let expected_fg = Theme::default().context_error.fg;
        assert_style_fg_contains(
            terminal.backend().buffer(),
            100,
            16,
            "error: command failed",
            expected_fg,
            "error notice should be rendered with the red context_error color",
        );
    }

    fn buffer_to_lines(buf: &ratatui::buffer::Buffer, width: u16, height: u16) -> Vec<String> {
        let mut lines = Vec::new();
        for y in 0..height {
            let mut line = String::new();
            for x in 0..width {
                line.push_str(buf[(x, y)].symbol());
            }
            lines.push(line);
        }
        lines
    }

    /// Find the row containing `needle`, then sample the ratatui `Style` of
    /// the cell that renders the needle's first character, asserting its
    /// foreground color matches `expected_fg`.
    fn assert_style_fg_contains(
        buf: &ratatui::buffer::Buffer,
        width: u16,
        height: u16,
        needle: &str,
        expected_fg: Option<Color>,
        message: &str,
    ) {
        for y in 0..height {
            let mut row = String::new();
            for x in 0..width {
                row.push_str(buf[(x, y)].symbol());
            }
            if let Some(start) = row.find(needle) {
                // Map needle char offset back to cell x. Walk forward from
                // the row start accumulating each cell's symbol char count
                // until we cover the needle's start offset.
                let mut consumed = 0usize;
                let mut style: Option<Style> = None;
                for x in 0..width {
                    let cell = &buf[(x, y)];
                    let w = cell.symbol().chars().count();
                    if style.is_none() && consumed + w > start {
                        style = Some(cell.style());
                    }
                    consumed += w;
                }
                let style = style.unwrap_or_else(|| panic!("needle `{needle}` cell not found"));
                assert_eq!(style.fg, expected_fg, "{message}");
                return;
            }
        }
        panic!("needle `{needle}` not found in buffer");
    }
}
