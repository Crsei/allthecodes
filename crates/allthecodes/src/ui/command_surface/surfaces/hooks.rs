use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent};
use serde_json::Value;

use crate::ui::better_view_panel::{plain_row, BetterViewPanel};
use crate::ui::command_surface::adapters::hooks::{
    get_hooks_for_matcher, get_sorted_matchers_for_event, group_hooks_by_event_and_matcher,
    hook_display_text, hook_event_metadata, hook_primary_field, hook_source_description,
    hook_source_header, hook_source_inline, hook_type, hooks_by_event_count, matcher_display_label,
    HooksByEventAndMatcher, IndividualHookConfig,
};
use crate::ui::command_surface::{cycle_index, CommandSurfaceOutcome};
use crate::ui::hooks::hooks_config_menu::{render_hooks_config_menu, HookConfigSummary};
use crate::ui::hooks::select_event_mode::{
    render_select_event_mode, HookEvent, HookEventRow, HOOK_EVENTS,
};
use crate::ui::hooks::select_hook_mode::{render_select_hook_mode, HookListItem};
use crate::ui::hooks::select_matcher_mode::{render_select_matcher_mode, HookMatcher};
use crate::ui::hooks::view_hook_mode::{render_view_hook_mode, HookView};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HooksSurface {
    pub(crate) grouped: HooksByEventAndMatcher,
    pub(crate) mode: HooksSurfaceMode,
    pub(crate) selected_event_index: usize,
    pub(crate) selected_matcher_index: usize,
    pub(crate) selected_hook_index: usize,
    pub(crate) scope_index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HooksSurfaceMode {
    SelectEvent,
    SelectMatcher {
        event: HookEvent,
    },
    SelectHook {
        event: HookEvent,
        matcher: String,
    },
    ViewHook {
        event: HookEvent,
        matcher: String,
        hook_index: usize,
    },
}

impl HooksSurface {
    pub(crate) fn new(hooks: &HashMap<String, Value>) -> Self {
        Self {
            grouped: group_hooks_by_event_and_matcher(hooks),
            mode: HooksSurfaceMode::SelectEvent,
            selected_event_index: 0,
            selected_matcher_index: 0,
            selected_hook_index: 0,
            scope_index: 0,
        }
    }

    pub(crate) fn render(&self) -> String {
        let mut detail_lines = self
            .mode_render()
            .lines()
            .map(str::to_string)
            .collect::<Vec<_>>();
        detail_lines.push(String::new());
        detail_lines.push("Actions".to_string());
        detail_lines.push(plain_row(
            "o open selected scope:",
            if self.scope_index == 0 {
                "/hooks open user"
            } else {
                "/hooks open project"
            },
        ));
        detail_lines.push(plain_row(
            "u/p open scope:",
            "/hooks open user | /hooks open project",
        ));
        detail_lines.push(plain_row(
            "l list selected event:",
            format!("/hooks list {}", self.current_event()),
        ));
        BetterViewPanel::new("Hooks")
            .summary(format!(
                "mode={} scope={} hooks={}",
                self.mode_label(),
                if self.scope_index == 0 {
                    "user"
                } else {
                    "project"
                },
                self.total_hooks_count()
            ))
            .sections_title("Settings")
            .sections(
                vec!["User settings".to_string(), "Project settings".to_string()],
                self.scope_index,
            )
            .detail_title(self.detail_title())
            .detail_lines(detail_lines)
            .footer(self.footer())
            .render()
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> CommandSurfaceOutcome {
        match key.code {
            KeyCode::Left | KeyCode::Char('[') => {
                self.scope_index = cycle_index(self.scope_index, 2, -1);
                CommandSurfaceOutcome::None
            }
            KeyCode::Right | KeyCode::Char(']') => {
                self.scope_index = cycle_index(self.scope_index, 2, 1);
                CommandSurfaceOutcome::None
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_prev();
                CommandSurfaceOutcome::None
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                self.move_next();
                CommandSurfaceOutcome::None
            }
            KeyCode::Enter => {
                self.enter_selected();
                CommandSurfaceOutcome::None
            }
            KeyCode::Esc => {
                if self.go_back() {
                    CommandSurfaceOutcome::None
                } else {
                    CommandSurfaceOutcome::Close
                }
            }
            KeyCode::Backspace => {
                self.go_back();
                CommandSurfaceOutcome::None
            }
            KeyCode::Char('u') => CommandSurfaceOutcome::Submit("/hooks open user".to_string()),
            KeyCode::Char('p') => CommandSurfaceOutcome::Submit("/hooks open project".to_string()),
            KeyCode::Char('o') => CommandSurfaceOutcome::Submit(format!(
                "/hooks open {}",
                if self.scope_index == 0 {
                    "user"
                } else {
                    "project"
                }
            )),
            KeyCode::Char('l') => {
                CommandSurfaceOutcome::Submit(format!("/hooks list {}", self.current_event()))
            }
            _ => CommandSurfaceOutcome::None,
        }
    }

    fn mode_render(&self) -> String {
        match &self.mode {
            HooksSurfaceMode::SelectEvent => self.render_event_mode(),
            HooksSurfaceMode::SelectMatcher { event } => self.render_matcher_mode(*event),
            HooksSurfaceMode::SelectHook { event, matcher } => {
                self.render_hook_mode(*event, matcher)
            }
            HooksSurfaceMode::ViewHook {
                event,
                matcher,
                hook_index,
            } => self.render_view_mode(*event, matcher, *hook_index),
        }
    }

    fn render_event_mode(&self) -> String {
        let counts = hooks_by_event_count(&self.grouped);
        let summaries = HOOK_EVENTS
            .iter()
            .copied()
            .map(|event| {
                let matchers = self.grouped.get(&event);
                HookConfigSummary {
                    event,
                    matcher_count: matchers.map_or(0, HashMap::len),
                    hook_count: matchers
                        .map(|by_matcher| by_matcher.values().map(Vec::len).sum())
                        .unwrap_or(0),
                }
            })
            .collect::<Vec<_>>();
        let rows = HOOK_EVENTS
            .iter()
            .copied()
            .map(|event| {
                let metadata = hook_event_metadata(event, &[]);
                HookEventRow::new(
                    event,
                    metadata.summary,
                    counts.get(&event).copied().unwrap_or(0),
                )
            })
            .collect::<Vec<_>>();
        format!(
            "{}\n\n{}",
            render_hooks_config_menu(&summaries, self.selected_event_index),
            render_select_event_mode(
                &rows,
                self.selected_event_index,
                self.total_hooks_count(),
                false
            )
        )
    }

    fn render_matcher_mode(&self, event: HookEvent) -> String {
        let metadata = hook_event_metadata(event, &[]);
        let mut description = metadata.description.clone();
        if let Some(matcher_metadata) = &metadata.matcher_metadata {
            description.push_str(&format!(
                "\nMatcher field: {}",
                matcher_metadata.field_to_match
            ));
            if !matcher_metadata.values.is_empty() {
                description.push_str(&format!(" ({})", matcher_metadata.values.join(", ")));
            }
        }
        let matchers = self.matcher_rows(event);
        render_select_matcher_mode(
            &event.to_string(),
            &description,
            &matchers,
            self.selected_matcher_index,
        )
    }

    fn render_hook_mode(&self, event: HookEvent, matcher: &str) -> String {
        let metadata = hook_event_metadata(event, &[]);
        let hooks = self.hook_items(event, matcher);
        let title = if metadata.matcher_metadata.is_some() {
            format!("{} - Matcher: {}", event, matcher_display_label(matcher))
        } else {
            event.to_string()
        };
        render_select_hook_mode(
            &title,
            &metadata.description,
            &hooks,
            self.selected_hook_index,
        )
    }

    fn render_view_mode(&self, event: HookEvent, matcher: &str, hook_index: usize) -> String {
        let hooks = get_hooks_for_matcher(&self.grouped, event, matcher);
        let Some(hook) = hooks.get(hook_index) else {
            return "Hook details\nNo hook selected".to_string();
        };
        render_view_hook_mode(&hook_view_model(hook))
    }

    fn matcher_rows(&self, event: HookEvent) -> Vec<HookMatcher> {
        get_sorted_matchers_for_event(&self.grouped, event)
            .into_iter()
            .map(|matcher| {
                let hooks = get_hooks_for_matcher(&self.grouped, event, &matcher);
                let mut sources = Vec::<String>::new();
                for hook in &hooks {
                    let label = hook_source_inline(hook.source).to_string();
                    if !sources.contains(&label) {
                        sources.push(label);
                    }
                }
                let mut row = if matcher.is_empty() {
                    HookMatcher::all_tools()
                } else {
                    HookMatcher::for_tool(matcher)
                };
                row.sources = sources;
                row.hook_count = hooks.len();
                row
            })
            .collect()
    }

    fn hook_items(&self, event: HookEvent, matcher: &str) -> Vec<HookListItem> {
        get_hooks_for_matcher(&self.grouped, event, matcher)
            .into_iter()
            .map(|hook| {
                let mut item =
                    HookListItem::new(hook_type(&hook.config), hook_display_text(&hook.config));
                item.source = hook_source_header(hook.source).to_string();
                item
            })
            .collect()
    }

    fn move_next(&mut self) {
        match &self.mode {
            HooksSurfaceMode::SelectEvent => {
                self.selected_event_index =
                    cycle_index(self.selected_event_index, HOOK_EVENTS.len(), 1);
            }
            HooksSurfaceMode::SelectMatcher { event } => {
                let len = get_sorted_matchers_for_event(&self.grouped, *event).len();
                self.selected_matcher_index = cycle_index(self.selected_matcher_index, len, 1);
            }
            HooksSurfaceMode::SelectHook { event, matcher } => {
                let len = get_hooks_for_matcher(&self.grouped, *event, matcher).len();
                self.selected_hook_index = cycle_index(self.selected_hook_index, len, 1);
            }
            HooksSurfaceMode::ViewHook { .. } => {}
        }
    }

    fn move_prev(&mut self) {
        match &self.mode {
            HooksSurfaceMode::SelectEvent => {
                self.selected_event_index =
                    cycle_index(self.selected_event_index, HOOK_EVENTS.len(), -1);
            }
            HooksSurfaceMode::SelectMatcher { event } => {
                let len = get_sorted_matchers_for_event(&self.grouped, *event).len();
                self.selected_matcher_index = cycle_index(self.selected_matcher_index, len, -1);
            }
            HooksSurfaceMode::SelectHook { event, matcher } => {
                let len = get_hooks_for_matcher(&self.grouped, *event, matcher).len();
                self.selected_hook_index = cycle_index(self.selected_hook_index, len, -1);
            }
            HooksSurfaceMode::ViewHook { .. } => {}
        }
    }

    fn enter_selected(&mut self) {
        match &self.mode {
            HooksSurfaceMode::SelectEvent => {
                let event = self.selected_event();
                self.selected_matcher_index = 0;
                self.selected_hook_index = 0;
                if hook_event_metadata(event, &[]).matcher_metadata.is_some() {
                    self.mode = HooksSurfaceMode::SelectMatcher { event };
                } else {
                    self.mode = HooksSurfaceMode::SelectHook {
                        event,
                        matcher: String::new(),
                    };
                }
            }
            HooksSurfaceMode::SelectMatcher { event } => {
                let matchers = get_sorted_matchers_for_event(&self.grouped, *event);
                if let Some(matcher) = matchers.get(self.selected_matcher_index).cloned() {
                    self.selected_hook_index = 0;
                    self.mode = HooksSurfaceMode::SelectHook {
                        event: *event,
                        matcher,
                    };
                }
            }
            HooksSurfaceMode::SelectHook { event, matcher } => {
                let hooks = get_hooks_for_matcher(&self.grouped, *event, matcher);
                if self.selected_hook_index < hooks.len() {
                    self.mode = HooksSurfaceMode::ViewHook {
                        event: *event,
                        matcher: matcher.clone(),
                        hook_index: self.selected_hook_index,
                    };
                }
            }
            HooksSurfaceMode::ViewHook { .. } => {}
        }
    }

    fn go_back(&mut self) -> bool {
        match &self.mode {
            HooksSurfaceMode::SelectEvent => false,
            HooksSurfaceMode::SelectMatcher { .. } => {
                self.mode = HooksSurfaceMode::SelectEvent;
                true
            }
            HooksSurfaceMode::SelectHook { event, .. } => {
                if hook_event_metadata(*event, &[]).matcher_metadata.is_some() {
                    self.mode = HooksSurfaceMode::SelectMatcher { event: *event };
                } else {
                    self.mode = HooksSurfaceMode::SelectEvent;
                }
                true
            }
            HooksSurfaceMode::ViewHook { event, matcher, .. } => {
                self.mode = HooksSurfaceMode::SelectHook {
                    event: *event,
                    matcher: matcher.clone(),
                };
                true
            }
        }
    }

    fn selected_event(&self) -> HookEvent {
        HOOK_EVENTS
            .get(self.selected_event_index)
            .copied()
            .unwrap_or(HookEvent::PreToolUse)
    }

    fn current_event(&self) -> HookEvent {
        match &self.mode {
            HooksSurfaceMode::SelectEvent => self.selected_event(),
            HooksSurfaceMode::SelectMatcher { event }
            | HooksSurfaceMode::SelectHook { event, .. }
            | HooksSurfaceMode::ViewHook { event, .. } => *event,
        }
    }

    fn total_hooks_count(&self) -> usize {
        hooks_by_event_count(&self.grouped).values().sum()
    }

    fn mode_label(&self) -> &'static str {
        match self.mode {
            HooksSurfaceMode::SelectEvent => "events",
            HooksSurfaceMode::SelectMatcher { .. } => "matchers",
            HooksSurfaceMode::SelectHook { .. } => "hooks",
            HooksSurfaceMode::ViewHook { .. } => "detail",
        }
    }

    fn detail_title(&self) -> &'static str {
        match self.mode {
            HooksSurfaceMode::SelectEvent => "Hook events",
            HooksSurfaceMode::SelectMatcher { .. } => "Matchers",
            HooksSurfaceMode::SelectHook { .. } => "Hooks",
            HooksSurfaceMode::ViewHook { .. } => "Hook detail",
        }
    }

    fn footer(&self) -> &'static str {
        match self.mode {
            HooksSurfaceMode::SelectEvent => {
                "Left/Right scope | Up/Down event | Enter browse | l list | o/u/p open | Esc close"
            }
            HooksSurfaceMode::SelectMatcher { .. } => {
                "Up/Down matcher | Enter hooks | Esc/Backspace events | l list | o/u/p open"
            }
            HooksSurfaceMode::SelectHook { .. } => {
                "Up/Down hook | Enter details | Esc/Backspace back | l list | o/u/p open"
            }
            HooksSurfaceMode::ViewHook { .. } => {
                "Esc/Backspace hooks | l list event | o/u/p open settings"
            }
        }
    }
}

fn hook_view_model(hook: &IndividualHookConfig) -> HookView {
    let metadata = hook_event_metadata(hook.event, &[]);
    let (content_label, content_value) = hook_primary_field(&hook.config);
    HookView {
        event: hook.event,
        matcher: Some(matcher_display_label(&hook.matcher)),
        event_supports_matcher: metadata.matcher_metadata.is_some(),
        hook_type: hook_type(&hook.config),
        source: hook_source_description(hook.source).to_string(),
        plugin_name: hook.plugin_name.clone(),
        content_label: content_label.to_string(),
        content_value,
        status_message: hook
            .config
            .get("statusMessage")
            .and_then(Value::as_str)
            .map(str::to_string),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::command_surface::adapters::hooks::hook_summary;
    use serde_json::json;

    #[test]
    fn summary_counts_all_configured_hooks() {
        let hooks = HashMap::from([(
            "PreToolUse".to_string(),
            json!([{ "matcher": "Bash", "hooks": [{ "type": "command", "command": "cargo test" }] }]),
        )]);

        let summary = hook_summary(HookEvent::PreToolUse, hooks.get("PreToolUse"));

        assert_eq!(summary.event, HookEvent::PreToolUse);
        assert_eq!(summary.matcher_count, 1);
        assert_eq!(summary.hook_count, 1);
    }
}
