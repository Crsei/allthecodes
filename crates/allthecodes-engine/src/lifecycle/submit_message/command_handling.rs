use std::sync::Arc;
use std::time::Duration;

use allthecodes_types::sdk::UsageTracking;
use tracing::warn;
use uuid::Uuid;

use crate::command_runtime::{CommandContext, CommandResult};
use crate::input_processing;
use crate::types::message::{Message, MessageContent, UserMessage};

use super::super::QueryEngineState;

pub(super) struct LocalCommandOutcome {
    pub(super) is_error: bool,
    pub(super) session_id: crate::bootstrap::SessionId,
}

impl LocalCommandOutcome {
    fn new(session_id: crate::bootstrap::SessionId) -> Self {
        Self {
            is_error: false,
            session_id,
        }
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "local command execution needs submit state, session state, and command adapters"
)]
pub(super) async fn handle_parsed_command(
    processed: &mut input_processing::ProcessedInput,
    current_messages: &[Message],
    config: &crate::types::config::QueryEngineConfig,
    state_ref: &Arc<parking_lot::RwLock<QueryEngineState>>,
    active_session_id_ref: &Arc<parking_lot::RwLock<crate::bootstrap::SessionId>>,
    session_id: &crate::bootstrap::SessionId,
    hook_runner: &Arc<dyn allthecodes_types::hooks::HookRunner>,
    command_dispatcher: &dyn allthecodes_types::commands::CommandDispatcher,
    command_executor: &dyn crate::command_runtime::CommandExecutor,
) -> LocalCommandOutcome {
    let mut outcome = LocalCommandOutcome::new(session_id.clone());
    let Some(parsed_command) = processed.parsed_command.take() else {
        return outcome;
    };

    let command_name = command_dispatcher
        .command_name_for_cwd(parsed_command.index, std::path::Path::new(&config.cwd))
        .unwrap_or_else(|| format!("#{}", parsed_command.index));

    let mut ctx = CommandContext {
        messages: current_messages.to_vec(),
        cwd: std::path::PathBuf::from(&config.cwd),
        app_state: state_ref.read().app_state.clone(),
        session_id: session_id.clone(),
        hook_runner: hook_runner.clone(),
    };

    match command_executor
        .execute(parsed_command, command_name.clone(), &mut ctx)
        .await
    {
        Ok(CommandResult::Output(text)) => {
            apply_command_state(state_ref, ctx);
            processed.result_text = Some(text);
            processed.should_query = false;
            processed.messages.clear();
        }
        Ok(CommandResult::Query(messages)) => {
            apply_command_state(state_ref, ctx);
            processed.messages = messages;
            processed.should_query = true;
            processed.result_text = None;
        }
        Ok(CommandResult::SwitchSession {
            session_id,
            messages,
            notice,
        }) => {
            switch_command_session(
                state_ref,
                active_session_id_ref,
                session_id.clone(),
                messages,
                ctx,
            );
            outcome.session_id = session_id;
            processed.result_text = Some(notice);
            processed.should_query = false;
            processed.messages.clear();
        }
        Ok(CommandResult::Clear) => {
            outcome.session_id =
                clear_command_session(state_ref, active_session_id_ref, config, ctx);
            processed.result_text = Some("Conversation cleared.".to_string());
            processed.should_query = false;
            processed.messages.clear();
        }
        Ok(CommandResult::Exit(text)) => {
            apply_command_state(state_ref, ctx);
            processed.result_text = Some(text);
            processed.should_query = false;
            processed.messages.clear();
        }
        Ok(CommandResult::None) => {
            apply_command_state(state_ref, ctx);
            processed.result_text = Some(String::new());
            processed.should_query = false;
            processed.messages.clear();
        }
        Err(err) => {
            outcome.is_error = true;
            processed.result_text = Some(format!("Command /{} failed: {}", command_name, err));
            processed.should_query = false;
            processed.messages.clear();
        }
    }

    outcome
}

fn apply_command_state(
    state_ref: &Arc<parking_lot::RwLock<QueryEngineState>>,
    ctx: CommandContext,
) {
    let mut state = state_ref.write();
    state.transcript.messages = ctx.messages;
    state.app_state = ctx.app_state;
}

fn switch_command_session(
    state_ref: &Arc<parking_lot::RwLock<QueryEngineState>>,
    active_session_id_ref: &Arc<parking_lot::RwLock<crate::bootstrap::SessionId>>,
    session_id: crate::bootstrap::SessionId,
    messages: Vec<Message>,
    ctx: CommandContext,
) {
    {
        let mut state = state_ref.write();
        state.transcript.messages = messages;
        state.app_state = ctx.app_state;
    }
    *active_session_id_ref.write() = session_id.clone();
    crate::bootstrap::PROCESS_STATE.write().session_id = session_id;
}

fn clear_command_session(
    state_ref: &Arc<parking_lot::RwLock<QueryEngineState>>,
    active_session_id_ref: &Arc<parking_lot::RwLock<crate::bootstrap::SessionId>>,
    config: &crate::types::config::QueryEngineConfig,
    ctx: CommandContext,
) -> crate::bootstrap::SessionId {
    let previous_id = active_session_id_ref.read().clone();
    if config.auto_save_session && !ctx.messages.is_empty() {
        if let Err(err) =
            crate::session::storage::save_session(previous_id.as_str(), &ctx.messages, &config.cwd)
        {
            warn!(
                error = %err,
                session = %previous_id,
                "failed to save previous session before command clear"
            );
        }
    }

    let new_session_id = crate::bootstrap::SessionId::new();
    {
        let mut state = state_ref.write();
        state.transcript.messages.clear();
        state.transcript.usage = UsageTracking::default();
        state.permissions.denials.clear();
        state.transcript.total_turn_count = 0;
        state.app_state = ctx.app_state;
    }
    *active_session_id_ref.write() = new_session_id.clone();
    crate::bootstrap::PROCESS_STATE.write().session_id = new_session_id.clone();
    super::super::set_proactive_context_blocked(false, "context_ready");
    new_session_id
}

pub(super) fn skill_args_from_prompt(prompt: &str, skill_name: &str) -> String {
    let trimmed = prompt.trim();
    let Some(without_slash) = trimmed.strip_prefix('/') else {
        return String::new();
    };
    without_slash
        .strip_prefix(skill_name)
        .unwrap_or_default()
        .trim_start()
        .to_string()
}

pub(super) async fn bash_mode_result_message(prompt: &str, cwd: &str) -> anyhow::Result<Message> {
    let command_text = prompt.trim().trim_start_matches('!').trim();
    if command_text.is_empty() {
        anyhow::bail!("bash mode command cannot be empty");
    }

    #[cfg(windows)]
    let mut command = {
        let mut cmd = tokio::process::Command::new("cmd");
        cmd.arg("/C").arg(command_text);
        cmd
    };

    #[cfg(not(windows))]
    let mut command = {
        let mut cmd = tokio::process::Command::new("sh");
        cmd.arg("-lc").arg(command_text);
        cmd
    };

    command.current_dir(cwd);
    crate::tools::exec::process_control::configure_process_group(&mut command);
    command.kill_on_drop(true);
    let child = command.spawn()?;
    let pid = child.id();
    let output = match tokio::time::timeout(Duration::from_secs(30), child.wait_with_output()).await
    {
        Ok(result) => result?,
        Err(_) => {
            crate::tools::exec::process_control::terminate_process_tree(pid).await?;
            return Err(anyhow::anyhow!("bash mode command timed out after 30s"));
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let exit_code = output
        .status
        .code()
        .map(|code| code.to_string())
        .unwrap_or_else(|| "terminated by signal".to_string());
    let content = format!(
        "<bash_command>\n$ {command_text}\n\nexit_code: {exit_code}\n\nstdout:\n{stdout}\n\nstderr:\n{stderr}\n</bash_command>"
    );

    Ok(Message::User(UserMessage {
        uuid: Uuid::new_v4(),
        timestamp: chrono::Utc::now().timestamp_millis(),
        role: "user".to_string(),
        content: MessageContent::Text(content),
        is_meta: true,
        tool_use_result: None,
        source_tool_assistant_uuid: None,
    }))
}
