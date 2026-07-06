//! `/team` slash command — manage Agent Teams from the REPL.
//!
//! Subcommands:
//!
//! | Command                         | Effect                                        |
//! |---------------------------------|-----------------------------------------------|
//! | `/team` or `/team status`       | Summarize the active team + teammate states   |
//! | `/team list`                    | List teams persisted under the data dir       |
//! | `/team create <name> [desc]`    | Create a new team and make it the active one  |
//! | `/team spawn <name> <prompt>`   | Spawn an in-process teammate in the active team |
//! | `/team send <name> <message>`   | Send a plain-text message into a teammate mailbox |
//! | `/team kill <name>`             | Force-kill a teammate (abort its tokio task)  |
//! | `/team leave`                   | Clear the session's active team (team stays on disk) |
//! | `/team delete <name>`           | Remove a team's config + mailboxes from disk  |
//!
//! The active team is mirrored into `AppState::team_context`; the ingress
//! layer syncs it back to the QueryEngine so subsequent tool invocations
//! (SendMessage, TeamSpawn) see the same team.

use crate::backend::TeammateExecutor;
use crate::types::{
    BackendType, TeamContext, TeamMember, TeammateInfo, TeammateMessage, TeammateSpawnConfig,
};
use crate::{backend, constants, helpers, identity, in_process::InProcessBackend, mailbox};

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub async fn execute_team_command(
    args: &str,
    ctx: &mut allthecodes_commands::CommandContext,
) -> String {
    let mut parts = args.trim().splitn(2, char_is_whitespace);
    let sub = parts.next().unwrap_or("").trim();
    let rest = parts.next().unwrap_or("").trim();

    match sub {
        "" | "status" => status(ctx),
        "list" => list_teams(),
        "create" => create(ctx, rest),
        "spawn" => spawn(ctx, rest).await,
        "send" => send(ctx, rest),
        "kill" => kill(ctx, rest).await,
        "leave" => leave(ctx),
        "delete" => delete(ctx, rest).await,
        "help" | "--help" | "-h" => help(),
        other => format!("Unknown /team subcommand: '{}'. Try /team help.", other),
    }
}

fn char_is_whitespace(c: char) -> bool {
    c.is_whitespace()
}

// ---------------------------------------------------------------------------
// Subcommand implementations
// ---------------------------------------------------------------------------

fn help() -> String {
    [
        "Agent Teams commands:",
        "  /team                     Show active team status",
        "  /team list                List teams on disk",
        "  /team create <name> [desc]  Create a team and make it active",
        "  /team spawn <name> <prompt> Spawn an in-process teammate",
        "  /team send <name> <msg>   Send a plain-text message to a teammate",
        "  /team kill <name>         Force-kill a teammate",
        "  /team leave               Clear the active team context",
        "  /team delete <name>       Remove a team's data from disk",
        "",
        backend::strategy_summary(),
    ]
    .join("\n")
}

fn status(ctx: &allthecodes_commands::CommandContext) -> String {
    if !crate::is_agent_teams_active(&ctx.app_state) {
        return "Agent Teams is inactive. Use '/team create <name>' to create one or \
                '/team list' to see teams on disk."
            .into();
    }
    let tc = match ctx.app_state.team_context.as_ref() {
        Some(t) if !t.team_name.is_empty() => t,
        _ => {
            return "No active team. Use '/team create <name>' to create one or \
                    '/team list' to see teams on disk."
                .into();
        }
    };

    let mut lines = vec![format!(
        "Active team: {} (lead: {})",
        tc.team_name, tc.lead_agent_id
    )];

    match helpers::read_team_file(&tc.team_name) {
        Ok(tf) => {
            let snapshots = InProcessBackend::task_snapshots();
            lines.push(format!(
                "  created_at: {}",
                chrono::DateTime::from_timestamp(tf.created_at, 0)
                    .map(|dt| dt.to_rfc3339())
                    .unwrap_or_else(|| tf.created_at.to_string())
            ));
            if let Some(desc) = &tf.description {
                lines.push(format!("  description: {}", desc));
            }
            lines.push(format!("  members ({}):", tf.members.len()));
            for m in &tf.members {
                let tag = if m.name == constants::TEAM_LEAD_NAME {
                    "lead"
                } else {
                    "teammate"
                };
                let active_marker = match m.is_active {
                    Some(true) => "●",
                    Some(false) => "○",
                    None => "·",
                };
                let color = m.color.as_deref().unwrap_or("-");
                let member_tasks = snapshots
                    .iter()
                    .filter(|snapshot| snapshot.agent_id == m.agent_id)
                    .collect::<Vec<_>>();
                let task_state = if member_tasks.iter().any(|snapshot| snapshot.has_error) {
                    "error".to_string()
                } else if member_tasks.iter().any(|snapshot| {
                    snapshot.status == crate::types::TaskStatus::Running && !snapshot.is_idle
                }) {
                    "working".to_string()
                } else if member_tasks.iter().any(|snapshot| {
                    snapshot.status == crate::types::TaskStatus::Running && snapshot.is_idle
                }) {
                    "idle".to_string()
                } else if member_tasks.is_empty() {
                    "no-task".to_string()
                } else {
                    format!("{} task(s)", member_tasks.len())
                };
                lines.push(format!(
                    "    {} {} [{}] color={} model={} backend={} status={}",
                    active_marker,
                    m.name,
                    tag,
                    color,
                    m.model.as_deref().unwrap_or("inherit"),
                    m.backend_type
                        .map(|backend| backend.to_string())
                        .unwrap_or_else(|| "unknown".to_string()),
                    task_state,
                ));
            }
        }
        Err(e) => {
            lines.push(format!("  (failed to read team file: {})", e));
        }
    }

    let running = InProcessBackend::running_task_count();
    lines.push(format!("  in-process running tasks: {}", running));
    lines.push(format!(
        "  in-process working: {}",
        InProcessBackend::has_working_teammates()
    ));
    lines.push(format!("  {}", backend::strategy_summary()));

    lines.join("\n")
}

fn list_teams() -> String {
    let teams_root = crate::storage_paths::teams_dir();
    if !teams_root.exists() {
        return "No teams on disk yet.".into();
    }
    let mut names: Vec<String> = match std::fs::read_dir(&teams_root) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| helpers::team_exists(n))
            .collect(),
        Err(e) => return format!("Failed to list teams: {}", e),
    };
    names.sort();

    if names.is_empty() {
        return "No teams on disk yet.".into();
    }
    let mut lines = vec![format!("Teams on disk ({}):", names.len())];
    for name in names {
        let member_count = helpers::read_team_file(&name)
            .map(|tf| tf.members.len())
            .unwrap_or(0);
        lines.push(format!("  - {} ({} member(s))", name, member_count));
    }
    lines.join("\n")
}

fn create(ctx: &mut allthecodes_commands::CommandContext, rest: &str) -> String {
    let mut parts = rest.splitn(2, char_is_whitespace);
    let name = parts.next().unwrap_or("").trim();
    let description = parts
        .next()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    if name.is_empty() {
        return "Usage: /team create <name> [description]".into();
    }

    let cwd = ctx.cwd.to_string_lossy().into_owned();
    let team_file = match helpers::create_team(
        name,
        description.clone(),
        Some(ctx.session_id.to_string()),
        &cwd,
    ) {
        Ok(tf) => tf,
        Err(e) => return format!("Failed to create team '{}': {}", name, e),
    };

    // Activate the new team as the session's team context.
    let team_name = team_file.name.clone();
    let tc = TeamContext {
        team_name: team_name.clone(),
        team_file_path: helpers::team_config_path(&team_name)
            .to_string_lossy()
            .into_owned(),
        lead_agent_id: team_file.lead_agent_id.clone(),
        self_agent_id: Some(team_file.lead_agent_id.clone()),
        self_agent_name: Some(constants::TEAM_LEAD_NAME.into()),
        is_leader: Some(true),
        self_agent_color: None,
        teammates: Default::default(),
    };
    ctx.app_state.team_context = Some(tc);

    format!(
        "Team '{}' created and activated. Spawn teammates with '/team spawn <name> <prompt>' \
         or ask the assistant to use the TeamSpawn tool.",
        team_name
    )
}

async fn spawn(ctx: &mut allthecodes_commands::CommandContext, rest: &str) -> String {
    let mut parts = rest.splitn(2, char_is_whitespace);
    let name = parts.next().unwrap_or("").trim();
    let prompt = parts.next().unwrap_or("").trim();
    if name.is_empty() || prompt.is_empty() {
        return "Usage: /team spawn <name> <prompt>".into();
    }
    if name == constants::TEAM_LEAD_NAME {
        return format!(
            "'{}' is reserved for the team lead",
            constants::TEAM_LEAD_NAME
        );
    }

    // Ensure a team exists.
    let team_name = match ctx.app_state.team_context.as_ref() {
        Some(tc) if !tc.team_name.is_empty() => tc.team_name.clone(),
        _ => {
            return "No active team. Create one first with '/team create <name>'.".into();
        }
    };

    let mut team_file = match helpers::read_team_file(&team_name) {
        Ok(tf) => tf,
        Err(e) => return format!("Failed to read team '{}': {}", team_name, e),
    };
    if team_file.members.iter().any(|m| m.name == name) {
        return format!("Teammate '{}' already exists in team '{}'", name, team_name);
    }

    let agent_id = identity::format_agent_id(name, &team_name);
    let color = helpers::assign_color_for_teammate(&team_file, &agent_id);
    let now = chrono::Utc::now().timestamp();
    let cwd = ctx.cwd.to_string_lossy().into_owned();
    let agent_type = crate::coordinator::default_teammate_agent_type().to_string();
    let system_prompt = (agent_type == crate::coordinator::WORKER_AGENT_TYPE)
        .then(|| {
            allthecodes_engine::agent_runtime::builtin_agent_prompt(
                crate::coordinator::WORKER_AGENT_TYPE,
            )
        })
        .flatten();
    let system_prompt_mode = system_prompt
        .as_ref()
        .map(|_| crate::types::SystemPromptMode::Append);

    team_file.members.push(TeamMember {
        agent_id: agent_id.clone(),
        name: name.into(),
        agent_type: Some(agent_type.clone()),
        model: None,
        prompt: Some(prompt.into()),
        color: Some(color.clone()),
        plan_mode_required: None,
        joined_at: now,
        tmux_pane_id: String::new(),
        cwd: cwd.clone(),
        worktree_path: None,
        session_id: None,
        task_id: None,
        task_path: None,
        subscriptions: vec![],
        backend_type: Some(BackendType::InProcess),
        is_active: Some(true),
        mode: None,
        close_state: None,
        close_requested_at: None,
    });
    if let Err(e) = helpers::write_team_file(&team_name, &team_file) {
        return format!("Failed to update team file: {}", e);
    }

    let backend = InProcessBackend::new();
    let spawn_result = match backend
        .spawn(TeammateSpawnConfig {
            name: name.into(),
            team_name: team_name.clone(),
            color: Some(color.clone()),
            plan_mode_required: false,
            prompt: prompt.into(),
            agent_type: Some(agent_type.clone()),
            cwd: cwd.clone(),
            model: None,
            system_prompt,
            system_prompt_mode,
            worktree_path: None,
            parent_session_id: ctx.session_id.to_string(),
            permissions: vec![],
            allow_permission_prompts: false,
            hooks: ctx.app_state.hooks.clone(),
            hook_runner: None,
        })
        .await
    {
        Ok(result) if result.success => result,
        Ok(result) => {
            let _ = helpers::set_member_active(&team_name, &agent_id, false);
            return result
                .error
                .unwrap_or_else(|| format!("Failed to spawn '{}' in team '{}'", name, team_name));
        }
        Err(e) => {
            let _ = helpers::set_member_active(&team_name, &agent_id, false);
            return format!("Failed to spawn '{}': {}", name, e);
        }
    };
    let task_id = spawn_result.task_id.unwrap_or_default();
    let task_path = if task_id.is_empty() {
        None
    } else {
        helpers::set_member_task(&team_name, &agent_id, &task_id)
            .ok()
            .map(|path| path.to_string_lossy().into_owned())
    };

    // Update session team_context.
    if let Some(tc) = ctx.app_state.team_context.as_mut() {
        tc.teammates.insert(
            agent_id.clone(),
            TeammateInfo {
                name: name.into(),
                agent_type: Some(agent_type.clone()),
                color: Some(color.clone()),
                tmux_session_name: String::new(),
                tmux_pane_id: String::new(),
                cwd,
                worktree_path: None,
                spawned_at: now,
            },
        );
    }

    format!(
        "Spawned '{}' in team '{}' (agent_id={}, task_id={}, task_path={}, backend=in-process, color={}).",
        name,
        team_name,
        agent_id,
        task_id,
        task_path.as_deref().unwrap_or("-"),
        color
    )
}

fn send(ctx: &allthecodes_commands::CommandContext, rest: &str) -> String {
    let mut parts = rest.splitn(2, char_is_whitespace);
    let to = parts.next().unwrap_or("").trim();
    let text = parts.next().unwrap_or("").trim();
    if to.is_empty() || text.is_empty() {
        return "Usage: /team send <name> <message>".into();
    }
    let Some(tc) = ctx.app_state.team_context.as_ref() else {
        return "No active team. Create one first with '/team create <name>'.".into();
    };

    let sender = tc
        .self_agent_name
        .clone()
        .unwrap_or_else(|| constants::TEAM_LEAD_NAME.into());
    let now = chrono::Utc::now();
    let msg = TeammateMessage {
        from: sender.clone(),
        text: text.into(),
        timestamp: now.to_rfc3339(),
        read: false,
        color: None,
        summary: None,
    };
    match mailbox::write_to_mailbox(to, msg, &tc.team_name) {
        Ok(()) => format!("Message queued for '{}'.", to),
        Err(e) => format!("Failed to send: {}", e),
    }
}

async fn kill(ctx: &mut allthecodes_commands::CommandContext, rest: &str) -> String {
    let name = rest.trim();
    if name.is_empty() {
        return "Usage: /team kill <name>".into();
    }
    let Some(tc) = ctx.app_state.team_context.as_ref() else {
        return "No active team.".into();
    };
    let agent_id = identity::format_agent_id(name, &tc.team_name);
    let backend = InProcessBackend::new();
    let killed = backend.kill(&agent_id).await;
    if killed {
        let unassigned = allthecodes_engine::agent_runtime::unassign_teammate_tasks(
            &tc.team_name,
            &agent_id,
            name,
            allthecodes_tasks::TeammateTaskExitReason::Terminated,
        );
        // Flip is_active in team file and remove from teammates map.
        let _ = helpers::set_member_active(&tc.team_name, &agent_id, false);
        if let Some(tc_mut) = ctx.app_state.team_context.as_mut() {
            tc_mut.teammates.remove(&agent_id);
        }
        if unassigned.unassigned_tasks.is_empty() {
            format!("Killed '{}' ({}).", name, agent_id)
        } else {
            format!(
                "Killed '{}' ({}). {}",
                name, agent_id, unassigned.notification_message
            )
        }
    } else {
        format!("No active teammate named '{}'.", name)
    }
}

fn leave(ctx: &mut allthecodes_commands::CommandContext) -> String {
    match ctx.app_state.team_context.take() {
        Some(tc) => format!(
            "Left team '{}' (team data still on disk; use '/team delete' to remove).",
            tc.team_name
        ),
        None => "No active team to leave.".into(),
    }
}

async fn delete(ctx: &mut allthecodes_commands::CommandContext, rest: &str) -> String {
    let name = rest.trim();
    if name.is_empty() {
        return "Usage: /team delete <name>".into();
    }
    if !helpers::team_exists(name) {
        return format!("Team '{}' does not exist.", name);
    }
    if let Ok(team_file) = helpers::read_team_file(name) {
        let backend = InProcessBackend::new();
        let mut unassigned_messages = Vec::new();
        for member in helpers::get_non_lead_members(&team_file) {
            if member.backend_type.unwrap_or(BackendType::InProcess) == BackendType::InProcess {
                let _ = backend.kill(&member.agent_id).await;
                let unassigned = allthecodes_engine::agent_runtime::unassign_teammate_tasks(
                    name,
                    &member.agent_id,
                    &member.name,
                    allthecodes_tasks::TeammateTaskExitReason::Terminated,
                );
                if !unassigned.unassigned_tasks.is_empty() {
                    unassigned_messages.push(unassigned.notification_message);
                }
            }
        }
        if !unassigned_messages.is_empty() {
            tracing::info!(
                team_name = %name,
                messages = ?unassigned_messages,
                "team delete unassigned teammate tasks"
            );
        }
    }
    match helpers::cleanup_team_directories(name) {
        Ok(()) => {
            // If the deleted team was active, clear the context.
            if ctx
                .app_state
                .team_context
                .as_ref()
                .map(|tc| tc.team_name == name)
                .unwrap_or(false)
            {
                ctx.app_state.team_context = None;
            }
            format!("Team '{}' deleted.", name)
        }
        Err(e) => format!("Failed to delete team '{}': {}", name, e),
    }
}

#[cfg(test)]
mod tests;
