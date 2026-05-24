use std::path::Path;

use crossterm::event::KeyCode;

use super::selection_surface::{SelectionItem, SelectionSurface};

mod edit_targets;
mod filter;
mod metadata;
mod render;
#[cfg(test)]
mod tests;

use edit_targets::{has_edit_target_picker, EditTarget};
use filter::{command_from_argument_input, filtered_commands};
#[cfg(test)]
use render::argument_edit_row_count;

const MAX_ROWS: usize = 20;
const RESERVED_NON_COMMAND_ROWS: usize = 7;
const BORDER_ROWS: u16 = 2;
#[cfg(test)]
const ARG_HELP_BASE_HEIGHT: u16 = 5;
const MAX_EDIT_ROWS: usize = 2;
const MAX_EDIT_TARGET_ROWS: usize = 4;

const _: fn() = production_symbol_anchors;

fn production_symbol_anchors() {
    let _ = filter::find_mid_input_slash_command("run /help", 9);
}

#[derive(Debug, Clone)]
pub struct CommandPalette {
    active: bool,
    query: String,
    selected: usize,
    filtered: Vec<CommandItem>,
    edit_target_picker: Option<SelectionSurface>,
}

#[derive(Debug, Clone)]
pub struct CommandItem {
    pub name: String,
    pub aliases: Vec<String>,
    pub description: String,
    pub usage: String,
    pub examples: Vec<String>,
    pub edit_targets: Vec<EditTarget>,
    /// Source group for grouping display (populated by filter).
    pub source_group: Option<&'static str>,
}

impl CommandPalette {
    pub fn new() -> Self {
        Self {
            active: false,
            query: String::new(),
            selected: 0,
            filtered: Vec::new(),
            edit_target_picker: None,
        }
    }

    pub fn active(&self) -> bool {
        self.active
    }

    pub fn sync_from_input(&mut self, input: &str, cwd: &Path) {
        let Some(without_slash) = input.strip_prefix('/') else {
            self.close();
            return;
        };

        if without_slash.contains(char::is_whitespace) {
            self.close();
            return;
        }

        self.active = true;
        self.query = without_slash.to_string();
        self.filtered = filtered_commands(&self.query, cwd);
        self.selected = self.selected.min(self.filtered.len().saturating_sub(1));
        self.edit_target_picker = None;
    }

    pub fn close(&mut self) {
        self.active = false;
        self.query.clear();
        self.selected = 0;
        self.filtered.clear();
        self.edit_target_picker = None;
    }

    pub fn handle_key(&mut self, code: KeyCode) -> bool {
        if !self.active {
            return false;
        }

        match code {
            KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
                true
            }
            KeyCode::Down => {
                if self.selected + 1 < self.filtered.len() {
                    self.selected += 1;
                }
                true
            }
            KeyCode::PageUp => {
                self.selected = self.selected.saturating_sub(MAX_ROWS);
                true
            }
            KeyCode::PageDown => {
                if !self.filtered.is_empty() {
                    self.selected = (self.selected + MAX_ROWS).min(self.filtered.len() - 1);
                }
                true
            }
            KeyCode::Esc => {
                self.close();
                true
            }
            _ => false,
        }
    }

    /// Get the input buffer text for the selected command (insert mode).
    pub fn selected_command_input(&self) -> Option<String> {
        self.filtered
            .get(self.selected)
            .map(|cmd| format!("/{} ", cmd.name))
    }

    pub fn selected_item(&self) -> Option<&CommandItem> {
        self.filtered.get(self.selected)
    }

    /// Apply the selected command suggestion.
    ///
    /// If `should_execute` is true and the command has no arguments, submit it
    /// directly instead of inserting into the prompt.
    pub fn apply_command_suggestion(
        &self,
        _item: &CommandItem,
        should_execute: bool,
    ) -> Option<CommandAction> {
        let cmd = self.filtered.get(self.selected)?;

        if should_execute
            && (cmd.accepts_no_arguments() || cmd.executes_on_exact_palette_match(&self.query))
        {
            Some(CommandAction::Execute(format!("/{}", cmd.name)))
        } else {
            Some(CommandAction::Insert(format!("/{} ", cmd.name)))
        }
    }

    /// Get the ghost suffix for the currently selected command.
    pub fn selected_ghost_suffix(&self) -> Option<String> {
        self.filtered.get(self.selected).map(|cmd| {
            let full = format!("/{}", cmd.name);
            let partial_len = self.query.len().min(full.len());
            full[partial_len..].to_string()
        })
    }

    pub fn selected_command_has_edit_targets(&self) -> bool {
        self.filtered
            .get(self.selected)
            .is_some_and(has_edit_target_picker)
    }

    pub fn edit_target_picker_active(&self) -> bool {
        self.edit_target_picker.is_some()
    }

    pub fn open_edit_target_picker(&mut self) -> bool {
        let Some(cmd) = self.filtered.get(self.selected) else {
            return false;
        };

        let items: Vec<SelectionItem> = cmd
            .edit_targets
            .iter()
            .filter(|target| !target.insert.is_empty())
            .map(|target| {
                let mut item = SelectionItem::new(target.insert.clone(), target.label.clone());
                item.description = target.display.clone();
                item
            })
            .collect();

        if items.is_empty() {
            return false;
        }

        self.edit_target_picker = Some(SelectionSurface::new(
            format!("Edit targets for /{}", cmd.name),
            items,
        ));
        true
    }

    pub fn close_edit_target_picker(&mut self) {
        self.edit_target_picker = None;
    }

    pub fn handle_edit_target_key(&mut self, code: KeyCode) -> bool {
        let Some(picker) = self.edit_target_picker.as_mut() else {
            return false;
        };

        match code {
            KeyCode::Up | KeyCode::Left => {
                picker.move_prev();
                true
            }
            KeyCode::Down | KeyCode::Right | KeyCode::Tab | KeyCode::BackTab => {
                picker.move_next();
                true
            }
            KeyCode::PageUp => {
                picker.move_prev();
                true
            }
            KeyCode::PageDown => {
                picker.move_next();
                true
            }
            KeyCode::Esc => {
                self.close_edit_target_picker();
                true
            }
            _ => false,
        }
    }

    pub fn selected_edit_target_input(&self) -> Option<String> {
        self.edit_target_picker
            .as_ref()
            .and_then(|picker| picker.selected_item())
            .map(|item| item.id.clone())
    }

    pub fn argument_hint(input: &str, cwd: &Path) -> Option<String> {
        command_from_argument_input(input, cwd).map(|item| item.usage)
    }

    #[cfg(test)]
    pub fn argument_help_height(input: &str, cwd: &Path) -> u16 {
        command_from_argument_input(input, cwd)
            .map(|item| {
                ARG_HELP_BASE_HEIGHT
                    + argument_edit_row_count(&item) as u16
                    + has_edit_target_picker(&item) as u16
            })
            .unwrap_or(0)
    }

    pub fn preferred_height(&self) -> u16 {
        if !self.active || self.filtered.is_empty() {
            return 0;
        }
        let list_rows = self.filtered.len().min(MAX_ROWS) as u16;
        let picker_rows = self
            .edit_target_picker
            .as_ref()
            .map(|picker| (picker.items.len().min(MAX_EDIT_TARGET_ROWS) + 3) as u16)
            .unwrap_or(0);
        list_rows + RESERVED_NON_COMMAND_ROWS as u16 + BORDER_ROWS + picker_rows
    }
}

impl CommandItem {
    pub fn accepts_no_arguments(&self) -> bool {
        self.usage.trim() == format!("/{}", self.name)
    }

    fn executes_on_exact_palette_match(&self, query: &str) -> bool {
        matches!(self.name.as_str(), "compact" | "plan") && query.eq_ignore_ascii_case(&self.name)
    }
}

impl Default for CommandPalette {
    fn default() -> Self {
        Self::new()
    }
}

/// Action to take when applying a command suggestion.
#[derive(Debug, Clone)]
pub enum CommandAction {
    /// Insert the command text into the prompt.
    Insert(String),
    /// Execute the command directly.
    Execute(String),
}
