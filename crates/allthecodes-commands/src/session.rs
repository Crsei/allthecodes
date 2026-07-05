//! /session command -- list and show session information.
//!
//! Subcommands:
//! - `/session`              -- show current session info + recent workspace sessions
//! - `/session list`         -- list saved sessions for the current workspace
//! - `/session list all`     -- list saved sessions across all workspaces
//!
//! The TypeScript version shows a remote session QR code via React.
//! In the Rust CLI we show a text listing of saved sessions instead.

use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandHandler, CommandResult};
use allthecodes_session::storage;

/// Handler for the `/session` slash command.
pub struct SessionHandler;

#[async_trait]
impl CommandHandler for SessionHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let parts: Vec<&str> = args.split_whitespace().collect();

        match parts.as_slice() {
            [] => handle_show(ctx),
            ["list"] | ["ls"] => handle_list(ctx, false),
            ["list", "all"] | ["ls", "all"] => handle_list(ctx, true),
            ["search", rest @ ..] | ["find", rest @ ..] => handle_search(ctx, rest),
            [sub, ..] => Ok(CommandResult::Output(format!(
                "Unknown session subcommand: '{}'\n\
                 Usage:\n  \
                   /session              -- show current session + recent workspace history\n  \
                   /session list         -- list saved sessions for this workspace\n  \
                   /session list all     -- list saved sessions from all workspaces\n  \
                   /session search <q>   -- search saved sessions in this workspace\n  \
                   /session search all <q> -- search saved sessions from all workspaces",
                sub
            ))),
        }
    }
}

fn format_session_table(
    sessions: &[storage::SessionInfo],
    limit: usize,
    title: &str,
    current_session_id: Option<&str>,
) -> Vec<String> {
    let visible: Vec<_> = sessions
        .iter()
        .filter(|session| current_session_id != Some(session.session_id.as_str()))
        .take(limit)
        .collect();

    let mut lines = Vec::new();
    lines.push(format!("{} ({}):", title, visible.len()));
    lines.push(String::new());
    lines.push(format!(
        "  {:<38} {:>6}  {:<20}  {}",
        "Session ID", "Msgs", "Last Modified", "Title / Directory"
    ));
    lines.push(format!(
        "  {:<38} {:>6}  {:<20}  {}",
        "----------", "----", "-------------", "-----------------"
    ));

    for session in visible {
        let ts = chrono::DateTime::from_timestamp(session.last_modified, 0)
            .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_else(|| "unknown".into());

        let label = if !session.title.is_empty() {
            let prefix = if session.custom_title.is_some() {
                "* "
            } else {
                ""
            };
            let max = 60;
            let truncated: String = session.title.chars().take(max).collect();
            if session.title.chars().count() > max {
                format!("{}{}...", prefix, truncated)
            } else {
                format!("{}{}", prefix, truncated)
            }
        } else if session.cwd.len() > 60 {
            format!("...{}", &session.cwd[session.cwd.len() - 57..])
        } else {
            session.cwd.clone()
        };

        lines.push(format!(
            "  {:<38} {:>6}  {:<20}  {}",
            session.session_id, session.message_count, ts, label
        ));
    }

    lines
}

/// Show information about the current session.
fn handle_show(ctx: &CommandContext) -> Result<CommandResult> {
    let msg_count = ctx.messages.len();
    let cwd = ctx.cwd.display();
    let previous_sessions: Vec<_> = storage::list_workspace_sessions(&ctx.cwd)?
        .into_iter()
        .filter(|session| session.session_id != ctx.session_id.as_str())
        .collect();

    let mut lines = Vec::new();
    lines.push("Current session:".into());
    lines.push(String::new());
    lines.push(format!("  Session ID:        {}", ctx.session_id));
    // Show the persisted title if we already have a session file; skip the
    // disk read otherwise so freshly-started sessions don't pay for it.
    if storage::get_session_file(ctx.session_id.as_str()).exists() {
        if let Ok(info) = storage::load_session_info(ctx.session_id.as_str()) {
            if let Some(custom) = info.custom_title.as_deref() {
                lines.push(format!("  Title (custom):    {}", custom));
            } else if !info.title.is_empty() {
                lines.push(format!("  Title (auto):      {}", info.title));
            }
        }
    }
    lines.push(format!("  Working directory: {}", cwd));
    lines.push(format!("  Messages:          {}", msg_count));
    lines.push(format!(
        "  Model:             {}",
        ctx.app_state.main_loop_model
    ));

    lines.push(String::new());

    if previous_sessions.is_empty() {
        lines.push("No previous sessions found for this workspace.".into());
        lines.push("Use /session list all to inspect sessions from other workspaces.".into());
    } else {
        lines.extend(format_session_table(
            &previous_sessions,
            10,
            "Recent workspace sessions",
            None,
        ));

        if previous_sessions.len() > 10 {
            lines.push(format!(
                "\n  ... and {} more workspace sessions",
                previous_sessions.len() - 10
            ));
        }

        lines.push(String::new());
        lines.push("Use /resume <session_id> to load one of these sessions.".into());
    }

    Ok(CommandResult::Output(lines.join("\n")))
}

/// List saved sessions from disk.
fn handle_list(ctx: &CommandContext, include_all: bool) -> Result<CommandResult> {
    let sessions = if include_all {
        storage::list_sessions()?
    } else {
        storage::list_workspace_sessions(&ctx.cwd)?
    };

    if sessions.is_empty() {
        let text = if include_all {
            "No saved sessions found.".into()
        } else {
            "No saved sessions found for this workspace.\nUse /session list all to inspect other workspaces.".into()
        };
        return Ok(CommandResult::Output(text));
    }

    let title = if include_all {
        "Saved sessions (all workspaces)"
    } else {
        "Saved sessions (current workspace)"
    };
    let mut lines = format_session_table(&sessions, 20, title, None);

    if sessions.len() > 20 {
        lines.push(format!("\n  ... and {} more sessions", sessions.len() - 20));
    }

    lines.push(String::new());
    lines.push("Use /resume <session_id> to resume a session.".into());

    Ok(CommandResult::Output(lines.join("\n")))
}

fn handle_search(ctx: &CommandContext, parts: &[&str]) -> Result<CommandResult> {
    let (include_all, query_parts) = match parts {
        ["all", rest @ ..] => (true, rest),
        _ => (false, parts),
    };
    let query = query_parts.join(" ");
    if query.trim().is_empty() {
        return Ok(CommandResult::Output(
            "Usage: /session search <query>\n       /session search all <query>".into(),
        ));
    }

    let hits = if include_all {
        storage::search_sessions(&query, 20)?
    } else {
        storage::search_workspace_sessions(&ctx.cwd, &query, 20)?
    };

    Ok(CommandResult::Output(format_session_search_hits(
        &hits,
        include_all,
    )))
}

fn format_session_search_hits(hits: &[storage::SessionSearchResult], include_all: bool) -> String {
    let scope = if include_all {
        "all workspaces"
    } else {
        "current workspace"
    };
    if hits.is_empty() {
        return format!("No session search results found ({scope}).");
    }

    let mut lines = Vec::new();
    lines.push(format!("Session search results ({scope}):"));
    lines.push(String::new());
    lines.push(format!(
        "  {:<38} {:>3}  {:<10}  {}",
        "Session ID", "Msg", "Role", "Title"
    ));
    lines.push(format!(
        "  {:<38} {:>3}  {:<10}  {}",
        "----------", "---", "----", "-----"
    ));

    for hit in hits {
        let role = hit.role.as_deref().unwrap_or("-");
        lines.push(format!(
            "  {:<38} {:>3}  {:<10}  {}",
            hit.session_id,
            hit.message_index,
            role,
            truncate_chars(&hit.title, 60)
        ));
        if !hit.snippet.is_empty() {
            lines.push(format!("      {}", hit.snippet));
        }
        lines.push(String::new());
    }

    if hits.len() == 1 {
        lines.push(format!(
            "Use /resume {} to load a result.",
            hits[0].session_id
        ));
    } else {
        lines.push("Use /resume <session_id> to load a result.".into());
    }
    lines.join("\n")
}

fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    format!("{}...", text.chars().take(max).collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
    use allthecodes_engine::types::app_state::AppState;
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
            cwd: PathBuf::from("/test/project"),
            app_state: AppState::default(),
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
    async fn test_session_show() {
        let handler = SessionHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Current session"));
                assert!(text.contains("Session ID"));
                assert!(text.contains("Working directory"));
                assert!(text.contains("Messages"));
            }
            _ => panic!("Expected Output result"),
        }
    }

    #[tokio::test]
    async fn test_session_unknown_subcommand() {
        let handler = SessionHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("foobar", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Unknown session subcommand"));
            }
            _ => panic!("Expected Output result"),
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_session_search_workspace_outputs_hits() {
        let home = tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let workspace = home.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();

        storage::save_session(
            "session-search-command",
            &[user_message("hermes style session search")],
            workspace.to_str().unwrap(),
        )
        .unwrap();

        let handler = SessionHandler;
        let mut ctx = test_ctx();
        ctx.cwd = workspace;

        let result = handler
            .execute("search hermes style", &mut ctx)
            .await
            .unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("session-search-command"));
                assert!(text.contains("hermes style"));
                assert!(text.contains("Use /resume session-search-command"));
            }
            _ => panic!("expected Output"),
        }
    }
}
