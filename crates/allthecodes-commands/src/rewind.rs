//! /rewind command -- rewind the conversation to an earlier user turn.
//!
//! Usage:
//! - `/rewind`                -- show a numbered list of user turns
//! - `/rewind list`           -- same as bare invocation
//! - `/rewind <n>`            -- rewind so the conversation keeps only the first `n` user turns
//! - `/rewind --to <uuid>`    -- rewind to the turn identified by a (prefix of a) user UUID
//!
//! Rewinding does two things: it trims the in-memory message buffer (which the
//! ingress layer mirrors back to the engine via `conversation_changed`), and
//! it truncates the on-disk session file via
//! [`allthecodes_session::storage::truncate_session`], which writes a recoverable
//! `*.rewind-<ts>.json` backup alongside the original.
//!
//! A "turn" here is anchored at a non-meta user message. Keeping `n` turns
//! means keeping every message up to (but not including) the (n+1)th user
//! anchor — i.e. we retain the assistant replies and tool traffic for the
//! turns we keep.

use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandHandler, CommandResult};
use allthecodes_session::storage;
use allthecodes_types::message::{Message, MessageContent};

/// Handler for the `/rewind` slash command.
pub struct RewindHandler;

#[async_trait]
impl CommandHandler for RewindHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let arg = args.trim();

        let anchors = collect_user_anchors(&ctx.messages);
        if anchors.is_empty() {
            return Ok(CommandResult::Output(
                "No user turns to rewind — the conversation is empty.".into(),
            ));
        }

        if arg.is_empty() || arg == "list" || arg == "--list" {
            return Ok(CommandResult::Output(format_turn_list(&anchors)));
        }

        // `--to <uuid-prefix>` lookup.
        if let Some(prefix) = arg.strip_prefix("--to").map(|s| s.trim()) {
            if prefix.is_empty() {
                return Ok(CommandResult::Output(
                    "Usage: /rewind --to <user-message-uuid-prefix>".into(),
                ));
            }
            let matches: Vec<&Anchor> = anchors
                .iter()
                .filter(|a| a.uuid.starts_with(prefix))
                .collect();
            return match matches.len() {
                0 => Ok(CommandResult::Output(format!(
                    "No user turn matches prefix '{}'. \
                     Run /rewind to see the numbered list.",
                    prefix
                ))),
                1 => rewind_to_turn(ctx, &anchors, matches[0].index),
                _ => {
                    let mut lines = vec![format!(
                        "Prefix '{}' is ambiguous — {} turns match:",
                        prefix,
                        matches.len()
                    )];
                    for m in matches.iter().take(10) {
                        lines.push(format!(
                            "  {}. {} — {}",
                            m.index,
                            short_uuid(&m.uuid),
                            m.preview
                        ));
                    }
                    Ok(CommandResult::Output(lines.join("\n")))
                }
            };
        }

        // Numeric `<n>` form.
        match arg.parse::<usize>() {
            Ok(0) => {
                ctx.messages.clear();
                let _ = storage::truncate_session(ctx.session_id.as_str(), 0);
                Ok(CommandResult::Output(
                    "Conversation rewound to the start (0 messages). Use /clear next time for \
                     a one-shot wipe."
                        .into(),
                ))
            }
            Ok(n) if n > anchors.len() => Ok(CommandResult::Output(format!(
                "Turn {} is out of range — there are only {} turns. \
                 Run /rewind to see the list.",
                n,
                anchors.len()
            ))),
            Ok(n) => rewind_to_turn(ctx, &anchors, n),
            Err(_) => Ok(CommandResult::Output(format!(
                "Unrecognized argument: '{}'.\n\n{}",
                arg,
                "Usage:\n  \
                   /rewind              -- show turn list\n  \
                   /rewind <n>          -- rewind to turn N\n  \
                   /rewind --to <uuid>  -- rewind to a specific user UUID prefix"
            ))),
        }
    }
}

/// Pointer to a single anchor user message in the history.
#[derive(Debug, Clone)]
struct Anchor {
    /// 1-based turn number as shown to the user.
    index: usize,
    /// Position of the anchor inside the full `messages` slice.
    message_index: usize,
    uuid: String,
    preview: String,
    timestamp: i64,
}

fn collect_user_anchors(messages: &[Message]) -> Vec<Anchor> {
    let mut anchors = Vec::new();
    for (i, msg) in messages.iter().enumerate() {
        let Message::User(u) = msg else { continue };
        if u.is_meta {
            continue;
        }
        // Skip synthetic user messages that wrap tool results — they are a
        // continuation of the assistant's turn, not a user input.
        if u.tool_use_result.is_some() || u.source_tool_assistant_uuid.is_some() {
            continue;
        }
        let preview = message_preview(&u.content);
        if preview.is_empty() {
            continue;
        }
        anchors.push(Anchor {
            index: anchors.len() + 1,
            message_index: i,
            uuid: u.uuid.to_string(),
            preview,
            timestamp: u.timestamp,
        });
    }
    anchors
}

fn message_preview(content: &MessageContent) -> String {
    let raw = match content {
        MessageContent::Text(t) => t.clone(),
        MessageContent::Blocks(blocks) => blocks
            .iter()
            .filter_map(|b| match b {
                allthecodes_types::message::ContentBlock::Text { text } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" "),
    };
    let first = raw
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim();
    const MAX: usize = 72;
    let trimmed: String = first.chars().take(MAX).collect();
    if first.chars().count() > MAX {
        format!("{}…", trimmed)
    } else {
        trimmed
    }
}

fn short_uuid(uuid: &str) -> String {
    uuid.chars().take(8).collect()
}

fn format_turn_list(anchors: &[Anchor]) -> String {
    let mut lines = Vec::new();
    lines.push(format!("Conversation turns ({}):", anchors.len()));
    lines.push(String::new());
    for a in anchors {
        let when = if a.timestamp > 0 {
            chrono::DateTime::from_timestamp(a.timestamp / 1000, 0)
                .map(|dt| dt.format("%H:%M:%S").to_string())
                .unwrap_or_else(|| "--:--:--".into())
        } else {
            "--:--:--".into()
        };
        lines.push(format!(
            "  {:>3}. [{}] {} — {}",
            a.index,
            short_uuid(&a.uuid),
            when,
            a.preview,
        ));
    }
    lines.push(String::new());
    lines.push("Use /rewind <n> or /rewind --to <uuid-prefix> to rewind.".into());
    lines.push("A backup of the pre-rewind session is written next to the session file.".into());
    lines.join("\n")
}

fn rewind_to_turn(
    ctx: &mut CommandContext,
    anchors: &[Anchor],
    turn: usize,
) -> Result<CommandResult> {
    // Keep everything strictly before the (turn+1)th anchor so the kept turns
    // retain their full assistant/tool traffic.
    let keep = if turn >= anchors.len() {
        ctx.messages.len()
    } else {
        anchors[turn].message_index
    };

    let before = ctx.messages.len();
    if keep >= before {
        return Ok(CommandResult::Output(format!(
            "Turn {} is already the last turn — nothing to rewind.",
            turn
        )));
    }

    ctx.messages.truncate(keep);

    // Mirror the truncation on disk. Missing session file is fine — the next
    // auto-save will create one from the truncated in-memory state.
    let session_path = storage::get_session_file(ctx.session_id.as_str());
    let backup_info = if session_path.exists() {
        match storage::truncate_session(ctx.session_id.as_str(), keep) {
            Ok(_) => " (disk session truncated, backup saved)",
            Err(_) => " (disk session unchanged — see logs)",
        }
    } else {
        " (in-memory only; session not yet persisted)"
    };

    Ok(CommandResult::Output(format!(
        "Rewound to turn {} of {} — kept {} messages (was {}).{}",
        turn,
        anchors.len(),
        keep,
        before,
        backup_info,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
    use allthecodes_types::message::{AssistantMessage, UserMessage};
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
            timestamp: 1_700_000_000_000,
            role: "user".into(),
            content: MessageContent::Text(text.into()),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        })
    }

    fn assistant(text: &str) -> Message {
        Message::Assistant(AssistantMessage {
            uuid: Uuid::new_v4(),
            timestamp: 1_700_000_000_000,
            role: "assistant".into(),
            content: vec![allthecodes_types::message::ContentBlock::Text { text: text.into() }],
            usage: None,
            stop_reason: Some("end_turn".into()),
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        })
    }

    fn tool_result_user(source_assistant: Uuid) -> Message {
        Message::User(UserMessage {
            uuid: Uuid::new_v4(),
            timestamp: 1_700_000_000_000,
            role: "user".into(),
            content: MessageContent::Text("tool_result".into()),
            is_meta: false,
            tool_use_result: Some("ok".into()),
            source_tool_assistant_uuid: Some(source_assistant),
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
    async fn test_rewind_empty_conversation() {
        let mut ctx = test_ctx("empty", Vec::new());
        let result = RewindHandler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(t) => {
                assert!(t.contains("No user turns"), "got: {}", t);
            }
            _ => panic!(),
        }
    }

    #[tokio::test]
    async fn test_rewind_list_numbers_turns() {
        let msgs = vec![
            user("first"),
            assistant("answer 1"),
            user("second"),
            assistant("answer 2"),
            user("third"),
        ];
        let mut ctx = test_ctx("list", msgs);
        let result = RewindHandler.execute("list", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(t) => {
                assert!(t.contains("Conversation turns (3)"));
                assert!(t.contains("1.") && t.contains("first"));
                assert!(t.contains("2.") && t.contains("second"));
                assert!(t.contains("3.") && t.contains("third"));
            }
            _ => panic!(),
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_rewind_to_turn_truncates_messages() {
        let temp = tempfile::tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        let msgs = vec![
            user("first"),
            assistant("answer 1"),
            user("second"),
            assistant("answer 2"),
            user("third"),
            assistant("answer 3"),
        ];
        let mut ctx = test_ctx("trunc", msgs);

        let result = RewindHandler.execute("2", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(t) => {
                assert!(t.contains("Rewound to turn 2"), "got: {}", t);
            }
            _ => panic!(),
        }

        // Kept: first user + answer 1 + second user + answer 2 == 4 messages.
        assert_eq!(ctx.messages.len(), 4);
        match &ctx.messages[2] {
            Message::User(u) => match &u.content {
                MessageContent::Text(s) => assert_eq!(s, "second"),
                _ => panic!(),
            },
            _ => panic!(),
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_rewind_zero_clears_everything() {
        let temp = tempfile::tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        let mut ctx = test_ctx("zero", vec![user("first"), assistant("a")]);
        let result = RewindHandler.execute("0", &mut ctx).await.unwrap();
        assert!(matches!(result, CommandResult::Output(_)));
        assert_eq!(ctx.messages.len(), 0);
    }

    #[tokio::test]
    async fn test_rewind_out_of_range() {
        let mut ctx = test_ctx("oor", vec![user("only")]);
        let result = RewindHandler.execute("99", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(t) => {
                assert!(t.contains("out of range"), "got: {}", t);
            }
            _ => panic!(),
        }
    }

    #[tokio::test]
    async fn test_rewind_invalid_arg() {
        let mut ctx = test_ctx("inv", vec![user("x")]);
        let result = RewindHandler.execute("xyz", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(t) => {
                assert!(t.contains("Unrecognized"), "got: {}", t);
            }
            _ => panic!(),
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_rewind_to_by_uuid_prefix() {
        let temp = tempfile::tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        let msgs = vec![
            user("first"),
            assistant("a1"),
            user("second"),
            assistant("a2"),
        ];
        // Grab the uuid of the *second* anchor so we know the prefix exists.
        let target_uuid = match &msgs[2] {
            Message::User(u) => u.uuid.to_string(),
            _ => unreachable!(),
        };
        let mut ctx = test_ctx("by_uuid", msgs);

        let prefix = &target_uuid[..6];
        let result = RewindHandler
            .execute(&format!("--to {}", prefix), &mut ctx)
            .await
            .unwrap();
        assert!(matches!(result, CommandResult::Output(_)));
        // We keep up to (turn 2), which means we keep turn 1 + turn 2 = 4 messages.
        assert_eq!(ctx.messages.len(), 4);
    }

    #[test]
    fn test_collect_user_anchors_skips_tool_results() {
        let assistant_msg = assistant("call tool");
        let assistant_uuid = match &assistant_msg {
            Message::Assistant(a) => a.uuid,
            _ => unreachable!(),
        };
        let msgs = vec![
            user("real"),
            assistant_msg,
            tool_result_user(assistant_uuid),
            user("real again"),
        ];
        let anchors = collect_user_anchors(&msgs);
        assert_eq!(anchors.len(), 2);
        assert_eq!(anchors[0].preview, "real");
        assert_eq!(anchors[1].preview, "real again");
    }

    #[test]
    fn test_collect_user_anchors_skips_meta() {
        let meta = Message::User(UserMessage {
            uuid: Uuid::new_v4(),
            timestamp: 0,
            role: "user".into(),
            content: MessageContent::Text("injected".into()),
            is_meta: true,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        });
        let anchors = collect_user_anchors(&[meta, user("real")]);
        assert_eq!(anchors.len(), 1);
        assert_eq!(anchors[0].preview, "real");
    }
}
