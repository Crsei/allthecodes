//! `/assistant` command -- view assistant (KAIROS) mode status.
//!
//! Displays the current state of KAIROS-related settings including
//! daemon status, brief mode, assistant mode, terminal focus,
//! autonomous tick interval, and active model.
//!
//! Requires `FEATURE_KAIROS=1`.

use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandHandler, CommandResult};
use allthecodes_config::features::{self, Feature};

pub struct AssistantHandler;

#[async_trait]
impl CommandHandler for AssistantHandler {
    async fn execute(&self, _args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        if !features::enabled(Feature::Kairos) {
            return Ok(CommandResult::Output(
                "Assistant mode requires FEATURE_KAIROS=1".into(),
            ));
        }

        let state = &ctx.app_state;
        let tick_display = match state.autonomous_tick_ms {
            Some(ms) => format!("{}ms", ms),
            None => "disabled".to_string(),
        };

        let output = format!(
            "=== Assistant (KAIROS) Status ===\n\
             Daemon active:     {}\n\
             Brief mode:        {}\n\
             Assistant mode:    {}\n\
             Terminal focus:    {}\n\
             Tick interval:     {}\n\
             Model:             {}",
            if state.kairos_active { "yes" } else { "no" },
            if state.is_brief_only { "ON" } else { "OFF" },
            if state.is_assistant_mode { "ON" } else { "OFF" },
            if state.terminal_focus { "yes" } else { "no" },
            tick_display,
            state.main_loop_model,
        );

        Ok(CommandResult::Output(output))
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
        let handler = AssistantHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => assert!(text.contains("FEATURE_KAIROS")),
            _ => panic!("Expected Output"),
        }
    }
}
