//! `/tasks` slash command: background task list + detail controls.
//!
//! Aggregates tool-driven tasks from `tools::tasks::GLOBAL_STORE` and
//! in-process teammate runners from `teams::in_process::TASK_REGISTRY`.
//! Tool task metadata is durable across process restarts; runtime cancellation
//! handles remain process-local and only exist while a supervisor is alive.

use allthecodes_tasks::{
    parse_tasks_command, TaskEntry, TaskStatus as ToolTaskStatus, TasksCommand,
};
use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Local, TimeZone};

use super::browser::{render_with_footer, TreeNode};
use crate::runtime::{TeamTaskSnapshot, TeamTaskStatus};
use crate::{CommandContext, CommandHandler, CommandResult};

pub struct TasksHandler;

#[async_trait]
impl CommandHandler for TasksHandler {
    async fn execute(&self, args: &str, _ctx: &mut CommandContext) -> Result<CommandResult> {
        match parse_tasks_command(args) {
            Ok(TasksCommand::List) => Ok(CommandResult::Output(render_list())),
            Ok(TasksCommand::Show { id }) => Ok(CommandResult::Output(render_detail(&id))),
            Ok(TasksCommand::Stop { id }) => Ok(CommandResult::Output(stop_task(&id))),
            Ok(TasksCommand::Delete { id }) => Ok(CommandResult::Output(delete_task(&id))),
            Ok(TasksCommand::Unknown { subcommand }) => Ok(CommandResult::Output(format!(
                "Unknown /tasks subcommand '{}'.\n\n\
                 Usage:\n  \
                 /tasks               - list current background tasks\n  \
                 /tasks show <id>     - drill into one task\n  \
                 /tasks stop <id>     - cancel a tool task\n  \
                 /tasks delete <id>   - delete a persisted tool task\n",
                subcommand
            ))),
            Err(error) => Ok(CommandResult::Output(error.message)),
        }
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn render_list() -> String {
    let tool_tasks = crate::runtime::tool_tasks();
    let team_tasks = crate::runtime::team_task_snapshots();

    let mut roots: Vec<TreeNode> = Vec::new();

    let mut tool_node = TreeNode::leaf(format!("Tool tasks ({})", tool_tasks.len()));
    if tool_tasks.is_empty() {
        tool_node.push_child(TreeNode::leaf("(none)"));
    } else {
        for task in &tool_tasks {
            let detail = if task.output_truncated {
                format!(
                    "{} | output {} bytes, truncated",
                    format_timestamp(task.created_at),
                    task.output_bytes
                )
            } else {
                format!(
                    "{} | output {} bytes",
                    format_timestamp(task.created_at),
                    task.output_bytes
                )
            };
            tool_node.push_child(
                TreeNode::leaf(format!("{}  {}", task.id, task.subject))
                    .with_badge(format!("{}:{}", task.kind, task.status.as_str()))
                    .with_detail(detail),
            );
        }
    }
    roots.push(tool_node);

    let mut team_node = TreeNode::leaf(format!("Team tasks ({})", team_tasks.len()));
    if team_tasks.is_empty() {
        team_node.push_child(TreeNode::leaf("(none)"));
    } else {
        for task in &team_tasks {
            let status_tag = format!(
                "team:{}",
                match task.status {
                    TeamTaskStatus::Running if task.is_idle => "running-idle",
                    TeamTaskStatus::Running => "running",
                    TeamTaskStatus::Stopped => "stopped",
                    TeamTaskStatus::Completed => "completed",
                }
            );
            team_node.push_child(
                TreeNode::leaf(format!(
                    "{}  {} ({})",
                    task.id, task.agent_name, task.team_name
                ))
                .with_badge(status_tag)
                .with_detail(if task.has_error {
                    format!("error: {}", task.error_message.clone().unwrap_or_default())
                } else {
                    format!("prompt: {}", first_line(&task.prompt))
                }),
            );
        }
    }
    roots.push(team_node);

    let footer = "\
Tool tasks persist across process restarts; team tasks live for the duration \
of the parent team. Use `/tasks show <id>` for retained output, or \
`/tasks stop <id>` to cancel a tool task.";

    render_with_footer("Background tasks", &roots, footer)
}

fn render_detail(id: &str) -> String {
    if let Some(task) = crate::runtime::get_tool_task(id) {
        return render_tool_detail(&task);
    }
    if let Some(task) = crate::runtime::team_task_snapshots()
        .into_iter()
        .find(|t| t.id == id)
    {
        return render_team_detail(&task);
    }
    format!(
        "No task with id '{}'. Run `/tasks` to list every current background task.",
        id
    )
}

fn render_tool_detail(task: &TaskEntry) -> String {
    let mut out = String::new();
    out.push_str(&format!("Tool task {}\n", task.id));
    out.push_str(&"-".repeat(11 + task.id.len()));
    out.push('\n');
    out.push_str(&format!("  Subject:     {}\n", task.subject));
    out.push_str(&format!("  Description: {}\n", task.description));
    out.push_str(&format!("  Kind:        {}\n", task.kind));
    out.push_str(&format!("  Status:      {}\n", task.status.as_str()));
    if let Some(previous) = task.previous_status {
        out.push_str(&format!("  Previous:    {}\n", previous.as_str()));
    }
    if let Some(parent_id) = &task.parent_id {
        out.push_str(&format!("  Parent:      {}\n", parent_id));
    }
    if let Some(tool_use_id) = &task.tool_use_id {
        out.push_str(&format!("  Tool use:    {}\n", tool_use_id));
    }
    if let Some(agent_id) = &task.agent_id {
        out.push_str(&format!("  Agent:       {}\n", agent_id));
    }
    if let Some(supervisor_id) = &task.supervisor_id {
        out.push_str(&format!("  Supervisor:  {}\n", supervisor_id));
    }
    if let Some(isolation) = &task.isolation {
        out.push_str(&format!("  Isolation:   {}\n", isolation));
    }
    if let Some(path) = &task.worktree_path {
        out.push_str(&format!("  Worktree:    {}\n", path));
    }
    if let Some(branch) = &task.worktree_branch {
        out.push_str(&format!("  Branch:      {}\n", branch));
    }
    if let Some(remote_task_type) = &task.remote_task_type {
        out.push_str(&format!("  Remote type: {}\n", remote_task_type));
    }
    if let Some(remote_session_id) = &task.remote_session_id {
        out.push_str(&format!("  Remote sess: {}\n", remote_session_id));
    }
    if let Some(poll_started_at) = task.poll_started_at {
        out.push_str(&format!("  Poll start:  {}\n", poll_started_at));
    }
    if let Some(remote_task_metadata) = &task.remote_task_metadata {
        out.push_str(&format!("  Remote meta: {}\n", remote_task_metadata));
    }
    if !task.depends_on.is_empty() {
        out.push_str(&format!("  Depends on:  {}\n", task.depends_on.join(", ")));
        let blocked = blocked_dependencies(task);
        if !blocked.is_empty() {
            out.push_str(&format!("  Blocked by:  {}\n", blocked.join(", ")));
        }
    }
    out.push_str(&format!(
        "  Created:     {}\n",
        format_timestamp(task.created_at)
    ));
    out.push_str(&format!(
        "  Updated:     {}\n",
        format_timestamp(task.updated_at)
    ));
    if let Some(recovered_at) = task.recovered_at {
        out.push_str(&format!(
            "  Recovered:   {}\n",
            format_timestamp(recovered_at)
        ));
    }
    if let Some(cancelled_at) = task.cancel_requested_at {
        out.push_str(&format!(
            "  Cancelled:   {}\n",
            format_timestamp(cancelled_at)
        ));
    }
    out.push_str(&format!(
        "  Output:      {} bytes{}\n",
        task.output_bytes,
        if task.output_truncated {
            " (retention truncated)"
        } else {
            ""
        }
    ));
    if task.output.is_empty() {
        out.push_str("  Retained log: (empty)\n");
    } else {
        out.push_str("  Retained log:\n");
        for line in task.output.lines() {
            out.push_str(&format!("    {}\n", line));
        }
    }
    if matches!(
        task.status,
        ToolTaskStatus::Pending
            | ToolTaskStatus::InProgress
            | ToolTaskStatus::Interrupted
            | ToolTaskStatus::Recoverable
    ) {
        out.push_str(
            "\nTip: `/tasks stop <id>` cancels the task if a runtime handle is still active.\n",
        );
    }
    out
}

fn render_team_detail(task: &TeamTaskSnapshot) -> String {
    let mut out = String::new();
    out.push_str(&format!("Team task {}\n", task.id));
    out.push_str(&"-".repeat(11 + task.id.len()));
    out.push('\n');
    out.push_str(&format!(
        "  Agent:    {} ({})\n",
        task.agent_name, task.agent_id
    ));
    out.push_str(&format!("  Team:     {}\n", task.team_name));
    out.push_str(&format!(
        "  Status:   {}{}\n",
        match task.status {
            TeamTaskStatus::Running => "running",
            TeamTaskStatus::Stopped => "stopped",
            TeamTaskStatus::Completed => "completed",
        },
        if task.is_idle { " (idle)" } else { "" }
    ));
    if task.awaiting_plan_approval {
        out.push_str("  Plan:     awaiting approval\n");
    }
    out.push_str(&format!("  Mode:     {}\n", task.permission_mode));
    if let Some(model) = &task.model {
        out.push_str(&format!("  Model:    {}\n", model));
    }
    if task.has_error {
        out.push_str(&format!(
            "  Error:    {}\n",
            task.error_message.clone().unwrap_or_default()
        ));
    }
    out.push_str("  Prompt:\n");
    for line in task.prompt.lines() {
        out.push_str(&format!("    {}\n", line));
    }
    out.push_str(
        "\nTip: team tasks are controlled through `/team`; `/tasks stop` only applies to tool tasks.\n",
    );
    out
}

fn blocked_dependencies(task: &TaskEntry) -> Vec<String> {
    task.depends_on
        .iter()
        .filter(|id| {
            crate::runtime::get_tool_task(id)
                .map(|dependency| dependency.status != ToolTaskStatus::Completed)
                .unwrap_or(true)
        })
        .cloned()
        .collect()
}

fn stop_task(id: &str) -> String {
    match crate::runtime::stop_tool_task(id) {
        Ok(Some(entry)) => {
            return format!(
                "Cancelled tool task '{}' (now {}).",
                entry.subject,
                entry.status.as_str()
            );
        }
        Ok(None) => {}
        Err(err) => return format!("Failed to stop task '{}': {err}", id),
    }
    if crate::runtime::team_task_snapshots()
        .iter()
        .any(|t| t.id == id)
    {
        return format!(
            "'{}' is a team task; use `/team kill <name>` to stop a teammate, \
             `/tasks stop` only applies to tool tasks.",
            id
        );
    }
    format!(
        "No task with id '{}'; run `/tasks` to see the current list.",
        id
    )
}

fn delete_task(id: &str) -> String {
    match crate::runtime::delete_tool_task(id) {
        Ok(Some(entry)) => return format!("Deleted persisted tool task '{}'.", entry.subject),
        Ok(None) => {}
        Err(err) => return format!("Failed to delete task '{}': {err}", id),
    }
    if crate::runtime::team_task_snapshots()
        .iter()
        .any(|t| t.id == id)
    {
        return format!(
            "'{}' is a team task; team tasks are runtime-only and cannot be deleted from persisted tool storage.",
            id
        );
    }
    format!(
        "No persisted tool task with id '{}'; run `/tasks` to see the current list.",
        id
    )
}

fn format_timestamp(ts: i64) -> String {
    match Local.timestamp_opt(ts, 0) {
        chrono::LocalResult::Single(dt) => dt.format("%Y-%m-%d %H:%M:%S").to_string(),
        _ => {
            let as_local: DateTime<Local> = Local::now();
            format!("unknown (now ~{})", as_local.format("%Y-%m-%d %H:%M"))
        }
    }
}

fn first_line(s: &str) -> String {
    const MAX: usize = 80;
    let line = s.lines().next().unwrap_or(s).trim();
    if line.chars().count() <= MAX {
        return line.to_string();
    }
    let truncated: String = line.chars().take(MAX).collect();
    format!("{}...", truncated)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
    use allthecodes_engine::types::app_state::AppState;
    use std::path::PathBuf;

    fn make_ctx() -> CommandContext {
        CommandContext {
            messages: vec![],
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            app_state: AppState::default(),
            session_id: SessionId::new(),
        }
    }

    #[tokio::test]
    async fn list_always_has_both_buckets() {
        let handler = TasksHandler;
        let mut ctx = make_ctx();
        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(s) => {
                assert!(s.contains("Tool tasks"));
                assert!(s.contains("Team tasks"));
            }
            _ => panic!("expected Output"),
        }
    }

    #[tokio::test]
    async fn show_without_id_returns_usage() {
        let handler = TasksHandler;
        let mut ctx = make_ctx();
        let result = handler.execute("show", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(s) => assert!(s.contains("Usage: /tasks show")),
            _ => panic!("expected Output"),
        }
    }

    #[tokio::test]
    async fn show_missing_id_reports_missing() {
        let handler = TasksHandler;
        let mut ctx = make_ctx();
        let result = handler.execute("show missing-id", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(s) => assert!(s.contains("No task with id")),
            _ => panic!("expected Output"),
        }
    }

    #[tokio::test]
    async fn stop_without_id_returns_usage() {
        let handler = TasksHandler;
        let mut ctx = make_ctx();
        let result = handler.execute("stop", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(s) => assert!(s.contains("Usage: /tasks stop")),
            _ => panic!("expected Output"),
        }
    }

    #[tokio::test]
    async fn delete_without_id_returns_usage() {
        let handler = TasksHandler;
        let mut ctx = make_ctx();
        let result = handler.execute("delete", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(s) => assert!(s.contains("Usage: /tasks delete")),
            _ => panic!("expected Output"),
        }
    }

    #[tokio::test]
    async fn unknown_subcommand_shows_usage() {
        let handler = TasksHandler;
        let mut ctx = make_ctx();
        let result = handler.execute("banana", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(s) => {
                assert!(s.contains("Unknown /tasks"));
                assert!(s.contains("Usage"));
            }
            _ => panic!("expected Output"),
        }
    }
}
