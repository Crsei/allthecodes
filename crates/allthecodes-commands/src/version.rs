//! /version command -- displays the current version.

use anyhow::Result;
use async_trait::async_trait;

use super::{CommandContext, CommandHandler, CommandResult};

/// Handler for the `/version` slash command.
pub struct VersionHandler;

#[async_trait]
impl CommandHandler for VersionHandler {
    async fn execute(&self, _args: &str, _ctx: &mut CommandContext) -> Result<CommandResult> {
        let version = env!("CARGO_PKG_VERSION");
        Ok(CommandResult::Output(format!("allthecodes {}", version,)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
    use allthecodes_engine::types::app_state::AppState;
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_version_output() {
        let handler = VersionHandler;
        let mut ctx = CommandContext {
            messages: Vec::new(),
            cwd: PathBuf::from("."),
            app_state: AppState::default(),
            session_id: SessionId::from_string("test-session"),
        };

        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("allthecodes"));
                assert!(text.contains(env!("CARGO_PKG_VERSION")));
            }
            _ => panic!("Expected Output result"),
        }
    }
}
