use super::app_event::AppEvent;
use super::workspace_trust::trusted_workspaces_path;
use super::*;
use crate::ui::completions::{CompletionItem, CompletionKind};
use crate::ui::notifications::in_app::{InAppNotification, NotificationPriority, NotificationTone};
use allthecodes_engine::types::app_state::AppState;
use allthecodes_engine::types::tool::PermissionMode;
use allthecodes_ipc_protocol::BackendMessage;
use allthecodes_keybindings::action::Action;
use allthecodes_services::prompt_suggestion::{PromptSuggestion, SuggestionCategory};
use allthecodes_types::agent_events::AgentEvent;
use allthecodes_types::callbacks::AskUserRequestPayload;
use allthecodes_types::message::{
    AssistantMessage, ContentBlock, MessageContent, ToolResultContent, UserMessage,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};
use ratatui::Terminal;
use serial_test::serial;
use std::path::Path;

struct EnvGuard {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvGuard {
    fn set_path(key: &'static str, value: &Path) -> Self {
        let previous = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.as_ref() {
            std::env::set_var(self.key, previous);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

#[test]
fn render_places_prompt_after_compact_welcome() {
    let mut app = App::new();
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("terminal");

    terminal.draw(|frame| app.render(frame)).expect("draw");

    let content = buffer_to_lines(terminal.backend().buffer(), 80, 24);
    assert!(
        content[..8].iter().any(|line| line.contains(" ▄▀▀▄ ")),
        "wide welcome should render the letter inside the nine-grid logo",
    );
    assert!(
        content[..8]
            .iter()
            .all(|line| !line.contains("ALLTHECODES")),
        "wide welcome must not render an external word tracker",
    );
    assert!(app.welcome_logo_visible);
    assert!(
        content[8].trim().is_empty(),
        "welcome panel and prompt input should have a blank spacer row"
    );
    assert!(
        content[10].trim_start().starts_with(">"),
        "prompt should sit on the middle line of the 3-line input area after the spacer row"
    );
    assert!(
        !content[22].trim_start().starts_with(">"),
        "prompt should not be pinned to the bottom row"
    );
}

#[test]
fn render_captures_debug_snapshot_and_exports_file() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let mut app = App::new();
    app.set_cwd(tempdir.path().display().to_string());
    app.set_session_id("debug-session".to_string());
    app.set_model_name("deepseek-v4-pro".to_string());
    app.set_backend_name("anthropic".to_string());
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("terminal");

    terminal.draw(|frame| app.render(frame)).expect("draw");
    let path = app.export_debug_snapshot().expect("export snapshot");

    let snapshot_dir = tempdir.path().join("target/tui-snapshots");
    assert_eq!(path.parent(), Some(snapshot_dir.as_path()));
    let file_name = path.file_name().and_then(|name| name.to_str()).unwrap();
    assert!(file_name.starts_with("snapshot-"));
    assert!(file_name.ends_with(".txt"));
    let exported = std::fs::read_to_string(path).expect("read snapshot");
    assert!(exported.contains("# allthecodes TUI debug snapshot"));
    assert!(exported.contains("exported_at: "));
    assert!(exported.contains("session_id: debug-session"));
    assert!(exported.contains("model: deepseek-v4-pro"));
    assert!(exported.contains("backend: anthropic"));
    assert!(
        !exported.contains("<no rendered frame captured yet>"),
        "snapshot should include the rendered terminal frame"
    );
}

#[test]
fn render_places_prompt_after_short_chat_content() {
    let mut app = App::new();
    app.add_message(Message::User(UserMessage {
        uuid: uuid::Uuid::new_v4(),
        timestamp: 0,
        role: "user".to_string(),
        content: MessageContent::Text("hello".to_string()),
        is_meta: false,
        tool_use_result: None,
        source_tool_assistant_uuid: None,
    }));
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("terminal");

    terminal.draw(|frame| app.render(frame)).expect("draw");

    let content = buffer_to_lines(terminal.backend().buffer(), 80, 24);
    assert!(content[1].contains("hello"));
    assert!(!content[1].contains("You:"));
    assert!(
        content[3].trim().is_empty(),
        "chat content and prompt input should have a blank spacer row"
    );
    assert!(
        content[5].trim_start().starts_with(">"),
        "prompt should sit on the middle line of the 3-line input area after the spacer row"
    );
    assert!(
        !content[22].trim_start().starts_with(">"),
        "prompt should not be pinned to the bottom row"
    );
}

#[test]
fn in_app_notification_renders_in_footer_region() {
    let mut app = App::new();
    app.add_notification(
        InAppNotification::new(
            "api-key-warning",
            NotificationPriority::High,
            "API key missing",
        )
        .with_tone(NotificationTone::Warning),
    );
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("terminal");

    terminal.draw(|frame| app.render(frame)).expect("draw");

    let content = buffer_to_lines(terminal.backend().buffer(), 80, 24).join("\n");
    assert!(content.contains("API key missing"));
}

#[test]
fn long_chat_renders_session_scrollbar() {
    let mut app = App::new();
    for i in 0..20 {
        app.add_message(Message::User(UserMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: i,
            role: "user".to_string(),
            content: MessageContent::Text(format!("message {i}")),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        }));
    }
    let mut terminal = Terminal::new(TestBackend::new(40, 12)).expect("terminal");

    terminal.draw(|frame| app.render(frame)).expect("draw");

    let buffer = terminal.backend().buffer();
    let right_edge = (0..12)
        .map(|y| buffer[(39, y)].symbol().to_string())
        .collect::<String>();
    assert!(right_edge.contains('█'));
    assert!(right_edge.contains('▲') || right_edge.contains('△'));
    assert!(right_edge.contains('▼') || right_edge.contains('▽'));
}

#[test]
fn long_chat_defaults_to_bottom_of_session() {
    let mut app = App::new();
    for i in 0..20 {
        app.add_message(Message::User(UserMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: i,
            role: "user".to_string(),
            content: MessageContent::Text(format!("message {i}")),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        }));
    }
    let mut terminal = Terminal::new(TestBackend::new(40, 12)).expect("terminal");

    terminal.draw(|frame| app.render(frame)).expect("draw");

    let content = buffer_to_lines(terminal.backend().buffer(), 40, 12).join("\n");
    assert!(content.contains("message 19"));
    assert!(!content.contains("message 0"));
}

#[test]
fn mouse_click_session_scrollbar_controls_prompt_messages() {
    let mut app = App::new();
    for i in 0..20 {
        app.add_message(Message::User(UserMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: i,
            role: "user".to_string(),
            content: MessageContent::Text(format!("message {i}")),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        }));
    }
    app.conversation.set_scroll_offset(0);
    let mut terminal = Terminal::new(TestBackend::new(40, 12)).expect("terminal");
    terminal.draw(|frame| app.render(frame)).expect("draw");
    let scrollbar = app
        .render_layout
        .session_scrollbar
        .expect("session scrollbar");

    assert_eq!(
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: scrollbar.area.x,
            row: scrollbar
                .area
                .y
                .saturating_add(scrollbar.area.height.saturating_sub(2)),
            modifiers: KeyModifiers::NONE,
        }),
        AppAction::ScrollDown
    );
    assert!(app.conversation.scroll_offset() > 0);
}

#[test]
fn immediate_notification_overrides_spinner_row() {
    let mut app = App::new();
    app.set_streaming(true);
    app.set_spinner_message("Thinking...".to_string());
    app.add_notification(
        InAppNotification::new(
            "rate-limit",
            NotificationPriority::Immediate,
            "Rate limit reached",
        )
        .with_tone(NotificationTone::Error),
    );
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("terminal");

    terminal.draw(|frame| app.render(frame)).expect("draw");

    let content = buffer_to_lines(terminal.backend().buffer(), 80, 24).join("\n");
    assert!(content.contains("Rate limit reached"));
    assert!(!content.contains("Thinking..."));
}

#[test]
fn prompt_stays_editable_while_streaming_and_tab_queues() {
    let mut app = App::new();
    app.set_streaming(true);

    assert_eq!(send_key(&mut app, KeyCode::Char('n')), AppAction::None);
    assert_eq!(send_key(&mut app, KeyCode::Char('e')), AppAction::None);
    assert_eq!(send_key(&mut app, KeyCode::Char('x')), AppAction::None);
    assert_eq!(send_key(&mut app, KeyCode::Char('t')), AppAction::None);

    assert_eq!(app.prompt.input, "next");
    assert_eq!(
        send_key(&mut app, KeyCode::Enter),
        AppAction::Steer("next".to_string())
    );
    assert!(app.prompt.input.is_empty());

    app.prompt.input = "queued".to_string();
    app.prompt.cursor_position = app.prompt.input.len();
    assert_eq!(
        send_key(&mut app, KeyCode::Tab),
        AppAction::Queue("queued".to_string())
    );
    assert!(app.prompt.input.is_empty());

    app.prompt.input = "/review these changes".to_string();
    app.prompt.cursor_position = app.prompt.input.len();
    assert_eq!(
        send_key(&mut app, KeyCode::Tab),
        AppAction::Queue("/review these changes".to_string())
    );
    assert!(app.prompt.input.is_empty());
}

#[test]
fn idle_tab_submits_instead_of_queueing() {
    let mut app = App::new();
    app.prompt.input = "send now".to_string();
    app.prompt.cursor_position = app.prompt.input.len();

    assert_eq!(
        send_key(&mut app, KeyCode::Tab),
        AppAction::Submit("send now".to_string())
    );
}

#[test]
fn app_facade_routes_messages_through_conversation_store() {
    let mut app = App::new();
    add_user_message(&mut app, "hello");

    assert_eq!(app.messages().len(), 1);
    let Message::User(message) = &app.messages()[0] else {
        panic!("expected user message");
    };
    assert!(matches!(
        &message.content,
        MessageContent::Text(text) if text == "hello"
    ));
}

#[test]
fn app_overlay_priority_is_stable_for_question_then_permission() {
    let mut app = App::new();
    app.show_question_dialog(
        "q-1",
        AskUserRequestPayload {
            question: "Pick one".to_string(),
            choices: vec!["A".to_string()],
            allow_free_text: false,
        },
    );
    app.show_permission_dialog("Bash", r#"{"command":"cargo test"}"#, "Run command?");

    assert_eq!(
        app.active_overlay_for_tests(),
        Some(ActiveOverlay::Question)
    );
}

#[test]
fn app_overlay_priority_falls_back_through_all_overlays() {
    let mut app = App::new();
    app.set_session_id("session-main".to_string());
    app.handle_app_event(AppEvent::Backend {
        message: Box::new(BackendMessage::AgentEvent {
            event: spawned_agent_event("worker-1", "Builder worker", Some("builder")),
        }),
    });
    app.push_history("previous prompt".to_string());
    app.open_history_search();
    app.open_command_surface(CommandSurface::Tasks(
        crate::ui::command_surface::TasksSurface::from_items(vec![]),
    ));
    app.dispatch_bound_action(&Action::new_static("agents:tree"));
    app.show_permission_dialog("Bash", r#"{"command":"cargo test"}"#, "Run command?");
    app.show_question_dialog(
        "q-1",
        AskUserRequestPayload {
            question: "Pick one".to_string(),
            choices: vec!["A".to_string()],
            allow_free_text: false,
        },
    );
    app.show_bypass_permissions_mode_dialog(false);

    assert_eq!(
        app.active_overlay_for_tests(),
        Some(ActiveOverlay::BypassPermissions)
    );

    app.overlays.bypass_permissions_mode_dialog = None;
    assert_eq!(
        app.active_overlay_for_tests(),
        Some(ActiveOverlay::Question)
    );

    app.overlays.clear_question();
    assert_eq!(
        app.active_overlay_for_tests(),
        Some(ActiveOverlay::Permission)
    );

    app.dismiss_permission_dialog();
    assert_eq!(
        app.active_overlay_for_tests(),
        Some(ActiveOverlay::AgentTree)
    );

    app.overlays.agent_tree_dialog = None;
    assert_eq!(
        app.active_overlay_for_tests(),
        Some(ActiveOverlay::HistorySearch)
    );

    app.overlays.history_search_dialog = None;
    assert_eq!(
        app.active_overlay_for_tests(),
        Some(ActiveOverlay::CommandSurface)
    );

    app.overlays.command_surface = None;
    assert_eq!(app.active_overlay_for_tests(), None);
}

#[test]
fn app_owns_queued_prompt_fifo() {
    let mut app = App::new();

    assert_eq!(app.queue_prompt("one".to_string()), 1);
    assert_eq!(app.queue_prompt("two".to_string()), 2);
    assert_eq!(app.queued_count(), 2);
    assert_eq!(app.pop_next_queued().as_deref(), Some("one"));
    assert_eq!(app.pop_next_queued().as_deref(), Some("two"));
    assert_eq!(app.pop_next_queued(), None);
}

#[test]
fn status_payload_reads_domain_stores_directly() {
    let cwd = tempfile::tempdir().expect("cwd");
    let cwd_text = cwd.path().display().to_string();
    let mut app = App::new();
    app.session_ui.session_id = "store-session".to_string();
    app.session_ui.model_name = "deepseek-v4-pro".to_string();
    app.session_ui.backend_name = "native".to_string();
    app.session_ui.cwd = cwd_text.clone();
    app.conversation.add_message(Message::User(UserMessage {
        uuid: uuid::Uuid::new_v4(),
        timestamp: 0,
        role: "user".to_string(),
        content: MessageContent::Text("store message".to_string()),
        is_meta: false,
        tool_use_result: None,
        source_tool_assistant_uuid: None,
    }));

    let payload = app.build_status_payload();

    assert_eq!(payload.session_id.as_deref(), Some("store-session"));
    let model = payload.model.expect("model");
    assert_eq!(model.id, "deepseek-v4-pro");
    assert_eq!(model.backend.as_deref(), Some("native"));
    assert_eq!(
        payload.workspace.expect("workspace").cwd,
        cwd.path().display().to_string()
    );
    assert_eq!(payload.message_count, 1);
}

#[test]
fn streaming_draft_renders_tab_queue_hint_below_prompt() {
    let mut app = App::new();
    app.set_streaming(true);
    app.prompt.input = "follow up".to_string();
    app.prompt.cursor_position = app.prompt.input.len();
    app.queue_prompt("one".to_string());
    app.queue_prompt("two".to_string());
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).expect("terminal");

    terminal.draw(|frame| app.render(frame)).expect("draw");

    let content = buffer_to_lines(terminal.backend().buffer(), 100, 24).join("\n");
    assert!(content.contains("tab to queue message"));
    assert!(content.contains("2 queued"));
}

#[test]
fn app_event_notification_is_routed_to_in_app_notification() {
    let mut app = App::new();

    app.handle_app_event(AppEvent::Notification {
        key: "rate-limit".to_string(),
        message: "Rate limit reached".to_string(),
        level: "error".to_string(),
        timeout_ms: Some(5000),
    });

    let notification = app.current_notification().expect("notification");
    assert_eq!(notification.key, "rate-limit");
    assert_eq!(notification.text, "Rate limit reached");
    assert_eq!(notification.priority, NotificationPriority::High);
    assert_eq!(notification.tone, NotificationTone::Error);
}

#[test]
fn backend_notification_event_is_routed_to_in_app_notification() {
    let mut app = App::new();

    app.handle_app_event(AppEvent::Backend {
        message: Box::new(BackendMessage::NotificationSent {
            title: "Background task finished".to_string(),
            level: "info".to_string(),
        }),
    });

    let notification = app.current_notification().expect("notification");
    assert_eq!(notification.key, "backend-notification");
    assert_eq!(notification.text, "Background task finished");
    assert_eq!(notification.priority, NotificationPriority::Medium);
}

#[test]
fn app_context_layer_includes_available_runtime_state() {
    let mut app = App::new();
    app.set_cwd("/repo/workspace".to_string());
    app.set_model_name("codex-high".to_string());
    app.update_goal_status(
        "updated",
        &serde_json::json!({
            "objective": "ship the release",
            "status": "active",
            "tokens_used": 42,
            "time_used_seconds": 9
        }),
    );
    app.update_session_usage(120, 30, 10, 5, 2, None, None);
    app.runtime_view
        .upsert_agent(crate::ui::app::agent_navigation::AgentThreadEntry {
            thread_id: "worker-1".to_string(),
            agent_nickname: Some("Build worker".to_string()),
            agent_role: Some("executor".to_string()),
            is_primary: false,
            is_closed: false,
        });
    app.runtime_view
        .set_current_agent_thread(Some("worker-1".to_string()));
    app.runtime_view
        .agent_nav_mut()
        .mark_tool_use("worker-1", "tool-1", "Bash", "cargo test");
    app.show_permission_dialog("Bash", r#"{"command":"cargo test"}"#, "Run command?");
    app.add_notification(
        InAppNotification::new("backend-error", NotificationPriority::High, "Build failed")
            .with_tone(NotificationTone::Error),
    );

    app.refresh_context_layer();
    let items = app.runtime_view.context_layer().items();

    assert_context_item(&items, "repo", "/repo/workspace");
    assert_context_item(&items, "model", "codex-high");
    assert_context_item(&items, "plan", "active ship the release");
    assert_context_item(&items, "ctx", "165t");
    assert_context_item(&items, "agent", "Build worker");
    assert_context_item(&items, "tool", "Bash cargo test");
    assert_context_item(&items, "permission", "pending approval");
    assert_context_item(&items, "error", "Build failed");
}

#[test]
fn ordinary_info_notification_does_not_create_sticky_context_item() {
    let mut app = App::new();
    app.add_notification(
        InAppNotification::new(
            "backend-notification",
            NotificationPriority::Medium,
            "Background task finished",
        )
        .with_tone(NotificationTone::Info),
    );

    app.refresh_context_layer();
    let items = app.runtime_view.context_layer().items();

    assert!(items
        .iter()
        .all(|item| !item.value.contains("Background task finished")));
    assert_eq!(
        app.current_notification()
            .map(|notification| notification.tone),
        Some(NotificationTone::Info)
    );
}

#[test]
fn context_layer_tool_summary_uses_latest_summary_without_duplicate_tool_name() {
    let mut app = App::new();
    app.runtime_view
        .upsert_agent(crate::ui::app::agent_navigation::AgentThreadEntry {
            thread_id: "worker-1".to_string(),
            agent_nickname: Some("Build worker".to_string()),
            agent_role: Some("executor".to_string()),
            is_primary: false,
            is_closed: false,
        });
    app.runtime_view
        .set_current_agent_thread(Some("worker-1".to_string()));
    app.runtime_view
        .agent_nav_mut()
        .mark_tool_use("worker-1", "z-old", "Bash", "Bash old command");
    app.runtime_view
        .agent_nav_mut()
        .mark_tool_use("worker-1", "a-new", "Bash", "Bash new command");

    app.refresh_context_layer();
    let items = app.runtime_view.context_layer().items();
    let tool = items
        .iter()
        .find(|item| item.label == "tool")
        .expect("tool context item");

    assert_eq!(tool.value, "Bash new command");
}

#[test]
fn context_layer_drops_current_tool_after_tool_result() {
    let mut app = App::new();
    app.handle_app_event(AppEvent::Backend {
        message: Box::new(BackendMessage::AgentEvent {
            event: spawned_agent_event("worker-1", "Build worker", Some("executor")),
        }),
    });
    app.handle_app_event(AppEvent::Backend {
        message: Box::new(BackendMessage::AgentEvent {
            event: AgentEvent::ToolUse {
                agent_id: "worker-1".to_string(),
                tool_use_id: "tool-1".to_string(),
                tool_name: "Bash".to_string(),
                input: serde_json::json!({ "command": "cargo test" }),
            },
        }),
    });

    app.refresh_context_layer();
    let items = app.runtime_view.context_layer().items();
    assert_context_item(&items, "tool", "Bash");

    app.handle_app_event(AppEvent::Backend {
        message: Box::new(BackendMessage::AgentEvent {
            event: AgentEvent::ToolResult {
                agent_id: "worker-1".to_string(),
                tool_use_id: "tool-1".to_string(),
                output: "finished".to_string(),
                is_error: false,
            },
        }),
    });

    app.refresh_context_layer();
    let items = app.runtime_view.context_layer().items();
    assert!(
        items.iter().all(|item| item.label != "tool"),
        "completed tool should not remain current in {items:?}"
    );
}

#[test]
fn tool_result_user_message_does_not_clear_latest_sticky_error() {
    let mut app = App::new();
    app.add_notification(
        InAppNotification::new("backend-error", NotificationPriority::High, "Build failed")
            .with_tone(NotificationTone::Error),
    );
    app.remove_notification("backend-error");

    app.add_message(Message::User(UserMessage {
        uuid: uuid::Uuid::new_v4(),
        timestamp: 0,
        role: "user".to_string(),
        content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
            tool_use_id: "tool-1".to_string(),
            content: ToolResultContent::Text("command output".to_string()),
            is_error: false,
        }]),
        is_meta: false,
        tool_use_result: Some("command output".to_string()),
        source_tool_assistant_uuid: None,
    }));

    app.refresh_context_layer();
    let items = app.runtime_view.context_layer().items();

    assert_context_item(&items, "error", "Build failed");
}

#[test]
fn prompt_render_populates_context_row_between_notification_and_agent_footer() {
    let mut app = App::new();
    add_user_message(&mut app, "hello");
    app.runtime_view
        .upsert_agent(crate::ui::app::agent_navigation::AgentThreadEntry {
            thread_id: "worker-1".to_string(),
            agent_nickname: Some("Build worker".to_string()),
            agent_role: Some("executor".to_string()),
            is_primary: false,
            is_closed: false,
        });
    app.runtime_view
        .set_current_agent_thread(Some("worker-1".to_string()));
    app.add_notification(
        InAppNotification::new("system-ready", NotificationPriority::Medium, "System ready")
            .with_tone(NotificationTone::Info),
    );
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).expect("terminal");

    terminal.draw(|frame| app.render(frame)).expect("draw");

    let content = buffer_to_lines(terminal.backend().buffer(), 100, 24);
    let notification_row = row_containing(&content, "System ready");
    let context_row = row_containing(&content, "agent: Build worker");
    let footer_row = row_containing(&content, "Running 1 agent");
    assert!(
        notification_row < context_row && context_row < footer_row,
        "context row should sit between notification and agent footer\n{}",
        content.join("\n")
    );
}

#[test]
fn task_events_update_runtime_state_before_tasks_surface_opens() {
    let mut app = App::new();

    app.handle_app_event(AppEvent::Backend {
        message: Box::new(BackendMessage::ToolProgress {
            tool_use_id: "tool-1".to_string(),
            tool: "cargo test".to_string(),
            output: "running 1 test".to_string(),
            elapsed_seconds: 2,
            total_lines: Some(1),
            total_bytes: None,
            timeout_ms: None,
            operation: None,
        }),
    });

    assert!(app
        .runtime_state()
        .tasks()
        .iter()
        .any(|task| task.id == "tool-1" && task.title == "cargo test"));

    app.open_command_surface(CommandSurface::Tasks(
        crate::ui::command_surface::TasksSurface::from_items(vec![]),
    ));

    let Some(CommandSurface::Tasks(surface)) = app.overlays.command_surface.as_ref() else {
        panic!("tasks surface should be open");
    };
    assert!(surface.render().contains("cargo test"));
    let task = &surface.selected_item().expect("selected task").task;
    assert_eq!(task.id, "tool-1");
    assert_eq!(task.output_lines, vec!["running 1 test".to_string()]);
}

#[test]
fn opening_tasks_surface_refreshes_runtime_from_live_items() {
    let mut app = App::new();

    let stale_item = crate::ui::command_surface::TaskSurfaceItem {
        task: crate::ui::tasks::TaskStatus::new(
            "stale-tool",
            "old live task",
            crate::ui::tasks::TaskKind::Shell,
        ),
        source: crate::ui::command_surface::TaskSurfaceSource::Tool,
    };
    let live_item = crate::ui::command_surface::TaskSurfaceItem {
        task: crate::ui::tasks::TaskStatus::new(
            "live-tool",
            "fresh live task",
            crate::ui::tasks::TaskKind::Shell,
        ),
        source: crate::ui::command_surface::TaskSurfaceSource::Tool,
    };
    app.open_command_surface(CommandSurface::Tasks(
        crate::ui::command_surface::TasksSurface::from_items(vec![stale_item]),
    ));
    app.open_command_surface(CommandSurface::Tasks(
        crate::ui::command_surface::TasksSurface::from_items(vec![live_item]),
    ));

    let runtime_tasks = app.runtime_state().tasks();
    assert_eq!(runtime_tasks.len(), 1);
    assert_eq!(runtime_tasks[0].id, "live-tool");

    let Some(CommandSurface::Tasks(surface)) = app.overlays.command_surface.as_ref() else {
        panic!("tasks surface should be open");
    };
    let selected = surface.selected_item().expect("selected task");
    assert_eq!(selected.task.id, "live-tool");
    assert!(surface.render().contains("fresh live task"));
}

#[test]
fn opening_tasks_surface_with_empty_live_snapshot_clears_stale_live_items() {
    let mut app = App::new();

    let stale_item = crate::ui::command_surface::TaskSurfaceItem {
        task: crate::ui::tasks::TaskStatus::new(
            "stale-tool",
            "old live task",
            crate::ui::tasks::TaskKind::Shell,
        ),
        source: crate::ui::command_surface::TaskSurfaceSource::Tool,
    };
    app.open_command_surface(CommandSurface::Tasks(
        crate::ui::command_surface::TasksSurface::from_items(vec![stale_item]),
    ));
    assert!(app
        .runtime_state()
        .tasks()
        .iter()
        .any(|task| task.id == "stale-tool"));

    app.open_command_surface(CommandSurface::Tasks(
        crate::ui::command_surface::TasksSurface::from_items(vec![]),
    ));

    assert!(app.runtime_state().tasks().is_empty());
    let Some(CommandSurface::Tasks(surface)) = app.overlays.command_surface.as_ref() else {
        panic!("tasks surface should be open");
    };
    assert!(surface.selected_item().is_none());
    assert!(!surface.render().contains("old live task"));
}

#[test]
fn high_priority_notification_preempts_verbose_indicator() {
    let mut app = App::new();
    let state = AppState {
        verbose: true,
        ..Default::default()
    };
    app.sync_status_context_from_state(&state);

    app.handle_app_event(AppEvent::Notification {
        key: "system-error".to_string(),
        message: "Subsystem failed".to_string(),
        level: "error".to_string(),
        timeout_ms: Some(5000),
    });

    let notification = app.current_notification().expect("notification");
    assert_eq!(notification.key, "system-error");
    assert_eq!(notification.text, "Subsystem failed");
}

#[test]
fn test_only_app_accessors_drive_state() {
    let mut app = App::new();
    assert_eq!(app.view_mode(), ViewMode::Prompt);
    app.cycle_view_mode();
    assert_eq!(app.view_mode(), ViewMode::Transcript);
    assert_eq!(app.transcript_state().scroll_offset, usize::MAX);

    app.set_suggestions(vec![PromptSuggestion {
        text: "next".to_string(),
        confidence: 0.8,
        category: SuggestionCategory::FollowUp,
    }]);
    assert_eq!(app.suggestions().expect("suggestions")[0].text, "next");
    app.clear_suggestions();
    assert!(app.suggestions().is_none());

    app.show_permission_dialog("bash", "ls", "Run command?");
    app.dismiss_permission_dialog();
    assert!(app.overlays.permission_dialog.is_none());
    let _runner = app.status_line_runner();
}

#[test]
fn configured_view_mode_applies_to_app_state() {
    let mut app = App::new();

    app.set_view_mode(ViewMode::Transcript);
    assert_eq!(app.view_mode(), ViewMode::Transcript);
    assert_eq!(app.transcript_state().scroll_offset, usize::MAX);

    app.set_view_mode(ViewMode::Prompt);
    assert_eq!(app.view_mode(), ViewMode::Prompt);
}

#[test]
fn configured_spinner_tips_rotate_and_disable() {
    let mut app = App::new();
    app.set_spinner_tips_settings(allthecodes_config::settings::SpinnerTipsSettings {
        enabled: Some(true),
        interval_ms: Some(16),
        custom_tips: vec!["alpha".to_string(), "beta".to_string()],
        extra: Default::default(),
    });

    app.set_streaming(true);
    app.tick();
    assert_eq!(app.spinner_message(), "Thinking...  Tip: alpha");
    app.tick();
    assert_eq!(app.spinner_message(), "Thinking...  Tip: beta");

    app.set_spinner_tips_settings(allthecodes_config::settings::SpinnerTipsSettings {
        enabled: Some(false),
        interval_ms: Some(16),
        custom_tips: vec!["alpha".to_string()],
        extra: Default::default(),
    });
    assert_eq!(app.spinner_message(), "Thinking...");
}

#[test]
fn agent_event_updates_navigation_and_footer_rendering() {
    let mut app = App::new();
    app.set_session_id("session-main".to_string());
    app.handle_app_event(AppEvent::Backend {
        message: Box::new(BackendMessage::AgentEvent {
            event: spawned_agent_event("worker-1", "Builder worker", Some("builder")),
        }),
    });

    assert_eq!(app.runtime_state().agent_nav().thread_count(), 2);
    assert!(app.agent_footer_visible());
    assert_eq!(app.current_agent_thread_id(), "worker-1");

    let mut terminal = Terminal::new(TestBackend::new(120, 24)).expect("terminal");
    terminal.draw(|frame| app.render(frame)).expect("draw");
    let content = buffer_to_lines(terminal.backend().buffer(), 120, 24).join("\n");
    assert!(content.contains("Ctrl+X Ctrl+A open tree"));
    assert!(content.contains("Builder worker |"));
}

#[test]
fn fork_agent_event_exposes_context_mode_in_navigation() {
    let mut app = App::new();
    app.set_session_id("session-main".to_string());
    let mut event = spawned_agent_event("fork-1", "Review latest", None);
    if let AgentEvent::Spawned { fork_metadata, .. } = &mut event {
        *fork_metadata = Some(allthecodes_types::agent_types::ForkLaunchMetadata {
            is_fork: true,
            context: allthecodes_types::agent_types::ForkContextMode::LiveReadonly,
            live_channel: Some(allthecodes_types::agent_types::LiveParentContextPaths {
                directory: "/tmp/fork-1".to_string(),
                snapshot: "/tmp/fork-1/parent-context.md".to_string(),
                updates: "/tmp/fork-1/parent-updates.ndjson".to_string(),
                latest_diff: None,
                latest_seq: 1,
            }),
        });
    }
    app.handle_app_event(AppEvent::Backend {
        message: Box::new(BackendMessage::AgentEvent { event }),
    });

    let entry = app
        .runtime_state()
        .agent_nav()
        .entry("fork-1")
        .expect("fork navigation entry");
    assert_eq!(
        entry.agent_role.as_deref(),
        Some("fork · context: live readonly")
    );
    assert!(app
        .runtime_state()
        .agent_nav()
        .runtime_info("fork-1")
        .and_then(|runtime| runtime.status_summary.as_deref())
        .is_some_and(|summary| summary.contains("context: live readonly")));
}

#[test]
fn agent_tree_dialog_navigation_select_and_close() {
    let mut app = App::new();
    app.set_session_id("session-main".to_string());
    app.handle_app_event(AppEvent::Backend {
        message: Box::new(BackendMessage::AgentEvent {
            event: spawned_agent_event("worker-1", "Builder one", Some("builder")),
        }),
    });
    app.handle_app_event(AppEvent::Backend {
        message: Box::new(BackendMessage::AgentEvent {
            event: spawned_agent_event("worker-2", "Builder two", Some("reviewer")),
        }),
    });

    assert_eq!(
        app.dispatch_bound_action(&Action::new_static("chat:killAgents")),
        Some(AppAction::KillAgentThreads(vec![
            "worker-1".to_string(),
            "worker-2".to_string()
        ]))
    );
    assert!(app.overlays.agent_tree_dialog.is_none());

    assert_eq!(
        app.dispatch_bound_action(&Action::new_static("agents:tree")),
        Some(AppAction::None)
    );
    assert!(app.overlays.agent_tree_dialog.is_some());

    assert_eq!(send_key(&mut app, KeyCode::Up), AppAction::None);
    assert_eq!(
        send_key(&mut app, KeyCode::Enter),
        AppAction::AgentThreadSelected("worker-1".to_string())
    );
    assert_eq!(app.current_agent_thread_id(), "worker-1");
    assert!(app.overlays.agent_tree_dialog.is_none());

    assert_eq!(
        app.dispatch_bound_action(&Action::new_static("agents:tree")),
        Some(AppAction::None)
    );
    assert!(app.overlays.agent_tree_dialog.is_some());
    assert_eq!(send_key(&mut app, KeyCode::Esc), AppAction::None);
    assert!(app.overlays.agent_tree_dialog.is_none());
}

#[test]
fn agent_tree_dialog_renders_above_prompt_input() {
    let mut app = App::new();
    add_chat_context(&mut app);
    app.set_session_id("session-main".to_string());
    app.handle_app_event(AppEvent::Backend {
        message: Box::new(BackendMessage::AgentEvent {
            event: spawned_agent_event("worker-1", "Builder one", Some("builder")),
        }),
    });
    app.handle_app_event(AppEvent::Backend {
        message: Box::new(BackendMessage::AgentEvent {
            event: spawned_agent_event("worker-2", "Builder two", Some("reviewer")),
        }),
    });
    assert_eq!(
        app.dispatch_bound_action(&Action::new_static("agents:tree")),
        Some(AppAction::None)
    );
    let mut terminal = Terminal::new(TestBackend::new(120, 24)).expect("terminal");

    terminal.draw(|frame| app.render(frame)).expect("draw");

    let content = buffer_to_lines(terminal.backend().buffer(), 120, 24);
    assert!(content.join("\n").contains("Builder one"));
    assert_title_above_prompt_area(
        &content,
        "Agent Threads",
        app.render_layout.prompt_area.expect("prompt area"),
    );
}

#[test]
#[serial]
fn status_bar_renders_only_model_and_workspace() {
    let home = tempfile::tempdir().expect("allthecodes home");
    let _home_guard = EnvGuard::set_path("ALLTHECODES_HOME", home.path());
    let mut app = App::new();
    app.set_model_name("deepseek-v4-pro".to_string());
    app.set_cwd("/repo/workspace".to_string());
    app.accept_workspace_trust();
    let mut state = AppState::default();
    state.tool_permission_context.mode = PermissionMode::AcceptEdits;
    state.settings.sandbox.enabled = Some(true);
    state.settings.sandbox.mode = Some("workspace".to_string());
    state.settings.sandbox.network.disabled = Some(true);
    state.effort_value = Some("medium".to_string());
    state.kairos_active = true;
    app.sync_status_context_from_state(&state);

    let mut terminal = Terminal::new(TestBackend::new(120, 24)).expect("terminal");
    terminal.draw(|frame| app.render(frame)).expect("draw");

    let content = buffer_to_lines(terminal.backend().buffer(), 120, 24).join("\n");
    assert!(content.contains("deepseek-v4-pro | /repo/workspace"));
    assert!(!content.contains("perm:acceptEdits"));
    assert!(!content.contains("sandbox:workspace,no-net"));
    assert!(!content.contains("effort:medium"));
    assert!(!content.contains("remote:"));
}

#[test]
#[serial]
fn status_bar_renders_goal_summary() {
    let home = tempfile::tempdir().expect("allthecodes home");
    let _home_guard = EnvGuard::set_path("ALLTHECODES_HOME", home.path());
    let mut app = App::new();
    app.set_model_name("deepseek-v4-pro".to_string());
    app.set_cwd("/repo/workspace".to_string());
    app.accept_workspace_trust();
    app.update_goal_status(
        "updated",
        &serde_json::json!({
            "objective": "ship the release",
            "status": "active",
            "tokens_used": 42,
            "time_used_seconds": 9
        }),
    );

    let mut terminal = Terminal::new(TestBackend::new(120, 24)).expect("terminal");
    terminal.draw(|frame| app.render(frame)).expect("draw");

    let content = buffer_to_lines(terminal.backend().buffer(), 120, 24).join("\n");
    assert!(content.contains("goal=active ship the release 9s 42t"));
}

#[test]
#[serial]
fn status_bar_renders_all_goal_statuses_distinctly() {
    let home = tempfile::tempdir().expect("allthecodes home");
    let _home_guard = EnvGuard::set_path("ALLTHECODES_HOME", home.path());

    for (status, label) in [
        ("active", "active"),
        ("paused", "paused"),
        ("blocked", "blocked"),
        ("usage_limited", "usage-limited"),
        ("budget_limited", "budget-limited"),
        ("complete", "complete"),
    ] {
        let mut app = App::new();
        app.accept_workspace_trust();
        app.update_goal_status(
            "updated",
            &serde_json::json!({
                "objective": "ship",
                "status": status,
                "tokens_used": 7,
                "time_used_seconds": 1
            }),
        );

        let mut terminal = Terminal::new(TestBackend::new(80, 12)).expect("terminal");
        terminal.draw(|frame| app.render(frame)).expect("draw");

        let content = buffer_to_lines(terminal.backend().buffer(), 80, 12).join("\n");
        assert!(
            content.contains(&format!("goal={label} ship 1s 7t")),
            "missing goal status label {label} in {content:?}"
        );
    }
}

#[test]
fn slash_palette_enter_executes_selected_command_once() {
    let mut app = App::new();

    assert_eq!(send_key(&mut app, KeyCode::Char('/')), AppAction::None);
    assert!(app.command_palette.active());

    assert_eq!(send_key(&mut app, KeyCode::Char('m')), AppAction::None);
    assert_eq!(send_key(&mut app, KeyCode::Char('c')), AppAction::None);
    assert_eq!(
        send_key(&mut app, KeyCode::Enter),
        AppAction::Submit("/mcp".to_string())
    );

    assert!(app.prompt.input.is_empty());
    assert!(!app.command_palette.active());
}

#[test]
fn slash_palette_space_enters_arguments_then_enter_submits() {
    let mut app = App::new();

    for ch in "/mc".chars() {
        assert_eq!(send_key(&mut app, KeyCode::Char(ch)), AppAction::None);
    }
    assert_eq!(send_key(&mut app, KeyCode::Char(' ')), AppAction::None);

    assert_eq!(app.prompt.input, "/mcp ");
    assert!(!app.command_palette.active());
    assert!(
        CommandPalette::argument_hint(&app.prompt.input, std::path::Path::new(app.cwd())).is_some()
    );

    for ch in "status".chars() {
        assert_eq!(send_key(&mut app, KeyCode::Char(ch)), AppAction::None);
    }
    assert_eq!(
        send_key(&mut app, KeyCode::Enter),
        AppAction::Submit("/mcp status".to_string())
    );
    assert!(app.prompt.input.is_empty());
}

#[test]
fn slash_palette_enter_submits_unmatched_command_once() {
    let mut app = App::new();
    app.prompt.input = "/definitely-unknown-command".to_string();
    app.prompt.cursor_position = app.prompt.input.len();
    app.sync_command_palette();

    assert!(app.command_palette.active());
    assert!(app
        .command_palette
        .selected_command_for_execution()
        .is_none());
    assert_eq!(
        send_key(&mut app, KeyCode::Enter),
        AppAction::Submit("/definitely-unknown-command".to_string())
    );
    assert!(app.prompt.input.is_empty());
    assert!(!app.command_palette.active());
}

#[test]
fn slash_palette_space_preserves_unmatched_command() {
    let mut app = App::new();
    app.prompt.input = "/definitely-unknown-command".to_string();
    app.prompt.cursor_position = app.prompt.input.len();
    app.sync_command_palette();

    assert_eq!(send_key(&mut app, KeyCode::Char(' ')), AppAction::None);
    assert_eq!(app.prompt.input, "/definitely-unknown-command ");
    assert_eq!(app.prompt.cursor_position, app.prompt.input.len());
    assert!(!app.command_palette.active());
}

#[test]
fn ctrl_e_opens_edit_target_picker_for_supported_commands() {
    let mut app = App::new();
    app.prompt.input = "/mcp".to_string();
    app.prompt.cursor_position = app.prompt.input.len();
    app.sync_command_palette();

    assert_eq!(
        send_key_with_modifiers(&mut app, KeyCode::Char('e'), KeyModifiers::CONTROL),
        AppAction::None
    );
    assert!(app.command_palette.active());
    assert!(app.command_palette.edit_target_picker_active());
    assert_eq!(
        app.command_palette.selected_edit_target_input().as_deref(),
        Some("edit user")
    );
}

#[test]
fn picker_enter_inserts_selected_target_into_prompt() {
    let mut app = App::new();
    app.prompt.input = "/mcp".to_string();
    app.prompt.cursor_position = app.prompt.input.len();
    app.sync_command_palette();

    assert_eq!(
        send_key_with_modifiers(&mut app, KeyCode::Char('e'), KeyModifiers::CONTROL),
        AppAction::None
    );
    assert_eq!(send_key(&mut app, KeyCode::Enter), AppAction::None);

    assert_eq!(app.prompt.input, "/mcp edit user");
    assert!(!app.command_palette.edit_target_picker_active());
}

#[test]
fn command_palette_renders_above_prompt_input() {
    let mut app = App::new();
    app.prompt.input = "/".to_string();
    app.prompt.cursor_position = app.prompt.input.len();
    app.sync_command_palette();
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).expect("terminal");

    terminal.draw(|frame| app.render(frame)).expect("draw");

    let content = buffer_to_lines(terminal.backend().buffer(), 100, 24);
    let prompt_row = content
        .iter()
        .position(|line| line.trim_start().starts_with(">"))
        .expect("prompt row");
    let commands_row = content
        .iter()
        .position(|line| line.contains(" Commands "))
        .expect("commands row");
    assert!(
        commands_row < prompt_row,
        "commands palette should render above the prompt input"
    );
    assert!(
        content.join("\n").contains(CommandPalette::input_hint()),
        "prompt should explain one-key command execution and argument entry"
    );
}

#[test]
fn completion_popup_renders_above_prompt_input() {
    let mut app = App::new();
    app.completion_state.active = true;
    app.completion_state.items = vec![CompletionItem::new(
        CompletionKind::Command,
        "/help",
        "/help",
        0..0,
    )];
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).expect("terminal");

    terminal.draw(|frame| app.render(frame)).expect("draw");

    let content = buffer_to_lines(terminal.backend().buffer(), 100, 24);
    let prompt_row = content
        .iter()
        .position(|line| line.trim_start().starts_with(">"))
        .expect("prompt row");
    let completions_row = content
        .iter()
        .position(|line| line.contains(" Completions "))
        .expect("completions row");
    assert!(
        completions_row < prompt_row,
        "completion popup should render above the prompt input"
    );
}

#[test]
fn completion_popup_selected_row_uses_theme_selected() {
    let mut app = App::new();
    app.completion_state.active = true;
    app.completion_state.items = vec![
        CompletionItem::new(CompletionKind::Command, "/help", "/help", 0..0),
        CompletionItem::new(CompletionKind::Command, "/status", "/status", 0..0),
    ];
    app.completion_state.selected = 1;
    let selected_style = app.theme.selected;
    assert_eq!(selected_style.fg, Some(Color::Rgb(0, 0, 0)));
    assert_eq!(selected_style.bg, Some(Color::Rgb(130, 200, 255)));
    assert!(selected_style.add_modifier.contains(Modifier::BOLD));
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).expect("terminal");

    terminal.draw(|frame| app.render(frame)).expect("draw");

    let buffer = terminal.backend().buffer();
    let selected_cell = (0..24)
        .flat_map(|y| (0..100).map(move |x| (x, y)))
        .map(|(x, y)| &buffer[(x, y)])
        .find(|cell| cell.style().bg == selected_style.bg)
        .expect("selected completion cell with theme background");
    let cell_style = selected_cell.style();
    assert_eq!(cell_style.fg, selected_style.fg);
    assert_eq!(cell_style.bg, selected_style.bg);
    assert!(cell_style
        .add_modifier
        .contains(selected_style.add_modifier));
}

#[test]
#[serial]
fn argument_entry_does_not_render_parameter_help_near_input() {
    let mut app = App::new();
    app.prompt.input = "/plugin ".to_string();
    app.prompt.cursor_position = app.prompt.input.len();
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).expect("terminal");

    terminal.draw(|frame| app.render(frame)).expect("draw");

    let content = buffer_to_lines(terminal.backend().buffer(), 100, 24).join("\n");
    assert!(!content.contains("/plugin arguments"));
    assert!(!content.contains("Usage: /plugin"));
    assert!(!content.contains("installed_plugins.json"));
}

#[test]
#[serial]
fn render_workspace_trust_prompt_after_cwd_is_set() {
    let home = tempfile::tempdir().expect("allthecodes home");
    let _home_guard = EnvGuard::set_path("ALLTHECODES_HOME", home.path());
    let workspace = tempfile::tempdir().expect("workspace");
    let cwd = workspace.path().display().to_string();

    let mut app = App::new();
    app.set_cwd(cwd);
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).expect("terminal");

    terminal.draw(|frame| app.render(frame)).expect("draw");

    let content = buffer_to_lines(terminal.backend().buffer(), 100, 24).join("\n");
    assert!(content.contains("Accessing workspace:"));
    assert!(content.contains("Yes, I trust this folder"));
    assert!(content.contains("No, exit"));
}

#[test]
#[serial]
fn workspace_trust_prompt_accepts_persists_and_exits() {
    let home = tempfile::tempdir().expect("allthecodes home");
    let _home_guard = EnvGuard::set_path("ALLTHECODES_HOME", home.path());
    let workspace = tempfile::tempdir().expect("workspace");
    let cwd = workspace.path().display().to_string();

    let mut app = App::new();
    app.set_cwd(cwd.clone());
    assert!(app.workspace_trust_pending);

    assert_eq!(send_key(&mut app, KeyCode::Enter), AppAction::None);
    assert!(!app.workspace_trust_pending);
    assert!(trusted_workspaces_path().exists());

    let mut reopened = App::new();
    reopened.set_cwd(cwd);
    assert!(!reopened.workspace_trust_pending);

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("terminal");
    terminal.draw(|frame| reopened.render(frame)).expect("draw");
    let content = buffer_to_lines(terminal.backend().buffer(), 80, 24).join("\n");
    assert!(content.contains("allthecodes"));
    assert!(!content.contains("Quick safety check"));

    let other_workspace = tempfile::tempdir().expect("other workspace");
    let mut app = App::new();
    app.set_cwd(other_workspace.path().display().to_string());
    assert!(app.workspace_trust_pending);
    assert_eq!(send_key(&mut app, KeyCode::Esc), AppAction::Quit);
    assert!(app.should_quit());
}

#[test]
fn mouse_wheel_scrolls_prompt_messages() {
    let mut app = App::new();
    app.conversation.set_scroll_offset(10);
    app.dirty = false;

    assert_eq!(
        send_mouse(&mut app, MouseEventKind::ScrollUp),
        AppAction::ScrollUp
    );
    assert_eq!(app.conversation.scroll_offset(), 9);
    assert!(app.dirty);

    app.dirty = false;
    assert_eq!(
        send_mouse(&mut app, MouseEventKind::ScrollDown),
        AppAction::ScrollDown
    );
    assert_eq!(app.conversation.scroll_offset(), 10);
    assert!(app.dirty);
}

#[test]
fn tick_marks_dirty_for_streaming_thinking_animation() {
    let mut app = App::new();
    app.is_streaming = true;
    app.spinner_state.active = false;
    app.tick_counter = 0;
    app.dirty = false;

    for _ in 0..4 {
        app.tick();
        assert!(!app.dirty);
    }

    app.tick();

    assert!(app.dirty);
    assert_eq!(app.thinking_animation_frame(), Some(1));
}

#[test]
fn visible_welcome_logo_marks_dirty_every_eighty_ms() {
    let mut app = App::new();
    app.show_welcome = true;
    app.workspace_trust_pending = false;
    app.welcome_logo_visible = true;
    app.dirty = false;

    for _ in 0..4 {
        app.tick();
        assert!(!app.dirty);
    }
    app.tick();
    assert!(app.dirty);
    assert_eq!(app.welcome_logo.step(), 1);
}

#[test]
fn hidden_or_trust_gated_logo_does_not_advance() {
    for (show_welcome, logo_visible, trust_pending) in [
        (false, true, false),
        (true, false, false),
        (true, true, true),
    ] {
        let mut app = App::new();
        app.show_welcome = show_welcome;
        app.welcome_logo_visible = logo_visible;
        app.workspace_trust_pending = trust_pending;
        app.dirty = false;
        for _ in 0..5 {
            app.tick();
        }
        assert_eq!(app.welcome_logo.step(), 0);
        assert!(!app.dirty);
    }
}

#[test]
fn narrow_render_keeps_welcome_animation_paused() {
    let mut app = App::new();
    let mut terminal = Terminal::new(TestBackend::new(47, 24)).expect("terminal");
    terminal.draw(|frame| app.render(frame)).expect("draw");
    assert!(!app.welcome_logo_visible);

    app.dirty = false;
    for _ in 0..5 {
        app.tick();
    }
    assert_eq!(app.welcome_logo.step(), 0);
    assert!(!app.dirty);
}

#[test]
fn tiny_render_clears_stale_logo_visibility_before_returning() {
    let mut app = App::new();
    app.welcome_logo_visible = true;
    let mut terminal = Terminal::new(TestBackend::new(9, 3)).expect("terminal");
    terminal.draw(|frame| app.render(frame)).expect("draw");
    assert!(!app.welcome_logo_visible);
}

#[test]
fn first_conversation_message_stops_welcome_animation() {
    let mut app = App::new();
    app.welcome_logo_visible = true;
    app.add_message(Message::User(UserMessage {
        uuid: uuid::Uuid::new_v4(),
        timestamp: 0,
        role: "user".to_string(),
        content: MessageContent::Text("hello".to_string()),
        is_meta: false,
        tool_use_result: None,
        source_tool_assistant_uuid: None,
    }));
    assert!(!app.show_welcome);
    assert!(!app.welcome_logo_visible);
}

#[test]
fn mouse_wheel_scrolls_transcript_view() {
    let mut app = App::new();
    app.view_mode = ViewMode::Transcript;
    app.transcript_state.scroll_offset = 10;

    assert_eq!(
        send_mouse(&mut app, MouseEventKind::ScrollUp),
        AppAction::ScrollUp
    );
    assert_eq!(app.transcript_state.scroll_offset, 9);

    assert_eq!(
        send_mouse(&mut app, MouseEventKind::ScrollDown),
        AppAction::ScrollDown
    );
    assert_eq!(app.transcript_state.scroll_offset, 10);
}

#[test]
fn mouse_wheel_over_prompt_scrolls_messages_not_input_history() {
    let mut app = App::new();
    app.conversation.set_scroll_offset(10);
    app.push_history("first".to_string());
    app.push_history("second".to_string());

    let mut terminal = Terminal::new(TestBackend::new(40, 12)).expect("terminal");
    terminal.draw(|frame| app.render(frame)).expect("draw");

    assert_eq!(
        send_mouse_at(&mut app, MouseEventKind::Down(MouseButton::Left), 1, 9),
        AppAction::None
    );
    assert_eq!(
        send_mouse_at(&mut app, MouseEventKind::ScrollUp, 1, 9),
        AppAction::ScrollUp
    );
    assert_eq!(app.conversation.scroll_offset(), 9);
    assert!(app.prompt.input.is_empty());

    assert_eq!(
        send_mouse_at(&mut app, MouseEventKind::ScrollUp, 1, 9),
        AppAction::ScrollUp
    );
    assert_eq!(app.conversation.scroll_offset(), 8);
    assert!(app.prompt.input.is_empty());

    assert_eq!(
        send_mouse_at(&mut app, MouseEventKind::ScrollDown, 1, 9),
        AppAction::ScrollDown
    );
    assert_eq!(app.conversation.scroll_offset(), 9);
    assert!(app.prompt.input.is_empty());
}

#[test]
fn mouse_wheel_over_messages_scrolls_history_after_prompt_focus() {
    let mut app = App::new();
    app.conversation.set_scroll_offset(10);

    let mut terminal = Terminal::new(TestBackend::new(40, 12)).expect("terminal");
    terminal.draw(|frame| app.render(frame)).expect("draw");

    assert_eq!(
        send_mouse_at(&mut app, MouseEventKind::Down(MouseButton::Left), 1, 9),
        AppAction::None
    );
    assert_eq!(
        send_mouse_at(&mut app, MouseEventKind::Down(MouseButton::Left), 1, 1),
        AppAction::None
    );
    assert_eq!(
        send_mouse_at(&mut app, MouseEventKind::ScrollUp, 1, 1),
        AppAction::ScrollUp
    );
    assert_eq!(app.conversation.scroll_offset(), 9);
}

#[test]
fn transcript_arrow_keys_scroll_instead_of_history_fallback() {
    let mut app = App::new();
    app.view_mode = ViewMode::Transcript;
    app.transcript_state.scroll_offset = 10;

    assert_eq!(send_key(&mut app, KeyCode::Up), AppAction::None);
    assert_eq!(app.transcript_state.scroll_offset, 9);

    assert_eq!(send_key(&mut app, KeyCode::Down), AppAction::None);
    assert_eq!(app.transcript_state.scroll_offset, 10);
}

#[test]
fn prompt_arrow_keys_still_drive_history() {
    let mut app = App::new();
    app.push_history("first".to_string());
    app.push_history("second".to_string());

    assert_eq!(send_key(&mut app, KeyCode::Up), AppAction::None);
    assert_eq!(app.prompt.input, "second");

    assert_eq!(send_key(&mut app, KeyCode::Up), AppAction::None);
    assert_eq!(app.prompt.input, "first");

    assert_eq!(send_key(&mut app, KeyCode::Down), AppAction::None);
    assert_eq!(app.prompt.input, "second");
}

#[test]
fn ctrl_r_opens_history_search_and_escape_closes() {
    let mut app = App::new();
    add_chat_context(&mut app);
    app.push_history("first prompt".to_string());

    assert_eq!(
        send_key_with_modifiers(&mut app, KeyCode::Char('r'), KeyModifiers::CONTROL),
        AppAction::None
    );
    assert!(app.history_search_active());

    let mut terminal = Terminal::new(TestBackend::new(100, 24)).expect("terminal");
    terminal.draw(|frame| app.render(frame)).expect("draw");
    let content = buffer_to_lines(terminal.backend().buffer(), 100, 24);
    assert!(content.join("\n").contains("History search"));
    assert!(content.join("\n").contains("first prompt"));
    assert_title_above_prompt_area(
        &content,
        "History Search",
        app.render_layout.prompt_area.expect("prompt area"),
    );

    assert_eq!(send_key(&mut app, KeyCode::Esc), AppAction::None);
    assert!(!app.history_search_active());
    assert!(app.prompt.input.is_empty());
}

#[test]
fn resume_history_seed_populates_ctrl_r_before_live_prompts() {
    let mut app = App::new();
    app.seed_persistent_history(vec![
        HistorySearchEntry::new("new saved prompt", 20),
        HistorySearchEntry::new("old saved prompt", 10),
    ]);

    assert_eq!(app.history_len(), 2);
    assert_eq!(
        send_key_with_modifiers(&mut app, KeyCode::Char('r'), KeyModifiers::CONTROL),
        AppAction::None
    );
    assert_eq!(send_key(&mut app, KeyCode::Enter), AppAction::None);
    assert_eq!(app.prompt.input, "new saved prompt");
}

#[test]
fn history_search_enter_fills_selected_prompt() {
    let mut app = App::new();
    app.push_history("first prompt".to_string());
    app.push_history("second prompt".to_string());

    assert_eq!(
        send_key_with_modifiers(&mut app, KeyCode::Char('r'), KeyModifiers::CONTROL),
        AppAction::None
    );
    assert_eq!(send_key(&mut app, KeyCode::Enter), AppAction::None);

    assert!(!app.history_search_active());
    assert_eq!(app.prompt.input, "second prompt");
    assert_eq!(app.prompt.cursor_position, app.prompt.input.len());
}

#[test]
fn history_search_filters_before_selecting() {
    let mut app = App::new();
    app.push_history("git status --short".to_string());
    app.push_history("cargo test -p allthecodes history_search".to_string());

    assert_eq!(
        send_key_with_modifiers(&mut app, KeyCode::Char('r'), KeyModifiers::CONTROL),
        AppAction::None
    );
    assert_eq!(send_key(&mut app, KeyCode::Char('g')), AppAction::None);
    assert_eq!(send_key(&mut app, KeyCode::Char('i')), AppAction::None);
    assert_eq!(send_key(&mut app, KeyCode::Char('t')), AppAction::None);
    assert_eq!(send_key(&mut app, KeyCode::Enter), AppAction::None);

    assert_eq!(app.prompt.input, "git status --short");
}

#[test]
fn command_surface_handles_selection_before_prompt_input() {
    let mut app = App::new();
    app.open_command_surface(CommandSurface::lsp_recommendation(
        lsp_recommendation_payload(None),
    ));

    assert_eq!(
        send_key(&mut app, KeyCode::Enter),
        AppAction::LspRecommendationResponse {
            request_id: "req-1".to_string(),
            plugin_name: "rust-analyzer".to_string(),
            decision: "yes".to_string(),
        }
    );
    assert_eq!(app.prompt.input, "/plugin install rust-analyzer ");
    assert!(!app.command_surface_active());
}

#[test]
fn command_surface_renders_as_overlay() {
    let mut app = App::new();
    add_chat_context(&mut app);
    app.open_command_surface(CommandSurface::lsp_recommendation(
        lsp_recommendation_payload(Some("Rust language server".to_string())),
    ));
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).expect("terminal");

    terminal.draw(|frame| app.render(frame)).expect("draw");

    let content = buffer_to_lines(terminal.backend().buffer(), 100, 24);
    assert!(content.join("\n").contains("LSP plugin recommendation"));
    assert!(content.join("\n").contains("rust-analyzer"));
    assert_title_above_prompt_area(
        &content,
        "LSP plugin recommendation",
        app.render_layout.prompt_area.expect("prompt area"),
    );
}

#[test]
fn model_surface_keeps_its_bottom_border_above_prompt_on_short_terminal() {
    let mut app = App::new();
    app.open_command_surface(CommandSurface::Model(
        crate::ui::command_surface::ModelSurface::new(&AppState::default()),
    ));
    let mut terminal = Terminal::new(TestBackend::new(180, 20)).expect("terminal");

    terminal.draw(|frame| app.render(frame)).expect("draw");

    let content = buffer_to_lines(terminal.backend().buffer(), 180, 20);
    let joined = content.join("\n");
    let title_line = content
        .iter()
        .find(|line| line.contains("┌ Model"))
        .expect("model panel title");
    assert!(
        title_line.starts_with("┌ Model"),
        "model panel should be anchored to the left edge: {title_line:?}"
    );
    assert!(joined.contains("Select the active model"));
    assert!(joined.contains("Type filter | Up/Down navigate | Enter select | Esc close"));
    assert!(
        joined.lines().any(|line| {
            let trimmed = line.trim();
            trimmed.starts_with('└') && trimmed.ends_with('┘')
        }),
        "rendered screen:\n{joined}"
    );
    assert_title_above_prompt_area(
        &content,
        "┌ Model",
        app.render_layout.prompt_area.expect("prompt area"),
    );
}

#[test]
fn web_fetch_permission_dialog_uses_dedicated_renderer() {
    let mut app = App::new();
    add_chat_context(&mut app);
    app.show_permission_dialog(
        "WebFetch",
        r#"{"url":"https://example.com/docs"}"#,
        "WebFetch: Allow tool?",
    );
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).expect("terminal");
    terminal.draw(|frame| app.render(frame)).expect("draw");

    let content = buffer_to_lines(terminal.backend().buffer(), 100, 24).join("\n");
    assert!(content.contains("Permission Required"));
    assert!(content.contains("method: GET"));
    assert!(content.contains("example.com/docs"));
}

#[test]
fn permission_dialog_renders_above_prompt_input() {
    let mut app = App::new();
    add_chat_context(&mut app);
    app.show_permission_dialog("Bash", r#"{"command":"cargo test"}"#, "Allow command?");
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).expect("terminal");

    terminal.draw(|frame| app.render(frame)).expect("draw");

    let content = buffer_to_lines(terminal.backend().buffer(), 100, 24);
    assert_title_above_prompt_area(
        &content,
        "Permission Required",
        app.render_layout.prompt_area.expect("prompt area"),
    );
}

#[test]
fn question_dialog_renders_above_prompt_input() {
    let mut app = App::new();
    add_chat_context(&mut app);
    app.show_question_dialog(
        "q-1",
        AskUserRequestPayload {
            question: "Continue?".to_string(),
            choices: vec!["Yes".to_string(), "No".to_string()],
            allow_free_text: false,
        },
    );
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).expect("terminal");

    terminal.draw(|frame| app.render(frame)).expect("draw");

    let content = buffer_to_lines(terminal.backend().buffer(), 100, 24);
    assert_title_above_prompt_area(
        &content,
        "Need Input",
        app.render_layout.prompt_area.expect("prompt area"),
    );
}

#[test]
fn bypass_permissions_dialog_renders_above_prompt_input() {
    let mut app = App::new();
    add_chat_context(&mut app);
    app.show_bypass_permissions_mode_dialog(false);
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).expect("terminal");

    terminal.draw(|frame| app.render(frame)).expect("draw");

    let content = buffer_to_lines(terminal.backend().buffer(), 100, 24);
    assert_title_above_prompt_area(
        &content,
        "Bypass Permissions mode",
        app.render_layout.prompt_area.expect("prompt area"),
    );
}

#[test]
fn messages_action_selection_toggles_detail_and_copies_selected_text() {
    let mut app = App::new();
    app.add_message(Message::User(UserMessage {
        uuid: uuid::Uuid::new_v4(),
        timestamp: 0,
        role: "user".to_string(),
        content: MessageContent::Text("copy this message".to_string()),
        is_meta: false,
        tool_use_result: None,
        source_tool_assistant_uuid: None,
    }));

    assert_eq!(
        send_key_with_modifiers(&mut app, KeyCode::Up, KeyModifiers::SHIFT),
        AppAction::None
    );
    assert_eq!(app.selected_message(), Some(0));
    assert_eq!(send_key(&mut app, KeyCode::Enter), AppAction::None);
    assert_eq!(
        send_key(&mut app, KeyCode::Char('c')),
        AppAction::CopyMessage("copy this message".to_string())
    );
}

#[test]
fn messages_action_copies_primary_path_reference() {
    let mut app = App::new();
    app.add_message(Message::User(UserMessage {
        uuid: uuid::Uuid::new_v4(),
        timestamp: 0,
        role: "user".to_string(),
        content: MessageContent::Blocks(vec![ContentBlock::ToolUse {
            id: "toolu_read".to_string(),
            name: "Read".to_string(),
            input: serde_json::json!({ "file_path": "src/lib.rs" }),
        }]),
        is_meta: false,
        tool_use_result: None,
        source_tool_assistant_uuid: None,
    }));

    assert_eq!(
        send_key_with_modifiers(&mut app, KeyCode::Up, KeyModifiers::SHIFT),
        AppAction::None
    );
    assert_eq!(
        send_key(&mut app, KeyCode::Char('p')),
        AppAction::CopyMessage("path=src/lib.rs".to_string())
    );
}

#[test]
fn messages_action_opens_code_path_from_assistant_text() {
    let mut app = App::new();
    app.add_message(Message::Assistant(AssistantMessage {
        uuid: uuid::Uuid::new_v4(),
        timestamp: 0,
        role: "assistant".to_string(),
        content: vec![ContentBlock::Text {
            text: "Updated `crates/allthecodes/src/ui/app.rs:42`.".to_string(),
        }],
        usage: None,
        stop_reason: None,
        is_api_error_message: false,
        api_error: None,
        cost_usd: 0.0,
    }));

    assert_eq!(
        send_key_with_modifiers(&mut app, KeyCode::Up, KeyModifiers::SHIFT),
        AppAction::None
    );
    assert_eq!(
        send_key(&mut app, KeyCode::Char('o')),
        AppAction::OpenPath("path=crates/allthecodes/src/ui/app.rs:42".to_string())
    );
}

#[test]
fn f12_emits_debug_snapshot_action() {
    let mut app = App::new();

    assert_eq!(send_key(&mut app, KeyCode::F(12)), AppAction::DebugSnapshot);
}

fn send_key(app: &mut App, code: KeyCode) -> AppAction {
    app.handle_key_event(KeyEvent::new(code, KeyModifiers::NONE))
}

fn lsp_recommendation_payload(
    plugin_description: Option<String>,
) -> allthecodes_ipc_protocol::subsystem_types::LspRecommendationPayload {
    allthecodes_ipc_protocol::subsystem_types::LspRecommendationPayload {
        request_id: "req-1".to_string(),
        plugin_name: "rust-analyzer".to_string(),
        plugin_description,
        file_extension: ".rs".to_string(),
        language_id: Some("rust".to_string()),
    }
}

fn spawned_agent_event(agent_id: &str, description: &str, agent_type: Option<&str>) -> AgentEvent {
    AgentEvent::Spawned {
        agent_id: agent_id.to_string(),
        parent_agent_id: None,
        description: description.to_string(),
        agent_type: agent_type.map(ToString::to_string),
        model: None,
        is_background: true,
        depth: 1,
        chain_id: "chain-main".to_string(),
        fork_metadata: None,
    }
}

fn send_key_with_modifiers(app: &mut App, code: KeyCode, modifiers: KeyModifiers) -> AppAction {
    app.handle_key_event(KeyEvent::new(code, modifiers))
}

fn send_mouse(app: &mut App, kind: MouseEventKind) -> AppAction {
    send_mouse_at(app, kind, 0, 0)
}

fn send_mouse_at(app: &mut App, kind: MouseEventKind, column: u16, row: u16) -> AppAction {
    app.handle_mouse_event(MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    })
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

fn row_containing(lines: &[String], needle: &str) -> usize {
    lines
        .iter()
        .position(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("{needle:?} row\n{}", lines.join("\n")))
}

fn assert_context_item(
    items: &[crate::ui::context_layer::ContextLayerItem],
    label: &str,
    expected_value_fragment: &str,
) {
    assert!(
        items
            .iter()
            .any(|item| { item.label == label && item.value.contains(expected_value_fragment) }),
        "missing {label}={expected_value_fragment:?} in {items:?}"
    );
}

fn add_user_message(app: &mut App, text: &str) {
    app.add_message(Message::User(UserMessage {
        uuid: uuid::Uuid::new_v4(),
        timestamp: 0,
        role: "user".to_string(),
        content: MessageContent::Text(text.to_string()),
        is_meta: false,
        tool_use_result: None,
        source_tool_assistant_uuid: None,
    }));
}

fn add_chat_context(app: &mut App) {
    for idx in 0..14 {
        add_user_message(app, &format!("previous context {idx}"));
    }
}

fn assert_title_above_prompt_area(lines: &[String], title: &str, prompt_area: Rect) {
    let prompt_row = prompt_area.y as usize;
    let title_row = lines
        .iter()
        .position(|line| line.contains(title))
        .unwrap_or_else(|| panic!("{title} title row"));
    assert!(
        title_row < prompt_row,
        "{title} should render above the prompt input (title_row={title_row}, prompt_row={prompt_row})\n{}",
        lines.join("\n")
    );
}
