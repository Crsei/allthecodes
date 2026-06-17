// Entry and feature-domain facades stay at the top level. Most leaf UI files
// live in responsibility folders below, while their public module names remain
// `crate::ui::<name>` for compatibility.
//
// Rust TUI source stays owned by the root binary crate.
pub mod agents;
pub mod app;
pub mod diff;
pub mod hooks;
pub mod lsp_recommendation;
pub mod mcp;
pub mod memory;
pub mod messages;
pub mod notifications;
pub mod permissions;
pub mod skills;
pub mod tasks;
pub mod teams;
pub mod terminal_progress;
pub mod tui;

// Components: interactive widgets and modal surfaces owned or orchestrated by
// App.
#[path = "components/approval_overlay.rs"]
pub mod approval_overlay;
#[path = "components/better_view_panel.rs"]
pub mod better_view_panel;
#[path = "components/bottom_pane.rs"]
pub mod bottom_pane;
#[cfg(test)]
#[path = "components/chat_composer.rs"]
pub mod chat_composer;
#[cfg(test)]
#[path = "components/chatwidget.rs"]
pub mod chatwidget;
pub mod command_palette;
pub mod command_surface;
#[cfg(test)]
#[path = "components/cwd_prompt.rs"]
pub mod cwd_prompt;
#[path = "components/divider.rs"]
pub mod divider;
#[cfg(test)]
#[path = "components/feature_panels.rs"]
pub mod feature_panels;
#[path = "components/fuzzy_match.rs"]
pub mod fuzzy_match;
#[cfg(test)]
#[path = "components/fuzzy_picker.rs"]
pub mod fuzzy_picker;
#[path = "components/history_search_dialog.rs"]
pub mod history_search_dialog;
#[path = "components/keyboard_shortcut.rs"]
pub mod keyboard_shortcut;
#[cfg(test)]
#[path = "components/list_item.rs"]
pub mod list_item;
#[cfg(test)]
#[path = "components/loading_state.rs"]
pub mod loading_state;
#[cfg(test)]
#[path = "components/pager_overlay.rs"]
pub mod pager_overlay;
#[path = "components/pane.rs"]
pub mod pane;
pub mod panel_layout;
pub mod prompt_input;
#[cfg(test)]
#[path = "components/ratchet.rs"]
pub mod ratchet;
#[cfg(test)]
#[path = "components/resume_picker.rs"]
pub mod resume_picker;
#[path = "components/search_box.rs"]
pub mod search_box;
pub mod selection_surface;
#[cfg(test)]
#[path = "components/status_icon.rs"]
pub mod status_icon;
#[cfg(test)]
#[path = "components/status_widget.rs"]
pub mod status_widget;
#[path = "components/tabs.rs"]
pub mod tabs;
#[cfg(test)]
#[path = "components/themed_box.rs"]
pub mod themed_box;
#[cfg(test)]
#[path = "components/themed_text.rs"]
pub mod themed_text;
#[cfg(test)]
#[path = "components/tooltops.rs"]
pub mod tooltops;
#[path = "components/welcome.rs"]
pub mod welcome;

// Input: keyboard editing, command parsing, mentions, clipboard, and picker
// navigation helpers.
#[path = "input/clipboard_paste.rs"]
pub mod clipboard_paste;
#[path = "input/clipboard_text.rs"]
pub mod clipboard_text;
#[path = "input/completions.rs"]
pub mod completions;
#[cfg(test)]
#[path = "input/file_search.rs"]
pub mod file_search;
#[path = "input/form_navigation.rs"]
pub mod form_navigation;
#[cfg(test)]
#[path = "input/insert_history.rs"]
pub mod insert_history;
#[path = "input/keybindings.rs"]
pub mod keybindings;
#[cfg(test)]
#[path = "input/mention_codec.rs"]
pub mod mention_codec;
#[path = "input/path_completion.rs"]
pub mod path_completion;
#[path = "input/shell_history_completion.rs"]
pub mod shell_history_completion;
#[path = "input/slack_channel_completion.rs"]
pub mod slack_channel_completion;
#[cfg(test)]
#[path = "input/slash_command.rs"]
pub mod slash_command;
#[path = "input/vim.rs"]
pub mod vim;

// Rendering: shared text shaping, visual effects, styles, scroll calculation,
// and message/tool presentation helpers.
#[cfg(test)]
#[path = "rendering/get_git_diff.rs"]
pub mod get_git_diff;
#[cfg(test)]
#[path = "rendering/history_cell.rs"]
pub mod history_cell;
#[path = "rendering/markdown.rs"]
pub mod markdown;
#[cfg(test)]
#[path = "rendering/markdown_render.rs"]
pub mod markdown_render;
#[cfg(test)]
#[path = "rendering/markdown_stream.rs"]
pub mod markdown_stream;
#[cfg(test)]
#[path = "rendering/progress_bar.rs"]
pub mod progress_bar;
#[path = "rendering/theme.rs"]
pub mod rendering_theme;
#[cfg(test)]
#[path = "rendering/shimmer.rs"]
pub mod shimmer;
#[path = "rendering/spinner.rs"]
pub mod spinner;
#[path = "rendering/syntax_highlight.rs"]
pub mod syntax_highlight;
#[path = "rendering/tool_activity.rs"]
pub mod tool_activity;
#[path = "rendering/virtual_scroll.rs"]
pub mod virtual_scroll;

// Runtime: non-visual state machines, event routing, streaming control, logs,
// transcript state, and visual regression fixtures.
#[cfg(test)]
#[path = "runtime/capability_contract.rs"]
pub mod capability_contract;
#[cfg(test)]
#[path = "runtime/event_router.rs"]
pub mod event_router;
#[cfg(test)]
#[path = "runtime/frame_requester.rs"]
pub mod frame_requester;
#[path = "runtime/persistent_history.rs"]
pub mod persistent_history;
#[cfg(test)]
#[path = "runtime/session_log.rs"]
pub mod session_log;
#[path = "runtime/snapshot_export.rs"]
pub mod snapshot_export;
#[cfg(test)]
#[path = "runtime/streaming_controller.rs"]
pub mod streaming_controller;
#[path = "runtime/transcript.rs"]
pub mod transcript;
#[cfg(test)]
#[path = "runtime/visual_regression.rs"]
pub mod visual_regression;

// Platform: terminal/browser/audio/environment adapters.
#[cfg(test)]
#[path = "platform/audio_device.rs"]
pub mod audio_device;
#[cfg(test)]
#[path = "platform/custom_terminal.rs"]
pub mod custom_terminal;
#[cfg(test)]
#[path = "platform/debug_config.rs"]
pub mod debug_config;
#[path = "platform/terminal_env.rs"]
pub mod terminal_env;
#[cfg(test)]
#[path = "platform/terminal_integration.rs"]
pub mod terminal_integration;

// Helpers and cross-crate status-line integration.
#[path = "helpers/skills_helpers.rs"]
pub mod skills_helpers;
// `status_line` moved to `cc-engine` in Phase 6 (issue #75). Downstream
// consumers should import from `allthecodes_engine::status_line` directly.
pub use allthecodes_engine::status_line;
#[path = "status/status_line_resolver.rs"]
pub mod status_line_resolver;

// Design-system theme provider.
pub mod theme;

// Overlay / modal infrastructure.
pub mod overlays;

const _: fn() = production_symbol_anchors;

fn production_symbol_anchors() {
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
    use ratatui::text::Line;

    let colors = theme::get_theme(&theme::ThemeName::Dark);
    let render_theme = theme::Theme::default();

    let _ = approval_overlay::ApprovalKind::Bash {
        command: "echo ok".to_string(),
    }
    .subject();
    let _ = better_view_panel::key_value_row("key", "value");
    let _ = divider::Divider::new().char('-').padding(1).title("Title");
    let _ =
        fuzzy_match::weighted_fuzzy_match("name", &["alias"], "description", "na", 1.0, 0.75, 0.5);
    let history = history_search_dialog::HistorySearchDialog::loading("", 0);
    let _ = history.query();
    let hint = keyboard_shortcut::ShortcutHint::new("Enter", "select")
        .with_bold_key()
        .with_color("accent");
    let _ = keyboard_shortcut::render_byline(&[hint], colors);
    let byline = keyboard_shortcut::Byline::new()
        .hint("Esc", "close")
        .push(hint);
    let _ = byline.len();
    let _ = byline.is_empty();
    let _ = byline.render(colors);
    let _ = search_box::SearchBox::new("agent")
        .focused(true)
        .terminal_focused(true)
        .prefix("/")
        .cursor_offset(1)
        .width(20);
    let tab = tabs::Tab::new("overview", "Overview");
    let _ = tab.id.as_str();
    let mut tabs = tabs::Tabs::new(vec![tab]);
    tabs.select_next();
    tabs.select_prev();
    let _ = tabs.selected();
    let _ = tabs.selected_tab();
    let _ = tabs
        .color("accent")
        .use_full_width()
        .content_height(3)
        .header_focus(true)
        .render_header(colors, 40);

    let completer = completions::CombinedCompleter::new();
    let _ = completer.is_empty();
    let _ = completer.provider_count();
    let _ = completions::find_command_token_range("run /help", 9);
    let form = form_navigation::TabbedFormState::new(
        "Form",
        vec![form_navigation::FormTab::new(
            "main",
            "Main",
            vec![form_navigation::FormOption::new("yes", "Yes")],
        )],
    );
    let _ = form.selected_option();
    let vim = vim::VimState::from_editor_mode(Some("vim"));
    let _ = vim.editor_mode_setting().as_str();
    let _ = vim::VimMode::Insert.short_indicator();

    let _ = messages::assistant_tool_use_message::ToolUseState::Queued;
    let _ = messages::assistant_tool_use_message::ToolUseState::WaitingForPermission;
    let _ = messages::assistant_tool_use_message::ToolUseState::ClassifierChecking;
    let _ = messages::user_tool_result_message::utils::line_to_text(&Line::from("tool output"));

    let mut activity = tool_activity::ToolActivity::new("Read", tool_activity::ToolState::Running);
    activity.progress = Some((1, 2));
    let _ = activity.compact_line();
    let _ = tool_activity::render_grouped_styled_activity(&[activity.clone()], &render_theme);
    let _ = tool_activity::render_grouped_activity(&[activity]);
    let scroll = virtual_scroll::VirtualScroll::new();
    let _ = scroll.total_lines();
    let _ = scroll.visible_range(0, 10);
    let _ = scroll.visual_height_of(0);
    let _ = scroll.cached_width();
    let _ = scroll.offset_of(0);

    let mut exit_guard = overlays::dialog::ExitGuard::new();
    let dialog = overlays::dialog::Dialog::new()
        .subtitle("Subtitle")
        .hide_border()
        .cancel_active(true)
        .input_guide(vec![hint]);
    let _ = dialog.handle_key(
        KeyEvent {
            code: KeyCode::Esc,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        },
        &mut exit_guard,
    );
}
