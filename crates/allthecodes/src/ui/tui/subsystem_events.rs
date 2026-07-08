use super::engine_events::now_ts;
use crate::ui::app::App;
use crate::ui::command_surface::CommandSurface;
use crate::ui::context_layer::{ContextLayerItem, ContextLayerKey, ContextTone};
use crate::ui::notifications::in_app::{InAppNotification, NotificationPriority, NotificationTone};
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

fn add_system_message(app: &mut App, text: &str, level: InfoLevel) {
    if matches!(level, InfoLevel::Info) && route_current_state_info(app, text) {
        return;
    }
    app.add_message(Message::System(SystemMessage {
        uuid: uuid::Uuid::new_v4(),
        timestamp: now_ts(),
        subtype: SystemSubtype::Informational {
            level: level.clone(),
        },
        content: text.to_string(),
    }));
    app.add_notification(system_notice_notification(text, level));
}

/// Add an error system message to the app.
pub(super) fn add_system_error(app: &mut App, text: &str) {
    add_system_message(app, text, InfoLevel::Error);
}

fn system_notice_notification(text: &str, level: InfoLevel) -> InAppNotification {
    let trimmed = text.trim();
    let message = if trimmed.is_empty() {
        "System notice"
    } else {
        trimmed
    };
    let (priority, tone, timeout_ms) = match level {
        InfoLevel::Error => (NotificationPriority::High, NotificationTone::Error, 8000),
        InfoLevel::Warning => (NotificationPriority::High, NotificationTone::Warning, 6500),
        InfoLevel::Info => (NotificationPriority::Medium, NotificationTone::Info, 4500),
    };
    InAppNotification::new("system-notice", priority, message)
        .with_tone(tone)
        .with_timeout_ms(timeout_ms)
        .with_fold(true)
}

fn route_current_state_info(app: &mut App, text: &str) -> bool {
    let Some((key, label, value)) = parse_current_state_info(text) else {
        return false;
    };
    app.upsert_context_layer_item(ContextLayerItem::keyed(
        key,
        ContextTone::Info,
        label,
        value,
    ));
    true
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
    use ratatui::backend::TestBackend;
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
    fn plain_context_prefix_stays_a_transient_info_notification() {
        let mut app = App::new();

        add_system_info(&mut app, "context: ordinary note");

        let notification = app.current_notification().expect("notification");
        assert_eq!(notification.tone, NotificationTone::Info);
        assert_eq!(notification.text, "context: ordinary note");
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
}
