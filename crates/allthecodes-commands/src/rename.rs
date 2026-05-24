//! /rename command -- set or clear the custom title on the current session.
//!
//! Usage:
//! - `/rename`                -- show the current title
//! - `/rename <title>`        -- set a custom title (persisted on the session file)
//! - `/rename --auto`         -- adopt the auto-derived title as a pinned custom title
//! - `/rename --clear`        -- remove the custom title (falling back to auto-derived)
//!
//! The title is stored on the `SessionFile` via
//! [`allthecodes_session::storage::set_session_title`]. If the session has not been
//! persisted yet (no assistant turn has triggered an auto-save) we persist the
//! current message buffer first so the rename has a file to land on.

use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandHandler, CommandResult};
use allthecodes_session::storage;
use allthecodes_types::message::{Message, MessageContent};

/// Handler for the `/rename` slash command.
pub struct RenameHandler;

#[async_trait]
impl CommandHandler for RenameHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let arg = args.trim();
        let session_id = ctx.session_id.as_str();

        // Ensure there is a session file on disk before we try to mutate it.
        // In long-running REPLs auto-save has usually already produced one;
        // new sessions that have not yet round-tripped through the engine
        // need a first write here.
        ensure_persisted(ctx)?;

        match arg {
            "" => show_current_title(session_id),
            "--clear" | "-c" | "clear" => clear_title(session_id),
            "--auto" | "-a" | "auto" => auto_rename(ctx, session_id),
            other => set_title(session_id, other),
        }
    }
}

fn ensure_persisted(ctx: &CommandContext) -> Result<()> {
    let path = storage::get_session_file(ctx.session_id.as_str());
    if path.exists() {
        return Ok(());
    }
    storage::save_session(
        ctx.session_id.as_str(),
        &ctx.messages,
        &ctx.cwd.to_string_lossy(),
    )
}

fn show_current_title(session_id: &str) -> Result<CommandResult> {
    let info = storage::load_session_info(session_id)?;
    let body = match info.custom_title.as_deref() {
        Some(custom) => format!(
            "Current title (custom): {}\n\n\
             Use /rename <new title> to change it,\n\
             or /rename --clear to fall back to the auto-derived title.",
            custom
        ),
        None if info.title.is_empty() => "No title yet — run /rename <name> to set one.".into(),
        None => format!(
            "Current title (auto): {}\n\n\
             Use /rename <new title> to pin a custom title,\n\
             or /rename --auto to lock in the current auto-derived title.",
            info.title
        ),
    };
    Ok(CommandResult::Output(body))
}

fn set_title(session_id: &str, raw: &str) -> Result<CommandResult> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(CommandResult::Output(
            "Title cannot be empty. Use /rename --clear to remove a custom title.".into(),
        ));
    }

    match storage::set_session_title(session_id, Some(trimmed))? {
        Some(stored) => Ok(CommandResult::Output(format!(
            "Session title set to: {}",
            stored
        ))),
        None => Ok(CommandResult::Output(
            "Title was rejected (empty after trimming).".into(),
        )),
    }
}

fn clear_title(session_id: &str) -> Result<CommandResult> {
    let before = storage::load_session_info(session_id)?;
    if before.custom_title.is_none() {
        return Ok(CommandResult::Output(
            "No custom title to clear. The title is already auto-derived.".into(),
        ));
    }
    storage::set_session_title(session_id, None)?;
    let after = storage::load_session_info(session_id)?;
    let fallback = if after.title.is_empty() {
        "(no auto-derived title available yet)".into()
    } else {
        after.title
    };
    Ok(CommandResult::Output(format!(
        "Custom title cleared. Falling back to: {}",
        fallback
    )))
}

fn auto_rename(ctx: &CommandContext, session_id: &str) -> Result<CommandResult> {
    let candidate = derive_auto_title_from_messages(&ctx.messages);
    let on_disk = storage::load_session_info(session_id)?.title;
    let chosen = if !candidate.is_empty() {
        candidate
    } else if !on_disk.is_empty() {
        on_disk
    } else {
        return Ok(CommandResult::Output(
            "Cannot auto-generate a title — no user messages yet.".into(),
        ));
    };

    let stored = storage::set_session_title(session_id, Some(&chosen))?;
    Ok(CommandResult::Output(format!(
        "Session title pinned to auto-derived value: {}",
        stored.unwrap_or(chosen)
    )))
}

/// Pull a reasonable title from the in-memory message list.
///
/// Mirrors the behavior of [`allthecodes_session::storage`]'s derived title: first
/// non-meta user message, first non-empty line, truncated to 80 chars.
fn derive_auto_title_from_messages(messages: &[Message]) -> String {
    const MAX: usize = 80;
    for msg in messages {
        let Message::User(u) = msg else { continue };
        if u.is_meta {
            continue;
        }
        let text = match &u.content {
            MessageContent::Text(t) => t.clone(),
            MessageContent::Blocks(blocks) => blocks
                .iter()
                .filter_map(|b| match b {
                    allthecodes_types::message::ContentBlock::Text { text } => Some(text.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n"),
        };
        let first_line = text
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("")
            .trim();
        if first_line.is_empty() {
            continue;
        }
        let out: String = first_line.chars().take(MAX).collect();
        return if first_line.chars().count() > MAX {
            format!("{}…", out)
        } else {
            out
        };
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
    use allthecodes_types::message::{MessageContent, UserMessage};
    use std::path::{Path, PathBuf};
    use uuid::Uuid;

    struct HomeGuard {
        previous: Option<String>,
    }

    impl HomeGuard {
        fn set(path: &Path) -> Self {
            let previous = std::env::var("ALLTHECODES_HOME").ok();
            std::env::set_var("ALLTHECODES_HOME", path);
            Self { previous }
        }
    }

    impl Drop for HomeGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(v) => std::env::set_var("ALLTHECODES_HOME", v),
                None => std::env::remove_var("ALLTHECODES_HOME"),
            }
        }
    }

    fn user(text: &str) -> Message {
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

    fn test_ctx(id: &str, messages: Vec<Message>) -> CommandContext {
        CommandContext {
            messages,
            cwd: PathBuf::from("/proj"),
            app_state: Default::default(),
            session_id: SessionId::from_string(id),
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_rename_without_args_shows_current_title() {
        let temp = tempfile::tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        let mut ctx = test_ctx("r1", vec![user("first prompt")]);
        let result = RenameHandler.execute("", &mut ctx).await.unwrap();
        let text = match result {
            CommandResult::Output(t) => t,
            _ => panic!("expected Output"),
        };
        assert!(text.contains("first prompt"), "got: {}", text);
        assert!(text.contains("auto"), "got: {}", text);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_rename_sets_and_persists_custom_title() {
        let temp = tempfile::tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        let mut ctx = test_ctx("r2", vec![user("original prompt")]);
        let result = RenameHandler
            .execute("Feature Brainstorm", &mut ctx)
            .await
            .unwrap();
        assert!(matches!(result, CommandResult::Output(_)));

        let info = storage::load_session_info("r2").unwrap();
        assert_eq!(info.custom_title.as_deref(), Some("Feature Brainstorm"));
        assert_eq!(info.title, "Feature Brainstorm");
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_rename_clear_restores_auto_title() {
        let temp = tempfile::tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        let mut ctx = test_ctx("r3", vec![user("auto fallback")]);
        RenameHandler.execute("pinned", &mut ctx).await.unwrap();
        RenameHandler.execute("--clear", &mut ctx).await.unwrap();

        let info = storage::load_session_info("r3").unwrap();
        assert_eq!(info.custom_title, None);
        assert_eq!(info.title, "auto fallback");
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_rename_auto_pins_derived_title() {
        let temp = tempfile::tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        let mut ctx = test_ctx("r4", vec![user("pin me\nsecond line")]);
        RenameHandler.execute("--auto", &mut ctx).await.unwrap();

        let info = storage::load_session_info("r4").unwrap();
        assert_eq!(info.custom_title.as_deref(), Some("pin me"));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_rename_clear_without_custom_title() {
        let temp = tempfile::tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        let mut ctx = test_ctx("r5", vec![user("hi")]);
        let result = RenameHandler.execute("--clear", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(
                    text.to_lowercase().contains("no custom title"),
                    "got: {}",
                    text
                );
            }
            _ => panic!("expected Output"),
        }
    }

    #[test]
    fn test_derive_auto_title_truncates() {
        let long = "x".repeat(200);
        let title = derive_auto_title_from_messages(&[user(&long)]);
        assert!(title.ends_with("…"));
        assert!(title.chars().count() <= 81);
    }

    #[test]
    fn test_derive_auto_title_skips_meta() {
        let meta = Message::User(UserMessage {
            uuid: Uuid::new_v4(),
            timestamp: 0,
            role: "user".into(),
            content: MessageContent::Text("inject".into()),
            is_meta: true,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        });
        let title = derive_auto_title_from_messages(&[meta, user("real question")]);
        assert_eq!(title, "real question");
    }
}
