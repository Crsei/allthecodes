//! Snapshot targets for the foundational Rust TUI surfaces.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::approval_overlay::{ApprovalChoice, ApprovalKind, ApprovalOverlay};
use super::bottom_pane::{BottomPane, BottomPaneView};
use super::capability_contract::{render_contract_table, surface_for_event, BackendEventKind};
use super::chat_composer::ChatComposerState;
use super::command_palette::CommandPalette;
use super::diff::render_diff;
use super::event_router::{render_route_trace, EngineEvent, RouteContext, TerminalEvent, UiEvent};
use super::feature_panels::{default_panels, render_panel_index, PanelState};
use super::frame_requester::{FrameReason, FrameRequester};
use super::history_cell::{render_history, HistoryCell, HistoryRenderMode};
use super::markdown::markdown_to_lines;
use super::messages::render_messages;
use super::permissions::PermissionDialog;
use super::selection_surface::{SelectionItem, SelectionSurface};
use super::status_widget::{StatusIndicator, StatusSnapshot};
use super::streaming_controller::{StreamingController, StreamingDelta};
use super::terminal_integration::{render_policy, TerminalEnvironment};
use super::theme::Theme;
use super::tool_activity::{render_grouped_activity, ToolActivity, ToolState};
use super::virtual_scroll::VirtualScroll;
use allthecodes_types::message::{
    AssistantMessage, ContentBlock, Message, MessageContent, UserMessage,
};
use uuid::Uuid;

pub fn render_foundation_snapshot() -> String {
    let mut sections = Vec::new();

    let mut approval = ApprovalOverlay::new(ApprovalKind::Bash {
        command: "cargo test -p allthecodes".into(),
    })
    .with_choices(vec![
        ApprovalChoice::AllowOnce,
        ApprovalChoice::AllowAlways,
        ApprovalChoice::Deny,
    ]);
    approval.select(1);
    sections.push(section("approval", approval.render_lines(72).join("\n")));

    let mut composer = ChatComposerState::new();
    composer.set_input("/model SOTA");
    composer.set_busy(true);
    let effect = composer.submit_or_queue();
    sections.push(section(
        "composer",
        format!(
            "effect={effect:?}\n{}",
            composer.render_lines(72).join("\n")
        ),
    ));

    let mut pane = BottomPane::new(composer.clone());
    pane.push(BottomPaneView::Approval(approval));
    sections.push(section("bottom-pane", pane.render_lines(72).join("\n")));

    sections.push(section(
        "capability-contract",
        format!(
            "permission surface={:?}\n{}",
            surface_for_event(BackendEventKind::PermissionRequest),
            render_contract_table()
        ),
    ));

    sections.push(section(
        "event-router",
        render_route_trace(
            &[
                UiEvent::Terminal(TerminalEvent::Key("Enter".into())),
                UiEvent::Engine(EngineEvent::PermissionRequested("bash".into())),
                UiEvent::OverlayResponse("allow".into()),
            ],
            RouteContext {
                busy: true,
                overlay_active: true,
            },
        ),
    ));

    let mut panels = default_panels();
    panels[0].badge = 2;
    panels[3].state = PanelState::NeedsConfig;
    sections.push(section("feature-panels", render_panel_index(&panels)));

    let mut requester = FrameRequester::new();
    requester.request(FrameReason::Input);
    requester.request(FrameReason::Stream);
    let pending_before_take = requester.snapshot();
    let took = requester.take_pending();
    sections.push(section(
        "frame-requester",
        format!(
            "before={pending_before_take:?}\ntake={took}\nafter={:?}",
            requester.snapshot()
        ),
    ));

    let cells = vec![
        HistoryCell::User("show status".into()),
        HistoryCell::Assistant("All systems visible.".into()),
        HistoryCell::Tool {
            name: "bash".into(),
            summary: "cargo test passed".into(),
        },
        HistoryCell::Diff {
            path: "src/ui/mod.rs".into(),
            added: 14,
            removed: 0,
        },
    ];
    sections.push(section(
        "history-cells",
        render_history(&cells, HistoryRenderMode::Transcript),
    ));

    let mut picker = SelectionSurface::new(
        "Slash commands",
        vec![
            SelectionItem {
                description: "show status".into(),
                ..SelectionItem::new("status", "/status")
            },
            SelectionItem {
                description: "manage plugins".into(),
                ..SelectionItem::new("plugin", "/plugin")
            },
            SelectionItem {
                description: "start agent".into(),
                ..SelectionItem::new("agent", "/agent")
            },
        ],
    );
    picker.set_filter("p");
    picker.move_next();
    sections.push(section("selection", picker.render_lines(4).join("\n")));

    let status = StatusSnapshot {
        model: "claude-sonnet".into(),
        cwd: "F:/repo".into(),
        permission_mode: "ask".into(),
        sandbox: "workspace-write".into(),
        cost_usd: 0.1234,
        running_tools: 1,
        active_agents: 2,
        subsystems_ok: false,
        indicators: vec![
            StatusIndicator::new("effort", "medium"),
            StatusIndicator::new("ide", "3 lines selected"),
            StatusIndicator::warning("memory", "1.4 GiB"),
            StatusIndicator::new("pr", "#42 approved"),
        ],
    };
    sections.push(section(
        "status-widget",
        format!("{}\n{}", status.render_line(), status.render_details()),
    ));

    let mut stream = StreamingController::new();
    stream.apply(StreamingDelta::Thinking("checking".into()));
    stream.apply(StreamingDelta::ToolCall("bash".into()));
    stream.apply(StreamingDelta::Assistant("partial answer".into()));
    stream.commit_visible();
    sections.push(section("streaming", stream.render_lines().join("\n")));

    sections.push(section(
        "terminal-policy",
        render_policy(&TerminalEnvironment {
            term: "xterm-256color".into(),
            tmux: true,
            zellij: false,
            ssh: true,
            windows_terminal: false,
        }),
    ));

    let activities = vec![
        {
            let mut activity = ToolActivity::from_tool_use(
                "read_file",
                r#"{"path":"src/ui/app.rs"}"#,
                ToolState::Succeeded,
            );
            activity.summary = "loaded file".into();
            activity.elapsed_ms = 12;
            activity.output_lines = 80;
            activity
        },
        {
            let mut activity = ToolActivity::from_tool_use(
                "bash",
                r#"{"command":"cargo test"}"#,
                ToolState::Running,
            );
            activity.summary = "running tests".into();
            activity.elapsed_ms = 240;
            activity.progress = Some((1, 4));
            activity.output_lines = 14;
            activity
        },
    ];
    sections.push(section(
        "tool-activity",
        render_grouped_activity(&activities),
    ));

    sections.join("\n\n")
}

fn section(name: &str, body: impl AsRef<str>) -> String {
    format!("## {name}\n{}", body.as_ref())
}

fn render_permission_dialog(dialog: PermissionDialog, width: u16, height: u16) -> String {
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    dialog.render(area, None, &mut buf, &Theme::default());
    normalize_snapshot_text(buffer_text(&buf, area))
}

fn render_command_palette_snapshot(palette: &CommandPalette, width: u16, height: u16) -> String {
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    palette.render(area, &mut buf, &Theme::default());
    normalize_snapshot_text(buffer_text(&buf, area))
}

fn render_markdown_snapshot() -> String {
    let lines = markdown_to_lines(
        "# Heading\n\nVisit [docs](https://example.com) for the current state.\n\n- first\n- second",
        &Theme::default(),
    );
    lines
        .into_iter()
        .map(|line| {
            line.spans
                .into_iter()
                .map(|span| span.content)
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_streaming_message_snapshot() -> String {
    let messages = vec![
        Message::User(UserMessage {
            uuid: Uuid::new_v4(),
            timestamp: 0,
            role: "user".to_string(),
            content: MessageContent::Text("show me the latest status".to_string()),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        }),
        Message::Assistant(AssistantMessage {
            uuid: Uuid::new_v4(),
            timestamp: 0,
            role: "assistant".to_string(),
            content: vec![ContentBlock::Text {
                text: "# Progress\n\nVisit [docs](https://example.com) for details.".to_string(),
            }],
            usage: None,
            stop_reason: None,
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0314,
        }),
    ];

    let area = Rect::new(0, 0, 96, 8);
    let mut buf = Buffer::empty(area);
    let mut vscroll = VirtualScroll::new();
    let theme = Theme::default();
    let render_context = super::messages::build_message_render_context(&messages, None, false);
    vscroll.ensure_up_to_date(&messages, area.width, &theme, &render_context);
    render_messages(
        &messages,
        area,
        &mut buf,
        &theme,
        true,
        0,
        &vscroll,
        &render_context,
    );
    normalize_snapshot_text(buffer_text(&buf, area))
}

fn render_diff_snapshot() -> String {
    let area = Rect::new(0, 0, 88, 8);
    let mut buf = Buffer::empty(area);
    let old = "alpha\nbeta\nA very long line: cafe deja vu";
    let new = "alpha\nbeta updated\nA very long line: cafe deja vu and more text";
    render_diff(old, new, area, &mut buf, &Theme::default());
    normalize_snapshot_text(buffer_text(&buf, area))
}

fn snapshot_cwd() -> std::path::PathBuf {
    std::path::PathBuf::from("C:\\allthecodes-snapshot")
}

fn buffer_text(buf: &Buffer, area: Rect) -> String {
    let mut lines = Vec::new();
    for y in area.y..area.y + area.height {
        let mut line = String::new();
        for x in area.x..area.x + area.width {
            line.push_str(buf[(x, y)].symbol());
        }
        lines.push(line.trim_end().to_string());
    }
    lines.join("\n")
}

fn normalize_snapshot_text(mut text: String) -> String {
    for raw_home in [std::env::var_os("USERPROFILE"), std::env::var_os("HOME")]
        .into_iter()
        .flatten()
    {
        let home = std::path::PathBuf::from(raw_home)
            .to_string_lossy()
            .replace('\\', "/");
        text = text.replace(&home, "<HOME>");
        text = text.replace(&home.replace(' ', "%20"), "<HOME>");
    }

    normalize_allthecodes_home_width(text)
}

fn normalize_allthecodes_home_width(text: String) -> String {
    if !text.contains("$ALLTHECODES_HOME") {
        return text;
    }
    text.lines()
        .map(|line| {
            let width = line.chars().count();
            let mut normalized = line.replace("$ALLTHECODES_HOME", "~/.allthecodes");
            let new_width = normalized.chars().count();
            if new_width < width && normalized.ends_with('│') {
                let pad = " ".repeat(width - new_width);
                let insert_at = normalized.len() - "│".len();
                normalized.insert_str(insert_at, &pad);
            }
            normalized
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_interactive_ui_surfaces() -> String {
    let mut sections = Vec::new();

    sections.push(section(
        "permission/bash",
        render_permission_dialog(
            PermissionDialog::new(
                "Bash",
                "cargo test -p allthecodes",
                "Run a build check before merging.",
            ),
            72,
            13,
        ),
    ));
    sections.push(section(
        "permission/file-edit",
        render_permission_dialog(
            PermissionDialog::new(
                "Write",
                "src/ui/command_palette.rs",
                "Edit the command palette picker flow.",
            ),
            72,
            13,
        ),
    ));

    let cwd = snapshot_cwd();
    let mut palette = CommandPalette::new();
    palette.sync_from_input("/mcp", &cwd);
    let _ = palette.open_edit_target_picker();
    sections.push(section(
        "command-palette-picker",
        render_command_palette_snapshot(&palette, 100, 14),
    ));

    sections.push(section("markdown", render_markdown_snapshot()));
    sections.push(section(
        "streaming-message",
        render_streaming_message_snapshot(),
    ));
    sections.push(section("diff", render_diff_snapshot()));

    sections.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_foundational_ui_surfaces() {
        insta::assert_snapshot!("foundational_ui_surfaces", render_foundation_snapshot());
    }

    #[test]
    fn snapshot_interactive_ui_surfaces() {
        insta::assert_snapshot!("interactive_ui_surfaces", render_interactive_ui_surfaces());
    }
}
