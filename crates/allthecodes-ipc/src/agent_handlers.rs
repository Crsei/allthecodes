//! Runtime IPC handlers for agent and team commands.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use allthecodes_ipc_protocol::protocol::BackendMessage;
use allthecodes_types::agent_events::{AgentCommand, AgentEvent, TeamCommand, TeamEvent};
use allthecodes_types::agent_types::{AgentNode, ForkLaunchMetadata, TeamMemberInfo};
use allthecodes_types::output::OutputReadBatch;

pub const DEFAULT_AGENT_OUTPUT_LIMIT_BYTES: usize = 64 * 1024;
pub const MAX_AGENT_OUTPUT_LIMIT_BYTES: usize = 64 * 1024;
pub const MAX_TEAM_MESSAGE_BYTES: usize = 16 * 1024;

const MAX_IDENTIFIER_BYTES: usize = 256;
const MAX_TREE_NODES: usize = 64;
const MAX_TREE_ROOTS: usize = 32;
const MAX_TREE_CHILDREN: usize = 32;
const MAX_DESCRIPTION_BYTES: usize = 1024;
const MAX_RESULT_PREVIEW_BYTES: usize = 2 * 1024;
const MAX_TEAM_MEMBERS: usize = 128;
const MAX_ROLE_BYTES: usize = 512;
const MAX_RESPONSE_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandScopePolicy {
    EnforceOwner,
    TrustedLocal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedCommandContext {
    session_id: String,
    canonical_workspace: PathBuf,
    server_sender_id: String,
    privileged_mutations: bool,
    scope_policy: CommandScopePolicy,
}

impl TrustedCommandContext {
    pub fn web(
        session_id: impl Into<String>,
        canonical_workspace: PathBuf,
        server_sender_id: impl Into<String>,
        privileged_mutations: bool,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            canonical_workspace,
            server_sender_id: server_sender_id.into(),
            privileged_mutations,
            scope_policy: CommandScopePolicy::EnforceOwner,
        }
    }

    pub fn trusted_local(
        session_id: impl Into<String>,
        canonical_workspace: PathBuf,
        server_sender_id: impl Into<String>,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            canonical_workspace,
            server_sender_id: server_sender_id.into(),
            privileged_mutations: true,
            scope_policy: CommandScopePolicy::TrustedLocal,
        }
    }

    pub fn legacy_trusted_local() -> Self {
        Self::trusted_local("__local__", PathBuf::new(), "__local__")
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn canonical_workspace(&self) -> &Path {
        &self.canonical_workspace
    }

    pub fn server_sender_id(&self) -> &str {
        &self.server_sender_id
    }

    pub fn privileged_mutations(&self) -> bool {
        self.privileged_mutations
    }

    pub fn scope_policy(&self) -> CommandScopePolicy {
        self.scope_policy
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeHostError {
    NotFound,
    Unauthorized,
    Unavailable,
    Conflict,
    Terminal,
    InvalidRecipient,
}

pub trait AgentRuntimeHost: Send + Sync + 'static {
    // Legacy methods remain as trusted-local compatibility points.
    fn cancel_agent(&self, _agent_id: &str) -> Option<String> {
        None
    }

    fn agent_output(&self, _agent_id: &str) -> Option<AgentTaskOutput> {
        None
    }

    fn agent_output_batch(
        &self,
        _agent_id: &str,
        _after_seq: Option<u64>,
        _limit_bytes: usize,
    ) -> Option<AgentTaskOutputBatch> {
        None
    }

    fn update_agent_state(
        &self,
        _agent_id: &str,
        _state: &str,
        _result_preview: Option<String>,
        _duration_ms: Option<u64>,
        _had_error: bool,
    ) {
    }

    fn agent_tree_snapshot(&self) -> Vec<AgentNode> {
        Vec::new()
    }

    fn write_team_message(&self, _team_name: &str, _to: &str, _text: &str) -> Result<(), String> {
        Err("team runtime unavailable".to_string())
    }

    fn team_members(&self, _team_name: &str) -> Result<Vec<TeamMemberInfo>, String> {
        Err("team runtime unavailable".to_string())
    }

    // Remote transports must use these owner-aware methods. The defaults are
    // fail-closed; the local path deliberately delegates to the legacy host.
    fn cancel_agent_for_scope(
        &self,
        context: &TrustedCommandContext,
        agent_id: &str,
    ) -> Result<Option<String>, RuntimeHostError> {
        if context.scope_policy() == CommandScopePolicy::TrustedLocal {
            Ok(self.cancel_agent(agent_id))
        } else {
            Err(RuntimeHostError::Unauthorized)
        }
    }

    fn agent_output_batch_for_scope(
        &self,
        context: &TrustedCommandContext,
        agent_id: &str,
        after_seq: Option<u64>,
        limit_bytes: usize,
    ) -> Result<Option<AgentTaskOutputBatch>, RuntimeHostError> {
        if context.scope_policy() == CommandScopePolicy::TrustedLocal {
            Ok(self.agent_output_batch(agent_id, after_seq, limit_bytes))
        } else {
            Err(RuntimeHostError::Unauthorized)
        }
    }

    fn agent_tree_snapshot_for_scope(
        &self,
        context: &TrustedCommandContext,
    ) -> Result<Vec<AgentNode>, RuntimeHostError> {
        if context.scope_policy() == CommandScopePolicy::TrustedLocal {
            Ok(self.agent_tree_snapshot())
        } else {
            Err(RuntimeHostError::Unauthorized)
        }
    }

    fn write_team_message_for_scope(
        &self,
        context: &TrustedCommandContext,
        team_name: &str,
        to: &str,
        _from: &str,
        text: &str,
    ) -> Result<(), RuntimeHostError> {
        if context.scope_policy() != CommandScopePolicy::TrustedLocal {
            return Err(RuntimeHostError::Unauthorized);
        }
        self.write_team_message(team_name, to, text)
            .map_err(|_| RuntimeHostError::Unavailable)
    }

    fn team_members_for_scope(
        &self,
        context: &TrustedCommandContext,
        team_name: &str,
    ) -> Result<Vec<TeamMemberInfo>, RuntimeHostError> {
        if context.scope_policy() != CommandScopePolicy::TrustedLocal {
            return Err(RuntimeHostError::Unauthorized);
        }
        self.team_members(team_name)
            .map_err(|_| RuntimeHostError::NotFound)
    }
}

#[derive(Debug, Clone)]
pub struct AgentTaskOutput {
    pub id: String,
    pub output: String,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct AgentTaskOutputBatch {
    pub id: String,
    pub output: OutputReadBatch,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default)]
pub struct CommandDispatch {
    pub direct: Vec<BackendMessage>,
    pub publish: Vec<BackendMessage>,
}

impl CommandDispatch {
    pub fn into_messages(mut self) -> Vec<BackendMessage> {
        self.direct.append(&mut self.publish);
        self.direct
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandErrorCode {
    InvalidCommand,
    InvalidOutputLimit,
    MessageTooLarge,
    TargetUnavailable,
    TargetAlreadyTerminal,
    MutationConflict,
    RuntimeUnavailable,
    OutputUnavailable,
    PrivilegedCapabilityRequired,
}

impl CommandErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidCommand => "invalid_command",
            Self::InvalidOutputLimit => "invalid_output_limit",
            Self::MessageTooLarge => "message_too_large",
            Self::TargetUnavailable => "target_unavailable",
            Self::TargetAlreadyTerminal => "target_already_terminal",
            Self::MutationConflict => "mutation_conflict",
            Self::RuntimeUnavailable => "runtime_unavailable",
            Self::OutputUnavailable => "output_unavailable",
            Self::PrivilegedCapabilityRequired => "privileged_capability_required",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandError {
    pub code: CommandErrorCode,
    pub message: &'static str,
}

impl CommandError {
    fn new(code: CommandErrorCode, message: &'static str) -> Self {
        Self { code, message }
    }

    pub fn into_backend_message(self) -> BackendMessage {
        BackendMessage::Error {
            message: format!("{}: {}", self.code.as_str(), self.message),
            recoverable: true,
        }
    }
}

static HOST: OnceLock<Arc<dyn AgentRuntimeHost>> = OnceLock::new();

pub fn set_runtime_host(host: Arc<dyn AgentRuntimeHost>) {
    let _ = HOST.set(host);
}

pub fn dispatch_agent_command(
    context: &TrustedCommandContext,
    command: AgentCommand,
) -> Result<CommandDispatch, CommandError> {
    dispatch_agent_command_with_host(HOST.get().map(Arc::as_ref), context, command)
}

pub fn dispatch_team_command(
    context: &TrustedCommandContext,
    command: TeamCommand,
) -> Result<CommandDispatch, CommandError> {
    dispatch_team_command_with_host(HOST.get().map(Arc::as_ref), context, command)
}

pub fn project_agent_event_for_web(
    context: &TrustedCommandContext,
    event: AgentEvent,
) -> Result<AgentEvent, CommandError> {
    let event = match event {
        AgentEvent::Spawned {
            agent_id,
            parent_agent_id,
            description,
            agent_type,
            model,
            is_background,
            depth,
            chain_id,
            mut fork_metadata,
        } => {
            if let Some(metadata) = &mut fork_metadata {
                metadata.live_channel = None;
            }
            AgentEvent::Spawned {
                agent_id: bounded_redacted(context, &agent_id, MAX_IDENTIFIER_BYTES),
                parent_agent_id: parent_agent_id
                    .as_deref()
                    .map(|value| bounded_redacted(context, value, MAX_IDENTIFIER_BYTES)),
                description: bounded_redacted(context, &description, MAX_DESCRIPTION_BYTES),
                agent_type: agent_type
                    .as_deref()
                    .map(|value| bounded_redacted(context, value, MAX_IDENTIFIER_BYTES)),
                model: model
                    .as_deref()
                    .map(|value| bounded_redacted(context, value, MAX_IDENTIFIER_BYTES)),
                is_background,
                depth,
                chain_id: bounded_redacted(context, &chain_id, MAX_IDENTIFIER_BYTES),
                fork_metadata,
            }
        }
        AgentEvent::Completed {
            agent_id,
            result_preview,
            had_error,
            completion_status,
            duration_ms,
            total_tokens,
            output_tokens,
            tool_uses,
            agent_type,
        } => AgentEvent::Completed {
            agent_id: bounded_redacted(context, &agent_id, MAX_IDENTIFIER_BYTES),
            result_preview: bounded_redacted(context, &result_preview, MAX_RESULT_PREVIEW_BYTES),
            had_error,
            completion_status,
            duration_ms,
            total_tokens,
            output_tokens,
            tool_uses,
            agent_type: agent_type
                .as_deref()
                .map(|value| bounded_redacted(context, value, MAX_IDENTIFIER_BYTES)),
        },
        AgentEvent::Error {
            agent_id,
            error,
            duration_ms,
        } => AgentEvent::Error {
            agent_id: bounded_redacted(context, &agent_id, MAX_IDENTIFIER_BYTES),
            error: bounded_redacted(context, &error, MAX_RESULT_PREVIEW_BYTES),
            duration_ms,
        },
        AgentEvent::Aborted { agent_id } => AgentEvent::Aborted {
            agent_id: bounded_redacted(context, &agent_id, MAX_IDENTIFIER_BYTES),
        },
        AgentEvent::StreamDelta { agent_id, text } => AgentEvent::StreamDelta {
            agent_id: bounded_redacted(context, &agent_id, MAX_IDENTIFIER_BYTES),
            text: bounded_redacted(context, &text, MAX_TEAM_MESSAGE_BYTES),
        },
        AgentEvent::ThinkingDelta { agent_id, thinking } => AgentEvent::ThinkingDelta {
            agent_id: bounded_redacted(context, &agent_id, MAX_IDENTIFIER_BYTES),
            thinking: bounded_redacted(context, &thinking, MAX_TEAM_MESSAGE_BYTES),
        },
        AgentEvent::ToolUse {
            agent_id,
            tool_use_id,
            tool_name,
            input,
        } => AgentEvent::ToolUse {
            agent_id: bounded_redacted(context, &agent_id, MAX_IDENTIFIER_BYTES),
            tool_use_id: bounded_redacted(context, &tool_use_id, MAX_IDENTIFIER_BYTES),
            tool_name: bounded_redacted(context, &tool_name, MAX_IDENTIFIER_BYTES),
            input: project_json_value(context, input, 0),
        },
        AgentEvent::ToolResult {
            agent_id,
            tool_use_id,
            output,
            is_error,
        } => AgentEvent::ToolResult {
            agent_id: bounded_redacted(context, &agent_id, MAX_IDENTIFIER_BYTES),
            tool_use_id: bounded_redacted(context, &tool_use_id, MAX_IDENTIFIER_BYTES),
            output: bounded_redacted(context, &output, MAX_TEAM_MESSAGE_BYTES),
            is_error,
        },
        AgentEvent::PermissionQueued {
            agent_id,
            tool_use_id,
            tool_name,
            summary,
            queue_position,
            pending_count,
        } => AgentEvent::PermissionQueued {
            agent_id: bounded_redacted(context, &agent_id, MAX_IDENTIFIER_BYTES),
            tool_use_id: bounded_redacted(context, &tool_use_id, MAX_IDENTIFIER_BYTES),
            tool_name: bounded_redacted(context, &tool_name, MAX_IDENTIFIER_BYTES),
            summary: bounded_redacted(context, &summary, MAX_DESCRIPTION_BYTES),
            queue_position,
            pending_count,
        },
        AgentEvent::PermissionResolved {
            agent_id,
            tool_use_id,
            tool_name,
            decision,
            pending_count,
        } => AgentEvent::PermissionResolved {
            agent_id: bounded_redacted(context, &agent_id, MAX_IDENTIFIER_BYTES),
            tool_use_id: bounded_redacted(context, &tool_use_id, MAX_IDENTIFIER_BYTES),
            tool_name: bounded_redacted(context, &tool_name, MAX_IDENTIFIER_BYTES),
            decision: bounded_redacted(context, &decision, MAX_IDENTIFIER_BYTES),
            pending_count,
        },
        AgentEvent::RuntimeActivity {
            task_id,
            agent_id,
            child_session_id,
            phase,
            last_heartbeat_at_ms,
            last_progress_at_ms,
            partial_output_bytes,
        } => AgentEvent::RuntimeActivity {
            task_id: bounded_redacted(context, &task_id, MAX_IDENTIFIER_BYTES),
            agent_id: bounded_redacted(context, &agent_id, MAX_IDENTIFIER_BYTES),
            child_session_id: bounded_redacted(context, &child_session_id, MAX_IDENTIFIER_BYTES),
            phase: bounded_redacted(context, &phase, MAX_IDENTIFIER_BYTES),
            last_heartbeat_at_ms,
            last_progress_at_ms,
            partial_output_bytes,
        },
        AgentEvent::TreeSnapshot { roots } => AgentEvent::TreeSnapshot {
            roots: project_agent_tree(context, roots),
        },
        AgentEvent::OutputBatch {
            agent_id,
            task_id,
            output,
            mut fork_metadata,
        } => {
            if let Some(metadata) = &mut fork_metadata {
                metadata.live_channel = None;
            }
            AgentEvent::OutputBatch {
                agent_id: bounded_redacted(context, &agent_id, MAX_IDENTIFIER_BYTES),
                task_id: bounded_redacted(context, &task_id, MAX_IDENTIFIER_BYTES),
                output: project_output_batch(context, output, None, MAX_AGENT_OUTPUT_LIMIT_BYTES)?,
                fork_metadata,
            }
        }
        AgentEvent::ExecutionRecord {
            agent_id,
            mut record,
        } => {
            record.session_id = bounded_redacted(context, &record.session_id, MAX_IDENTIFIER_BYTES);
            record.agent_id = bounded_redacted(context, &record.agent_id, MAX_IDENTIFIER_BYTES);
            record.parent_agent_id = record
                .parent_agent_id
                .as_deref()
                .map(|value| bounded_redacted(context, value, MAX_IDENTIFIER_BYTES));
            record.agent_role = record
                .agent_role
                .as_deref()
                .map(|value| bounded_redacted(context, value, MAX_IDENTIFIER_BYTES));
            record.tool = bounded_redacted(context, &record.tool, MAX_IDENTIFIER_BYTES);
            record.tool_use_id = record
                .tool_use_id
                .as_deref()
                .map(|value| bounded_redacted(context, value, MAX_IDENTIFIER_BYTES));
            record.command = None;
            record.cwd = None;
            record.stdout_digest = None;
            record.stderr_digest = None;
            record.model = record
                .model
                .as_deref()
                .map(|value| bounded_redacted(context, value, MAX_IDENTIFIER_BYTES));
            AgentEvent::ExecutionRecord {
                agent_id: bounded_redacted(context, &agent_id, MAX_IDENTIFIER_BYTES),
                record,
            }
        }
    };
    let message = BackendMessage::AgentEvent {
        event: event.clone(),
    };
    ensure_response_bound(&message)?;
    Ok(event)
}

pub fn project_team_event_for_web(
    context: &TrustedCommandContext,
    event: TeamEvent,
) -> Result<TeamEvent, CommandError> {
    let event = match event {
        TeamEvent::MemberJoined {
            team_name,
            agent_id,
            agent_name,
            role,
        } => TeamEvent::MemberJoined {
            team_name: bounded_redacted(context, &team_name, MAX_IDENTIFIER_BYTES),
            agent_id: bounded_redacted(context, &agent_id, MAX_IDENTIFIER_BYTES),
            agent_name: bounded_redacted(context, &agent_name, MAX_IDENTIFIER_BYTES),
            role: role
                .as_deref()
                .map(|value| bounded_redacted(context, value, MAX_ROLE_BYTES)),
        },
        TeamEvent::MemberLeft {
            team_name,
            agent_id,
            agent_name,
        } => TeamEvent::MemberLeft {
            team_name: bounded_redacted(context, &team_name, MAX_IDENTIFIER_BYTES),
            agent_id: bounded_redacted(context, &agent_id, MAX_IDENTIFIER_BYTES),
            agent_name: bounded_redacted(context, &agent_name, MAX_IDENTIFIER_BYTES),
        },
        TeamEvent::MessageRouted {
            team_name,
            from,
            to,
            text,
            timestamp,
            summary,
        } => TeamEvent::MessageRouted {
            team_name: bounded_redacted(context, &team_name, MAX_IDENTIFIER_BYTES),
            from: bounded_redacted(context, &from, MAX_IDENTIFIER_BYTES),
            to: bounded_redacted(context, &to, MAX_IDENTIFIER_BYTES),
            text: bounded_redacted(context, &text, MAX_TEAM_MESSAGE_BYTES),
            timestamp: bounded_redacted(context, &timestamp, MAX_IDENTIFIER_BYTES),
            summary: summary
                .as_deref()
                .map(|value| bounded_redacted(context, value, MAX_DESCRIPTION_BYTES)),
        },
        TeamEvent::StatusSnapshot {
            team_name,
            members,
            pending_messages,
        } => TeamEvent::StatusSnapshot {
            team_name: bounded_redacted(context, &team_name, MAX_IDENTIFIER_BYTES),
            members: project_team_members(context, members),
            pending_messages,
        },
    };
    let message = BackendMessage::TeamEvent {
        event: event.clone(),
    };
    ensure_response_bound(&message)?;
    Ok(event)
}

pub fn handle_agent_command(command: AgentCommand) -> Vec<BackendMessage> {
    dispatch_agent_command(&TrustedCommandContext::legacy_trusted_local(), command)
        .map(CommandDispatch::into_messages)
        .unwrap_or_else(|error| vec![error.into_backend_message()])
}

pub fn handle_team_command(command: TeamCommand) -> Vec<BackendMessage> {
    dispatch_team_command(&TrustedCommandContext::legacy_trusted_local(), command)
        .map(CommandDispatch::into_messages)
        .unwrap_or_else(|error| vec![error.into_backend_message()])
}

pub fn build_team_status_events(team_name: &str) -> Vec<BackendMessage> {
    handle_team_command(TeamCommand::QueryTeamStatus {
        team_name: team_name.to_string(),
    })
}

fn dispatch_agent_command_with_host(
    host: Option<&dyn AgentRuntimeHost>,
    context: &TrustedCommandContext,
    command: AgentCommand,
) -> Result<CommandDispatch, CommandError> {
    let host = host.ok_or_else(runtime_unavailable)?;
    match command {
        AgentCommand::QueryActiveAgents => {
            let roots = host
                .agent_tree_snapshot_for_scope(context)
                .map_err(map_agent_host_error)?;
            let roots = project_agent_tree(context, roots);
            let message = BackendMessage::AgentEvent {
                event: AgentEvent::TreeSnapshot { roots },
            };
            ensure_response_bound(&message)?;
            Ok(CommandDispatch {
                direct: vec![message],
                publish: Vec::new(),
            })
        }
        AgentCommand::QueryAgentOutput {
            agent_id,
            after_seq,
            limit_bytes,
        } => {
            validate_identifier(&agent_id)?;
            let limit_bytes = validate_output_limit(limit_bytes)?;
            let task = host
                .agent_output_batch_for_scope(context, &agent_id, after_seq, limit_bytes)
                .map_err(map_agent_host_error)?
                .ok_or_else(output_unavailable)?;
            let output = project_output_batch(context, task.output, after_seq, limit_bytes)?;
            let message = BackendMessage::AgentEvent {
                event: AgentEvent::OutputBatch {
                    agent_id: bounded_redacted(context, &agent_id, MAX_IDENTIFIER_BYTES),
                    task_id: bounded_redacted(context, &task.id, MAX_IDENTIFIER_BYTES),
                    output,
                    fork_metadata: display_safe_fork_metadata(task.metadata.as_ref()),
                },
            };
            ensure_response_bound(&message)?;
            Ok(CommandDispatch {
                direct: vec![message],
                publish: Vec::new(),
            })
        }
        AgentCommand::AbortAgent { agent_id } => {
            validate_identifier(&agent_id)?;
            require_privileged(context)?;
            let task_id = host
                .cancel_agent_for_scope(context, &agent_id)
                .map_err(map_agent_host_error)?
                .ok_or_else(target_unavailable)?;
            if task_id.is_empty() {
                return Err(target_unavailable());
            }

            host.update_agent_state(&agent_id, "aborted", None, None, false);
            let mut publish = vec![BackendMessage::AgentEvent {
                event: AgentEvent::Aborted {
                    agent_id: bounded_redacted(context, &agent_id, MAX_IDENTIFIER_BYTES),
                },
            }];
            if let Ok(roots) = host.agent_tree_snapshot_for_scope(context) {
                let snapshot = BackendMessage::AgentEvent {
                    event: AgentEvent::TreeSnapshot {
                        roots: project_agent_tree(context, roots),
                    },
                };
                if ensure_response_bound(&snapshot).is_ok() {
                    publish.push(snapshot);
                }
            }
            ensure_response_bound(&publish[0])?;
            Ok(CommandDispatch {
                direct: Vec::new(),
                publish,
            })
        }
    }
}

fn dispatch_team_command_with_host(
    host: Option<&dyn AgentRuntimeHost>,
    context: &TrustedCommandContext,
    command: TeamCommand,
) -> Result<CommandDispatch, CommandError> {
    let host = host.ok_or_else(runtime_unavailable)?;
    match command {
        TeamCommand::QueryTeamStatus { team_name } => {
            validate_identifier(&team_name)?;
            let members = host
                .team_members_for_scope(context, &team_name)
                .map_err(map_team_host_error)?;
            let members = project_team_members(context, members);
            let pending_messages = members.iter().fold(0usize, |sum, member| {
                sum.saturating_add(member.unread_messages)
            });
            let message = BackendMessage::TeamEvent {
                event: TeamEvent::StatusSnapshot {
                    team_name: bounded_redacted(context, &team_name, MAX_IDENTIFIER_BYTES),
                    members,
                    pending_messages,
                },
            };
            ensure_response_bound(&message)?;
            Ok(CommandDispatch {
                direct: vec![message],
                publish: Vec::new(),
            })
        }
        TeamCommand::InjectMessage {
            team_name,
            to,
            text,
        } => {
            validate_identifier(&team_name)?;
            validate_identifier(&to)?;
            require_privileged(context)?;
            if text.is_empty() {
                return Err(CommandError::new(
                    CommandErrorCode::InvalidCommand,
                    "team message must not be empty",
                ));
            }
            if text.len() > MAX_TEAM_MESSAGE_BYTES {
                return Err(CommandError::new(
                    CommandErrorCode::MessageTooLarge,
                    "team message exceeds the 16 KiB limit",
                ));
            }
            let sender =
                bounded_redacted(context, context.server_sender_id(), MAX_IDENTIFIER_BYTES);
            host.write_team_message_for_scope(context, &team_name, &to, &sender, &text)
                .map_err(map_team_host_error)?;

            let message = BackendMessage::TeamEvent {
                event: TeamEvent::MessageRouted {
                    team_name: bounded_redacted(context, &team_name, MAX_IDENTIFIER_BYTES),
                    from: sender,
                    to: bounded_redacted(context, &to, MAX_IDENTIFIER_BYTES),
                    text: bounded_redacted(context, &text, MAX_TEAM_MESSAGE_BYTES),
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    summary: None,
                },
            };
            ensure_response_bound(&message)?;
            Ok(CommandDispatch {
                direct: Vec::new(),
                publish: vec![message],
            })
        }
    }
}

fn validate_output_limit(limit_bytes: Option<usize>) -> Result<usize, CommandError> {
    let limit = limit_bytes.unwrap_or(DEFAULT_AGENT_OUTPUT_LIMIT_BYTES);
    if limit == 0 || limit > MAX_AGENT_OUTPUT_LIMIT_BYTES {
        return Err(CommandError::new(
            CommandErrorCode::InvalidOutputLimit,
            "limit_bytes must be between 1 and 65536",
        ));
    }
    Ok(limit)
}

fn validate_identifier(value: &str) -> Result<(), CommandError> {
    if value.is_empty() || value.len() > MAX_IDENTIFIER_BYTES || value.chars().any(char::is_control)
    {
        return Err(CommandError::new(
            CommandErrorCode::InvalidCommand,
            "identifier is empty, oversized, or contains control characters",
        ));
    }
    Ok(())
}

fn require_privileged(context: &TrustedCommandContext) -> Result<(), CommandError> {
    if context.privileged_mutations() {
        Ok(())
    } else {
        Err(CommandError::new(
            CommandErrorCode::PrivilegedCapabilityRequired,
            "this mutation requires the privileged Web capability",
        ))
    }
}

fn project_agent_tree(context: &TrustedCommandContext, roots: Vec<AgentNode>) -> Vec<AgentNode> {
    let mut remaining = MAX_TREE_NODES;
    roots
        .into_iter()
        .take(MAX_TREE_ROOTS)
        .filter_map(|node| project_agent_node(context, node, &mut remaining))
        .collect()
}

fn project_agent_node(
    context: &TrustedCommandContext,
    mut node: AgentNode,
    remaining: &mut usize,
) -> Option<AgentNode> {
    if *remaining == 0 {
        return None;
    }
    *remaining -= 1;
    node.agent_id = bounded_redacted(context, &node.agent_id, MAX_IDENTIFIER_BYTES);
    node.parent_agent_id = node
        .parent_agent_id
        .as_deref()
        .map(|value| bounded_redacted(context, value, MAX_IDENTIFIER_BYTES));
    node.description = bounded_redacted(context, &node.description, MAX_DESCRIPTION_BYTES);
    node.agent_type = node
        .agent_type
        .as_deref()
        .map(|value| bounded_redacted(context, value, MAX_IDENTIFIER_BYTES));
    node.model = node
        .model
        .as_deref()
        .map(|value| bounded_redacted(context, value, MAX_IDENTIFIER_BYTES));
    node.state = bounded_redacted(context, &node.state, MAX_IDENTIFIER_BYTES);
    node.chain_id = bounded_redacted(context, &node.chain_id, MAX_IDENTIFIER_BYTES);
    node.result_preview = node
        .result_preview
        .as_deref()
        .map(|value| bounded_redacted(context, value, MAX_RESULT_PREVIEW_BYTES));
    if let Some(metadata) = &mut node.fork_metadata {
        metadata.live_channel = None;
    }
    node.children = node
        .children
        .into_iter()
        .take(MAX_TREE_CHILDREN)
        .filter_map(|child| project_agent_node(context, child, remaining))
        .collect();
    Some(node)
}

fn project_output_batch(
    context: &TrustedCommandContext,
    mut batch: OutputReadBatch,
    after_seq: Option<u64>,
    limit_bytes: usize,
) -> Result<OutputReadBatch, CommandError> {
    let mut previous = after_seq.unwrap_or(0);
    let mut output_bytes = 0usize;
    for event in &mut batch.events {
        if event.seq <= previous {
            return Err(runtime_unavailable());
        }
        previous = event.seq;
        if event.chunk.len() > limit_bytes {
            return Err(output_unavailable());
        }
        output_bytes = output_bytes.saturating_add(event.chunk.len());
        if output_bytes > limit_bytes {
            return Err(output_unavailable());
        }
        event.chunk = bounded_redacted(context, &event.chunk, limit_bytes);
        event.process_or_run_id =
            bounded_redacted(context, &event.process_or_run_id, MAX_IDENTIFIER_BYTES);
    }
    if let Some(last) = batch.events.last() {
        if batch.next_seq <= last.seq {
            return Err(runtime_unavailable());
        }
    }
    Ok(batch)
}

fn display_safe_fork_metadata(metadata: Option<&serde_json::Value>) -> Option<ForkLaunchMetadata> {
    let mut metadata = metadata
        .and_then(|metadata| metadata.get("fork"))
        .cloned()
        .and_then(|metadata| serde_json::from_value::<ForkLaunchMetadata>(metadata).ok())?;
    metadata.live_channel = None;
    Some(metadata)
}

fn project_team_members(
    context: &TrustedCommandContext,
    members: Vec<TeamMemberInfo>,
) -> Vec<TeamMemberInfo> {
    members
        .into_iter()
        .take(MAX_TEAM_MEMBERS)
        .map(|mut member| {
            member.agent_id = bounded_redacted(context, &member.agent_id, MAX_IDENTIFIER_BYTES);
            member.agent_name = bounded_redacted(context, &member.agent_name, MAX_IDENTIFIER_BYTES);
            member.role = member
                .role
                .as_deref()
                .map(|role| bounded_redacted(context, role, MAX_ROLE_BYTES));
            member
        })
        .collect()
}

fn project_json_value(
    context: &TrustedCommandContext,
    value: serde_json::Value,
    depth: usize,
) -> serde_json::Value {
    if depth >= 8 {
        return serde_json::Value::String("[truncated]".to_string());
    }
    match value {
        serde_json::Value::String(value) => {
            serde_json::Value::String(bounded_redacted(context, &value, MAX_DESCRIPTION_BYTES))
        }
        serde_json::Value::Array(values) => serde_json::Value::Array(
            values
                .into_iter()
                .take(64)
                .map(|value| project_json_value(context, value, depth + 1))
                .collect(),
        ),
        serde_json::Value::Object(values) => {
            let values = values
                .into_iter()
                .take(64)
                .map(|(key, value)| {
                    let projected_key = bounded_redacted(context, &key, MAX_IDENTIFIER_BYTES);
                    let lowered = key.to_ascii_lowercase();
                    let projected_value = if [
                        "authorization",
                        "api_key",
                        "apikey",
                        "password",
                        "secret",
                        "token",
                        "credential",
                    ]
                    .iter()
                    .any(|marker| lowered.contains(marker))
                    {
                        serde_json::Value::String("[redacted]".to_string())
                    } else {
                        project_json_value(context, value, depth + 1)
                    };
                    (projected_key, projected_value)
                })
                .collect();
            serde_json::Value::Object(values)
        }
        other => other,
    }
}

fn bounded_redacted(context: &TrustedCommandContext, value: &str, max_bytes: usize) -> String {
    let workspace = context.canonical_workspace().to_string_lossy();
    let mut redacted = if workspace.is_empty() {
        value.to_string()
    } else {
        value.replace(workspace.as_ref(), "<workspace>")
    };
    redacted = redact_sensitive_lines_and_paths(&redacted);
    truncate_utf8(redacted, max_bytes)
}

/// Apply the same workspace/path/secret projection used by Web IPC agent
/// events to an incremental runtime output chunk.
pub fn project_agent_output_for_web(
    context: &TrustedCommandContext,
    value: &str,
    max_bytes: usize,
) -> String {
    bounded_redacted(context, value, max_bytes.min(MAX_AGENT_OUTPUT_LIMIT_BYTES))
}

fn redact_sensitive_lines_and_paths(value: &str) -> String {
    value
        .split_inclusive('\n')
        .map(|line| {
            let lowercase = line.to_ascii_lowercase();
            if [
                "authorization:",
                "bearer ",
                "api_key=",
                "apikey=",
                "password=",
                "secret=",
                "token=",
                "sk-",
                "ghp_",
                "xoxb-",
            ]
            .iter()
            .any(|marker| lowercase.contains(marker))
            {
                if line.ends_with('\n') {
                    "[redacted]\n".to_string()
                } else {
                    "[redacted]".to_string()
                }
            } else {
                line.split_inclusive(char::is_whitespace)
                    .map(|token| {
                        let trimmed = token.trim_matches(|character: char| {
                            character.is_whitespace()
                                || matches!(character, '"' | '\'' | '(' | ')' | '[' | ']' | ',')
                        });
                        let looks_absolute = trimmed.starts_with('/')
                            || (trimmed.len() > 2
                                && trimmed.as_bytes()[1] == b':'
                                && matches!(trimmed.as_bytes()[2], b'\\' | b'/'));
                        if looks_absolute {
                            let content_end = token.trim_end_matches(char::is_whitespace).len();
                            let suffix = &token[content_end..];
                            format!("<redacted-path>{suffix}")
                        } else {
                            token.to_string()
                        }
                    })
                    .collect()
            }
        })
        .collect()
}

fn truncate_utf8(mut value: String, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value;
    }
    let mut boundary = max_bytes;
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value.truncate(boundary);
    value
}

fn ensure_response_bound(message: &BackendMessage) -> Result<(), CommandError> {
    let bytes = serde_json::to_vec(message).map_err(|_| runtime_unavailable())?;
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(CommandError::new(
            CommandErrorCode::RuntimeUnavailable,
            "runtime response exceeds the bounded IPC projection",
        ));
    }
    Ok(())
}

fn map_agent_host_error(error: RuntimeHostError) -> CommandError {
    match error {
        RuntimeHostError::Terminal => CommandError::new(
            CommandErrorCode::TargetAlreadyTerminal,
            "agent is already terminal",
        ),
        RuntimeHostError::Conflict => CommandError::new(
            CommandErrorCode::MutationConflict,
            "agent mutation conflicts with another transition",
        ),
        RuntimeHostError::Unavailable => runtime_unavailable(),
        RuntimeHostError::NotFound
        | RuntimeHostError::Unauthorized
        | RuntimeHostError::InvalidRecipient => target_unavailable(),
    }
}

fn map_team_host_error(error: RuntimeHostError) -> CommandError {
    match error {
        RuntimeHostError::Conflict => CommandError::new(
            CommandErrorCode::MutationConflict,
            "team mutation conflicts with another transition",
        ),
        RuntimeHostError::Unavailable => runtime_unavailable(),
        RuntimeHostError::Terminal
        | RuntimeHostError::NotFound
        | RuntimeHostError::Unauthorized
        | RuntimeHostError::InvalidRecipient => target_unavailable(),
    }
}

fn runtime_unavailable() -> CommandError {
    CommandError::new(
        CommandErrorCode::RuntimeUnavailable,
        "agent and team runtime is unavailable",
    )
}

fn target_unavailable() -> CommandError {
    CommandError::new(
        CommandErrorCode::TargetUnavailable,
        "target is unavailable in this session and workspace",
    )
}

fn output_unavailable() -> CommandError {
    CommandError::new(
        CommandErrorCode::OutputUnavailable,
        "bounded incremental output is unavailable for this agent",
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use allthecodes_types::agent_runtime_record::AgentRuntimeExecutionRecord;
    use allthecodes_types::agent_types::{ForkContextMode, LiveParentContextPaths};
    use allthecodes_types::output::{OutputEvent, OutputLifecycleState, OutputStream};

    use super::*;

    struct TestHost {
        cancel_result: Mutex<Result<Option<String>, RuntimeHostError>>,
        output: Mutex<Option<AgentTaskOutputBatch>>,
        team_write_result: Mutex<Result<(), RuntimeHostError>>,
    }

    impl Default for TestHost {
        fn default() -> Self {
            Self {
                cancel_result: Mutex::new(Ok(None)),
                output: Mutex::new(None),
                team_write_result: Mutex::new(Err(RuntimeHostError::Unavailable)),
            }
        }
    }

    impl AgentRuntimeHost for TestHost {
        fn cancel_agent_for_scope(
            &self,
            _context: &TrustedCommandContext,
            _agent_id: &str,
        ) -> Result<Option<String>, RuntimeHostError> {
            self.cancel_result.lock().unwrap().clone()
        }

        fn agent_output_batch_for_scope(
            &self,
            _context: &TrustedCommandContext,
            _agent_id: &str,
            _after_seq: Option<u64>,
            _limit_bytes: usize,
        ) -> Result<Option<AgentTaskOutputBatch>, RuntimeHostError> {
            Ok(self.output.lock().unwrap().clone())
        }

        fn agent_tree_snapshot_for_scope(
            &self,
            _context: &TrustedCommandContext,
        ) -> Result<Vec<AgentNode>, RuntimeHostError> {
            Ok(vec![test_node()])
        }

        fn write_team_message_for_scope(
            &self,
            _context: &TrustedCommandContext,
            _team_name: &str,
            _to: &str,
            _from: &str,
            _text: &str,
        ) -> Result<(), RuntimeHostError> {
            *self.team_write_result.lock().unwrap()
        }

        fn team_members_for_scope(
            &self,
            _context: &TrustedCommandContext,
            _team_name: &str,
        ) -> Result<Vec<TeamMemberInfo>, RuntimeHostError> {
            Ok(vec![TeamMemberInfo {
                agent_id: "agent-1".to_string(),
                agent_name: "worker".to_string(),
                role: Some("reviewer".to_string()),
                is_active: true,
                unread_messages: 2,
            }])
        }
    }

    struct PagingHost {
        events: Vec<OutputEvent>,
    }

    impl AgentRuntimeHost for PagingHost {
        fn agent_output_batch_for_scope(
            &self,
            _context: &TrustedCommandContext,
            _agent_id: &str,
            after_seq: Option<u64>,
            limit_bytes: usize,
        ) -> Result<Option<AgentTaskOutputBatch>, RuntimeHostError> {
            let after_seq = after_seq.unwrap_or(0);
            let mut bytes = 0usize;
            let mut events = Vec::new();
            for event in self.events.iter().filter(|event| event.seq > after_seq) {
                if !events.is_empty() && bytes.saturating_add(event.chunk.len()) > limit_bytes {
                    break;
                }
                bytes = bytes.saturating_add(event.chunk.len());
                events.push(event.clone());
            }
            let next_seq = events
                .last()
                .map(|event| event.seq.saturating_add(1))
                .unwrap_or_else(|| after_seq.saturating_add(1));
            Ok(Some(AgentTaskOutputBatch {
                id: "task-paged".to_string(),
                output: OutputReadBatch {
                    truncated: self.events.iter().any(|event| event.seq >= next_seq),
                    first_available_seq: 1,
                    state: OutputLifecycleState::Running,
                    events,
                    next_seq,
                },
                metadata: None,
            }))
        }
    }

    fn context() -> TrustedCommandContext {
        TrustedCommandContext::web(
            "session-1",
            PathBuf::from("/workspace"),
            "web:connection-1",
            true,
        )
    }

    fn test_node() -> AgentNode {
        AgentNode {
            agent_id: "agent-1".to_string(),
            parent_agent_id: None,
            description: "read /workspace/secret.txt".to_string(),
            agent_type: Some("review".to_string()),
            model: None,
            state: "running".to_string(),
            is_background: true,
            depth: 1,
            chain_id: "chain-1".to_string(),
            spawned_at: 1,
            completed_at: None,
            duration_ms: None,
            result_preview: None,
            had_error: false,
            fork_metadata: Some(ForkLaunchMetadata {
                is_fork: true,
                context: ForkContextMode::LiveReadonly,
                live_channel: Some(LiveParentContextPaths {
                    directory: "/workspace/.live".to_string(),
                    snapshot: "/workspace/.live/snapshot".to_string(),
                    updates: "/workspace/.live/updates".to_string(),
                    latest_diff: None,
                    latest_seq: 1,
                }),
            }),
            children: Vec::new(),
        }
    }

    fn output_batch(chunk: String) -> AgentTaskOutputBatch {
        AgentTaskOutputBatch {
            id: "task-1".to_string(),
            output: OutputReadBatch {
                events: vec![OutputEvent {
                    seq: 1,
                    stream: OutputStream::Stdout,
                    chunk,
                    timestamp_ms: 1,
                    process_or_run_id: "task-1".to_string(),
                }],
                next_seq: 2,
                truncated: false,
                first_available_seq: 1,
                state: OutputLifecycleState::Running,
            },
            metadata: Some(serde_json::json!({
                "fork": {
                    "is_fork": true,
                    "fork_context": "live_readonly",
                    "live_channel": {
                        "directory": "/workspace/.live",
                        "snapshot": "/workspace/.live/snapshot",
                        "updates": "/workspace/.live/updates",
                        "latest_seq": 1
                    }
                }
            })),
        }
    }

    #[test]
    fn read_commands_are_direct_and_redact_live_paths() {
        let host = TestHost::default();
        let dispatch = dispatch_agent_command_with_host(
            Some(&host),
            &context(),
            AgentCommand::QueryActiveAgents,
        )
        .unwrap();

        assert_eq!(dispatch.direct.len(), 1);
        assert!(dispatch.publish.is_empty());
        let BackendMessage::AgentEvent {
            event: AgentEvent::TreeSnapshot { roots },
        } = &dispatch.direct[0]
        else {
            panic!("expected tree snapshot");
        };
        assert_eq!(roots[0].description, "read <workspace>/secret.txt");
        assert!(roots[0]
            .fork_metadata
            .as_ref()
            .unwrap()
            .live_channel
            .is_none());
    }

    #[test]
    fn live_spawn_projection_removes_internal_channel_paths() {
        let projected = project_agent_event_for_web(
            &context(),
            AgentEvent::Spawned {
                agent_id: "agent-1".to_string(),
                parent_agent_id: None,
                description: "read /workspace/secret.txt".to_string(),
                agent_type: Some("review".to_string()),
                model: Some("model".to_string()),
                is_background: true,
                depth: 1,
                chain_id: "chain-1".to_string(),
                fork_metadata: test_node().fork_metadata,
            },
        )
        .unwrap();

        let AgentEvent::Spawned {
            description,
            fork_metadata,
            ..
        } = projected
        else {
            panic!("expected spawned event");
        };
        assert_eq!(description, "read <workspace>/secret.txt");
        assert!(fork_metadata.unwrap().live_channel.is_none());
    }

    #[test]
    fn live_execution_record_projection_removes_raw_process_details() {
        let projected = project_agent_event_for_web(
            &context(),
            AgentEvent::ExecutionRecord {
                agent_id: "agent-1".to_string(),
                record: Box::new(AgentRuntimeExecutionRecord {
                    session_id: "session-1".to_string(),
                    agent_id: "agent-1".to_string(),
                    tool: "shell".to_string(),
                    command: Some("cat /workspace/secret.txt".to_string()),
                    cwd: Some(PathBuf::from("/workspace/private")),
                    stdout_digest: Some("stdout-digest".to_string()),
                    stderr_digest: Some("stderr-digest".to_string()),
                    ..Default::default()
                }),
            },
        )
        .unwrap();

        let AgentEvent::ExecutionRecord { record, .. } = projected else {
            panic!("expected execution record");
        };
        assert!(record.command.is_none());
        assert!(record.cwd.is_none());
        assert!(record.stdout_digest.is_none());
        assert!(record.stderr_digest.is_none());
    }

    #[test]
    fn live_tool_projection_redacts_credentials_and_paths() {
        let projected = project_agent_event_for_web(
            &context(),
            AgentEvent::ToolUse {
                agent_id: "agent-1".to_string(),
                tool_use_id: "toolu-1".to_string(),
                tool_name: "shell".to_string(),
                input: serde_json::json!({
                    "api_key": "sk-secret",
                    "nested": {
                        "Authorization": "Bearer secret",
                        "path": "/workspace/src/main.rs"
                    },
                    "argv": ["read", "/tmp/private"]
                }),
            },
        )
        .unwrap();

        let AgentEvent::ToolUse { input, .. } = projected else {
            panic!("expected tool use");
        };
        assert_eq!(input["api_key"], "[redacted]");
        assert_eq!(input["nested"]["Authorization"], "[redacted]");
        assert_eq!(input["nested"]["path"], "<workspace>/src/main.rs");
        assert_eq!(input["argv"][1], "<redacted-path>");
    }

    #[test]
    fn abort_without_runtime_or_target_never_publishes_aborted() {
        let error = dispatch_agent_command_with_host(
            None,
            &context(),
            AgentCommand::AbortAgent {
                agent_id: "agent-1".to_string(),
            },
        )
        .unwrap_err();
        assert_eq!(error.code, CommandErrorCode::RuntimeUnavailable);

        let host = TestHost::default();
        let error = dispatch_agent_command_with_host(
            Some(&host),
            &context(),
            AgentCommand::AbortAgent {
                agent_id: "agent-1".to_string(),
            },
        )
        .unwrap_err();
        assert_eq!(error.code, CommandErrorCode::TargetUnavailable);
    }

    #[test]
    fn successful_abort_is_publish_only() {
        let host = TestHost::default();
        *host.cancel_result.lock().unwrap() = Ok(Some("task-1".to_string()));

        let dispatch = dispatch_agent_command_with_host(
            Some(&host),
            &context(),
            AgentCommand::AbortAgent {
                agent_id: "agent-1".to_string(),
            },
        )
        .unwrap();

        assert!(dispatch.direct.is_empty());
        assert!(matches!(
            dispatch.publish.first(),
            Some(BackendMessage::AgentEvent {
                event: AgentEvent::Aborted { .. }
            })
        ));
        assert_eq!(dispatch.publish.len(), 2);
    }

    #[test]
    fn output_limit_rejects_zero_over_max_and_oversized_legacy_event() {
        let host = TestHost::default();
        for limit in [0, MAX_AGENT_OUTPUT_LIMIT_BYTES + 1] {
            let error = dispatch_agent_command_with_host(
                Some(&host),
                &context(),
                AgentCommand::QueryAgentOutput {
                    agent_id: "agent-1".to_string(),
                    after_seq: None,
                    limit_bytes: Some(limit),
                },
            )
            .unwrap_err();
            assert_eq!(error.code, CommandErrorCode::InvalidOutputLimit);
        }

        *host.output.lock().unwrap() = Some(output_batch("x".repeat(65)));
        let error = dispatch_agent_command_with_host(
            Some(&host),
            &context(),
            AgentCommand::QueryAgentOutput {
                agent_id: "agent-1".to_string(),
                after_seq: None,
                limit_bytes: Some(64),
            },
        )
        .unwrap_err();
        assert_eq!(error.code, CommandErrorCode::OutputUnavailable);
    }

    #[test]
    fn output_projection_is_direct_bounded_and_clears_live_channel() {
        let host = TestHost::default();
        *host.output.lock().unwrap() = Some(output_batch(
            "token=secret\nread /workspace/file.txt".to_string(),
        ));

        let dispatch = dispatch_agent_command_with_host(
            Some(&host),
            &context(),
            AgentCommand::QueryAgentOutput {
                agent_id: "agent-1".to_string(),
                after_seq: None,
                limit_bytes: Some(1024),
            },
        )
        .unwrap();

        let BackendMessage::AgentEvent {
            event:
                AgentEvent::OutputBatch {
                    output,
                    fork_metadata,
                    ..
                },
        } = &dispatch.direct[0]
        else {
            panic!("expected output batch");
        };
        assert_eq!(
            output.events[0].chunk,
            "[redacted]\nread <workspace>/file.txt"
        );
        assert!(fork_metadata.as_ref().unwrap().live_channel.is_none());
        assert!(dispatch.publish.is_empty());
    }

    #[test]
    fn repeated_output_pages_have_no_gaps_or_duplicates() {
        let host = PagingHost {
            events: (1..=4)
                .map(|seq| OutputEvent {
                    seq,
                    stream: OutputStream::Stdout,
                    chunk: format!("{seq}{seq}"),
                    timestamp_ms: seq,
                    process_or_run_id: "task-paged".to_string(),
                })
                .collect(),
        };
        let mut after_seq = None;
        let mut consumed = Vec::new();

        while consumed.len() < host.events.len() {
            let dispatch = dispatch_agent_command_with_host(
                Some(&host),
                &context(),
                AgentCommand::QueryAgentOutput {
                    agent_id: "agent-1".to_string(),
                    after_seq,
                    limit_bytes: Some(3),
                },
            )
            .unwrap();
            let BackendMessage::AgentEvent {
                event: AgentEvent::OutputBatch { output, .. },
            } = &dispatch.direct[0]
            else {
                panic!("expected output batch");
            };
            assert!(!output.events.is_empty());
            consumed.extend(output.events.iter().map(|event| event.seq));
            let response_cursor = output.next_seq.saturating_sub(1);
            assert_eq!(
                Some(response_cursor),
                output.events.last().map(|event| event.seq)
            );
            after_seq = Some(response_cursor);
        }

        assert_eq!(consumed, vec![1, 2, 3, 4]);
    }

    #[test]
    fn redaction_preserves_token_boundaries_and_utf8_limits() {
        assert_eq!(
            bounded_redacted(&context(), "read /tmp/private then", 1024),
            "read <redacted-path> then"
        );
        let bounded = bounded_redacted(&context(), "ééé", 5);
        assert_eq!(bounded, "éé");
        assert_eq!(bounded.len(), 4);
    }

    #[test]
    fn team_query_is_direct_and_failed_write_never_publishes() {
        let host = TestHost::default();
        let status = dispatch_team_command_with_host(
            Some(&host),
            &context(),
            TeamCommand::QueryTeamStatus {
                team_name: "team-1".to_string(),
            },
        )
        .unwrap();
        assert_eq!(status.direct.len(), 1);
        assert!(status.publish.is_empty());

        *host.team_write_result.lock().unwrap() = Err(RuntimeHostError::Unavailable);
        let error = dispatch_team_command_with_host(
            Some(&host),
            &context(),
            TeamCommand::InjectMessage {
                team_name: "team-1".to_string(),
                to: "worker".to_string(),
                text: "hello".to_string(),
            },
        )
        .unwrap_err();
        assert_eq!(error.code, CommandErrorCode::RuntimeUnavailable);
    }

    #[test]
    fn oversized_team_message_is_rejected_before_host_write() {
        let host = TestHost::default();
        *host.team_write_result.lock().unwrap() = Ok(());
        let error = dispatch_team_command_with_host(
            Some(&host),
            &context(),
            TeamCommand::InjectMessage {
                team_name: "team-1".to_string(),
                to: "worker".to_string(),
                text: "x".repeat(MAX_TEAM_MESSAGE_BYTES + 1),
            },
        )
        .unwrap_err();
        assert_eq!(error.code, CommandErrorCode::MessageTooLarge);
    }
}
