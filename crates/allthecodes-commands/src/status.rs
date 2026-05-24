//! /status command -- shows session status.

use anyhow::Result;
use async_trait::async_trait;

use crate::runtime::{self, TeamTaskStatus};
use crate::{CommandContext, CommandHandler, CommandResult};
use allthecodes_types::message::Message;

pub struct StatusHandler;

#[async_trait]
impl CommandHandler for StatusHandler {
    async fn execute(&self, _args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let message_count = ctx.messages.len();

        let user_count = ctx
            .messages
            .iter()
            .filter(|m| matches!(m, Message::User(_)))
            .count();
        let assistant_count = ctx
            .messages
            .iter()
            .filter(|m| matches!(m, Message::Assistant(_)))
            .count();

        let model = &ctx.app_state.main_loop_model;

        let fast_mode = if ctx.app_state.fast_mode {
            "enabled"
        } else {
            "disabled"
        };

        let effort = ctx.app_state.effort_value.as_deref().unwrap_or("default");

        let permission_mode = format!("{:?}", ctx.app_state.tool_permission_context.mode);

        let mut lines = Vec::new();
        lines.push("Session Status".to_string());
        lines.push("─".repeat(30));
        lines.push(format!(
            "Messages:    {} total ({} user, {} assistant)",
            message_count, user_count, assistant_count
        ));
        lines.push(format!("Model:       {}", model));
        lines.push(format!("Fast mode:   {}", fast_mode));
        lines.push(format!("Effort:      {}", effort));
        lines.push(format!("Permissions: {}", permission_mode));
        lines.push(format!(
            "Coordinator: {}",
            if allthecodes_config::features::enabled(
                allthecodes_config::features::Feature::Coordinator
            ) {
                "ON"
            } else {
                "OFF"
            }
        ));
        if let Some(team) = ctx.app_state.team_context.as_ref() {
            let snapshots = runtime::team_task_snapshots()
                .into_iter()
                .filter(|task| task.team_name == team.team_name)
                .collect::<Vec<_>>();
            let running = snapshots
                .iter()
                .filter(|task| task.status == TeamTaskStatus::Running)
                .count();
            let idle = snapshots.iter().filter(|task| task.is_idle).count();
            let errored = snapshots
                .iter()
                .filter(|task| task.has_error || task.error_message.is_some())
                .count();
            let awaiting_plan = snapshots
                .iter()
                .filter(|task| task.awaiting_plan_approval)
                .count();
            let working = running > idle;
            lines.push(format!(
                "Team:        {} ({} teammate(s), {} running, {} idle, {} error, {} awaiting plan, working={})",
                team.team_name,
                team.teammates.len(),
                running,
                idle,
                errored,
                awaiting_plan,
                working
            ));
        } else {
            lines.push("Team:        none".to_string());
        }
        let tool_tasks = runtime::tool_tasks();
        let active_tool_tasks = tool_tasks
            .iter()
            .filter(|task| {
                matches!(
                    task.status,
                    allthecodes_tasks::TaskStatus::Pending
                        | allthecodes_tasks::TaskStatus::InProgress
                        | allthecodes_tasks::TaskStatus::Interrupted
                        | allthecodes_tasks::TaskStatus::Recoverable
                )
            })
            .count();
        lines.push(format!(
            "Tasks:       {} tool task(s), {} active",
            tool_tasks.len(),
            active_tool_tasks
        ));

        Ok(CommandResult::Output(lines.join("\n")))
    }
}

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
    async fn test_status_output() {
        let handler = StatusHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Session Status"));
                assert!(text.contains("Model:"));
                assert!(text.contains("Fast mode:"));
                assert!(text.contains("Effort:"));
                assert!(text.contains("Permissions:"));
                assert!(text.contains("Team:"));
                assert!(text.contains("Tasks:"));
            }
            _ => panic!("Expected Output"),
        }
    }
}
