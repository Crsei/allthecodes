//! Rust-side hooks configuration UI surfaces.
pub mod hooks_config_menu;
pub mod select_event_mode;
pub mod select_hook_mode;
pub mod select_matcher_mode;
pub mod view_hook_mode;

#[cfg(test)]
mod tests {
    use super::hooks_config_menu::{render_hooks_config_menu, HookConfigSummary};
    use super::select_event_mode::{
        render_select_event_mode, HookEvent, HookEventRow, HOOK_EVENTS,
    };
    use super::select_hook_mode::{render_select_hook_mode, HookListItem};
    use super::select_matcher_mode::{render_select_matcher_mode, HookMatcher};
    use super::view_hook_mode::{render_view_hook_mode, HookView};

    #[test]
    fn snapshot_hooks_surfaces() {
        let rows = HOOK_EVENTS
            .iter()
            .copied()
            .map(|event| {
                HookEventRow::new(
                    event,
                    format!("{event} summary"),
                    usize::from(event == HookEvent::PreToolUse),
                )
            })
            .collect::<Vec<_>>();
        let hooks = vec![
            HookListItem::new("command", "cargo fmt --check"),
            HookListItem {
                hook_type: "http".to_string(),
                display_text: "https://hooks.example/run".to_string(),
                source: "Effective Settings".to_string(),
            },
        ];
        let mut matchers = vec![HookMatcher::all_tools(), HookMatcher::for_tool("Bash")];
        matchers[0].hook_count = 1;
        matchers[1].hook_count = 2;
        let view = HookView {
            event: HookEvent::PreToolUse,
            matcher: Some("Bash".to_string()),
            event_supports_matcher: true,
            hook_type: "command".to_string(),
            source: "Effective settings (merged runtime hooks)".to_string(),
            plugin_name: None,
            content_label: "Command".to_string(),
            content_value: "cargo fmt --check".to_string(),
            status_message: Some("Format check".to_string()),
        };

        let rendered = [
            section(
                "config",
                render_hooks_config_menu(
                    &[HookConfigSummary {
                        event: HookEvent::PreToolUse,
                        matcher_count: 2,
                        hook_count: 2,
                    }],
                    0,
                ),
            ),
            section("event", render_select_event_mode(&rows, 2, 1, false)),
            section(
                "matcher",
                render_select_matcher_mode(
                    "PreToolUse",
                    "Input to command is JSON of tool call arguments.",
                    &matchers,
                    1,
                ),
            ),
            section(
                "hook",
                render_select_hook_mode(
                    "PreToolUse - Matcher: Bash",
                    "Input to command is JSON of tool call arguments.",
                    &hooks,
                    1,
                ),
            ),
            section("view", render_view_hook_mode(&view)),
        ]
        .join("\n\n");

        insta::assert_snapshot!("hooks_surfaces", rendered);
    }

    fn section(name: &str, body: impl AsRef<str>) -> String {
        format!("## {name}\n{}", body.as_ref())
    }
}
