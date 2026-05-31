//! `/goal` slash command -- create and inspect durable session goals.

use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use uuid::Uuid;

use crate::{CommandContext, CommandHandler, CommandResult};
use allthecodes_tools::goals::{self, GoalRecord, GoalStatus};
use allthecodes_types::message::{Message, MessageContent, UserMessage};

pub struct GoalHandler;

#[derive(Debug, PartialEq, Eq)]
enum GoalCommand {
    Status,
    Create {
        objective: String,
        token_budget: Option<u64>,
    },
    Complete {
        reason: Option<String>,
    },
    Block {
        reason: Option<String>,
    },
    Help,
}

#[async_trait]
impl CommandHandler for GoalHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        match parse_goal_command(args) {
            Ok(GoalCommand::Status) => render_status(ctx),
            Ok(GoalCommand::Create {
                objective,
                token_budget,
            }) => create_goal(ctx, objective, token_budget),
            Ok(GoalCommand::Complete { reason }) => update_goal(ctx, GoalStatus::Complete, reason),
            Ok(GoalCommand::Block { reason }) => update_goal(ctx, GoalStatus::Blocked, reason),
            Ok(GoalCommand::Help) => Ok(CommandResult::Output(usage())),
            Err(message) => Ok(CommandResult::Output(format!("{message}\n\n{}", usage()))),
        }
    }
}

fn parse_goal_command(args: &str) -> std::result::Result<GoalCommand, String> {
    let trimmed = args.trim();
    if trimmed.is_empty() || matches!(trimmed, "status" | "show") {
        return Ok(GoalCommand::Status);
    }
    if matches!(trimmed, "help" | "-h" | "--help") {
        return Ok(GoalCommand::Help);
    }

    let (first, rest) = split_first_word(trimmed);
    match first {
        "complete" | "completed" | "done" => {
            return Ok(GoalCommand::Complete {
                reason: non_empty(rest).map(str::to_string),
            });
        }
        "block" | "blocked" => {
            return Ok(GoalCommand::Block {
                reason: non_empty(rest).map(str::to_string),
            });
        }
        "set" | "create" => parse_create_args(rest),
        _ => parse_create_args(trimmed),
    }
}

fn parse_create_args(args: &str) -> std::result::Result<GoalCommand, String> {
    let mut remaining = args.trim();
    let mut token_budget = None;

    loop {
        let (word, rest) = split_first_word(remaining);
        match word {
            "--tokens" | "--token-budget" | "--budget" => {
                let (value, after_value) = split_first_word(rest);
                if value.is_empty() {
                    return Err(format!("{word} requires a positive integer value."));
                }
                let parsed = value
                    .parse::<u64>()
                    .map_err(|_| format!("{word} requires a positive integer value."))?;
                if parsed == 0 {
                    return Err(format!("{word} requires a positive integer value."));
                }
                token_budget = Some(parsed);
                remaining = after_value.trim();
            }
            _ => break,
        }
    }

    let Some(objective) = non_empty(remaining) else {
        return Err("A goal objective is required.".to_string());
    };

    Ok(GoalCommand::Create {
        objective: objective.to_string(),
        token_budget,
    })
}

fn split_first_word(input: &str) -> (&str, &str) {
    let trimmed = input.trim_start();
    match trimmed.find(char::is_whitespace) {
        Some(idx) => (&trimmed[..idx], trimmed[idx..].trim_start()),
        None => (trimmed, ""),
    }
}

fn non_empty(input: &str) -> Option<&str> {
    let trimmed = input.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn create_goal(
    ctx: &CommandContext,
    objective: String,
    token_budget: Option<u64>,
) -> Result<CommandResult> {
    if let Some(existing) = goals::load_goal_for_session(ctx.session_id.as_str())? {
        if is_open(&existing.status) {
            return Ok(CommandResult::Output(format!(
                "A session can only have one unfinished goal.\n\n{}",
                format_goal(&existing)
            )));
        }
    }

    let now = Utc::now().to_rfc3339();
    let goal = GoalRecord {
        objective: objective.clone(),
        token_budget,
        tokens_used: 0,
        time_used_seconds: 0,
        status: GoalStatus::Active,
        created_at: now.clone(),
        updated_at: now,
        completed_at: None,
        status_reason: None,
    };
    goals::save_goal_for_session(ctx.session_id.as_str(), &goal)?;

    let prompt = format!(
        "A session goal has been set:\n\n{}\n\n\
         Work toward this completion condition across turns. Use UpdateGoal with status=complete only when the objective is genuinely achieved, or status=blocked when progress cannot continue without external input.",
        goal.objective
    );

    Ok(CommandResult::Query(vec![Message::User(UserMessage {
        uuid: Uuid::new_v4(),
        timestamp: Utc::now().timestamp(),
        role: "user".to_string(),
        content: MessageContent::Text(prompt),
        is_meta: false,
        tool_use_result: None,
        source_tool_assistant_uuid: None,
    })]))
}

fn update_goal(
    ctx: &CommandContext,
    status: GoalStatus,
    reason: Option<String>,
) -> Result<CommandResult> {
    let Some(mut goal) = goals::load_goal_for_session(ctx.session_id.as_str())? else {
        return Ok(CommandResult::Output("No session goal exists.".to_string()));
    };

    let now = Utc::now();
    goal.time_used_seconds = elapsed_goal_seconds(&goal.created_at, now);
    goal.updated_at = now.to_rfc3339();
    goal.completed_at = (status == GoalStatus::Complete).then(|| goal.updated_at.clone());
    goal.status = status;
    goal.status_reason = reason;
    goals::save_goal_for_session(ctx.session_id.as_str(), &goal)?;

    Ok(CommandResult::Output(format_goal(&goal)))
}

fn render_status(ctx: &CommandContext) -> Result<CommandResult> {
    match goals::load_goal_for_session(ctx.session_id.as_str())? {
        Some(goal) => Ok(CommandResult::Output(format_goal(&goal))),
        None => Ok(CommandResult::Output(
            "No session goal exists. Use `/goal <objective>` to set one.".to_string(),
        )),
    }
}

fn is_open(status: &GoalStatus) -> bool {
    matches!(status, GoalStatus::Active | GoalStatus::BudgetLimited)
}

fn elapsed_goal_seconds(created_at: &str, now: chrono::DateTime<Utc>) -> u64 {
    chrono::DateTime::parse_from_rfc3339(created_at)
        .map(|created| {
            now.signed_duration_since(created.with_timezone(&Utc))
                .num_seconds()
                .max(0) as u64
        })
        .unwrap_or_default()
}

fn format_goal(goal: &GoalRecord) -> String {
    let mut lines = Vec::new();
    lines.push("Session Goal".to_string());
    lines.push("-".repeat(30));
    lines.push(format!("Objective: {}", goal.objective));
    lines.push(format!("Status:    {}", status_label(&goal.status)));
    lines.push(format!(
        "Runtime:   {} tokens, {} elapsed",
        format_tokens(goal.tokens_used),
        format_duration(goal.time_used_seconds)
    ));
    if let Some(budget) = goal.token_budget {
        lines.push(format!(
            "Budget:    {} tokens ({} remaining)",
            format_tokens(budget),
            format_tokens(budget.saturating_sub(goal.tokens_used))
        ));
    }
    if let Some(reason) = non_empty(goal.status_reason.as_deref().unwrap_or_default()) {
        lines.push(format!("Reason:    {reason}"));
    }
    lines.push(format!("Updated:   {}", goal.updated_at));
    lines.join("\n")
}

fn status_label(status: &GoalStatus) -> &'static str {
    match status {
        GoalStatus::Active => "active",
        GoalStatus::Complete => "complete",
        GoalStatus::Blocked => "blocked",
        GoalStatus::BudgetLimited => "budget_limited",
    }
}

fn format_tokens(n: u64) -> String {
    if n == 0 {
        return "0".to_string();
    }
    let s = n.to_string();
    let mut out = String::new();
    for (idx, ch) in s.chars().rev().enumerate() {
        if idx > 0 && idx % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

fn format_duration(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;
    if hours > 0 {
        format!("{hours}h {minutes}m {secs}s")
    } else if minutes > 0 {
        format!("{minutes}m {secs}s")
    } else {
        format!("{secs}s")
    }
}

fn usage() -> String {
    "Usage: /goal [status]\n\
     Usage: /goal [--tokens N] <objective>\n\
     Usage: /goal set [--tokens N] <objective>\n\
     Usage: /goal complete [reason]\n\
     Usage: /goal block [reason]"
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
    use allthecodes_engine::types::app_state::AppState;
    use serial_test::serial;
    use std::path::{Path, PathBuf};

    struct EnvGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set_path(key: &'static str, value: &Path) -> Self {
            let previous = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(previous) = self.previous.as_ref() {
                std::env::set_var(self.key, previous);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    fn test_ctx(session_id: &str) -> CommandContext {
        CommandContext {
            messages: Vec::new(),
            cwd: PathBuf::from("/test"),
            app_state: AppState::default(),
            session_id: SessionId::from_string(session_id),
        }
    }

    #[test]
    fn parses_goal_commands() {
        assert_eq!(
            parse_goal_command("--tokens 120 ship the feature").unwrap(),
            GoalCommand::Create {
                objective: "ship the feature".to_string(),
                token_budget: Some(120)
            }
        );
        assert_eq!(
            parse_goal_command("set --budget 5 finish").unwrap(),
            GoalCommand::Create {
                objective: "finish".to_string(),
                token_budget: Some(5)
            }
        );
        assert_eq!(
            parse_goal_command("complete all tests pass").unwrap(),
            GoalCommand::Complete {
                reason: Some("all tests pass".to_string())
            }
        );
    }

    #[tokio::test]
    #[serial]
    async fn creates_goal_and_returns_query() {
        let tempdir = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set_path("ALLTHECODES_HOME", tempdir.path());
        let mut ctx = test_ctx("goal-create");

        let result = GoalHandler
            .execute("--tokens 100 ship the release", &mut ctx)
            .await
            .unwrap();

        match result {
            CommandResult::Query(messages) => {
                assert_eq!(messages.len(), 1);
                let stored = goals::load_goal_for_session("goal-create")
                    .unwrap()
                    .expect("goal");
                assert_eq!(stored.objective, "ship the release");
                assert_eq!(stored.token_budget, Some(100));
            }
            _ => panic!("expected query"),
        }
    }

    #[tokio::test]
    #[serial]
    async fn rejects_duplicate_active_goal() {
        let tempdir = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set_path("ALLTHECODES_HOME", tempdir.path());
        let mut ctx = test_ctx("goal-duplicate");

        GoalHandler.execute("first", &mut ctx).await.unwrap();
        let result = GoalHandler.execute("second", &mut ctx).await.unwrap();

        match result {
            CommandResult::Output(text) => assert!(text.contains("one unfinished goal")),
            _ => panic!("expected output"),
        }
    }

    #[tokio::test]
    #[serial]
    async fn completes_goal() {
        let tempdir = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set_path("ALLTHECODES_HOME", tempdir.path());
        let mut ctx = test_ctx("goal-complete");

        GoalHandler.execute("ship", &mut ctx).await.unwrap();
        let result = GoalHandler
            .execute("complete shipped", &mut ctx)
            .await
            .unwrap();

        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Status:    complete"));
                assert!(text.contains("Reason:    shipped"));
            }
            _ => panic!("expected output"),
        }
    }
}
