use super::*;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

fn action(id: &str, label: &str) -> SelectionAction {
    SelectionAction {
        id: id.into(),
        label: label.into(),
        enabled: true,
        disabled_reason: None,
    }
}

#[test]
fn render_supports_preview_actions_and_disabled_reasons() {
    let surface = SelectionSurface::new(
        "Plugins",
        vec![
            SelectionItem {
                description: "installed".into(),
                preview_lines: vec!["tool: shell".into(), "skill: review".into()],
                actions: vec![
                    action("enable", "Enable"),
                    SelectionAction {
                        disabled_reason: Some("requires disable first".into()),
                        enabled: false,
                        ..action("remove", "Remove")
                    },
                ],
                ..SelectionItem::new("alpha", "Alpha")
            },
            SelectionItem {
                enabled: false,
                disabled_reason: Some("missing manifest".into()),
                ..SelectionItem::new("beta", "Beta")
            },
        ],
    );

    let rendered = surface.render_lines(6).join("\n");

    assert!(rendered.contains("> Alpha - installed"));
    assert!(rendered.contains("  tool: shell"));
    assert!(rendered.contains("  action enable: Enable"));
    assert!(rendered.contains("action remove: Remove disabled: requires disable first"));
    assert!(rendered.contains("Beta -  disabled: missing manifest"));
}

#[test]
fn fuzzy_filter_matches_preview_actions_disabled_reason_and_extra_terms() {
    let mut surface = SelectionSurface::new(
        "Plugins",
        vec![
            SelectionItem {
                preview_lines: vec!["provides shell tool".into()],
                actions: vec![action("enable", "Enable plugin")],
                search_terms: vec!["marketplace:local".into()],
                ..SelectionItem::new("alpha", "Alpha")
            },
            SelectionItem {
                enabled: false,
                disabled_reason: Some("missing manifest".into()),
                ..SelectionItem::new("beta", "Beta")
            },
        ],
    );

    surface.set_filter("shell");
    assert_eq!(
        surface.selected_item().map(|item| item.id.as_str()),
        Some("alpha")
    );

    surface.set_filter("manifest");
    assert_eq!(
        surface.selected_item().map(|item| item.id.as_str()),
        Some("beta")
    );

    surface.set_filter("local");
    assert_eq!(
        surface.selected_item().map(|item| item.id.as_str()),
        Some("alpha")
    );
}

#[test]
fn selected_enabled_action_respects_row_and_action_disabled_state() {
    let mut surface = SelectionSurface::new(
        "Plugins",
        vec![
            SelectionItem {
                actions: vec![
                    action("enable", "Enable"),
                    SelectionAction {
                        enabled: false,
                        disabled_reason: Some("blocked".into()),
                        ..action("remove", "Remove")
                    },
                ],
                ..SelectionItem::new("alpha", "Alpha")
            },
            SelectionItem {
                enabled: false,
                disabled_reason: Some("blocked".into()),
                actions: vec![action("enable", "Enable")],
                ..SelectionItem::new("beta", "Beta")
            },
        ],
    );

    assert_eq!(
        surface
            .selected_enabled_action("enable")
            .map(|action| action.id.as_str()),
        Some("enable")
    );
    assert!(surface.selected_enabled_action("remove").is_none());

    surface.handle_key(key(KeyCode::Down));
    assert!(surface.selected_item().is_some_and(|item| !item.enabled));
    assert!(surface.selected_enabled_action("enable").is_none());
}
