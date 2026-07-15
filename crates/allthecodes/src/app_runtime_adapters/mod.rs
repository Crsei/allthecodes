//! Host adapters that bind the extracted `cc-ipc` runtime facade to this binary.

use std::path::Path;
use std::sync::{Arc, Once};

use parking_lot::Mutex;

pub(crate) mod callbacks;
mod ingress;
mod sdk_mapper;

use allthecodes_ipc::agent_handlers::{
    project_agent_output_for_web, AgentRuntimeHost, AgentTaskOutput, AgentTaskOutputBatch,
    CommandScopePolicy, RuntimeHostError, TrustedCommandContext,
};
use allthecodes_ipc::headless::{
    BackgroundAgentCompletion, BoxHeadlessFuture, HeadlessRuntimeConfig, HeadlessRuntimeHost,
    PendingPermissions, PendingQuestions, SessionRuntime,
};
use allthecodes_ipc::subsystem_handlers::{
    BoxRuntimeFuture, McpRuntimeOperation, McpRuntimeReport, SubsystemRuntimeHost,
};
use allthecodes_ipc::transport::FrontendSink;
use allthecodes_ipc_protocol::protocol::{BackendMessage, FrontendMessage};
use allthecodes_ipc_protocol::subsystem_events::{
    IdeCommand, LspCommand, McpCommand, PluginCommand, SkillCommand, SubsystemEvent,
};
use allthecodes_ipc_protocol::subsystem_types::*;
use allthecodes_services::prompt_suggestion::PromptSuggestionService;
use allthecodes_types::agent_types::TeamMemberInfo;
use allthecodes_types::output::{OutputLifecycleState, OutputStream};
use allthecodes_web::handlers::group_chat::{
    GroupChatLaunchRequest, GroupChatLaunchResult, GroupChatObserveRequest, GroupChatRuntimeFuture,
    GroupChatRuntimeHost, GroupChatRuntimeObservation, GroupChatRuntimeOutput,
};

static INSTALL: Once = Once::new();

pub fn ensure_installed() {
    INSTALL.call_once(|| {
        allthecodes_ipc::agent_handlers::set_runtime_host(Arc::new(RootAgentHost));
        allthecodes_ipc::subsystem_handlers::set_runtime_host(Arc::new(RootSubsystemHost));
        allthecodes_services::agent_definitions::set_runtime_host(Arc::new(
            RootAgentDefinitionsHost,
        ));
        allthecodes_tools::runtime::system_status::set_runtime_host(Arc::new(RootSystemStatusHost));
        allthecodes_web::handlers::group_chat::set_group_chat_runtime_host(Arc::new(
            RootGroupChatRuntimeHost,
        ));
        let mut adapters = allthecodes_engine::agent_runtime::agent_runtime_adapters();
        adapters.builtin_agents = Arc::new(RootAgentDefinitionRegistry);
        allthecodes_engine::agent_runtime::set_agent_runtime_adapters(adapters);
    });
}

struct RootGroupChatRuntimeHost;

impl GroupChatRuntimeHost for RootGroupChatRuntimeHost {
    fn launch(
        &self,
        foreground: Arc<allthecodes_engine::lifecycle::QueryEngine>,
        request: GroupChatLaunchRequest,
    ) -> GroupChatRuntimeFuture<GroupChatLaunchResult> {
        Box::pin(async move {
            let coordinator = group_chat_coordinator_engine(&foreground, &request)?;
            let result = coordinator
                .execute_server_tool(
                    "DelegateTask",
                    serde_json::json!({
                        "role": request.role,
                        "prompt": request.prompt,
                        "model": request.model,
                        "cwd": request.canonical_workspace.to_string_lossy(),
                    }),
                    "web-group-chat",
                )
                .await
                .map_err(|_| "delegated Agent launch was denied or failed".to_string())?;

            Ok(GroupChatLaunchResult {
                task_id: required_group_chat_launch_field(&result.data, "task_id")?,
                child_session_id: required_group_chat_launch_field(
                    &result.data,
                    "child_session_id",
                )?,
                runtime_agent_id: required_group_chat_launch_field(&result.data, "agent_id")?,
            })
        })
    }

    fn observe(
        &self,
        request: GroupChatObserveRequest,
    ) -> Result<GroupChatRuntimeObservation, String> {
        let output_limit = request.limit_bytes.clamp(
            1,
            allthecodes_ipc::agent_handlers::MAX_AGENT_OUTPUT_LIMIT_BYTES,
        );
        let context = TrustedCommandContext::web(
            request.coordinator_session_id.clone(),
            request.canonical_workspace.clone(),
            "group-chat-runtime",
            false,
        );
        let Some((_task_id, batch, _metadata)) =
            allthecodes_engine::agent::supervisor::output_events_for_owner(
                &request.runtime_agent_id,
                &request.coordinator_session_id,
                &request.canonical_workspace,
                request.after_seq,
                output_limit,
            )
            .map_err(|_| "group chat runtime target is unavailable".to_string())?
        else {
            return Err("group chat runtime output is unavailable".to_string());
        };

        let status = match batch.state {
            OutputLifecycleState::Starting | OutputLifecycleState::Running => {
                allthecodes_protocol::v1::group_chat::GroupChatDispatchStatus::Running
            }
            OutputLifecycleState::Exited => {
                allthecodes_protocol::v1::group_chat::GroupChatDispatchStatus::Completed
            }
            OutputLifecycleState::Expired => {
                allthecodes_protocol::v1::group_chat::GroupChatDispatchStatus::Cancelled
            }
            OutputLifecycleState::Failed => {
                allthecodes_protocol::v1::group_chat::GroupChatDispatchStatus::Failed
            }
        };
        let output = batch
            .events
            .into_iter()
            .map(|event| GroupChatRuntimeOutput {
                sequence: event.seq,
                stream: match event.stream {
                    OutputStream::Stdout => "stdout",
                    OutputStream::Stderr => "stderr",
                    OutputStream::Pty => "pty",
                    OutputStream::System => "system",
                }
                .to_string(),
                chunk: project_agent_output_for_web(&context, &event.chunk, output_limit),
            })
            .collect();
        let (summary, error) = match status {
            allthecodes_protocol::v1::group_chat::GroupChatDispatchStatus::Completed => {
                (Some("Agent completed".to_string()), None)
            }
            allthecodes_protocol::v1::group_chat::GroupChatDispatchStatus::Cancelled => {
                (Some("Agent was cancelled".to_string()), None)
            }
            allthecodes_protocol::v1::group_chat::GroupChatDispatchStatus::Failed => (
                Some("Agent failed".to_string()),
                Some("Agent execution failed".to_string()),
            ),
            _ => (None, None),
        };

        Ok(GroupChatRuntimeObservation {
            status,
            output,
            next_seq: batch.next_seq,
            truncated: batch.truncated,
            summary,
            error,
        })
    }
}

fn group_chat_coordinator_engine(
    foreground: &allthecodes_engine::lifecycle::QueryEngine,
    request: &GroupChatLaunchRequest,
) -> Result<Arc<allthecodes_engine::lifecycle::QueryEngine>, String> {
    if request.coordinator_session_id.trim().is_empty()
        || request.coordinator_session_id.len() > 256
    {
        return Err("invalid group chat coordinator session".to_string());
    }
    let foreground_workspace = std::fs::canonicalize(foreground.cwd())
        .map_err(|_| "foreground workspace is unavailable".to_string())?;
    if foreground_workspace != request.canonical_workspace {
        return Err("group chat workspace binding changed before launch".to_string());
    }

    let mut config = foreground.config_ref().clone();
    config.cwd = request.canonical_workspace.to_string_lossy().into_owned();
    config.tools = foreground.tools_snapshot();
    config.initial_messages = None;
    config.persist_session = true;
    config.auto_save_session = true;
    config.agent_context = None;

    let mut coordinator = allthecodes_engine::lifecycle::QueryEngine::new(config);
    coordinator.set_hook_runner(foreground.hook_runner());
    coordinator.set_command_dispatcher(foreground.command_dispatcher());
    coordinator.set_command_executor(foreground.command_executor());
    coordinator.set_auto_classifier_fn(foreground.auto_classifier_fn());
    coordinator.set_audit_context(foreground.audit_context());
    let mut app_state = foreground.app_state();
    app_state.team_context = None;
    app_state.tool_permission_context.clear_session_grants();
    coordinator.update_app_state(|state| *state = app_state);
    coordinator.assign_server_owned_session_id(
        allthecodes_engine::bootstrap::SessionId::from_string(&request.coordinator_session_id),
    );

    let (agent_tx, mut agent_rx) = allthecodes_types::agent_channel::agent_channel();
    coordinator.set_bg_agent_tx(agent_tx);
    tokio::spawn(async move { while agent_rx.recv().await.is_some() {} });
    Ok(Arc::new(coordinator))
}

fn required_group_chat_launch_field(
    value: &serde_json::Value,
    field: &'static str,
) -> Result<String, String> {
    value
        .get(field)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty() && value.len() <= 256)
        .map(ToString::to_string)
        .ok_or_else(|| "delegated Agent returned an invalid launch identity".to_string())
}

pub fn headless_config(
    engine: Arc<allthecodes_engine::lifecycle::QueryEngine>,
    model: String,
) -> HeadlessRuntimeConfig {
    ensure_installed();
    HeadlessRuntimeConfig::new(
        Arc::new(RootHeadlessHost {
            engine,
            suggestion_svc: Arc::new(Mutex::new(PromptSuggestionService::new(true))),
        }),
        model,
    )
}

struct RootAgentDefinitionRegistry;

impl allthecodes_engine::agent_runtime::BuiltinAgentRegistry for RootAgentDefinitionRegistry {
    fn builtin_agent_entries(&self) -> Vec<AgentDefinitionEntry> {
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        allthecodes_services::agent_definitions::list_all_agents(&cwd)
    }

    fn builtin_agent_prompt(&self, name: &str) -> Option<String> {
        allthecodes_services::agent_definitions::builtin::builtin_agent_prompt(name)
            .map(ToOwned::to_owned)
    }
}

struct RootAgentDefinitionsHost;

impl allthecodes_services::agent_definitions::AgentDefinitionsRuntimeHost
    for RootAgentDefinitionsHost
{
    fn build_mcp_server_info_list(&self) -> Vec<McpServerStatusInfo> {
        crate::app_subsystem_handlers::build_mcp_server_info_list()
    }
}

struct RootAgentHost;

impl AgentRuntimeHost for RootAgentHost {
    fn cancel_agent(&self, agent_id: &str) -> Option<String> {
        allthecodes_engine::agent::supervisor::cancel_agent(agent_id)
    }

    fn agent_output(&self, agent_id: &str) -> Option<AgentTaskOutput> {
        allthecodes_engine::agent::supervisor::output_for_agent(agent_id).map(|task| {
            AgentTaskOutput {
                id: task.id,
                output: task.output,
                metadata: task.metadata,
            }
        })
    }

    fn agent_output_batch(
        &self,
        agent_id: &str,
        after_seq: Option<u64>,
        limit_bytes: usize,
    ) -> Option<AgentTaskOutputBatch> {
        let (task_id, output, metadata) =
            allthecodes_engine::agent::supervisor::output_events_for_agent(
                agent_id,
                after_seq,
                limit_bytes,
            )
            .ok()
            .flatten()?;
        Some(AgentTaskOutputBatch {
            id: task_id,
            output,
            metadata,
        })
    }

    fn update_agent_state(
        &self,
        agent_id: &str,
        state: &str,
        result_preview: Option<String>,
        duration_ms: Option<u64>,
        had_error: bool,
    ) {
        allthecodes_engine::agent_runtime::update_agent_state(
            agent_id,
            state,
            result_preview,
            duration_ms,
            had_error,
        );
    }

    fn agent_tree_snapshot(&self) -> Vec<allthecodes_types::agent_types::AgentNode> {
        allthecodes_engine::agent_runtime::agent_tree_snapshot()
    }

    fn write_team_message(&self, team_name: &str, to: &str, text: &str) -> Result<(), String> {
        let msg = allthecodes_teams::types::TeammateMessage {
            from: "__local__".to_string(),
            text: text.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            read: false,
            color: None,
            summary: None,
        };
        allthecodes_teams::mailbox::write_to_mailbox(to, msg, team_name).map_err(|e| e.to_string())
    }

    fn team_members(&self, team_name: &str) -> Result<Vec<TeamMemberInfo>, String> {
        let tf =
            allthecodes_teams::helpers::read_team_file(team_name).map_err(|e| e.to_string())?;
        Ok(tf
            .members
            .iter()
            .map(|m| TeamMemberInfo {
                agent_id: m.agent_id.clone(),
                agent_name: m.name.clone(),
                role: m.agent_type.clone(),
                is_active: m.is_active.unwrap_or(true),
                unread_messages: allthecodes_teams::mailbox::read_unread_messages(
                    &m.name, team_name,
                )
                .map(|v| v.len())
                .unwrap_or(0),
            })
            .collect())
    }

    fn cancel_agent_for_scope(
        &self,
        context: &TrustedCommandContext,
        agent_id: &str,
    ) -> Result<Option<String>, RuntimeHostError> {
        if context.scope_policy() == CommandScopePolicy::TrustedLocal {
            return Ok(self.cancel_agent(agent_id));
        }
        allthecodes_engine::agent::supervisor::cancel_agent_for_owner(
            agent_id,
            context.session_id(),
            context.canonical_workspace(),
        )
        .map(Some)
        .map_err(map_agent_owner_error)
    }

    fn agent_output_batch_for_scope(
        &self,
        context: &TrustedCommandContext,
        agent_id: &str,
        after_seq: Option<u64>,
        limit_bytes: usize,
    ) -> Result<Option<AgentTaskOutputBatch>, RuntimeHostError> {
        if context.scope_policy() == CommandScopePolicy::TrustedLocal {
            return Ok(self.agent_output_batch(agent_id, after_seq, limit_bytes));
        }
        allthecodes_engine::agent::supervisor::output_events_for_owner(
            agent_id,
            context.session_id(),
            context.canonical_workspace(),
            after_seq,
            limit_bytes,
        )
        .map(|task| {
            task.map(|(id, output, metadata)| AgentTaskOutputBatch {
                id,
                output,
                metadata,
            })
        })
        .map_err(map_agent_owner_error)
    }

    fn agent_tree_snapshot_for_scope(
        &self,
        context: &TrustedCommandContext,
    ) -> Result<Vec<allthecodes_types::agent_types::AgentNode>, RuntimeHostError> {
        if context.scope_policy() == CommandScopePolicy::TrustedLocal {
            return Ok(self.agent_tree_snapshot());
        }
        Ok(
            allthecodes_engine::agent_runtime::agent_tree_snapshot_for_owner(
                context.session_id(),
                context.canonical_workspace(),
            ),
        )
    }

    fn write_team_message_for_scope(
        &self,
        context: &TrustedCommandContext,
        team_name: &str,
        to: &str,
        from: &str,
        text: &str,
    ) -> Result<(), RuntimeHostError> {
        if context.scope_policy() == CommandScopePolicy::TrustedLocal {
            let message = allthecodes_teams::types::TeammateMessage {
                from: from.to_string(),
                text: text.to_string(),
                timestamp: chrono::Utc::now().to_rfc3339(),
                read: false,
                color: None,
                summary: None,
            };
            return allthecodes_teams::mailbox::write_to_mailbox(to, message, team_name)
                .map_err(|_| RuntimeHostError::Unavailable);
        }
        if from != context.server_sender_id() {
            return Err(RuntimeHostError::Unauthorized);
        }
        let team = scoped_team_file(context, team_name)?;
        let recipient = team
            .members
            .iter()
            .find(|member| member.name == to && member.is_active != Some(false))
            .ok_or(RuntimeHostError::InvalidRecipient)?;
        let message = allthecodes_teams::types::TeammateMessage {
            from: context.server_sender_id().to_string(),
            text: text.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            read: false,
            color: None,
            summary: None,
        };
        allthecodes_teams::mailbox::write_to_mailbox(&recipient.name, message, &team.name)
            .map_err(|_| RuntimeHostError::Unavailable)
    }

    fn team_members_for_scope(
        &self,
        context: &TrustedCommandContext,
        team_name: &str,
    ) -> Result<Vec<TeamMemberInfo>, RuntimeHostError> {
        if context.scope_policy() == CommandScopePolicy::TrustedLocal {
            return self
                .team_members(team_name)
                .map_err(|_| RuntimeHostError::NotFound);
        }
        let team = scoped_team_file(context, team_name)?;
        team.members
            .iter()
            .map(|member| {
                let unread_messages =
                    allthecodes_teams::mailbox::read_unread_messages(&member.name, &team.name)
                        .map_err(|_| RuntimeHostError::Unavailable)?
                        .len();
                Ok(TeamMemberInfo {
                    agent_id: member.agent_id.clone(),
                    agent_name: member.name.clone(),
                    role: member.agent_type.clone(),
                    is_active: member.is_active.unwrap_or(true),
                    unread_messages,
                })
            })
            .collect()
    }
}

fn map_agent_owner_error(
    error: allthecodes_engine::agent::supervisor::AgentOwnerLookupError,
) -> RuntimeHostError {
    use allthecodes_engine::agent::supervisor::AgentOwnerLookupError;

    match error {
        AgentOwnerLookupError::NotFound => RuntimeHostError::NotFound,
        AgentOwnerLookupError::Unauthorized => RuntimeHostError::Unauthorized,
        AgentOwnerLookupError::Terminal => RuntimeHostError::Terminal,
        AgentOwnerLookupError::Conflict => RuntimeHostError::Conflict,
        AgentOwnerLookupError::Unavailable => RuntimeHostError::Unavailable,
    }
}

fn scoped_team_file(
    context: &TrustedCommandContext,
    team_name: &str,
) -> Result<allthecodes_teams::types::TeamFile, RuntimeHostError> {
    let team = allthecodes_teams::helpers::read_team_file(team_name)
        .map_err(|_| RuntimeHostError::NotFound)?;
    if team.name != team_name || team.lead_session_id.as_deref() != Some(context.session_id()) {
        return Err(RuntimeHostError::Unauthorized);
    }
    let lead = team
        .members
        .iter()
        .find(|member| member.agent_id == team.lead_agent_id)
        .ok_or(RuntimeHostError::Unauthorized)?;
    let workspace = std::fs::canonicalize(&lead.cwd).map_err(|_| RuntimeHostError::Unauthorized)?;
    if workspace != context.canonical_workspace() {
        return Err(RuntimeHostError::Unauthorized);
    }
    Ok(team)
}

struct RootSubsystemHost;

impl SubsystemRuntimeHost for RootSubsystemHost {
    fn handle_lsp_command(&self, cmd: LspCommand) -> Vec<BackendMessage> {
        crate::app_subsystem_handlers::handle_lsp_command(cmd)
    }

    fn handle_mcp_command(&self, cmd: McpCommand) -> Vec<BackendMessage> {
        crate::app_subsystem_handlers::handle_mcp_command(cmd)
    }

    fn handle_mcp_command_with_runtime<'a>(
        &'a self,
        cmd: McpCommand,
        cwd: &'a Path,
    ) -> BoxRuntimeFuture<'a, Vec<BackendMessage>> {
        Box::pin(crate::app_subsystem_handlers::handle_mcp_command_with_runtime(cmd, cwd))
    }

    fn handle_plugin_command(&self, cmd: PluginCommand) -> Vec<BackendMessage> {
        crate::app_subsystem_handlers::handle_plugin_command(cmd)
    }

    fn handle_skill_command(&self, cmd: SkillCommand) -> Vec<BackendMessage> {
        crate::app_subsystem_handlers::handle_skill_command(cmd)
    }

    fn handle_ide_command(&self, cmd: IdeCommand) -> Vec<BackendMessage> {
        crate::app_subsystem_handlers::handle_ide_command(cmd)
    }

    fn build_subsystem_status_snapshot(&self) -> SubsystemStatusSnapshot {
        crate::app_subsystem_handlers::build_subsystem_status_snapshot()
    }

    fn build_lsp_server_info_list(&self) -> Vec<LspServerInfo> {
        crate::app_subsystem_handlers::build_lsp_server_info_list()
    }

    fn load_lsp_recommendation_settings(&self) -> LspRecommendationSettings {
        crate::app_subsystem_handlers::load_lsp_recommendation_settings()
    }

    fn build_mcp_server_info_list(&self) -> Vec<McpServerStatusInfo> {
        crate::app_subsystem_handlers::build_mcp_server_info_list()
    }

    fn build_mcp_server_info_list_for_cwd_async<'a>(
        &'a self,
        cwd: &'a Path,
    ) -> BoxRuntimeFuture<'a, Vec<McpServerStatusInfo>> {
        Box::pin(crate::app_subsystem_handlers::build_mcp_server_info_list_for_cwd_async(cwd))
    }

    fn build_mcp_server_config_entries(&self, cwd: &Path) -> Vec<McpServerConfigEntry> {
        crate::app_subsystem_handlers::build_mcp_server_config_entries(cwd)
    }

    fn run_mcp_runtime_operation<'a>(
        &'a self,
        cwd: &'a Path,
        operation: McpRuntimeOperation,
        server_name: &'a str,
    ) -> BoxRuntimeFuture<'a, McpRuntimeReport> {
        Box::pin(async move {
            let operation = match operation {
                McpRuntimeOperation::Connect => {
                    crate::app_subsystem_handlers::McpRuntimeOperation::Connect
                }
                McpRuntimeOperation::Disconnect => {
                    crate::app_subsystem_handlers::McpRuntimeOperation::Disconnect
                }
                McpRuntimeOperation::Reconnect => {
                    crate::app_subsystem_handlers::McpRuntimeOperation::Reconnect
                }
            };
            let report = crate::app_subsystem_handlers::run_mcp_runtime_operation(
                cwd,
                operation,
                server_name,
            )
            .await;
            McpRuntimeReport {
                server_name: report.server_name,
                state: report.state,
                error: report.error,
                text: report.text,
                level: report.level,
            }
        })
    }

    fn build_plugin_info_list(&self) -> Vec<PluginInfo> {
        crate::app_subsystem_handlers::build_plugin_info_list()
    }

    fn build_skill_info_list(&self) -> Vec<SkillInfo> {
        crate::app_subsystem_handlers::build_skill_info_list()
    }

    fn build_ide_info_list(&self) -> Vec<IdeInfo> {
        crate::app_subsystem_handlers::build_ide_info_list()
    }
}

struct RootSystemStatusHost;

impl allthecodes_tools::runtime::system_status::SystemStatusRuntimeHost for RootSystemStatusHost {
    fn build_lsp_server_info_list(&self) -> Vec<LspServerInfo> {
        crate::app_subsystem_handlers::build_lsp_server_info_list()
    }

    fn build_mcp_server_info_list(&self) -> Vec<McpServerStatusInfo> {
        crate::app_subsystem_handlers::build_mcp_server_info_list()
    }

    fn build_plugin_info_list(&self) -> Vec<PluginInfo> {
        crate::app_subsystem_handlers::build_plugin_info_list()
    }

    fn build_skill_info_list(&self) -> Vec<SkillInfo> {
        crate::app_subsystem_handlers::build_skill_info_list()
    }

    fn build_ide_info_list(&self) -> Vec<IdeInfo> {
        crate::app_subsystem_handlers::build_ide_info_list()
    }

    fn active_agents(&self) -> Vec<allthecodes_types::agent_types::AgentNode> {
        fn collect(
            node: &allthecodes_types::agent_types::AgentNode,
            out: &mut Vec<allthecodes_types::agent_types::AgentNode>,
        ) {
            if node.state == "running" {
                let mut cloned = node.clone();
                cloned.children.clear();
                out.push(cloned);
            }
            for child in &node.children {
                collect(child, out);
            }
        }

        let mut out = Vec::new();
        for root in allthecodes_engine::agent_runtime::agent_tree_snapshot() {
            collect(&root, &mut out);
        }
        out
    }
}

fn find_agent_node(agent_id: &str) -> Option<allthecodes_types::agent_types::AgentNode> {
    fn find_in(
        node: &allthecodes_types::agent_types::AgentNode,
        agent_id: &str,
    ) -> Option<allthecodes_types::agent_types::AgentNode> {
        if node.agent_id == agent_id {
            return Some(node.clone());
        }
        node.children
            .iter()
            .find_map(|child| find_in(child, agent_id))
    }

    allthecodes_engine::agent_runtime::agent_tree_snapshot()
        .iter()
        .find_map(|root| find_in(root, agent_id))
}

struct RootHeadlessHost {
    engine: Arc<allthecodes_engine::lifecycle::QueryEngine>,
    suggestion_svc: Arc<Mutex<PromptSuggestionService>>,
}

impl HeadlessRuntimeHost for RootHeadlessHost {
    fn install(
        &self,
        pending_permissions: PendingPermissions,
        pending_questions: PendingQuestions,
        sink: FrontendSink,
        agent_tx: allthecodes_types::agent_channel::AgentSender,
        subsystem_tx: tokio::sync::broadcast::Sender<SubsystemEvent>,
    ) {
        ensure_installed();
        callbacks::install_permission_callback(&self.engine, pending_permissions, sink.clone());
        callbacks::install_ask_user_callback(&self.engine, pending_questions, sink.clone());
        callbacks::install_permission_event_callback(&self.engine, sink.clone());
        callbacks::install_tool_progress_callback(&self.engine, sink);
        self.engine.set_bg_agent_tx(agent_tx);
        install_root_subsystem_event_sinks(subsystem_tx);
    }

    fn ready_message(&self, model: String) -> BackendMessage {
        let app_state = self.engine.app_state();
        let keybindings = app_state
            .keybindings
            .user_path()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
        BackendMessage::Ready {
            session_id: self.engine.current_session_id().to_string(),
            model,
            cwd: self.engine.cwd().to_string(),
            permission_mode: app_state.tool_permission_context.mode.as_str().to_string(),
            available_models: app_state.settings.available_models.clone(),
            plan_workflow: app_state.plan_workflow.clone(),
            editor_mode: app_state.settings.editor_mode.clone(),
            view_mode: app_state.settings.view_mode.clone(),
            keybindings,
        }
    }

    fn dispatch_frontend<'a>(
        &'a self,
        msg: FrontendMessage,
        runtime: &'a SessionRuntime,
        sink: &'a FrontendSink,
    ) -> BoxHeadlessFuture<'a, bool> {
        Box::pin(async move {
            ingress::dispatch(msg, &self.engine, runtime, &self.suggestion_svc, sink).await
        })
    }

    fn background_agent_completed(
        &self,
        agent_id: &str,
        result_preview: &str,
        had_error: bool,
        completion_status: allthecodes_types::agent_events::AgentCompletionStatus,
        duration_ms: u64,
        total_tokens: Option<u64>,
        tool_uses: Option<u64>,
        agent_type: Option<&str>,
    ) -> Option<BackgroundAgentCompletion> {
        let (is_bg, desc, node_agent_type, fork_metadata) = find_agent_node(agent_id)
            .map(|node| {
                (
                    node.is_background,
                    node.description,
                    node.agent_type,
                    node.fork_metadata,
                )
            })
            .unwrap_or((true, "unknown".to_string(), None, None));
        if !is_bg {
            return None;
        }
        let agent_type = agent_type.map(str::to_string).or(node_agent_type);

        let result_text = allthecodes_engine::agent::supervisor::output_for_agent(agent_id)
            .map(|task| task.output)
            .filter(|output| !output.is_empty())
            .unwrap_or_else(|| result_preview.to_string());

        self.engine.pending_background_results().push(
            allthecodes_engine::agent_runtime::CompletedBackgroundAgent {
                agent_id: agent_id.to_string(),
                description: desc.clone(),
                agent_type,
                result_text,
                had_error,
                completion_status,
                duration: std::time::Duration::from_millis(duration_ms),
                total_tokens,
                tool_uses,
            },
        );

        Some(BackgroundAgentCompletion {
            agent_id: agent_id.to_string(),
            description: desc,
            result_preview: result_preview.to_string(),
            had_error,
            duration_ms,
            fork_metadata,
        })
    }

    fn shutdown_background_agents<'a>(&'a self, reason: &'a str) -> BoxHeadlessFuture<'a, usize> {
        Box::pin(allthecodes_engine::agent::supervisor::shutdown_all(reason))
    }
}

// MCP skill discovery needs a stable manager view while querying one server's
// resources, so this refresh intentionally holds the manager guard across the
// adapter's async discovery call.
#[allow(clippy::await_holding_invalid_type)]
fn schedule_mcp_skill_resource_refresh(server_name: String) {
    if !allthecodes_config::features::enabled(allthecodes_config::features::Feature::McpSkills) {
        return;
    }
    tokio::spawn(async move {
        let Some(manager) = allthecodes_mcp::runtime::current_manager() else {
            return;
        };
        let project_path = std::env::current_dir()
            .ok()
            .map(|cwd| allthecodes_mcp::bindings::canonical_workspace_root(&cwd));
        let binding_context = allthecodes_mcp::McpBindingContext::startup(project_path);
        let manager = manager.lock().await;
        let (skills, diagnostics) =
            allthecodes_engine::mcp_tool_adapter::discover_mcp_skill_resources_for_server_context(
                &manager,
                &binding_context,
                &server_name,
            )
            .await;
        drop(manager);

        let report = allthecodes_skills::replace_mcp_skills_for_server(
            &server_name,
            skills,
            diagnostics,
            allthecodes_skills::SkillLoadOptions::for_app_version(env!("CARGO_PKG_VERSION")),
        );
        if report.error_count() > 0 || report.warning_count() > 0 {
            tracing::warn!(
                server = %server_name,
                loaded = report.loaded,
                skipped = report.skipped,
                errors = report.error_count(),
                warnings = report.warning_count(),
                "MCP skill resource refresh completed with diagnostics"
            );
        }
    });
}

fn install_root_subsystem_event_sinks(event_tx: tokio::sync::broadcast::Sender<SubsystemEvent>) {
    let (lsp_tx, mut lsp_rx) = tokio::sync::broadcast::channel(128);
    allthecodes_lsp_service::set_event_sender(lsp_tx);
    let lsp_event_tx = event_tx.clone();
    tokio::spawn(async move {
        while let Ok(event) = lsp_rx.recv().await {
            let _ = lsp_event_tx.send(lsp_event_to_subsystem(event));
        }
    });

    allthecodes_lsp_service::ide::set_event_sender(event_tx.clone());
    allthecodes_services::agent_definitions::generate::set_event_sender(event_tx.clone());

    let skills_tx = event_tx.clone();
    allthecodes_skills::set_event_callback(move |e| {
        let adapted = match e {
            allthecodes_skills::SkillSubsystemEvent::SkillsLoaded { count } => {
                SubsystemEvent::Skill(
                    allthecodes_ipc_protocol::subsystem_events::SkillEvent::SkillsLoaded { count },
                )
            }
        };
        let _ = skills_tx.send(adapted);
    });

    let mcp_tx = event_tx.clone();
    let mcp_sink: allthecodes_mcp::SharedMcpEventSink = Arc::new(move |e| {
        let adapted = match e {
            allthecodes_mcp::McpSubsystemEvent::ServerStateChanged {
                server_name,
                state,
                error,
            } => {
                allthecodes_mcp::runtime::record_server_state(
                    server_name.clone(),
                    state.clone(),
                    error.clone(),
                );
                SubsystemEvent::Mcp(
                    allthecodes_ipc_protocol::subsystem_events::McpEvent::ServerStateChanged {
                        server_name,
                        state,
                        error,
                    },
                )
            }
            allthecodes_mcp::McpSubsystemEvent::ToolsDiscovered { server_name, tools } => {
                SubsystemEvent::Mcp(
                    allthecodes_ipc_protocol::subsystem_events::McpEvent::ToolsDiscovered {
                        server_name,
                        tools: tools
                            .into_iter()
                            .map(|t| allthecodes_ipc_protocol::subsystem_types::McpToolInfo {
                                name: t.tool_name,
                                description: Some(t.description),
                            })
                            .collect(),
                    },
                )
            }
            allthecodes_mcp::McpSubsystemEvent::ResourcesDiscovered {
                server_name,
                resources,
            } => {
                if resources
                    .iter()
                    .any(|resource| resource.uri.starts_with("skill://"))
                {
                    schedule_mcp_skill_resource_refresh(server_name.clone());
                }
                SubsystemEvent::Mcp(
                    allthecodes_ipc_protocol::subsystem_events::McpEvent::ResourcesDiscovered {
                        server_name,
                        resources: resources
                            .into_iter()
                            .map(
                                |r| allthecodes_ipc_protocol::subsystem_types::McpResourceInfo {
                                    uri: r.uri,
                                    name: Some(r.name),
                                    mime_type: r.mime_type,
                                },
                            )
                            .collect(),
                    },
                )
            }
            allthecodes_mcp::McpSubsystemEvent::ChannelNotification {
                server_name,
                content,
                meta,
            } => SubsystemEvent::Mcp(
                allthecodes_ipc_protocol::subsystem_events::McpEvent::ChannelNotification {
                    server_name,
                    content,
                    meta,
                },
            ),
            allthecodes_mcp::McpSubsystemEvent::OAuthLoginCompleted {
                server_name,
                success,
                error,
            } => SubsystemEvent::Mcp(
                allthecodes_ipc_protocol::subsystem_events::McpEvent::AuthCompleted {
                    server_name,
                    success,
                    error,
                },
            ),
            allthecodes_mcp::McpSubsystemEvent::BindingsUpdated { bindings } => {
                SubsystemEvent::Mcp(
                    allthecodes_ipc_protocol::subsystem_events::McpEvent::BindingsUpdated {
                        bindings,
                    },
                )
            }
            allthecodes_mcp::McpSubsystemEvent::BindingError { server_id, error } => {
                SubsystemEvent::Mcp(
                    allthecodes_ipc_protocol::subsystem_events::McpEvent::BindingError {
                        server_id,
                        error,
                    },
                )
            }
        };
        let _ = mcp_tx.send(adapted);
    });
    if let Some(manager) = allthecodes_mcp::runtime::current_manager() {
        tokio::spawn(async move {
            manager.lock().await.set_event_sink(Some(mcp_sink));
        });
    }

    let plugin_tx = event_tx.clone();
    allthecodes_plugins::set_event_sink(Some(Arc::new(move |event| {
        let adapted = match event {
            allthecodes_plugins::PluginSubsystemEvent::Reloaded { count, had_error } => {
                SubsystemEvent::Plugin(
                    allthecodes_ipc_protocol::subsystem_events::PluginEvent::Reloaded {
                        count,
                        had_error,
                    },
                )
            }
            allthecodes_plugins::PluginSubsystemEvent::RefreshNeeded { reason } => {
                SubsystemEvent::Plugin(
                    allthecodes_ipc_protocol::subsystem_events::PluginEvent::RefreshNeeded {
                        reason,
                    },
                )
            }
            allthecodes_plugins::PluginSubsystemEvent::StatusChanged {
                plugin_id,
                name,
                status,
                error,
            } => SubsystemEvent::Plugin(
                allthecodes_ipc_protocol::subsystem_events::PluginEvent::StatusChanged {
                    plugin_id,
                    name,
                    status,
                    error,
                },
            ),
            // ── Phase 2 integration (Serial Integration Lane) ──
            // Properly mapped to new IPC PluginEvent variants.
            allthecodes_plugins::PluginSubsystemEvent::Installed {
                plugin_id,
                name,
                version,
            } => SubsystemEvent::Plugin(
                allthecodes_ipc_protocol::subsystem_events::PluginEvent::Installed {
                    plugin_id,
                    name,
                    version,
                },
            ),
            allthecodes_plugins::PluginSubsystemEvent::Updated {
                plugin_id,
                name,
                old_version: _old,
                new_version,
            } => SubsystemEvent::Plugin(
                allthecodes_ipc_protocol::subsystem_events::PluginEvent::Updated {
                    plugin_id,
                    name,
                    version: new_version,
                },
            ),
            allthecodes_plugins::PluginSubsystemEvent::Uninstalled { plugin_id, name } => {
                SubsystemEvent::Plugin(
                    allthecodes_ipc_protocol::subsystem_events::PluginEvent::Uninstalled {
                        plugin_id,
                        name,
                    },
                )
            }
            allthecodes_plugins::PluginSubsystemEvent::ValidationFailed { plugin_id, errors } => {
                SubsystemEvent::Plugin(
                    allthecodes_ipc_protocol::subsystem_events::PluginEvent::ValidationFailed {
                        plugin_id,
                        name: String::new(),
                        errors,
                    },
                )
            }
            allthecodes_plugins::PluginSubsystemEvent::ConfigChanged { plugin_id } => {
                SubsystemEvent::Plugin(
                    allthecodes_ipc_protocol::subsystem_events::PluginEvent::ConfigChanged {
                        plugin_id,
                        name: String::new(),
                    },
                )
            }
        };
        let _ = plugin_tx.send(adapted);
    })));

    allthecodes_mcp::discovery::set_plugin_hook(allthecodes_plugins::discover_plugin_mcp_servers);
    allthecodes_mcp::discovery::set_scoped_plugin_hook(
        allthecodes_plugins::discover_plugin_mcp_servers_scoped,
    );
    allthecodes_mcp::discovery::set_ide_hook(allthecodes_lsp_service::ide::selected_ide_mcp_config);
}

fn lsp_event_to_subsystem(event: allthecodes_lsp_service::LspEvent) -> SubsystemEvent {
    use allthecodes_ipc_protocol::subsystem_events::LspEvent;
    let event = match event {
        allthecodes_lsp_service::LspEvent::ServerStateChanged {
            language_id,
            state,
            error,
        } => LspEvent::ServerStateChanged {
            language_id,
            state,
            error,
        },
        allthecodes_lsp_service::LspEvent::DocumentSynced {
            uri,
            language_id,
            version,
            change_kind,
        } => LspEvent::DocumentSynced {
            uri,
            language_id,
            version,
            change_kind,
        },
        allthecodes_lsp_service::LspEvent::DiagnosticsPublished { uri, diagnostics } => {
            LspEvent::DiagnosticsPublished {
                uri,
                diagnostics: diagnostics.into_iter().map(lsp_diagnostic_to_ipc).collect(),
            }
        }
        allthecodes_lsp_service::LspEvent::CompletionResults {
            request_id,
            uri,
            items,
        } => LspEvent::CompletionResults {
            request_id,
            uri,
            items: items.into_iter().map(lsp_completion_to_ipc).collect(),
        },
        allthecodes_lsp_service::LspEvent::CommandError {
            request_id,
            message,
        } => LspEvent::CommandError {
            request_id,
            message,
        },
    };
    SubsystemEvent::Lsp(event)
}

fn lsp_diagnostic_to_ipc(
    diagnostic: allthecodes_lsp_service::LspDiagnostic,
) -> allthecodes_ipc_protocol::subsystem_types::LspDiagnostic {
    allthecodes_ipc_protocol::subsystem_types::LspDiagnostic {
        range: allthecodes_ipc_protocol::subsystem_types::DiagnosticRange {
            start_line: diagnostic.range.start_line,
            start_character: diagnostic.range.start_character,
            end_line: diagnostic.range.end_line,
            end_character: diagnostic.range.end_character,
        },
        severity: diagnostic.severity,
        message: diagnostic.message,
        source: diagnostic.source,
        code: diagnostic.code,
    }
}

fn lsp_completion_to_ipc(
    item: allthecodes_lsp_service::CompletionItemInfo,
) -> allthecodes_ipc_protocol::CompletionItemInfo {
    allthecodes_ipc_protocol::CompletionItemInfo {
        label: item.label,
        kind: item.kind,
        detail: item.detail,
        documentation: item.documentation,
        insert_text: item.insert_text,
        sort_text: item.sort_text,
        filter_text: item.filter_text,
    }
}
