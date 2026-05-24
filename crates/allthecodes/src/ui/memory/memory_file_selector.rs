//! Memory file selector model.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryFileKind {
    User,
    Project,
    Nested,
    Folder,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryFileOption {
    pub path: PathBuf,
    pub kind: MemoryFileKind,
    pub exists: bool,
    pub parent: Option<PathBuf>,
    pub description: String,
}

impl MemoryFileOption {
    pub fn new(path: impl Into<PathBuf>, kind: MemoryFileKind) -> Self {
        Self {
            path: path.into(),
            kind,
            exists: true,
            parent: None,
            description: String::new(),
        }
    }

    pub fn missing(mut self) -> Self {
        self.exists = false;
        self
    }

    pub fn with_parent(mut self, parent: impl Into<PathBuf>) -> Self {
        self.parent = Some(parent.into());
        self
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryFileSelectorState {
    pub options: Vec<MemoryFileOption>,
    pub selected_index: usize,
}

impl MemoryFileSelectorState {
    pub fn new(options: Vec<MemoryFileOption>) -> Self {
        Self {
            options,
            selected_index: 0,
        }
    }
    pub fn selected_path(&self) -> Option<&Path> {
        self.options
            .get(self.selected_index)
            .map(|option| option.path.as_path())
    }

    pub fn move_next(&mut self) {
        if self.options.is_empty() {
            return;
        }
        self.selected_index = (self.selected_index + 1) % self.options.len();
    }

    pub fn move_prev(&mut self) {
        if self.options.is_empty() {
            return;
        }
        self.selected_index = if self.selected_index == 0 {
            self.options.len() - 1
        } else {
            self.selected_index - 1
        };
    }

    pub fn render(&self, cwd: &Path, home: &Path) -> String {
        self.options
            .iter()
            .enumerate()
            .map(|(idx, option)| {
                let marker = if idx == self.selected_index { ">" } else { " " };
                let exists = if option.exists { "" } else { " (new)" };
                let label = option_label(option, cwd, home);
                let description = if option.description.is_empty() {
                    default_description(option)
                } else {
                    option.description.clone()
                };
                format!("{marker} {label}{exists} - {description}")
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

pub fn relative_memory_path(path: &Path, cwd: &Path, home: &Path) -> String {
    let path_s = normalize_path(path);
    let cwd_s = normalize_path(cwd);
    let home_s = normalize_path(home);

    let home_relative = path_s
        .strip_prefix(&(home_s.clone() + "/"))
        .map(|rest| format!("~/{rest}"));
    let cwd_relative = path_s
        .strip_prefix(&(cwd_s.clone() + "/"))
        .map(|rest| format!("./{rest}"));

    match (home_relative, cwd_relative) {
        (Some(home), Some(cwd)) => {
            if home.len() <= cwd.len() {
                home
            } else {
                cwd
            }
        }
        (Some(home), None) => home,
        (None, Some(cwd)) => cwd,
        (None, None) => path_s,
    }
}

fn option_label(option: &MemoryFileOption, cwd: &Path, home: &Path) -> String {
    match option.kind {
        MemoryFileKind::User => "User memory".to_string(),
        MemoryFileKind::Project => "Project memory".to_string(),
        MemoryFileKind::Folder => format!("Open {}", relative_memory_path(&option.path, cwd, home)),
        MemoryFileKind::Nested => format!("L {}", relative_memory_path(&option.path, cwd, home)),
    }
}

fn default_description(option: &MemoryFileOption) -> String {
    match option.kind {
        MemoryFileKind::User => "Saved in ~/.allthecodes/AGENTS.md".to_string(),
        MemoryFileKind::Project => "Saved in ./AGENTS.md".to_string(),
        MemoryFileKind::Nested => "@-imported".to_string(),
        MemoryFileKind::Folder => "folder".to_string(),
    }
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_path_tracks_selected_index() {
        let mut state = MemoryFileSelectorState::new(vec![
            MemoryFileOption::new("/repo/AGENTS.md", MemoryFileKind::Project),
            MemoryFileOption::new("/home/me/.allthecodes/AGENTS.md", MemoryFileKind::User),
        ]);

        assert_eq!(state.selected_path(), Some(Path::new("/repo/AGENTS.md")));
        state.move_next();
        assert_eq!(
            state.selected_path(),
            Some(Path::new("/home/me/.allthecodes/AGENTS.md"))
        );
    }
}
