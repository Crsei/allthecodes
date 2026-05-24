//! Persistent prompt-history reader for Ctrl+R.
//!
//! The in-memory prompt history is still the fast path while a TUI session is
//! running. This reader opportunistically seeds it from saved sessions when
//! backend session data exists, so Ctrl+R works before the first prompt.

use std::collections::HashSet;
use std::path::Path;

use anyhow::Result;

use crate::ui::history_search_dialog::HistorySearchEntry;
use allthecodes_types::message::{Message, MessageContent};

const MAX_PERSISTENT_HISTORY: usize = 200;

pub fn load_persistent_history_for_workspace(cwd: &Path) -> Result<Vec<HistorySearchEntry>> {
    let mut entries = Vec::new();
    let mut seen = HashSet::new();

    for session in allthecodes_session::storage::list_workspace_sessions(cwd)? {
        let messages = match allthecodes_session::storage::load_session(&session.session_id) {
            Ok(messages) => messages,
            Err(error) => {
                tracing::warn!(
                    session_id = %session.session_id,
                    error = %error,
                    "skipping unreadable session while loading prompt history"
                );
                continue;
            }
        };

        for message in messages.into_iter().rev() {
            let Some((text, timestamp)) = prompt_from_message(message) else {
                continue;
            };
            let text = text.trim().to_string();
            if text.is_empty() || !seen.insert(text.clone()) {
                continue;
            }
            let source_label = if session.workspace_name.is_empty() {
                session.cwd.clone()
            } else {
                session.workspace_name.clone()
            };
            entries.push(HistorySearchEntry::new(text, timestamp).with_source(
                session.session_id.clone(),
                session.title.clone(),
                source_label,
            ));
            if entries.len() >= MAX_PERSISTENT_HISTORY {
                return Ok(entries);
            }
        }
    }

    Ok(entries)
}

fn prompt_from_message(message: Message) -> Option<(String, i64)> {
    let Message::User(user) = message else {
        return None;
    };
    if user.is_meta {
        return None;
    }

    let text = match user.content {
        MessageContent::Text(text) => text,
        MessageContent::Blocks(blocks) => blocks
            .into_iter()
            .filter_map(|block| match block {
                allthecodes_types::message::ContentBlock::Text { text } => Some(text),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
    };

    Some((text, user.timestamp))
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_types::message::UserMessage;
    use uuid::Uuid;

    #[test]
    fn resume_history_reader_extracts_non_meta_user_prompts() {
        let message = Message::User(UserMessage {
            uuid: Uuid::new_v4(),
            timestamp: 123,
            role: "user".to_string(),
            content: MessageContent::Text("hello from saved session".to_string()),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        });

        assert_eq!(
            prompt_from_message(message),
            Some(("hello from saved session".to_string(), 123))
        );
    }

    #[test]
    fn resume_history_reader_skips_meta_messages() {
        let message = Message::User(UserMessage {
            uuid: Uuid::new_v4(),
            timestamp: 123,
            role: "user".to_string(),
            content: MessageContent::Text("hidden".to_string()),
            is_meta: true,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        });

        assert_eq!(prompt_from_message(message), None);
    }
}
