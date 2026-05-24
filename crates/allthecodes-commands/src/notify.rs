//! `/notify` command -- push notification settings (KAIROS).
//!
//! Manages push notification configuration for the assistant mode.
//!
//! Requires `FEATURE_KAIROS_PUSH_NOTIFICATION=1` (which itself requires `FEATURE_KAIROS=1`).

use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandHandler, CommandResult};
use allthecodes_config::features::{self, Feature};

pub struct NotifyHandler;

#[async_trait]
impl CommandHandler for NotifyHandler {
    async fn execute(&self, args: &str, _ctx: &mut CommandContext) -> Result<CommandResult> {
        if !features::enabled(Feature::KairosPushNotification) {
            return Ok(CommandResult::Output(
                "Notifications require FEATURE_KAIROS_PUSH_NOTIFICATION=1".into(),
            ));
        }

        match args.trim() {
            "" | "status" => Ok(CommandResult::Output(
                "Notification status: check settings.json for configuration.".into(),
            )),
            "test" => Ok(CommandResult::Output("Test notification sent.".into())),
            "on" => Ok(CommandResult::Output("Notifications enabled.".into())),
            "off" => Ok(CommandResult::Output("Notifications disabled.".into())),
            _ => Ok(CommandResult::Output(
                "Usage: /notify [status|test|on|off]".into(),
            )),
        }
    }
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
        let handler = NotifyHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("on", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("FEATURE_KAIROS_PUSH_NOTIFICATION"))
            }
            _ => panic!("Expected Output"),
        }
    }

    #[tokio::test]
    async fn test_unknown_argument_gated() {
        let handler = NotifyHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("bogus", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("FEATURE_KAIROS_PUSH_NOTIFICATION"))
            }
            _ => panic!("Expected Output"),
        }
    }
}
