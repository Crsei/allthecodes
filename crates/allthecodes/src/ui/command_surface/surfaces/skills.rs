use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::ui::better_view_panel::{plain_row, selected_row, BetterViewPanel};
use crate::ui::command_surface::{cycle_index, CommandSurfaceCursorAnchor, CommandSurfaceOutcome};
use crate::ui::skills::skills_menu::SkillMenuItem;
use crate::ui::skills_helpers::{skill_description, skill_display_name};
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillsSurface {
    pub(crate) items: Vec<SkillMenuItem>,
    pub(crate) selected_index: usize,
    pub(crate) filter: String,
}

impl SkillsSurface {
    pub(crate) fn new() -> Self {
        let mut items = allthecodes_skills::get_all_skills()
            .into_iter()
            .map(skill_menu_item)
            .collect::<Vec<_>>();
        items.sort_by(|a, b| a.name.cmp(&b.name));
        Self {
            items,
            selected_index: 0,
            filter: String::new(),
        }
    }

    pub(crate) fn render(&self) -> String {
        let visible = self.visible_indices();
        let mut detail_lines = if visible.is_empty() {
            vec!["No matching skills".to_string()]
        } else {
            visible
                .iter()
                .enumerate()
                .filter_map(|(visible_idx, item_idx)| {
                    let item = self.items.get(*item_idx)?;
                    let mut detail = format!("enabled={} source={}", item.enabled, item.source);
                    if !item.description.is_empty() {
                        detail.push_str(&format!("  {}", item.description));
                    }
                    Some(selected_row(
                        &item.name,
                        detail,
                        visible_idx == self.selected_index,
                    ))
                })
                .collect::<Vec<_>>()
        };
        if let Some(item) = self.selected_item() {
            detail_lines.push(String::new());
            detail_lines.push("Commands".to_string());
            detail_lines.push(plain_row("Enter:", format!("/skills {}", item.name)));
            if self.filter.is_empty() {
                detail_lines.push(plain_row("r:", "/skills reload"));
                detail_lines.push(plain_row("d:", "/skills diagnostics"));
            }
        }
        BetterViewPanel::new("Skills")
            .summary(format!(
                "filter={} visible={} total={}",
                self.filter,
                visible.len(),
                self.items.len()
            ))
            .sections_title("Skills")
            .sections(vec!["All skills".to_string()], 0)
            .detail_title("Skill details")
            .detail_lines(detail_lines)
            .footer("Type filter | Ctrl+S search | Up/Down skill | Enter details | Esc close")
            .render()
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> CommandSurfaceOutcome {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_selection(-1);
                CommandSurfaceOutcome::None
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                self.move_selection(1);
                CommandSurfaceOutcome::None
            }
            KeyCode::Enter => self
                .selected_item()
                .map(|item| CommandSurfaceOutcome::Submit(format!("/skills {}", item.name)))
                .unwrap_or(CommandSurfaceOutcome::None),
            KeyCode::Char('s')
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && !self.filter.trim().is_empty() =>
            {
                CommandSurfaceOutcome::Submit(format!("/skills search {}", self.filter.trim()))
            }
            KeyCode::Backspace => {
                self.filter.pop();
                self.selected_index = 0;
                CommandSurfaceOutcome::None
            }
            KeyCode::Char('r') if self.filter.is_empty() => {
                CommandSurfaceOutcome::Submit("/skills reload".to_string())
            }
            KeyCode::Char('d') if self.filter.is_empty() => {
                CommandSurfaceOutcome::Submit("/skills diagnostics".to_string())
            }
            KeyCode::Char(ch) if !ch.is_control() => {
                self.filter.push(ch);
                self.selected_index = 0;
                CommandSurfaceOutcome::None
            }
            _ => CommandSurfaceOutcome::None,
        }
    }

    pub(crate) fn cursor_anchor(&self) -> Option<CommandSurfaceCursorAnchor> {
        Some(CommandSurfaceCursorAnchor::Search {
            marker: "filter=".to_string(),
            value: self.filter.clone(),
        })
    }

    pub(crate) fn move_selection(&mut self, direction: isize) {
        let visible_len = self.visible_indices().len();
        self.selected_index = cycle_index(self.selected_index, visible_len, direction);
    }

    pub(crate) fn selected_item(&self) -> Option<&SkillMenuItem> {
        let visible_indices = self.visible_indices();
        visible_indices
            .get(self.selected_index)
            .and_then(|idx| self.items.get(*idx))
    }

    pub(crate) fn visible_indices(&self) -> Vec<usize> {
        let filter = self.filter.to_ascii_lowercase();
        self.items
            .iter()
            .enumerate()
            .filter_map(|(idx, item)| {
                (filter.is_empty()
                    || item.name.to_ascii_lowercase().contains(&filter)
                    || item.description.to_ascii_lowercase().contains(&filter))
                .then_some(idx)
            })
            .collect()
    }
}

pub(crate) fn skill_menu_item(skill: allthecodes_skills::SkillDefinition) -> SkillMenuItem {
    SkillMenuItem {
        name: skill_display_name(&skill).to_string(),
        description: skill_description(&skill).to_string(),
        enabled: skill.is_user_invocable(),
        source: skill_source_label(&skill.source),
    }
}

pub(crate) fn skill_source_label(source: &allthecodes_skills::SkillSource) -> String {
    match source {
        allthecodes_skills::SkillSource::Bundled => "bundled".to_string(),
        allthecodes_skills::SkillSource::User => "user".to_string(),
        allthecodes_skills::SkillSource::Project => "project".to_string(),
        allthecodes_skills::SkillSource::Plugin(name) => format!("plugin:{name}"),
        allthecodes_skills::SkillSource::Mcp(name) => format!("mcp:{name}"),
    }
}
