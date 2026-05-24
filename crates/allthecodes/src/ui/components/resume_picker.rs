//! Session resume picker data model.

use std::path::PathBuf;

use allthecodes_session::storage::SessionInfo;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionTarget {
    pub session_id: String,
    pub title: String,
    pub cwd: PathBuf,
    pub last_modified: i64,
    pub message_count: usize,
}

impl SessionTarget {
    pub fn from_info(info: SessionInfo) -> Self {
        Self {
            session_id: info.session_id,
            title: if info.title.trim().is_empty() {
                "(untitled session)".to_string()
            } else {
                info.title
            },
            cwd: PathBuf::from(info.cwd),
            last_modified: info.last_modified,
            message_count: info.message_count,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionSelection {
    Selected(SessionTarget),
    Cancelled,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionPickerAction {
    MoveUp,
    MoveDown,
    PageUp,
    PageDown,
    Select,
    Cancel,
}

impl SessionPickerAction {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Select | Self::Cancel)
    }
}

#[derive(Debug, Clone, Default)]
pub struct ResumePicker {
    sessions: Vec<SessionTarget>,
    selected: usize,
}

impl ResumePicker {
    pub fn new(mut sessions: Vec<SessionTarget>) -> Self {
        sessions.sort_by_key(|session| std::cmp::Reverse(session.last_modified));
        Self {
            sessions,
            selected: 0,
        }
    }
    pub fn from_session_info(sessions: Vec<SessionInfo>) -> Self {
        Self::new(sessions.into_iter().map(SessionTarget::from_info).collect())
    }

    pub fn selected(&self) -> Option<&SessionTarget> {
        self.sessions.get(self.selected)
    }
    pub fn sessions(&self) -> &[SessionTarget] {
        &self.sessions
    }
    pub fn apply(
        &mut self,
        action: SessionPickerAction,
        page_size: usize,
    ) -> Option<SessionSelection> {
        match action {
            SessionPickerAction::MoveUp => self.selected = self.selected.saturating_sub(1),
            SessionPickerAction::MoveDown => {
                if !self.sessions.is_empty() {
                    self.selected = (self.selected + 1).min(self.sessions.len() - 1);
                }
            }
            SessionPickerAction::PageUp => self.selected = self.selected.saturating_sub(page_size),
            SessionPickerAction::PageDown => {
                if !self.sessions.is_empty() {
                    self.selected = self
                        .selected
                        .saturating_add(page_size)
                        .min(self.sessions.len() - 1);
                }
            }
            SessionPickerAction::Select => {
                return self.selected().cloned().map(SessionSelection::Selected);
            }
            SessionPickerAction::Cancel => return Some(SessionSelection::Cancelled),
        }
        None
    }
}
pub fn load_resume_targets() -> anyhow::Result<Vec<SessionTarget>> {
    Ok(allthecodes_session::storage::list_sessions()?
        .into_iter()
        .map(SessionTarget::from_info)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_newest_first() {
        let picker = ResumePicker::new(vec![
            SessionTarget {
                session_id: "old".into(),
                title: "old".into(),
                cwd: ".".into(),
                last_modified: 1,
                message_count: 0,
            },
            SessionTarget {
                session_id: "new".into(),
                title: "new".into(),
                cwd: ".".into(),
                last_modified: 2,
                message_count: 0,
            },
        ]);
        assert_eq!(picker.selected().unwrap().session_id, "new");
    }

    #[test]
    fn session_info_picker_applies_navigation_actions() {
        let info = SessionInfo {
            session_id: "s1".to_string(),
            created_at: 1,
            last_modified: 10,
            message_count: 4,
            cwd: "/repo".to_string(),
            title: "".to_string(),
            custom_title: None,
            workspace_key: "repo".to_string(),
            workspace_root: "/repo".to_string(),
            workspace_name: "repo".to_string(),
        };
        let mut picker = ResumePicker::from_session_info(vec![info]);

        assert_eq!(picker.sessions().len(), 1);
        assert_eq!(picker.selected().unwrap().title, "(untitled session)");
        assert!(SessionPickerAction::Select.is_terminal());
        assert!(!SessionPickerAction::MoveDown.is_terminal());
        assert!(picker.apply(SessionPickerAction::MoveUp, 10).is_none());
        assert!(picker.apply(SessionPickerAction::PageUp, 10).is_none());
        assert!(picker.apply(SessionPickerAction::PageDown, 10).is_none());
        assert!(matches!(
            picker.apply(SessionPickerAction::Select, 10),
            Some(SessionSelection::Selected(_))
        ));
        assert_eq!(
            picker.apply(SessionPickerAction::Cancel, 10),
            Some(SessionSelection::Cancelled)
        );

        let _loader: fn() -> anyhow::Result<Vec<SessionTarget>> = load_resume_targets;
    }
}
