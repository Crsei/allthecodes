use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent};

use crate::ui::command_surface::{cycle_index, CommandSurfaceOutcome};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumeSurface {
    pub(crate) sessions: Vec<ResumeSessionItem>,
    pub(crate) selected_index: usize,
    pub(crate) error: Option<String>,
    pub(crate) cwd: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResumeSessionItem {
    pub(crate) session_id: String,
    pub(crate) title: String,
    pub(crate) cwd: String,
    pub(crate) last_modified: i64,
    pub(crate) message_count: usize,
}

impl ResumeSurface {
    pub(crate) fn new(cwd: &Path) -> Self {
        match allthecodes_session::storage::list_workspace_sessions(cwd) {
            Ok(sessions) => Self {
                sessions: sessions
                    .into_iter()
                    .map(|session| ResumeSessionItem {
                        session_id: session.session_id,
                        title: if session.title.trim().is_empty() {
                            "(untitled session)".to_string()
                        } else {
                            session.title
                        },
                        cwd: session.cwd,
                        last_modified: session.last_modified,
                        message_count: session.message_count,
                    })
                    .collect(),
                selected_index: 0,
                error: None,
                cwd: cwd.to_path_buf(),
            },
            Err(error) => Self {
                sessions: Vec::new(),
                selected_index: 0,
                error: Some(error.to_string()),
                cwd: cwd.to_path_buf(),
            },
        }
    }

    pub(crate) fn render(&self) -> String {
        if let Some(error) = &self.error {
            return format!("Resume Sessions\nFailed to list saved sessions: {error}\n\nEsc close");
        }

        let mut lines = vec![
            "Resume Sessions".to_string(),
            format!("Workspace: {}", self.cwd.display()),
            String::new(),
        ];

        if self.sessions.is_empty() {
            lines.push("No saved sessions for this workspace.".to_string());
            lines.push("Use /session list to inspect all saved sessions.".to_string());
            lines.push(String::new());
            lines.push("Esc close".to_string());
            return lines.join("\n");
        }

        for (idx, session) in self.sessions.iter().take(12).enumerate() {
            let marker = if idx == self.selected_index { ">" } else { " " };
            lines.push(format!(
                "{marker} {}  {} msg(s)  {}",
                short_id(&session.session_id),
                session.message_count,
                format_timestamp(session.last_modified),
            ));
            lines.push(format!("  {}", truncate(&session.title, 72)));
        }

        if self.sessions.len() > 12 {
            lines.push(format!(
                "  ... {} more session(s); use /session list for the full list",
                self.sessions.len() - 12
            ));
        }

        lines.push(String::new());
        lines.push("Up/Down select | Enter resume | r resume recent | Esc close".to_string());
        lines.join("\n")
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> CommandSurfaceOutcome {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected_index = cycle_index(self.selected_index, self.sessions.len(), -1);
                CommandSurfaceOutcome::None
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                self.selected_index = cycle_index(self.selected_index, self.sessions.len(), 1);
                CommandSurfaceOutcome::None
            }
            KeyCode::Enter => self
                .sessions
                .get(self.selected_index)
                .map(|session| {
                    CommandSurfaceOutcome::Submit(format!("/resume {}", session.session_id))
                })
                .unwrap_or(CommandSurfaceOutcome::None),
            KeyCode::Char('r') => CommandSurfaceOutcome::Submit("/resume recent".to_string()),
            _ => CommandSurfaceOutcome::None,
        }
    }
}

fn short_id(session_id: &str) -> &str {
    session_id.get(..8).unwrap_or(session_id)
}

fn format_timestamp(timestamp: i64) -> String {
    chrono::DateTime::from_timestamp(timestamp, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn truncate(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut out: String = value.chars().take(max_chars.saturating_sub(3)).collect();
    out.push_str("...");
    out
}
