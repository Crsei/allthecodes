//! `/team` slash command dispatcher.

use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandHandler, CommandResult};

pub struct TeamHandler;

#[async_trait]
impl CommandHandler for TeamHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        Ok(CommandResult::Output(
            crate::runtime::execute_team_command(args, ctx).await,
        ))
    }
}
