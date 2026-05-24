//! /clear command -- starts a fresh visible conversation session.

use anyhow::Result;
use async_trait::async_trait;

use super::{CommandContext, CommandHandler, CommandResult};

/// Handler for the `/clear` slash command.
pub struct ClearHandler;

#[async_trait]
impl CommandHandler for ClearHandler {
    async fn execute(&self, _args: &str, _ctx: &mut CommandContext) -> Result<CommandResult> {
        // The caller owns the live QueryEngine and rotates the active session.
        Ok(CommandResult::Clear)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
    use allthecodes_engine::types::app_state::AppState;
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_clear_returns_clear_result() {
        let handler = ClearHandler;
        let mut ctx = CommandContext {
            messages: Vec::new(),
            cwd: PathBuf::from("."),
            app_state: AppState::default(),
            session_id: SessionId::from_string("test-session"),
        };

        let result = handler.execute("", &mut ctx).await.unwrap();
        assert!(matches!(result, CommandResult::Clear));
    }
}
