//! Integrated TUI runner -- connects the App UI with the QueryEngine.
//!
//! This module bridges the terminal UI (ratatui + crossterm) with the async
//! QueryEngine. It uses:
//!   - A dedicated thread for reading crossterm terminal events
//!   - tokio::spawn tasks for driving engine queries
//!   - mpsc channels for communication between UI and engine
//!
//! The main entry point is [`run_tui`].

use std::io;
use std::sync::Arc;
use std::time::Duration;

use allthecodes_engine::types::config::QuerySource;
use allthecodes_engine::types::tool::PermissionMode;
use allthecodes_types::callbacks::PermissionResponsePayload;
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event};
use crossterm::terminal::{
    self, BeginSynchronizedUpdate, EndSynchronizedUpdate, EnterAlternateScreen,
    LeaveAlternateScreen,
};
use crossterm::{cursor, execute};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use tracing::debug;

#[path = "tui/command_availability.rs"]
mod command_availability;
#[path = "tui/commands.rs"]
mod commands;
#[path = "tui/engine_events.rs"]
mod engine_events;
#[path = "tui/export.rs"]
mod export;
#[path = "tui/proactive.rs"]
mod proactive;
#[path = "tui/subsystem_events.rs"]
mod subsystem_events;
#[path = "tui/terminal_guard.rs"]
mod terminal_guard;
#[cfg(test)]
#[path = "tui/tests.rs"]
mod tests;

use command_availability::{slash_command_availability_during_task, TaskCommandAvailability};
use commands::{query_prompt_text, try_execute_command, CmdAction};
use engine_events::{
    create_user_message, handle_sdk_message, handle_tool_progress, install_tui_ask_user_callback,
    install_tui_permission_callback, install_tui_permission_event_callback,
    install_tui_tool_progress_callback, make_assistant_text, normalize_message_timestamp,
    permission_choice_to_response, spawn_engine_query, spawn_engine_query_with_source, EngineEvent,
    StreamingState,
};
use export::{export_to_editor, open_reference_in_editor};
use subsystem_events::{
    add_system_error, add_system_info, handle_lsp_recommendation_response, handle_subsystem_event,
};
use terminal_guard::TerminalGuard;

use crate::ui::messages::user_text_message::CONVERSATION_INTERRUPTED_MESSAGE;
use allthecodes_engine::lifecycle::QueryEngine;
use allthecodes_ipc::subsystem_events::SubsystemEventBus;
use allthecodes_ipc_protocol::BackendMessage;
use allthecodes_types::agent_channel::AgentIpcEvent;
use allthecodes_types::agent_events::AgentCommand;

use super::app::{app_event::AppEvent, app_event_sender, App, AppAction};
use super::notifications::in_app::{InAppNotification, NotificationPriority, NotificationTone};
use super::notifications::{detect_backend, DesktopNotificationBackend, NotificationMethod};
use super::permissions::hooks::{render_permission_hook_event, PermissionHookEvent};
use super::permissions::permission_decision_debug_info::render_permission_decision_debug_info;
use super::permissions::BypassPermissionsModeChoice;
use super::terminal_progress::TerminalProgressBackend;
use super::transcript::ViewMode;

fn permission_event_notification(text: String, tone: NotificationTone) -> InAppNotification {
    InAppNotification::new("permission-event", NotificationPriority::Low, text)
        .with_tone(tone)
        .with_fold(true)
        .with_timeout_ms(5000)
}

fn notify_desktop(backend: &mut DesktopNotificationBackend, message: &str) {
    if let Err(error) = backend.notify(message) {
        debug!(error = %error, "TUI: desktop notification failed");
    }
}

fn notify_desktop_for_app_event(backend: &mut DesktopNotificationBackend, event: &AppEvent) {
    match event {
        AppEvent::Notification { message, .. } | AppEvent::LocalNotice { message } => {
            notify_desktop(backend, message);
        }
        AppEvent::Backend { message } => match message.as_ref() {
            BackendMessage::NotificationSent { title, .. } => notify_desktop(backend, title),
            BackendMessage::Error { message, .. } => notify_desktop(backend, message),
            _ => {}
        },
        AppEvent::Tick | AppEvent::Shutdown => {}
    }
}

fn clear_terminal_progress<W: io::Write>(backend: &mut TerminalProgressBackend, writer: &mut W) {
    if let Err(error) = backend.clear(writer) {
        debug!(error = %error, "TUI: terminal progress clear failed");
    }
}

fn set_terminal_progress_enabled<W: io::Write>(
    backend: &mut TerminalProgressBackend,
    enabled: bool,
    writer: &mut W,
) {
    if let Err(error) = backend.set_enabled(enabled, writer) {
        debug!(error = %error, "TUI: terminal progress setting update failed");
    }
}

fn mark_terminal_progress_indeterminate<W: io::Write>(
    backend: &mut TerminalProgressBackend,
    writer: &mut W,
) {
    if let Err(error) = backend.indeterminate(writer) {
        debug!(error = %error, "TUI: terminal progress update failed");
    }
}

fn persist_skip_dangerous_mode_prompt() -> anyhow::Result<()> {
    let mut raw = allthecodes_config::settings::load_global_config()?;
    let mut permissions = raw.permissions.unwrap_or_default();
    permissions.skip_dangerous_mode_permission_prompt = Some(true);
    raw.permissions = Some(permissions);
    allthecodes_config::settings::write_user_settings(&raw)?;
    Ok(())
}

fn lsp_event_to_subsystem(
    event: allthecodes_lsp_service::LspEvent,
) -> allthecodes_ipc_protocol::subsystem_events::SubsystemEvent {
    use allthecodes_ipc_protocol::subsystem_events::{LspEvent, SubsystemEvent};
    let event = match event {
        allthecodes_lsp_service::LspEvent::ServerStateChanged {
            language_id,
            state,
            error,
        } => LspEvent::ServerStateChanged {
            language_id,
            state,
            error,
        },
        allthecodes_lsp_service::LspEvent::DocumentSynced {
            uri,
            language_id,
            version,
            change_kind,
        } => LspEvent::DocumentSynced {
            uri,
            language_id,
            version,
            change_kind,
        },
        allthecodes_lsp_service::LspEvent::DiagnosticsPublished { uri, diagnostics } => {
            LspEvent::DiagnosticsPublished {
                uri,
                diagnostics: diagnostics
                    .into_iter()
                    .map(
                        |diagnostic| allthecodes_ipc_protocol::subsystem_types::LspDiagnostic {
                            range: allthecodes_ipc_protocol::subsystem_types::DiagnosticRange {
                                start_line: diagnostic.range.start_line,
                                start_character: diagnostic.range.start_character,
                                end_line: diagnostic.range.end_line,
                                end_character: diagnostic.range.end_character,
                            },
                            severity: diagnostic.severity,
                            message: diagnostic.message,
                            source: diagnostic.source,
                            code: diagnostic.code,
                        },
                    )
                    .collect(),
            }
        }
        allthecodes_lsp_service::LspEvent::CompletionResults {
            request_id,
            uri,
            items,
        } => LspEvent::CompletionResults {
            request_id,
            uri,
            items: items
                .into_iter()
                .map(|item| allthecodes_ipc_protocol::CompletionItemInfo {
                    label: item.label,
                    kind: item.kind,
                    detail: item.detail,
                    documentation: item.documentation,
                    insert_text: item.insert_text,
                    sort_text: item.sort_text,
                    filter_text: item.filter_text,
                })
                .collect(),
        },
        allthecodes_lsp_service::LspEvent::CommandError {
            request_id,
            message,
        } => LspEvent::CommandError {
            request_id,
            message,
        },
    };
    SubsystemEvent::Lsp(event)
}

fn handle_agent_ipc_event(app: &mut App, event: AgentIpcEvent) {
    let message = match event {
        AgentIpcEvent::Agent(event) => BackendMessage::AgentEvent { event },
        AgentIpcEvent::Team(event) => BackendMessage::TeamEvent { event },
    };
    app.handle_app_event(AppEvent::backend(message));
}

fn handle_agent_backend_messages(app: &mut App, messages: Vec<BackendMessage>) {
    for message in messages {
        match message {
            BackendMessage::SystemInfo { text, level } => match level.as_str() {
                "error" => add_system_error(app, &text),
                _ => add_system_info(app, &text),
            },
            BackendMessage::BriefMessage {
                message, timestamp, ..
            } => {
                app.add_message(make_assistant_text(
                    message,
                    timestamp.map(normalize_message_timestamp),
                ));
            }
            BackendMessage::AgentEvent { .. }
            | BackendMessage::TeamEvent { .. }
            | BackendMessage::NotificationSent { .. }
            | BackendMessage::Error { .. } => {
                app.handle_app_event(AppEvent::backend(message));
            }
            _ => {}
        }
    }
}

/// Run the full TUI application, connecting the App UI with the QueryEngine.
///
/// This function takes ownership of the terminal (raw mode + alternate screen)
/// for its duration, restoring it on exit.
///
/// # Arguments
/// * `engine` - Shared QueryEngine instance
/// * `initial_prompt` - Optional prompt to submit immediately on startup
/// * `model_name` - Model name for display in the status bar
/// * `shutdown_token` - Cancellation token for graceful shutdown
pub async fn run_tui(
    engine: Arc<QueryEngine>,
    initial_prompt: Option<String>,
    model_name: &str,
    shutdown_token: CancellationToken,
) -> anyhow::Result<()> {
    // ── Setup terminal ─────────────────────────────────────────────
    let terminal_env = super::terminal_env::TerminalEnvConfig::from_env();
    let mouse_capture_enabled = !terminal_env.disable_mouse;

    terminal::enable_raw_mode()?;
    let mut stdout = io::stdout();
    if mouse_capture_enabled {
        execute!(
            stdout,
            EnterAlternateScreen,
            EnableMouseCapture,
            cursor::Hide
        )?;
    } else {
        execute!(stdout, EnterAlternateScreen, cursor::Hide)?;
    }
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Terminal guard ensures cleanup even on early return / panic.
    let _guard = TerminalGuard::new(mouse_capture_enabled);

    // ── Create the App ─────────────────────────────────────────────
    let mut app = App::new();
    let mut desktop_notifications = detect_backend(NotificationMethod::Auto);
    app.set_model_name(model_name.to_string());
    app.set_backend_name(engine.app_state().main_loop_backend.clone());
    app.set_session_id(engine.current_session_id().to_string());
    app.set_cwd(engine.cwd().to_string());
    let app_state = engine.app_state();
    app.set_sound_effects(app_state.settings.sound_effects.unwrap_or(true));
    app.set_terminal_progress_bar_enabled(
        app_state
            .settings
            .terminal_progress_bar_enabled
            .unwrap_or(true),
    );
    if app_state.tool_permission_context.mode == PermissionMode::Bypass {
        let disabled = !app_state
            .tool_permission_context
            .is_bypass_permissions_mode_available;
        let skip_prompt = app_state
            .settings
            .permissions
            .skip_dangerous_mode_permission_prompt
            .unwrap_or(false);
        if disabled || !skip_prompt {
            app.show_bypass_permissions_mode_dialog(disabled);
        }
    }
    let mut terminal_progress =
        TerminalProgressBackend::detect(app.terminal_progress_bar_enabled());
    let (app_event_sender, mut app_event_rx) = app_event_sender::channel();
    match super::persistent_history::load_persistent_history_for_workspace(std::path::Path::new(
        engine.cwd(),
    )) {
        Ok(entries) if !entries.is_empty() => app.seed_persistent_history(entries),
        Ok(_) => {}
        Err(error) => {
            tracing::warn!(error = %error, "persistent prompt history unavailable");
            let _ =
                app_event_sender.notice(format!("Persistent prompt history unavailable: {error}"));
        }
    }

    // Scriptable status line (issue #11) — seed from the effective
    // settings snapshot held on AppState. Subsequent edits via
    // `/statusline` reach the App through the command handler.
    //
    // We also adopt the runner handle from AppState so `/statusline`'s
    // status-subcommand reads the live counters from the same instance
    // the TUI is driving (the Arc<Mutex<...>> inside StatusLineRunner
    // makes the clones observe each other).
    {
        let app_state = engine.app_state();
        app.set_status_line_runner(app_state.status_line_runner.clone());
        app.set_status_line_settings(app_state.settings.status_line.clone());
        app.set_keybindings(app_state.keybindings.clone());
        app.set_editor_mode(app_state.settings.editor_mode.as_deref());
        app.set_view_mode(
            app_state
                .settings
                .view_mode
                .as_deref()
                .and_then(|value| ViewMode::parse_configured(value).ok())
                .unwrap_or_default(),
        );
        app.set_spinner_tips_settings(app_state.settings.spinner_tips.clone());
        app.set_output_style(app_state.settings.output_style.clone());
        app.set_theme_setting(app_state.settings.theme.as_deref());
        app.sync_status_context_from_state(&app_state);
    }

    // Terminal env config (issue #12) — `ALLTHECODES_NO_FLICKER`,
    // `ALLTHECODES_ENABLE_MOUSE_CAPTURE`, `ALLTHECODES_DISABLE_MOUSE`,
    // `ALLTHECODES_SCROLL_SPEED`. Cached for the duration of the session.
    app.set_terminal_env(terminal_env);

    for message in engine.messages() {
        app.add_message(message);
    }

    // Voice dictation (issue #13) — build a controller from the null
    // backends for now. Real cpal + voice_stream backends can replace
    // these arguments without changing anything above.
    {
        use std::sync::Arc;
        let audio = Arc::new(allthecodes_voice::audio::NullAudioBackend::new());
        let stt = Arc::new(allthecodes_voice::stt::NullTranscriptionClient::new());
        let voice_controller = allthecodes_voice::VoiceController::new(audio, stt);
        app.set_voice_controller(voice_controller);

        let app_state = engine.app_state();
        let lang = allthecodes_voice::language::normalize_language_for_stt(
            app_state.settings.language.as_deref(),
        );
        let voice_supported = matches!(
            allthecodes_commands::voice_cmd::current_feasibility(),
            allthecodes_voice::Feasibility::Ready { .. }
        );
        app.set_voice_settings(
            app_state.settings.voice_enabled.unwrap_or(false),
            lang.code,
            voice_supported,
        );
    }

    // ── Create channels ────────────────────────────────────────────
    let (engine_tx, mut engine_rx) = mpsc::unbounded_channel::<EngineEvent>();
    install_tui_permission_callback(&engine, engine_tx.clone());
    install_tui_ask_user_callback(&engine, engine_tx.clone());
    install_tui_permission_event_callback(&engine, engine_tx.clone());
    install_tui_tool_progress_callback(&engine, engine_tx.clone());
    let (agent_tx, mut agent_rx) = allthecodes_types::agent_channel::agent_channel();
    engine.set_bg_agent_tx(agent_tx);
    let mut pending_permission_response: Option<oneshot::Sender<PermissionResponsePayload>> = None;
    let mut pending_question_response: Option<oneshot::Sender<String>> = None;
    let mut streaming_state = StreamingState::new();

    let subsystem_bus = SubsystemEventBus::new();
    let mut subsystem_rx = subsystem_bus.subscribe();
    let (lsp_tx, mut lsp_rx) = tokio::sync::broadcast::channel(128);
    allthecodes_lsp_service::set_event_sender(lsp_tx);
    let lsp_event_tx = subsystem_bus.sender();
    tokio::spawn(async move {
        while let Ok(event) = lsp_rx.recv().await {
            let _ = lsp_event_tx.send(lsp_event_to_subsystem(event));
        }
    });

    // ── Spawn terminal event reader thread ─────────────────────────
    //
    // crossterm::event::read() is blocking, so we read events in a
    // dedicated OS thread and forward them through an mpsc channel.
    let (term_tx, mut term_rx) = mpsc::unbounded_channel::<Event>();
    std::thread::spawn(move || {
        loop {
            if term_tx.is_closed() {
                break;
            }
            match event::poll(Duration::from_millis(50)) {
                Ok(true) => {
                    if let Ok(evt) = event::read() {
                        if term_tx.send(evt).is_err() {
                            break; // receiver dropped
                        }
                    }
                }
                Ok(false) => {} // timeout, try again
                Err(_) => break,
            }
        }
    });

    // ── Handle initial prompt ──────────────────────────────────────
    if let Some(ref prompt) = initial_prompt {
        app.add_message(create_user_message(prompt));
        app.push_history(prompt.clone());
        app.set_streaming(true);
        engine.reset_abort();
        spawn_engine_query(engine.clone(), prompt.clone(), engine_tx.clone());
    }

    // ── Main event loop ────────────────────────────────────────────
    let mut tick_interval = tokio::time::interval(Duration::from_millis(16));
    tick_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut proactive_interval = tokio::time::interval(Duration::from_secs(1));
    proactive_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let proactive_tick_driver = proactive::ProactiveTickDriver::new();

    loop {
        // Draw the UI only when something changed (dirty flag).
        // `ALLTHECODES_NO_FLICKER=0` bypasses synchronized-update escapes
        // (issue #12) for users on terminals that handle them poorly.
        if app.is_dirty() {
            let sync = app.terminal_env().sync_updates;
            if sync {
                execute!(terminal.backend_mut(), BeginSynchronizedUpdate)?;
            }
            terminal.draw(|frame| app.render(frame))?;
            if sync {
                execute!(terminal.backend_mut(), EndSynchronizedUpdate)?;
            }
            app.mark_clean();
        }

        // Wait for the next event
        tokio::select! {
            biased;

            // Shutdown signal (highest priority)
            _ = shutdown_token.cancelled() => {
                debug!("TUI: shutdown signal received");
                break;
            }

            // Terminal events (keys, resize)
            Some(term_event) = term_rx.recv() => {
                match term_event {
                    Event::Key(key) => {
                        let action = app.handle_key_event(key);
                        match action {
                            AppAction::Submit(text) => {
                                if app.is_streaming() {
                                    match slash_command_availability_during_task(&text) {
                                        TaskCommandAvailability::Allowed => {
                                            if submit_prompt_to_engine(
                                                text,
                                                &engine,
                                                &mut app,
                                                &engine_tx,
                                            ).await {
                                                break;
                                            }
                                        }
                                        TaskCommandAvailability::Disabled { .. } => {
                                            reject_unavailable_streaming_command(text, &mut app);
                                        }
                                        TaskCommandAvailability::NotCommand
                                        | TaskCommandAvailability::UnknownCommand => {
                                            steer_prompt_to_engine(text, &engine, &mut app);
                                        }
                                    }
                                    continue;
                                }
                                if submit_prompt_to_engine(
                                    text,
                                    &engine,
                                    &mut app,
                                    &engine_tx,
                                ).await {
                                    break;
                                }
                            }
                            AppAction::Steer(text) => {
                                match slash_command_availability_during_task(&text) {
                                    TaskCommandAvailability::Allowed => {
                                        if submit_prompt_to_engine(
                                            text,
                                            &engine,
                                            &mut app,
                                            &engine_tx,
                                        ).await {
                                            break;
                                        }
                                    }
                                    TaskCommandAvailability::Disabled { .. } => {
                                        reject_unavailable_streaming_command(text, &mut app);
                                    }
                                    TaskCommandAvailability::NotCommand
                                    | TaskCommandAvailability::UnknownCommand => {
                                        steer_prompt_to_engine(text, &engine, &mut app);
                                    }
                                }
                            }
                            AppAction::Queue(text) => {
                                let queued = app.queue_prompt(text);
                                app.handle_app_event(AppEvent::LocalNotice {
                                    message: format!(
                                        "Queued next prompt ({} pending).",
                                        queued
                                    ),
                                });
                            }
                            AppAction::Abort => {
                                engine.abort();
                                app.set_streaming(false);
                                clear_terminal_progress(
                                    &mut terminal_progress,
                                    terminal.backend_mut(),
                                );
                                app.add_message(create_user_message(CONVERSATION_INTERRUPTED_MESSAGE));
                            }
                            AppAction::Quit => {
                                debug!("TUI: quit requested");
                                break;
                            }
                            AppAction::PermissionResponse(choice) => {
                                if let Some(response_tx) = pending_permission_response.take() {
                                    let _ = response_tx.send(permission_choice_to_response(&choice));
                                }
                            }
                            AppAction::BypassPermissionsModeResponse(choice) => {
                                match choice {
                                    BypassPermissionsModeChoice::Accept => {
                                        if let Err(err) = persist_skip_dangerous_mode_prompt() {
                                            add_system_error(
                                                &mut app,
                                                &format!(
                                                    "Failed to persist bypass permissions acknowledgement: {err}"
                                                ),
                                            );
                                        }
                                    }
                                    BypassPermissionsModeChoice::Decline => {
                                        debug!("TUI: bypass permissions mode declined");
                                        break;
                                    }
                                }
                            }
                            AppAction::QuestionResponse(answer) => {
                                if let Some(response_tx) = pending_question_response.take() {
                                    let _ = response_tx.send(answer);
                                }
                            }
                            AppAction::AgentThreadSelected(agent_id) => {
                                let messages =
                                    allthecodes_ipc::agent_handlers::handle_agent_command(
                                        AgentCommand::QueryAgentOutput {
                                            agent_id,
                                            after_seq: None,
                                            limit_bytes: None,
                                        },
                                    );
                                handle_agent_backend_messages(&mut app, messages);
                            }
                            AppAction::KillAgentThreads(agent_ids) => {
                                for agent_id in agent_ids {
                                    let messages = allthecodes_ipc::agent_handlers::handle_agent_command(
                                        AgentCommand::AbortAgent { agent_id },
                                    );
                                    handle_agent_backend_messages(&mut app, messages);
                                }
                            }
                            AppAction::LspRecommendationResponse {
                                request_id,
                                plugin_name,
                                decision,
                            } => {
                                handle_lsp_recommendation_response(
                                    &mut app,
                                    request_id,
                                    plugin_name,
                                    decision,
                                );
                            }
                            AppAction::ExportTranscript(body) => {
                                match export_to_editor(&body).await {
                                    Ok(path) => add_system_info(
                                        &mut app,
                                        &format!(
                                            "Transcript exported to {}",
                                            path.display()
                                        ),
                                    ),
                                    Err(e) => add_system_info(
                                        &mut app,
                                        &format!("Transcript export failed: {}", e),
                                    ),
                                }
                            }
                            AppAction::CopyMessage(text) => {
                                match crate::ui::clipboard_text::copy_text_to_clipboard(&text) {
                                    Ok(()) => add_system_info(
                                        &mut app,
                                        &format!("Copied message to clipboard ({} chars)", text.len()),
                                    ),
                                    Err(e) => add_system_info(
                                        &mut app,
                                        &format!("Copy failed: {e}"),
                                    ),
                                }
                            }
                            AppAction::OpenPath(reference) => {
                                let cwd = app.cwd().to_string();
                                match open_reference_in_editor(&reference, &cwd).await {
                                    Ok(path) => add_system_info(
                                        &mut app,
                                        &format!("Opened {}", path.display()),
                                    ),
                                    Err(e) => add_system_info(
                                        &mut app,
                                        &format!("Open failed: {e}"),
                                    ),
                                }
                            }
                            AppAction::DebugSnapshot => {
                                export_debug_snapshot(&mut app);
                            }
                            // Scroll actions are handled internally by App
                            _ => {}
                        }
                    }
                    Event::Resize(_, _) => {
                        app.mark_dirty();
                    }
                    Event::Mouse(mouse) => {
                        let _ = app.handle_mouse_event(mouse);
                    }
                    Event::Paste(text) => {
                        let _ = app.handle_paste_event(text);
                    }
                    _ => {}
                }
            }

            Some(agent_event) = agent_rx.recv() => {
                handle_agent_ipc_event(&mut app, agent_event);
            }

            Some(app_event) = app_event_rx.recv() => {
                if app.sound_effects_enabled() {
                    notify_desktop_for_app_event(&mut desktop_notifications, &app_event);
                }
                app.handle_app_event(app_event);
            }

            // Engine events (query results)
            engine_event = engine_rx.recv() => {
                let Some(engine_event) = engine_event else {
                    app.handle_app_event(AppEvent::Shutdown);
                    continue;
                };
                match engine_event {
                    EngineEvent::Sdk(sdk_msg) => {
                        handle_sdk_message(&mut app, *sdk_msg, &mut streaming_state);
                    }
                    EngineEvent::ToolProgress(progress) => {
                        handle_tool_progress(&mut app, progress);
                        mark_terminal_progress_indeterminate(
                            &mut terminal_progress,
                            terminal.backend_mut(),
                        );
                    }
                    EngineEvent::PermissionRequest { request, response_tx } => {
                        if let Some(previous) = pending_permission_response.take() {
                            let _ = previous.send(PermissionResponsePayload::deny());
                        }
                        app.show_permission_request(request.into());
                        pending_permission_response = Some(response_tx);
                    }
                    EngineEvent::QuestionRequest {
                        id,
                        request,
                        response_tx,
                    } => {
                        if let Some(previous) = pending_question_response.take() {
                            let _ = previous.send(String::new());
                        }
                        app.show_question_dialog(id, request);
                        pending_question_response = Some(response_tx);
                    }
                    EngineEvent::HookPermissionDecision(event) => {
                        let notification = permission_event_notification(
                            render_permission_hook_event(&PermissionHookEvent {
                                hook_name: event.hook_name,
                                matcher: event.matcher,
                                decision: event.decision,
                                notes: event.notes,
                            }),
                            NotificationTone::Info,
                        );
                        if app.sound_effects_enabled() {
                            notify_desktop(&mut desktop_notifications, &notification.text);
                        }
                        app.add_notification(notification);
                    }
                    EngineEvent::PermissionDecisionDebug(event) => {
                        let notification = permission_event_notification(
                            render_permission_decision_debug_info(
                                &event.tool_name,
                                &event.matcher,
                                &event.source,
                                event.matched_rule.as_deref(),
                            ),
                            NotificationTone::Dim,
                        );
                        if app.sound_effects_enabled() {
                            notify_desktop(&mut desktop_notifications, &notification.text);
                        }
                        app.add_notification(notification);
                    }
                    EngineEvent::PermissionAutoReview(event) => {
                        let text = format!(
                            "auto review: {}\naction: {}\nsource: {}",
                            event.status,
                            event.action.as_deref().unwrap_or("permission request"),
                            event.decision_source
                        );
                        let notification =
                            permission_event_notification(text, NotificationTone::Info);
                        if app.sound_effects_enabled() && event.status != "started" {
                            notify_desktop(&mut desktop_notifications, &notification.text);
                        }
                        app.add_notification(notification);
                    }
                    EngineEvent::Done => {
                        app.set_streaming(false);
                        clear_terminal_progress(
                            &mut terminal_progress,
                            terminal.backend_mut(),
                        );
                        while let Some(text) = app.pop_next_queued() {
                            let remaining = app.queued_count();
                            app.handle_app_event(AppEvent::LocalNotice {
                                message: format!(
                                    "Running queued prompt ({} remaining).",
                                    remaining
                                ),
                            });
                            if submit_prompt_to_engine(text, &engine, &mut app, &engine_tx).await {
                                break;
                            }
                            if app.is_streaming() {
                                break;
                            }
                        }
                    }
                }
            }

            subsystem_event = subsystem_rx.recv() => {
                match subsystem_event {
                    Ok(event) => handle_subsystem_event(&mut app, event),
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        add_system_error(&mut app, "A subsystem UI event was dropped because the TUI fell behind.");
                        let _ = app_event_sender.notification(
                            "subsystem-lag",
                            "A subsystem UI event was dropped because the TUI fell behind.",
                            "error",
                            Some(8000),
                        );
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {}
                }
            }

            _ = proactive_interval.tick() => {
                let decision = proactive_tick_driver.decide(
                    app.is_streaming(),
                    pending_permission_response.is_some(),
                    pending_question_response.is_some(),
                    chrono::Utc::now(),
                );
                if decision == proactive::ProactiveTickDecision::Submit {
                    let prompt = proactive_tick_driver.build_prompt(chrono::Local::now(), true);
                    app.set_streaming(true);
                    engine.reset_abort();
                    spawn_engine_query_with_source(
                        engine.clone(),
                        prompt,
                        QuerySource::ProactiveTick,
                        engine_tx.clone(),
                    );
                    proactive_tick_driver.mark_tick_submitted();
                }
            }

            // Tick timer (spinner animation, ~80ms)
            _ = tick_interval.tick() => {
                app.handle_app_event(AppEvent::Tick);
                // Drain any pending voice-controller events (issue #13).
                // Returns true when something was inserted into the
                // prompt or the state changed — both cases need a redraw.
                app.drain_voice_events();
            }
        }

        if app.should_quit() {
            break;
        }

        set_terminal_progress_enabled(
            &mut terminal_progress,
            app.terminal_progress_bar_enabled(),
            terminal.backend_mut(),
        );
    }

    // ── Restore terminal ───────────────────────────────────────────
    // (TerminalGuard::drop also handles this, but explicit cleanup is
    // cleaner for the normal exit path.)
    clear_terminal_progress(&mut terminal_progress, terminal.backend_mut());
    terminal::disable_raw_mode()?;
    if mouse_capture_enabled {
        execute!(
            terminal.backend_mut(),
            DisableMouseCapture,
            LeaveAlternateScreen,
            cursor::Show
        )?;
    } else {
        execute!(terminal.backend_mut(), LeaveAlternateScreen, cursor::Show)?;
    }
    terminal.show_cursor()?;

    Ok(())
}

fn export_debug_snapshot(app: &mut App) {
    match app.export_debug_snapshot() {
        Ok(path) => add_system_info(
            app,
            &format!("TUI debug snapshot exported to {}", path.display()),
        ),
        Err(error) => add_system_error(app, &format!("TUI debug snapshot failed: {error}")),
    }
}

fn reject_unavailable_streaming_command(text: String, app: &mut App) -> bool {
    let TaskCommandAvailability::Disabled { command } =
        slash_command_availability_during_task(&text)
    else {
        return false;
    };

    app.restore_prompt_text(text);
    add_system_error(
        app,
        &format!("'/{command}' is disabled while a task is in progress."),
    );
    true
}

async fn submit_prompt_to_engine(
    text: String,
    engine: &Arc<QueryEngine>,
    app: &mut App,
    engine_tx: &mpsc::UnboundedSender<EngineEvent>,
) -> bool {
    app.push_history(text.clone());
    let _ = allthecodes_services::proactive::clear_sleep_state("user_submit");

    if let Some(action) = try_execute_command(&text, engine, app).await {
        match action {
            CmdAction::Handled => {}
            CmdAction::Quit(msg) => {
                add_system_info(app, &msg);
                return true;
            }
            CmdAction::Query(msgs) => {
                let prompt = query_prompt_text(&msgs);
                for message in msgs {
                    app.add_message(message);
                }
                app.set_streaming(true);
                engine.reset_abort();
                spawn_engine_query(
                    engine.clone(),
                    if prompt.trim().is_empty() {
                        text
                    } else {
                        prompt
                    },
                    engine_tx.clone(),
                );
            }
        }
    } else {
        app.add_message(create_user_message(&text));
        app.set_streaming(true);
        engine.reset_abort();
        spawn_engine_query(engine.clone(), text, engine_tx.clone());
    }

    false
}

fn steer_prompt_to_engine(text: String, engine: &Arc<QueryEngine>, app: &mut App) {
    match engine.submit_steer_message(text.clone()) {
        Ok(()) => {
            app.push_history(text.clone());
            app.add_message(create_user_message(&text));
            app.handle_app_event(AppEvent::LocalNotice {
                message: "Sent steer message to the active turn.".to_string(),
            });
        }
        Err(err) => {
            app.restore_prompt_text(text);
            app.handle_app_event(AppEvent::Notification {
                key: "steer-failed".to_string(),
                message: err.to_string(),
                level: "warning".to_string(),
                timeout_ms: Some(4000),
            });
        }
    }
}
