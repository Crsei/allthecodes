//! `/branch` command — fork the current conversation (transcript-level).
//!
//! Issue #36: align `/branch` with the TypeScript reference behavior. This is
//! **not** a git-checkout wrapper — that's `/gbranch`. Instead, `/branch`
//! produces a copy of the current transcript under a freshly allocated
//! session ID, preserving message UUIDs/content while rewriting the envelope
//! `session_id`, and writes a `session_header` record carrying
//! `forked_from` + `forked_at_uuid` metadata.
//!
//! Usage:
//! - `/branch`            — fork the current conversation at the latest message
//!
//! The command returns a session-switch result so runtime hosts can land the
//! user directly in the fork.
//!
//! To use the git-branch wrapper, run `/gbranch` or `/gitbranch`.

use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandHandler, CommandResult};
use allthecodes_bootstrap::SessionId;
use allthecodes_session::fork as session_fork;

pub struct BranchHandler;

#[async_trait]
impl CommandHandler for BranchHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        // Currently /branch takes no arguments. Accept-and-ignore any extra
        // input so users who habitually append context don't see a cryptic
        // error, but warn them about `/gbranch` for the git case.
        let args = args.trim();
        if !args.is_empty() {
            return Ok(CommandResult::Output(format!(
                "/branch takes no arguments and forks the current conversation.\n\
                 Did you mean `/gbranch {}` (git branch wrapper)?",
                args
            )));
        }

        let parent_session_id = ctx.session_id.as_str().to_string();
        let new_session_id = SessionId::new();
        let cwd = ctx.cwd.to_string_lossy().to_string();

        let outcome = session_fork::fork_session(
            &parent_session_id,
            new_session_id.as_str(),
            &ctx.messages,
            &cwd,
            None,
        )?;

        let parent_session_id = outcome.parent_session_id.clone();
        let new_session_id_text = outcome.new_session_id.clone();
        let binary = current_binary_name();
        let lines = [
            format!(
                "Branched conversation. You are now in the new branch (session {}).",
                new_session_id_text
            ),
            format!(
                "Use /resume {} to return to the original conversation.",
                parent_session_id
            ),
            format!("From a terminal, run: {} -r {}", binary, parent_session_id),
        ];

        ctx.session_id = new_session_id.clone();

        Ok(CommandResult::SwitchSession {
            session_id: new_session_id,
            messages: ctx.messages.clone(),
            notice: lines.join("\n"),
        })
    }
}

#[cfg(test)]
fn short_id(id: &str) -> String {
    id.chars().take(8).collect()
}

fn current_binary_name() -> String {
    std::env::args()
        .next()
        .and_then(|arg| {
            std::path::Path::new(&arg)
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_string)
        })
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "allthecodes".to_string())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_engine::types::app_state::AppState;
    use allthecodes_session::{storage, transcript};
    use allthecodes_types::message::{Message, MessageContent, UserMessage};
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

    fn user_msg(text: &str) -> Message {
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

    fn seed_parent(session_id: &str, messages: &[Message], cwd: &str) {
        // Write a matching transcript on disk.
        let dir = transcript::get_transcript_dir();
        std::fs::create_dir_all(&dir).unwrap();
        let path = transcript::get_transcript_file(session_id);
        let mut buf = String::new();
        for (i, m) in messages.iter().enumerate() {
            let entry = serde_json::json!({
                "timestamp": 1_700_000_000_000_i64 + i as i64,
                "session_id": session_id,
                "msg_type": "user",
                "uuid": m.uuid().to_string(),
                "payload": { "text": format!("msg {}", i) }
            });
            buf.push_str(&serde_json::to_string(&entry).unwrap());
            buf.push('\n');
        }
        std::fs::write(&path, buf).unwrap();
        // And a session file so load_session_info works during title derivation.
        storage::save_session(session_id, messages, cwd).unwrap();
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_branch_forks_current_conversation() {
        let temp = tempfile::tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        let messages = vec![user_msg("first prompt"), user_msg("second prompt")];
        let parent_id = "parent-branch-test";
        seed_parent(parent_id, &messages, "/proj");

        let mut ctx = CommandContext {
            messages: messages.clone(),
            cwd: PathBuf::from("/proj"),
            app_state: AppState::default(),
            session_id: SessionId::from_string(parent_id),
        };

        let result = BranchHandler.execute("", &mut ctx).await.unwrap();
        let (session_id, messages, text) = match result {
            CommandResult::SwitchSession {
                session_id,
                messages,
                notice,
            } => (session_id, messages, notice),
            _ => panic!("expected SwitchSession"),
        };

        assert!(text.starts_with("Branched conversation."), "got: {}", text);
        assert!(text.contains("You are now in the new branch"));
        assert!(
            text.contains("-r"),
            "terminal resume command missing: {text}"
        );
        assert!(
            !text.contains("Forked session ->"),
            "internal fork details leaked: {text}"
        );
        assert!(text.contains(parent_id), "parent id missing: {}", text);
        assert_eq!(session_id, ctx.session_id);
        assert_eq!(messages.len(), ctx.messages.len());

        // The parent transcript must still be intact and unchanged.
        let parent_transcript =
            std::fs::read_to_string(transcript::get_transcript_file(parent_id)).unwrap();
        let parent_lines: Vec<&str> = parent_transcript
            .lines()
            .filter(|l| !l.is_empty())
            .collect();
        assert_eq!(parent_lines.len(), 2);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_branch_rejects_arguments_and_suggests_gbranch() {
        let temp = tempfile::tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        let mut ctx = CommandContext {
            messages: Vec::new(),
            cwd: PathBuf::from("/proj"),
            app_state: AppState::default(),
            session_id: SessionId::from_string("noop"),
        };

        let result = BranchHandler
            .execute("feature/foo", &mut ctx)
            .await
            .unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("/gbranch"), "got: {}", text);
                assert!(text.contains("feature/foo"));
            }
            _ => panic!("expected Output"),
        }
    }

    #[test]
    fn test_short_id_truncates_to_eight_chars() {
        assert_eq!(short_id("abcdef12-3456-7890-abcd-ef1234567890"), "abcdef12");
    }
}
