//! `/channels` command -- view connected channels (KAIROS).
//!
//! Shows the list of connected communication channels for the assistant mode.
//!
//! Requires `FEATURE_KAIROS_CHANNELS=1` (which itself requires `FEATURE_KAIROS=1`).

use anyhow::Result;
use async_trait::async_trait;

use crate::remote_cmd;
use crate::{CommandContext, CommandHandler, CommandResult};
use allthecodes_config::features::{self, Feature};

pub struct ChannelsHandler;

#[async_trait]
impl CommandHandler for ChannelsHandler {
    async fn execute(&self, args: &str, _ctx: &mut CommandContext) -> Result<CommandResult> {
        if !features::enabled(Feature::KairosChannels) {
            return Ok(CommandResult::Output(
                "Channels require FEATURE_KAIROS_CHANNELS=1".into(),
            ));
        }

        match args.trim() {
            "" | "list" | "status" => Ok(CommandResult::Output(render_channels().await)),
            _ => Ok(CommandResult::Output(
                "Usage: /channels [list|status]".into(),
            )),
        }
    }
}

async fn render_channels() -> String {
    let mut lines = vec![
        "Channels".to_string(),
        "Inbound channel sessions are deferred; outbound remote adapters are the real channel surface currently wired.".to_string(),
        String::new(),
        remote_cmd::render_adapters().await,
    ];
    lines.push(String::new());
    lines.push("Use `/remote adapters` for the same gateway-backed adapter status.".to_string());
    lines.join("\n")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
    use std::path::PathBuf;

    fn test_ctx() -> CommandContext {
        CommandContext {
            messages: Vec::new(),
            cwd: PathBuf::from("/test"),
            app_state: Default::default(),
            session_id: SessionId::from_string("test-session"),
        }
    }

    #[tokio::test]
    async fn test_feature_gate() {
        let handler = ChannelsHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("list", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => assert!(text.contains("FEATURE_KAIROS_CHANNELS")),
            _ => panic!("Expected Output"),
        }
    }

    #[tokio::test]
    async fn test_unknown_argument_gated() {
        let handler = ChannelsHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("connect foo", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => assert!(text.contains("FEATURE_KAIROS_CHANNELS")),
            _ => panic!("Expected Output"),
        }
    }
}
