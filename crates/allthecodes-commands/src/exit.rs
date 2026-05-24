//! /exit command -- exits the REPL.
//!
//! Displays a random goodbye message and signals the REPL loop to terminate.

use anyhow::Result;
use async_trait::async_trait;

use super::{CommandContext, CommandHandler, CommandResult};

/// Goodbye messages displayed on exit.
const GOODBYE_MESSAGES: &[&str] = &["Goodbye!", "See ya!", "Bye!", "Catch you later!"];

/// Pick a random goodbye message.
fn get_random_goodbye_message() -> &'static str {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0) as usize;
    GOODBYE_MESSAGES[nanos % GOODBYE_MESSAGES.len()]
}

/// Handler for the `/exit` slash command.
pub struct ExitHandler;

#[async_trait]
impl CommandHandler for ExitHandler {
    async fn execute(&self, _args: &str, _ctx: &mut CommandContext) -> Result<CommandResult> {
        let message = get_random_goodbye_message().to_string();
        Ok(CommandResult::Exit(message))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
    use allthecodes_engine::types::app_state::AppState;
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_exit_returns_exit_result() {
        let handler = ExitHandler;
        let mut ctx = CommandContext {
            messages: Vec::new(),
            cwd: PathBuf::from("."),
            app_state: AppState::default(),
            session_id: SessionId::from_string("test-session"),
        };

        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Exit(msg) => {
                assert!(
                    GOODBYE_MESSAGES.contains(&msg.as_str()),
                    "Expected a goodbye message, got: {}",
                    msg
                );
            }
            _ => panic!("Expected Exit result"),
        }
    }
}
