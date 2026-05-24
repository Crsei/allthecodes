use super::engine_events::now_ts;
use crate::ui::app::App;
use crate::ui::command_surface::CommandSurface;
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
