use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent};

use crate::ui::command_surface::adapters::diff::build_diff_sources;
use crate::ui::command_surface::{cycle_index, CommandSurfaceOutcome};
use crate::ui::diff::diff_dialog::{render_diff_dialog_lines, DiffDialogMode, DiffSource};
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffSurface {
    pub(crate) sources: Vec<DiffSource>,
    pub(crate) source_index: usize,
    pub(crate) selected_index: usize,
    pub(crate) mode: DiffDialogMode,
    pub(crate) error: Option<String>,
}

impl DiffSurface {
    pub(crate) fn new(cwd: &Path) -> Self {
        match build_diff_sources(cwd) {
            Ok(sources) => Self {
                sources,
                source_index: 0,
                selected_index: 0,
                mode: DiffDialogMode::List,
                error: None,
            },
            Err(error) => Self {
                sources: vec![DiffSource::current()],
                source_index: 0,
                selected_index: 0,
                mode: DiffDialogMode::List,
                error: Some(error),
            },
        }
    }

    pub(crate) fn render(&self) -> String {
        if let Some(error) = &self.error {
            return format!("Diff\n{error}\n\nEsc close");
        }

        let subtitle = self
            .sources
            .get(self.source_index)
            .map(|source| source.label.as_str());
        let mut lines = render_diff_dialog_lines(
            "Uncommitted changes",
            subtitle,
            &self.sources,
            self.source_index,
            self.selected_index,
            self.mode,
            80,
        );
        lines.push(String::new());
        lines.push("Esc close".to_string());
        lines.join("\n")
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> CommandSurfaceOutcome {
        match key.code {
            KeyCode::Left | KeyCode::Char('[') if self.mode == DiffDialogMode::List => {
                self.switch_source(-1);
                CommandSurfaceOutcome::None
            }
            KeyCode::Right | KeyCode::Char(']') if self.mode == DiffDialogMode::List => {
                self.switch_source(1);
                CommandSurfaceOutcome::None
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_file(-1);
                CommandSurfaceOutcome::None
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                self.move_file(1);
                CommandSurfaceOutcome::None
            }
            KeyCode::Enter
                if self.mode == DiffDialogMode::List && self.current_file_count() > 0 =>
            {
                self.mode = DiffDialogMode::Detail;
                CommandSurfaceOutcome::None
            }
            KeyCode::Char('b') if self.mode == DiffDialogMode::Detail => {
                self.mode = DiffDialogMode::List;
                CommandSurfaceOutcome::None
            }
            _ => CommandSurfaceOutcome::None,
        }
    }

    pub(crate) fn switch_source(&mut self, direction: isize) {
        self.source_index = cycle_index(self.source_index, self.sources.len(), direction);
        self.selected_index = 0;
    }

    pub(crate) fn move_file(&mut self, direction: isize) {
        self.selected_index =
            cycle_index(self.selected_index, self.current_file_count(), direction);
    }

    pub(crate) fn current_file_count(&self) -> usize {
        self.sources
            .get(self.source_index)
            .map(|source| source.data.files.len())
            .unwrap_or(0)
    }
}
