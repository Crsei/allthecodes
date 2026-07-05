use super::subsystem_events::{add_system_error, add_system_info};
use crate::ui::app::App;
use crate::ui::command_surface::{CommandSurface, CommandSurfaceTarget};
use crate::ui::transcript::ViewMode;
use allthecodes_commands as slash_commands;
use allthecodes_engine::command_runtime::{CommandContext, CommandResult};
use allthecodes_engine::lifecycle::QueryEngine;
use allthecodes_types::commands::CommandDispatcher;
use allthecodes_types::message::{ContentBlock, Message, MessageContent};
use std::sync::Arc;
// ---------------------------------------------------------------------------
// Slash-command execution
// ---------------------------------------------------------------------------

/// Internal action returned after executing a slash command.
pub(super) enum CmdAction {
    /// Command fully handled, output already added to app.
    Handled,
    /// Command requested exit.
    Quit(String),
    /// Command produced messages to send to the model.
    Query(Vec<Message>),
}

fn conversation_changed(before: &[Message], after: &[Message]) -> bool {
    before.len() != after.len()
        || before
            .iter()
            .zip(after.iter())
            .any(|(lhs, rhs)| lhs.uuid() != rhs.uuid())
}

fn replace_app_messages(app: &mut App, messages: &[Message]) {
    app.clear_messages();
    for message in messages {
        app.add_message(message.clone());
    }
}

/// Try to execute a slash command. Returns `None` if the input is not a command.
pub(super) async fn try_execute_command(
    text: &str,
    engine: &Arc<QueryEngine>,
    app: &mut App,
) -> Option<CmdAction> {
    let trimmed = text.trim();
    if !trimmed.starts_with('/') {
        return None;
    }

    if is_debug_snapshot_command(trimmed) {
        match app.export_debug_snapshot() {
            Ok(path) => add_system_info(
                app,
                &format!("TUI debug snapshot exported to {}", path.display()),
            ),
            Err(error) => add_system_error(app, &format!("TUI debug snapshot failed: {error}")),
        }
        return Some(CmdAction::Handled);
    }

    let cwd = std::path::PathBuf::from(engine.cwd());
    let dispatcher = slash_commands::DefaultCommandDispatcher::for_cwd(&cwd);
    let parsed = dispatcher.parse_command_input(trimmed)?;
    let args = parsed.args.clone();
    let command_name = dispatcher
        .command_name(parsed.index)
        .unwrap_or_else(|| trimmed.trim_start_matches('/').to_string());
    let original_messages = engine.messages();

    let mut ctx = CommandContext {
        messages: original_messages.clone(),
        cwd,
        app_state: engine.app_state(),
        session_id: engine.current_session_id(),
    };

    if let Some(surface) =
        CommandSurface::for_slash_command(&command_name, &args, &ctx.app_state, &ctx.cwd)
    {
        app.open_command_surface(surface);
        return Some(CmdAction::Handled);
    }

    let command_executor = engine.command_executor();
    match command_executor
        .execute(parsed, command_name.clone(), &mut ctx)
        .await
    {
        Ok(result) => match result {
            CommandResult::Output(text) => {
                if conversation_changed(&original_messages, &ctx.messages) {
                    engine.replace_messages(ctx.messages.clone());
                    replace_app_messages(app, &ctx.messages);
                }
                sync_app_runtime_from_state(engine, app, &ctx.app_state);
                add_system_info(app, &text);
                open_pending_command_surface(app, &ctx.app_state, &ctx.cwd);
                Some(CmdAction::Handled)
            }
            CommandResult::SwitchSession {
                session_id,
                messages,
                notice,
            } => {
                engine.set_current_session_id(session_id.clone());
                engine.replace_messages(messages.clone());
                replace_app_messages(app, &messages);
                app.set_session_id(session_id.to_string());
                sync_app_runtime_from_state(engine, app, &ctx.app_state);
                add_system_info(app, &notice);
                Some(CmdAction::Handled)
            }
            CommandResult::Clear => {
                let new_session_id = engine.start_new_session();
                app.clear_messages();
                app.set_session_id(new_session_id.to_string());
                add_system_info(app, "Started a new session.");
                Some(CmdAction::Handled)
            }
            CommandResult::Exit(msg) => Some(CmdAction::Quit(msg)),
            CommandResult::Query(msgs) => {
                if conversation_changed(&original_messages, &ctx.messages) {
                    engine.replace_messages(ctx.messages.clone());
                    replace_app_messages(app, &ctx.messages);
                }
                sync_app_runtime_from_state(engine, app, &ctx.app_state);
                open_pending_command_surface(app, &ctx.app_state, &ctx.cwd);
                Some(CmdAction::Query(msgs))
            }
            CommandResult::None => {
                if conversation_changed(&original_messages, &ctx.messages) {
                    engine.replace_messages(ctx.messages.clone());
                    replace_app_messages(app, &ctx.messages);
                }
                sync_app_runtime_from_state(engine, app, &ctx.app_state);
                open_pending_command_surface(app, &ctx.app_state, &ctx.cwd);
                Some(CmdAction::Handled)
            }
        },
        Err(e) => {
            let _ = app.take_pending_command_surface_after_submit();
            add_system_error(app, &format!("Command error: {e}"));
            Some(CmdAction::Handled)
        }
    }
}

fn open_pending_command_surface(
    app: &mut App,
    state: &allthecodes_engine::types::app_state::AppState,
    cwd: &std::path::Path,
) {
    let Some(target) = app.take_pending_command_surface_after_submit() else {
        return;
    };
    let surface = match target {
        CommandSurfaceTarget::Effort => CommandSurface::for_slash_command("effort", "", state, cwd),
    };
    if let Some(surface) = surface {
        app.open_command_surface(surface);
    }
}

fn is_debug_snapshot_command(input: &str) -> bool {
    let mut parts = input.split_whitespace();
    matches!(parts.next(), Some("/debug"))
        && matches!(parts.next(), Some("snapshot"))
        && parts.next().is_none()
}

fn sync_app_runtime_from_state(
    engine: &Arc<QueryEngine>,
    app: &mut App,
    state: &allthecodes_engine::types::app_state::AppState,
) {
    engine.update_app_state(|engine_state| {
        engine_state.main_loop_model = state.main_loop_model.clone();
        engine_state.main_loop_backend = state.main_loop_backend.clone();
        engine_state.settings = state.settings.clone();
        engine_state.tool_permission_context = state.tool_permission_context.clone();
        engine_state.thinking_enabled = state.thinking_enabled;
        engine_state.fast_mode = state.fast_mode;
        engine_state.is_brief_only = state.is_brief_only;
        engine_state.effort_value = state.effort_value.clone();
        engine_state.keybindings = state.keybindings.clone();
    });

    app.set_model_name(state.main_loop_model.clone());
    app.set_backend_name(state.main_loop_backend.clone());
    app.set_status_line_settings(state.settings.status_line.clone());
    app.set_output_style(state.settings.output_style.clone());
    app.set_theme_setting(state.settings.theme.as_deref());
    app.set_editor_mode(state.settings.editor_mode.as_deref());
    app.set_view_mode(
        state
            .settings
            .view_mode
            .as_deref()
            .and_then(|value| ViewMode::parse_configured(value).ok())
            .unwrap_or_default(),
    );
    app.set_spinner_tips_settings(state.settings.spinner_tips.clone());
    app.set_keybindings(state.keybindings.clone());
    app.set_sound_effects(state.settings.sound_effects.unwrap_or(true));
    app.set_terminal_progress_bar_enabled(
        state.settings.terminal_progress_bar_enabled.unwrap_or(true),
    );
    app.sync_status_context_from_state(state);

    let lang =
        allthecodes_voice::language::normalize_language_for_stt(state.settings.language.as_deref());
    let voice_supported = matches!(
        allthecodes_commands::voice_cmd::current_feasibility(),
        allthecodes_voice::Feasibility::Ready { .. }
    );
    app.set_voice_settings(
        state.settings.voice_enabled.unwrap_or(false),
        lang.code,
        voice_supported,
    );
}

pub(super) fn query_prompt_text(messages: &[Message]) -> String {
    let mut parts = Vec::new();

    for msg in messages {
        let text = match msg {
            Message::User(user) => message_content_text(&user.content),
            Message::Assistant(assistant) => assistant
                .content
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::Text { text } => Some(text.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n"),
            Message::System(system) => system.content.clone(),
            Message::Progress(progress) => progress.data.to_string(),
            Message::Attachment(attachment) => format!("{:?}", attachment.attachment),
        };

        if !text.trim().is_empty() {
            parts.push(text);
        }
    }

    parts.join("\n\n")
}

fn message_content_text(content: &MessageContent) -> String {
    match content {
        MessageContent::Text(text) => text.clone(),
        MessageContent::Blocks(blocks) => blocks
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

#[cfg(test)]
mod tests {
    use super::is_debug_snapshot_command;

    #[test]
    fn debug_snapshot_command_matches_exact_subcommand() {
        assert!(is_debug_snapshot_command("/debug snapshot"));
        assert!(is_debug_snapshot_command("  /debug   snapshot  "));
        assert!(!is_debug_snapshot_command("/debug"));
        assert!(!is_debug_snapshot_command("/debug snapshot extra"));
        assert!(!is_debug_snapshot_command("/debug-snapshot"));
    }
}
