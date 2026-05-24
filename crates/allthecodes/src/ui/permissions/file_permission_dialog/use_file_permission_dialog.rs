//! File permission dialog state transitions.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePermissionDialogState {
    pub path: String,
    pub selected_index: usize,
    pub show_diff: bool,
    pub persistent_rule: bool,
}

impl FilePermissionDialogState {
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            selected_index: 0,
            show_diff: true,
            persistent_rule: false,
        }
    }

    pub fn select_next(&mut self, option_count: usize) {
        if option_count > 0 {
            self.selected_index = (self.selected_index + 1) % option_count;
        }
    }
}
