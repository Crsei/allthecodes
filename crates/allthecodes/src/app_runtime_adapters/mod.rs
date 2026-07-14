//! Host adapters that bind the extracted `cc-ipc` runtime facade to this binary.

use std::path::Path;
use std::sync::{Arc, Once};

use parking_lot::Mutex;

pub(crate) mod callbacks;
mod ingress;
mod sdk_mapper;

use allthecodes_ipc::agent_handlers::{AgentRuntimeHost, AgentTaskOutput, AgentTaskOutputBatch};
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

static INSTALL: Once = Once::new();

pub fn ensure_installed() {
    INSTALL.call_once(|| {
        allthecodes_ipc::agent_handlers::set_runtime_host(Arc::new(RootAgentHost));
        allthecodes_ipc::subsystem_handlers::set_runtime_host(Arc::new(RootSubsystemHost));
        allthecodes_services::agent_definitions::set_runtime_host(Arc::new(
            RootAgentDefinitionsHost,
        ));
        allthecodes_tools::runtime::system_status::set_runtime_host(Arc::new(RootSystemStatusHost));
        let mut adapters = allthecodes_engine::agent_runtime::agent_runtime_adapters();
        adapters.builtin_agents = Arc::new(RootAgentDefinitionRegistry);
        allthecodes_engine::agent_runtime::set_agent_runtime_adapters(adapters);
    });
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
            from: "__frontend__".to_string(),
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
