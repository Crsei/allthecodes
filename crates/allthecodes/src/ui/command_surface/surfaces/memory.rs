use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent};

use crate::ui::better_view_panel::{plain_row, BetterViewPanel};
use crate::ui::command_surface::adapters::memory::{memory_options, selected_memory_open_command};
use crate::ui::command_surface::{cycle_index, CommandSurfaceOutcome};
use crate::ui::memory::memory_file_selector::{MemoryFileKind, MemoryFileSelectorState};
use crate::ui::memory::memory_update_notification::render_memory_update_notification;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemorySurface {
    pub(crate) state: MemoryFileSelectorState,
    pub(crate) cwd: PathBuf,
    pub(crate) home: PathBuf,
    pub(crate) action_index: usize,
}

impl MemorySurface {
    pub(crate) fn new(cwd: &Path) -> Self {
        let home = allthecodes_config::paths::data_root();
        let project_memory = cwd.join("AGENTS.md");
        let options = memory_options(cwd, &home)
            .into_iter()
            .map(|option| {
                if option.kind == MemoryFileKind::Nested && option.parent.is_none() {
                    option.with_parent(project_memory.clone())
                } else {
                    option
                }
            })
            .collect();
        Self {
            state: MemoryFileSelectorState::new(options),
            cwd: cwd.to_path_buf(),
            home,
            action_index: 0,
        }
    }

    pub(crate) fn render(&self) -> String {
        let sections = vec![
            "Edit".to_string(),
            "Show".to_string(),
            "Paths".to_string(),
            "Open".to_string(),
        ];
        let mut detail_lines = self
            .state
            .render(&self.cwd, &self.home)
            .lines()
            .map(str::to_string)
            .collect::<Vec<_>>();
        detail_lines.push(String::new());
        detail_lines.push("Commands".to_string());
        detail_lines.push(match self.action_index {
            0 => plain_row("Enter:", "/memory edit"),
            1 => plain_row("Enter:", "/memory show"),
            2 => plain_row("Enter:", "/memory path"),
            3 => plain_row("Enter:", "/memory open <target>"),
            _ => plain_row("Enter:", "select"),
        });
        if self.action_index == 0 {
            if let Some(path) = self.state.selected_path() {
                detail_lines.push(plain_row(
                    "After save:",
                    render_memory_update_notification(path, &self.cwd, &self.home),
                ));
            }
        }
        BetterViewPanel::new("Memory")
            .summary(format!(
                "action={} targets={}",
                sections.get(self.action_index).cloned().unwrap_or_default(),
                self.state.options.len()
            ))
            .sections_title("Actions")
            .sections(sections, self.action_index)
            .detail_title("Memory files")
            .detail_lines(detail_lines)
            .footer("Left/Right action | Up/Down target | Enter select | Esc close")
            .render()
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> CommandSurfaceOutcome {
        match key.code {
            KeyCode::Left | KeyCode::Char('[') => {
                self.action_index = cycle_index(self.action_index, 4, -1);
                CommandSurfaceOutcome::None
            }
            KeyCode::Right | KeyCode::Char(']') => {
                self.action_index = cycle_index(self.action_index, 4, 1);
                CommandSurfaceOutcome::None
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.state.move_prev();
                CommandSurfaceOutcome::None
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                self.state.move_next();
                CommandSurfaceOutcome::None
            }
            KeyCode::Enter => self.selected_memory_action(),
            KeyCode::Char('s') => CommandSurfaceOutcome::Submit("/memory show".to_string()),
            KeyCode::Char('p') => CommandSurfaceOutcome::Submit("/memory path".to_string()),
            KeyCode::Char('a') => CommandSurfaceOutcome::Submit("/memory open auto".to_string()),
            KeyCode::Char('t') => CommandSurfaceOutcome::Submit("/memory open team".to_string()),
            KeyCode::Char('g') => CommandSurfaceOutcome::Submit("/memory open global".to_string()),
            KeyCode::Char('o') => CommandSurfaceOutcome::Submit("/memory open project".to_string()),
            _ => CommandSurfaceOutcome::None,
        }
    }

    pub(crate) fn selected_memory_action(&self) -> CommandSurfaceOutcome {
        match self.action_index {
            0 => CommandSurfaceOutcome::Submit("/memory edit".to_string()),
            1 => CommandSurfaceOutcome::Submit("/memory show".to_string()),
            2 => CommandSurfaceOutcome::Submit("/memory path".to_string()),
            3 => selected_memory_open_command(&self.state),
            _ => CommandSurfaceOutcome::None,
        }
    }
}
