use std::path::{Path, PathBuf};

use crossterm::event::KeyCode;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};
use ratatui::Terminal;

use crate::ui::theme::Theme;
use allthecodes_commands as commands;

use super::edit_targets::{display_path, file_uri};
use super::metadata::command_meta;
use super::render::visible_window_start;
use super::{CommandAction, CommandPalette, ARG_HELP_BASE_HEIGHT, MAX_EDIT_ROWS, MAX_ROWS};
#[test]
fn slash_opens_filtered_palette() {
    let mut palette = CommandPalette::new();
    palette.sync_from_input("/mc", Path::new("/repo"));
    assert!(palette.active());
    assert_eq!(palette.filtered[0].name, "mcp");
}

#[test]
fn whitespace_closes_palette_for_argument_entry() {
    let mut palette = CommandPalette::new();
    palette.sync_from_input("/mcp ", Path::new("/repo"));
    assert!(!palette.active());
}

#[test]
fn selected_command_keeps_space_for_arguments() {
    let mut palette = CommandPalette::new();
    palette.sync_from_input("/mcp", Path::new("/repo"));
    assert_eq!(palette.selected_command_input().as_deref(), Some("/mcp "));
}

#[test]
fn command_suggestion_action_inserts_or_executes() {
    let mut palette = CommandPalette::new();
    palette.sync_from_input("/status", Path::new("/repo"));
    let item = palette.filtered[palette.selected].clone();
    match palette.apply_command_suggestion(&item, true).unwrap() {
        CommandAction::Execute(command) => assert_eq!(command, "/status"),
        CommandAction::Insert(command) => panic!("expected execute action, got {command}"),
    }

    palette.sync_from_input("/mcp", Path::new("/repo"));
    let item = palette.filtered[palette.selected].clone();
    match palette.apply_command_suggestion(&item, true).unwrap() {
        CommandAction::Insert(command) => assert_eq!(command, "/mcp "),
        CommandAction::Execute(command) => panic!("expected insert action, got {command}"),
    }
}

#[test]
fn exact_plan_suggestion_executes_bare_plan_command() {
    let mut palette = CommandPalette::new();
    palette.sync_from_input("/plan", Path::new("/repo"));
    let item = palette.filtered[palette.selected].clone();

    match palette.apply_command_suggestion(&item, true).unwrap() {
        CommandAction::Execute(command) => assert_eq!(command, "/plan"),
        CommandAction::Insert(command) => panic!("expected execute action, got {command}"),
    }
}

#[test]
fn exact_compact_suggestion_executes_bare_compact_command() {
    let mut palette = CommandPalette::new();
    palette.sync_from_input("/compact", Path::new("/repo"));
    let item = palette.filtered[palette.selected].clone();

    match palette.apply_command_suggestion(&item, true).unwrap() {
        CommandAction::Execute(command) => assert_eq!(command, "/compact"),
        CommandAction::Insert(command) => panic!("expected execute action, got {command}"),
    }
}

#[test]
fn advisor_is_hidden_from_slash_palette() {
    let mut palette = CommandPalette::new();
    palette.sync_from_input("/advisor", Path::new("/repo"));

    assert!(palette.active());
    assert!(!palette.filtered.iter().any(|item| item.name == "advisor"));
    assert!(CommandPalette::argument_hint("/advisor ", Path::new("/repo")).is_some());
}

#[test]
fn command_aliases_render_without_gap_before_details() {
    let mut palette = CommandPalette::new();
    palette.sync_from_input("/assistant", Path::new("/repo"));

    let area = Rect::new(0, 0, 100, palette.preferred_height());
    let mut buf = Buffer::empty(area);
    palette.render(area, &mut buf, &Theme::default());

    let rendered = buffer_text(&buf, area);
    assert!(rendered.contains("/assistant(kairos)"));
    assert!(rendered.contains("Command details"));
}

#[test]
fn command_palette_selected_row_uses_theme_selected() {
    let mut palette = CommandPalette::new();
    palette.sync_from_input("/", Path::new("/repo"));
    let theme = Theme::default();
    assert_eq!(theme.selected.fg, Some(Color::Rgb(0, 0, 0)));
    assert_eq!(theme.selected.bg, Some(Color::Rgb(130, 200, 255)));
    assert!(theme.selected.add_modifier.contains(Modifier::BOLD));

    let area = Rect::new(0, 0, 100, palette.preferred_height());
    let mut buf = Buffer::empty(area);
    palette.render(area, &mut buf, &theme);

    let selected_cell = (area.y..area.y + area.height)
        .flat_map(|y| (area.x..area.x + area.width).map(move |x| (x, y)))
        .map(|(x, y)| &buf[(x, y)])
        .find(|cell| cell.style().bg == theme.selected.bg)
        .expect("selected command cell with theme background");
    let selected_style = selected_cell.style();
    assert_eq!(selected_style.fg, theme.selected.fg);
    assert_eq!(selected_style.bg, theme.selected.bg);
    assert!(selected_style
        .add_modifier
        .contains(theme.selected.add_modifier));
}

#[test]
fn experimental_command_is_visible_in_slash_palette() {
    let mut palette = CommandPalette::new();
    palette.sync_from_input("/exp", Path::new("/repo"));
    assert!(palette
        .filtered
        .iter()
        .any(|item| item.name == "experimental"));
    assert_eq!(
        CommandPalette::argument_hint("/experimental ", Path::new("/repo")).as_deref(),
        Some("/experimental [status|list|on|off|reset]")
    );
}

#[test]
fn complex_commands_show_edit_targets() {
    let mut palette = CommandPalette::new();
    palette.sync_from_input("/plugin", Path::new("/repo"));
    let item = palette
        .filtered
        .iter()
        .find(|item| item.name == "plugin")
        .expect("plugin command");
    assert!(item
        .edit_targets
        .iter()
        .any(|target| target.display.ends_with("installed_plugins.json")));
}

#[test]
fn argument_hint_is_shown_after_command_selection() {
    let hint = CommandPalette::argument_hint("/mcp ", Path::new("/repo"));
    assert_eq!(
        hint.as_deref(),
        Some("/mcp <list|status|add|edit|remove|approve|reject|connect|disconnect|reconnect>")
    );
    assert_eq!(
        CommandPalette::argument_help_height("/mcp ", Path::new("/repo")),
        ARG_HELP_BASE_HEIGHT + MAX_EDIT_ROWS as u16 + 1
    );
}

#[test]
fn argument_help_uses_three_content_rows() {
    let area = Rect::new(
        0,
        0,
        100,
        CommandPalette::argument_help_height("/status ", Path::new("/repo")),
    );
    let mut buf = Buffer::empty(area);

    CommandPalette::render_argument_help(
        "/status ",
        Path::new("/repo"),
        area,
        &mut buf,
        &Theme::default(),
    );

    let rendered = buffer_text(&buf, area);
    assert!(rendered.contains("Usage: /status"));
    assert!(rendered.contains("Example: /status"));
    assert!(rendered.contains("About: Show session status"));
}

#[test]
fn argument_help_wraps_edit_targets_without_dropping_file_names() {
    let cwd = snapshot_cwd();
    let area = Rect::new(
        0,
        0,
        110,
        CommandPalette::argument_help_height("/mcp ", &cwd),
    );
    let mut buf = Buffer::empty(area);

    CommandPalette::render_argument_help("/mcp ", &cwd, area, &mut buf, &Theme::default());

    let rendered = buffer_text(&buf, area);
    assert!(rendered.contains("settings.local.json"));
}

#[test]
fn command_meta_fallback_does_not_show_generic_args_placeholder() {
    let meta = command_meta("future-command", Path::new("/repo"));
    assert_eq!(meta.usage, "/future-command");
    assert!(!meta.usage.contains("[args]"));
    assert_eq!(meta.examples, vec!["/future-command".to_string()]);
}

#[test]
fn command_palette_surfaces_session_help_diagnostics_and_keybindings() {
    let cwd = Path::new("/repo");

    let resume = command_meta("resume", cwd);
    assert!(resume
        .examples
        .iter()
        .any(|example| example.contains("recent")));

    let session_export = command_meta("session-export", cwd);
    assert!(session_export.usage.contains("list"));

    let doctor = command_meta("doctor", cwd);
    assert_eq!(doctor.usage, "/doctor [summary|raw]");

    let keybindings = command_meta("keybindings", cwd);
    assert!(keybindings.usage.contains("status"));

    let memory = command_meta("memory", cwd);
    assert!(memory.usage.contains("search"));
    assert!(memory.usage.contains("open"));
}

#[test]
fn visible_window_tracks_selection_beyond_first_page() {
    let total = MAX_ROWS + 12;
    assert_eq!(visible_window_start(total, 0, MAX_ROWS), 0);
    assert_eq!(visible_window_start(total, MAX_ROWS - 1, MAX_ROWS), 0);
    assert_eq!(visible_window_start(total, MAX_ROWS, MAX_ROWS), 1);
    assert_eq!(visible_window_start(total, total - 1, MAX_ROWS), 12);
}

#[test]
fn render_scrolls_command_list_to_selected_item() {
    let mut palette = CommandPalette::new();
    palette.sync_from_input("/", Path::new("/repo"));
    for _ in 0..MAX_ROWS {
        palette.handle_key(KeyCode::Down);
    }

    let selected_name = palette.filtered[palette.selected].name.clone();
    let first_name = palette.filtered[0].name.clone();
    let area = Rect::new(0, 0, 100, palette.preferred_height());
    let mut buf = Buffer::empty(area);
    palette.render(area, &mut buf, &Theme::default());

    let rendered = buffer_text(&buf, area);
    assert!(rendered.contains(&format!("/{selected_name}")));
    assert!(!rendered.contains(&format!("/{first_name}  ")));
}

#[test]
fn preferred_height_allows_twenty_command_rows() {
    let mut palette = CommandPalette::new();
    palette.sync_from_input("/", Path::new("/repo"));

    let area = Rect::new(0, 0, 100, palette.preferred_height());
    let mut buf = Buffer::empty(area);
    palette.render(area, &mut buf, &Theme::default());

    let rendered = buffer_text(&buf, area);
    let command_rows = rendered
        .lines()
        .filter(|line| line.starts_with("│/"))
        .count();
    assert_eq!(command_rows, palette.filtered.len().min(MAX_ROWS));
}

#[test]
fn edit_targets_render_as_file_links() {
    let uri = file_uri(Path::new("C:\\Users\\me\\settings.json"));
    assert_eq!(uri, "file:///C:/Users/me/settings.json");
}

#[test]
fn display_path_prefers_readable_project_relative_paths() {
    let cwd = Path::new("C:\\repo");
    let path = cwd.join(".allthecodes").join("settings.json");
    assert_eq!(display_path(&path, cwd), "./.allthecodes/settings.json");
}

#[test]
#[serial_test::serial]
fn snapshot_command_palette_root_and_filtered_views() {
    let _home = EnvVarGuard::set("ALLTHECODES_HOME", "/tmp/allthecodes-snapshot-home");
    let cwd = snapshot_cwd();

    insta::assert_snapshot!(
        "command_palette_root_100x12",
        render_palette_snapshot("/", &cwd, 100, 12, &[])
    );
    insta::assert_snapshot!(
        "command_palette_after_page_down_100x12",
        render_palette_snapshot("/", &cwd, 100, 12, &[KeyCode::PageDown])
    );
    insta::assert_snapshot!(
        "command_palette_mcp_filtered_100x12",
        render_palette_snapshot("/mcp", &cwd, 100, 12, &[])
    );
    insta::assert_snapshot!(
        "command_palette_plugin_filtered_100x12",
        render_palette_snapshot("/plugin", &cwd, 100, 12, &[])
    );
}

#[test]
#[serial_test::serial]
fn snapshot_all_command_argument_help_views() {
    let _home = EnvVarGuard::set("ALLTHECODES_HOME", "/tmp/allthecodes-snapshot-home");
    let cwd = snapshot_cwd();
    let mut rendered = String::new();

    for command in commands::get_all_commands() {
        let input = format!("/{} ", command.name);
        let area = Rect::new(
            0,
            0,
            110,
            CommandPalette::argument_help_height(&input, &cwd),
        );
        let mut buf = Buffer::empty(area);

        CommandPalette::render_argument_help(&input, &cwd, area, &mut buf, &Theme::default());

        rendered.push_str(&format!("## /{}\n", command.name));
        rendered.push_str(&buffer_text_trimmed(&buf, area));
        rendered.push('\n');
    }

    insta::assert_snapshot!(
        "command_argument_help_all_commands_110w",
        normalize_snapshot_text(rendered)
    );
}

fn render_palette_snapshot(
    input: &str,
    cwd: &Path,
    width: u16,
    height: u16,
    keys: &[KeyCode],
) -> String {
    let mut palette = CommandPalette::new();
    palette.sync_from_input(input, cwd);
    for key in keys {
        palette.handle_key(*key);
    }

    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
    terminal
        .draw(|frame| {
            palette.render(frame.area(), frame.buffer_mut(), &Theme::default());
        })
        .expect("draw");

    normalize_snapshot_text(terminal.backend().to_string())
}

fn snapshot_cwd() -> PathBuf {
    PathBuf::from("C:\\allthecodes-snapshot")
}

struct EnvVarGuard {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let previous = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
    }
}

fn normalize_snapshot_text(mut text: String) -> String {
    for raw_home in [std::env::var_os("USERPROFILE"), std::env::var_os("HOME")]
        .into_iter()
        .flatten()
    {
        let home = PathBuf::from(raw_home).to_string_lossy().replace('\\', "/");
        text = text.replace(&home, "<HOME>");
        text = text.replace(&home.replace(' ', "%20"), "<HOME>");
    }

    text = normalize_managed_settings_path_width(text);
    text = normalize_allthecodes_home_width(text);
    text.replace("C:/allthecodes-snapshot", "<WORKSPACE>")
}

fn normalize_managed_settings_path_width(text: String) -> String {
    const PLACEHOLDER: &str = "<MANAGED_SETTINGS>";
    const PATHS: [&str; 2] = [
        "file:///C:/ProgramData/allthecodes/settings.json",
        "file:///etc/allthecodes/managed-settings.json",
    ];

    text.lines()
        .map(|line| {
            let width = line.chars().count();
            let mut normalized = line.to_string();
            for path in PATHS {
                normalized = normalized.replace(path, PLACEHOLDER);
            }
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

fn buffer_text_trimmed(buf: &Buffer, area: Rect) -> String {
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

fn buffer_text(buf: &Buffer, area: Rect) -> String {
    let mut text = String::new();
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            text.push_str(buf[(x, y)].symbol());
        }
        text.push('\n');
    }
    text
}
