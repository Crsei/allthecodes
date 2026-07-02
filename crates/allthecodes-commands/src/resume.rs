//! /resume command -- resume a previous session.
//!
//! Usage:
//! - `/resume`            -- load the most recent session for the current workspace
//! - `/resume <id>`       -- load a specific session by ID (or prefix)
//!
//! The TypeScript version uses an interactive React-based log selector.
//! In the Rust CLI we accept a session ID argument directly.

use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandHandler, CommandResult};
use allthecodes_session::resume as session_resume;
use allthecodes_session::storage;

/// Handler for the `/resume` slash command.
pub struct ResumeHandler;

#[async_trait]
impl CommandHandler for ResumeHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let target = args.trim();

        if target.is_empty() || target.eq_ignore_ascii_case("recent") {
            // Try to resume the most recent session for the current directory.
            return handle_resume_latest(ctx);
        }

        // Try exact match first, then prefix match.
        handle_resume_by_id(target, ctx)
    }
}

/// Resume the most recent session matching the current working directory.
fn handle_resume_latest(ctx: &mut CommandContext) -> Result<CommandResult> {
    let session = storage::list_workspace_sessions(&ctx.cwd)?
        .into_iter()
        .find(|session| session.session_id != ctx.session_id.as_str());

    match session {
        Some(info) => resume_session_by_id(&info.session_id, ctx),
        None => Ok(CommandResult::Output(format!(
            "No previous session found for workspace: {}",
            ctx.cwd.display()
        ))),
    }
}

/// Resume a session by ID or prefix.
fn handle_resume_by_id(target: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
    let workspace_sessions = storage::list_workspace_sessions(&ctx.cwd)?;
    let all_sessions = storage::list_sessions()?;

    // Exact match, preferring the current workspace first.
    if let Some(session) = workspace_sessions.iter().find(|s| s.session_id == target) {
        return resume_session_by_id(&session.session_id, ctx);
    }
    if let Some(session) = all_sessions.iter().find(|s| s.session_id == target) {
        return resume_session_by_id(&session.session_id, ctx);
    }

    // Prefix match, preferring the current workspace first.
    let workspace_matches: Vec<_> = workspace_sessions
        .iter()
        .filter(|s| s.session_id.starts_with(target))
        .collect();
    let matches: Vec<_> = if workspace_matches.is_empty() {
        all_sessions
            .iter()
            .filter(|s| s.session_id.starts_with(target))
            .collect()
    } else {
        workspace_matches
    };

    match matches.len() {
        0 => Ok(CommandResult::Output(format!(
            "Session '{}' was not found.\n\
             Use /session or /session list to see available sessions.",
            target
        ))),
        1 => resume_session_by_id(&matches[0].session_id, ctx),
        n => {
            let mut lines = Vec::new();
            lines.push(format!(
                "Found {} sessions matching '{}'. Please be more specific:",
                n, target
            ));
            lines.push(String::new());
            for s in matches.iter().take(10) {
                let ts = chrono::DateTime::from_timestamp(s.last_modified, 0)
                    .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_else(|| "unknown".into());
                lines.push(format!(
                    "  {} ({} msgs, {})",
                    s.session_id, s.message_count, ts
                ));
            }
            Ok(CommandResult::Output(lines.join("\n")))
        }
    }
}

/// Load a session's messages into the command context.
fn resume_session_by_id(session_id: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
    let resumed = session_resume::resume_session_detail(session_id)?;
    let messages = resumed.messages;
    if messages.is_empty() {
        return Ok(CommandResult::Output(format!(
            "Session {} has no saved conversation messages. Start a new prompt or use /session list to choose another session.",
            session_id
        )));
    }

    let msg_count = messages.len();
    if let Some(team_context) = crate::runtime::team_context_for_session(session_id) {
        ctx.app_state.team_context = Some(team_context);
    } else {
        ctx.app_state.team_context = None;
    }

    let mut notice = format!(
        "Loaded history from session {} ({} messages) into the current conversation.",
        session_id, msg_count,
    );
    if let Some(last_seq) = resumed.last_seq {
        notice.push_str(&format!("\nReplay log last seq: {last_seq}."));
    }
    if !resumed.replay_warnings.is_empty() {
        notice.push_str(&format!(
            "\nReplay produced {} warning(s); see debug logs for details.",
            resumed.replay_warnings.len()
        ));
    }
    if !resumed.pending_interactions.is_empty() {
        notice.push_str(&format!(
            "\n{} pending permission/question interaction(s) were marked interrupted and will not be replayed.",
            resumed.pending_interactions.len()
        ));
    }

    Ok(CommandResult::SwitchSession {
        session_id: allthecodes_bootstrap::SessionId::from_string(session_id),
        messages,
        notice,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
    use allthecodes_types::message::{Message, MessageContent, UserMessage};
    use std::path::PathBuf;
    use tempfile::tempdir;
    use uuid::Uuid;

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &std::path::Path) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    fn test_ctx() -> CommandContext {
        CommandContext {
            messages: Vec::new(),
            cwd: PathBuf::from("/nonexistent/test/path"),
            app_state: Default::default(),
            session_id: SessionId::from_string("test-session"),
        }
    }

    fn user_message(text: &str) -> Message {
        Message::User(UserMessage {
            uuid: Uuid::new_v4(),
            timestamp: 0,
            role: "user".into(),
            content: MessageContent::Text(text.into()),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        })
    }

    #[tokio::test]
    async fn test_resume_no_session() {
        let handler = ResumeHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(
                    text.contains("No previous session")
                        || text.contains("not found")
                        || text.contains("session")
                );
            }
            _ => panic!("Expected Output result"),
        }
    }

    #[tokio::test]
    async fn test_resume_nonexistent_id() {
        let handler = ResumeHandler;
        let mut ctx = test_ctx();
        let result = handler
            .execute("nonexistent-id-12345", &mut ctx)
            .await
            .unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("not found"));
            }
            _ => panic!("Expected Output result"),
        }
    }

    #[tokio::test]
    async fn test_resume_recent_alias_uses_latest_flow() {
        let handler = ResumeHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("recent", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(
                    text.contains("No previous session found"),
                    "recent should use latest-session flow, got: {text}"
                );
            }
            _ => panic!("expected Output"),
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_resume_empty_session_is_friendly() {
        let home = tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let workspace = home.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        storage::save_session("empty-session", &[], workspace.to_str().unwrap()).unwrap();

        let handler = ResumeHandler;
        let mut ctx = test_ctx();
        ctx.cwd = workspace;

        let result = handler.execute("empty-session", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("no saved conversation messages"));
                assert!(ctx.messages.is_empty());
            }
            _ => panic!("Expected Output result"),
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_resume_nonempty_session_switches_runtime_session() {
        let home = tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let workspace = home.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        storage::save_session(
            "resume-session",
            &[user_message("hello")],
            workspace.to_str().unwrap(),
        )
        .unwrap();

        let handler = ResumeHandler;
        let mut ctx = test_ctx();
        ctx.cwd = workspace;
        ctx.app_state.team_context = Some(allthecodes_types::teams::TeamContext {
            team_name: "stale-team".into(),
            ..Default::default()
        });

        let result = handler.execute("resume-session", &mut ctx).await.unwrap();
        match result {
            CommandResult::SwitchSession {
                session_id,
                messages,
                notice,
            } => {
                assert_eq!(session_id.as_str(), "resume-session");
                assert_eq!(messages.len(), 1);
                assert!(notice.contains("Loaded history"));
                assert!(ctx.app_state.team_context.is_none());
            }
            _ => panic!("Expected SwitchSession result"),
        }
    }
}
