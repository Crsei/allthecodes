//! `/debug` command family.

use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandHandler, CommandResult};

pub struct DebugHandler;

#[async_trait]
impl CommandHandler for DebugHandler {
    async fn execute(&self, args: &str, _ctx: &mut CommandContext) -> Result<CommandResult> {
        let arg = args.trim();
        if arg == "snapshot" {
            return Ok(CommandResult::Output(
                "TUI debug snapshots are available in the interactive TUI via /debug snapshot or F12."
                    .to_string(),
            ));
        }

        Ok(CommandResult::Output(
            "Usage: /debug snapshot\n\nExports the current Rust TUI frame to a timestamped target/tui-snapshots/snapshot-*.txt file when run inside the interactive TUI.".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
    use allthecodes_engine::types::app_state::AppState;
    use std::path::PathBuf;

    fn test_ctx() -> CommandContext {
        CommandContext {
            messages: Vec::new(),
            cwd: PathBuf::from("."),
            app_state: AppState::default(),
            session_id: SessionId::from_string("test-session"),
        }
    }

    #[tokio::test]
    async fn snapshot_describes_tui_runtime_export() {
        let handler = DebugHandler;
        let mut ctx = test_ctx();

        let result = handler.execute("snapshot", &mut ctx).await.unwrap();

        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("/debug snapshot"));
                assert!(text.contains("F12"));
            }
            _ => panic!("expected output"),
        }
    }

    #[tokio::test]
    async fn empty_args_show_snapshot_usage() {
        let handler = DebugHandler;
        let mut ctx = test_ctx();

        let result = handler.execute("", &mut ctx).await.unwrap();

        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Usage: /debug snapshot"));
                assert!(text.contains("target/tui-snapshots/snapshot-*.txt"));
            }
            _ => panic!("expected output"),
        }
    }
}
