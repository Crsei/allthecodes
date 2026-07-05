use std::future::Future;
use std::sync::Arc;

use allthecodes_commands::CommandContext;
use allthecodes_gateway::{
    AdapterProvider, AdapterStatus, OutputReadBatch, RunEvent, RunId, RunMeta,
};

pub(crate) fn install_command_runtime_providers() {
    allthecodes_commands::runtime::set_runtime_installer(
        crate::app_runtime_adapters::ensure_installed,
    );
    allthecodes_commands::runtime::set_lsp_runtime_providers(
        allthecodes_ipc::subsystem_handlers::build_lsp_server_info_list,
        allthecodes_ipc::subsystem_handlers::load_lsp_recommendation_settings,
    );
    allthecodes_commands::runtime::set_lsp_recommendations_provider(
        lsp_recommendations_for_commands,
    );
    allthecodes_commands::runtime::set_agent_runtime_providers(
        builtin_agent_entries_for_commands,
        builtin_agent_prompt_for_commands,
    );
    allthecodes_commands::runtime::set_task_runtime_providers(
        tool_tasks_for_commands,
        get_tool_task_for_commands,
        stop_tool_task_for_commands,
        delete_tool_task_for_commands,
        team_task_snapshots_for_commands,
    );
    allthecodes_commands::runtime::set_team_command_executor(team_command_for_commands);
    allthecodes_commands::runtime::set_team_context_for_session_provider(team_context_for_session);
    allthecodes_commands::runtime::set_command_metadata_provider(command_metadata_for_commands);
    allthecodes_commands::runtime::set_worktree_status_provider(
        crate::ui::status_line_resolver::current_worktree_status,
    );
    allthecodes_commands::runtime::set_remote_daemon_status_provider(
        remote_daemon_status_for_commands,
    );
    allthecodes_commands::runtime::set_remote_token_path_provider(
        allthecodes_daemon::process_state::control_token_path,
    );
    allthecodes_commands::runtime::set_tool_policy_names_provider(tool_policy_names_for_commands);
    allthecodes_commands::runtime::set_tool_list_provider(all_tools_for_commands);
    allthecodes_commands::runtime::set_fork_runner(fork_runner_for_commands);
    allthecodes_tools::discovery_search::install_discovery_search_runtime(
        allthecodes_tools::discovery_search::DiscoverySearchRuntime::new()
            .with_mcp_items_provider(mcp_discovery_rows_for_tools)
            .with_plugin_items_provider(plugin_discovery_rows_for_tools),
    );
    allthecodes_mcp::runtime::install_mcp_skill_cleanup_hook(|server_name| {
        let _ = allthecodes_skills::clear_mcp_skills_for_server(server_name);
    });

    allthecodes_commands::copy::set_clipboard_copy_provider(
        crate::ui::clipboard_text::copy_text_to_clipboard,
    );
    allthecodes_commands::logout::set_onboarding_logout_clearer(
        onboarding_logout_clear_for_commands,
    );
    allthecodes_commands::skills_cmd::set_plugin_skills_provider(
        discover_plugin_skills_for_commands,
    );
    allthecodes_commands::ide_cmd::set_ide_command_runtime(
        allthecodes_commands::ide_cmd::IdeCommandRuntime {
            detect_ides: allthecodes_lsp_service::ide::detect_ides,
            selected_ide: allthecodes_lsp_service::ide::selected_ide,
            select_ide: allthecodes_lsp_service::ide::select_ide,
            clear_selection: allthecodes_lsp_service::ide::clear_selection,
            reconnect_selected: allthecodes_lsp_service::ide::reconnect_selected,
        },
    );
    allthecodes_commands::plugin_cmd::set_plugin_command_runtime(
        allthecodes_commands::plugin_cmd::PluginCommandRuntime {
            load_installed_plugins: allthecodes_plugins::loader::load_installed_plugins,
            save_installed_plugins: allthecodes_plugins::loader::save_installed_plugins,
            get_all_plugins: allthecodes_plugins::get_all_plugins,
            needs_refresh: allthecodes_plugins::needs_refresh,
            find_plugin: allthecodes_plugins::find_plugin,
            set_plugin_status: allthecodes_plugins::set_plugin_status,
            register_plugin: allthecodes_plugins::register_plugin,
            emit_event_external: emit_plugin_event_external_for_commands,
            uninstall_plugin: allthecodes_plugins::uninstall_plugin,
            // Marketplace / installation / validation (Phase 2, Serial Integration Lane)
            install_plugin: install_plugin_for_commands,
            list_marketplace: list_marketplace_for_commands,
            refresh_marketplace_cache: refresh_marketplace_cache_for_commands,
            update_plugin: update_plugin_for_commands,
            validate_plugin: validate_plugin_for_commands,
            get_plugin_info: get_plugin_info_for_commands,
        },
    );
    allthecodes_commands::reload_plugins_cmd::set_reload_plugins_runtime(
        allthecodes_commands::reload_plugins_cmd::ReloadPluginsRuntime {
            reload_plugins: reload_plugins_for_commands,
            discover_plugin_skills: discover_plugin_skills_for_commands,
        },
    );
    allthecodes_commands::brief::set_brief_command_runtime(
        allthecodes_commands::brief::BriefCommandRuntime {
            clear_prompt_cache: allthecodes_engine::prompt_sections::clear_cache,
        },
    );
    allthecodes_commands::daemon_cmd::set_daemon_command_runtime(
        allthecodes_commands::daemon_cmd::DaemonCommandRuntime {
            status_snapshot: daemon_status_snapshot_for_commands,
            state_path: allthecodes_daemon::process_state::state_path,
            request_shutdown: allthecodes_daemon::process_state::request_shutdown,
        },
    );
    allthecodes_commands::sleep_cmd::set_sleep_command_runtime(
        allthecodes_commands::sleep_cmd::SleepCommandRuntime {
            write_sleep_state: sleep_state_for_commands,
        },
    );
    allthecodes_commands::remote_cmd::set_remote_gateway_adapter(
        allthecodes_commands::remote_cmd::RemoteGatewayAdapter {
            capabilities: remote_capabilities_for_commands,
            adapters: remote_adapters_for_commands,
            connect_adapter: remote_connect_adapter_for_commands,
            test_adapter_message: remote_test_adapter_message_for_commands,
            show_run: remote_show_run_for_commands,
            run_output: remote_run_output_for_commands,
            run_timeline: remote_run_timeline_for_commands,
            stop_run: remote_stop_run_for_commands,
        },
    );
    allthecodes_commands::install_engine_command_executor();
}

fn builtin_agent_entries_for_commands() -> Vec<allthecodes_commands::runtime::BuiltinAgentEntry> {
    allthecodes_engine::agent_runtime::builtin_agent_entries()
        .into_iter()
        .map(|entry| allthecodes_commands::runtime::BuiltinAgentEntry {
            name: entry.name,
            description: entry.description,
        })
        .collect()
}

fn builtin_agent_prompt_for_commands(name: &str) -> Option<String> {
    allthecodes_engine::agent_runtime::builtin_agent_prompt(name)
}

fn tool_tasks_for_commands() -> Vec<allthecodes_tasks::TaskEntry> {
    allthecodes_tasks::global_store().list()
}

fn get_tool_task_for_commands(id: &str) -> Option<allthecodes_tasks::TaskEntry> {
    allthecodes_tasks::global_store().get(id)
}

fn stop_tool_task_for_commands(id: &str) -> Result<Option<allthecodes_tasks::TaskEntry>, String> {
    allthecodes_tasks::global_store()
        .try_stop(id)
        .map_err(|err| err.to_string())
}

fn delete_tool_task_for_commands(id: &str) -> Result<Option<allthecodes_tasks::TaskEntry>, String> {
    allthecodes_tasks::global_store()
        .try_delete(id)
        .map_err(|err| err.to_string())
}

fn team_task_snapshots_for_commands() -> Vec<allthecodes_commands::runtime::TeamTaskSnapshot> {
    allthecodes_teams::in_process::InProcessBackend::task_snapshots()
        .into_iter()
        .map(|snapshot| allthecodes_commands::runtime::TeamTaskSnapshot {
            id: snapshot.id,
            agent_id: snapshot.agent_id,
            agent_name: snapshot.agent_name,
            team_name: snapshot.team_name,
            status: match snapshot.status {
                allthecodes_teams::types::TaskStatus::Running => {
                    allthecodes_commands::runtime::TeamTaskStatus::Running
                }
                allthecodes_teams::types::TaskStatus::Stopped => {
                    allthecodes_commands::runtime::TeamTaskStatus::Stopped
                }
                allthecodes_teams::types::TaskStatus::Completed => {
                    allthecodes_commands::runtime::TeamTaskStatus::Completed
                }
            },
            is_idle: snapshot.is_idle,
            has_error: snapshot.has_error,
            error_message: snapshot.error_message,
            prompt: snapshot.prompt,
            model: snapshot.model,
            awaiting_plan_approval: snapshot.awaiting_plan_approval,
            permission_mode: snapshot.permission_mode.as_str().to_string(),
        })
        .collect()
}

fn team_command_for_commands<'a>(
    args: &'a str,
    ctx: &'a mut CommandContext,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = String> + Send + 'a>> {
    Box::pin(allthecodes_teams::command::execute_team_command(args, ctx))
}

fn team_context_for_session(session_id: &str) -> Option<allthecodes_types::teams::TeamContext> {
    allthecodes_teams::reconnection::restore_team_context_for_session(session_id)
        .map_err(|err| {
            tracing::warn!(
                session_id,
                error = %err,
                "failed to restore team context for session"
            );
            err
        })
        .ok()
        .flatten()
}

fn command_metadata_for_commands() -> Vec<allthecodes_commands::CommandMetadata> {
    allthecodes_commands::get_dynamic_metadata()
}

fn lsp_recommendations_for_commands(
) -> Vec<allthecodes_commands::runtime::LspPluginRecommendationInfo> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let installed: Vec<String> = allthecodes_plugins::loader::load_installed_plugins()
        .into_iter()
        .map(|plugin| plugin.id)
        .collect();
    let settings = allthecodes_ipc::subsystem_handlers::load_lsp_recommendation_settings();

    allthecodes_lsp_service::generate_recommendations(&cwd, &installed)
        .into_iter()
        .map(
            |rec| allthecodes_commands::runtime::LspPluginRecommendationInfo {
                is_dismissed: settings
                    .muted_plugins
                    .iter()
                    .any(|plugin| plugin == &rec.plugin_id || plugin == &rec.plugin_name),
                plugin_id: rec.plugin_id,
                plugin_name: rec.plugin_name,
                description: rec.description,
                languages: rec.languages,
                confidence: rec.confidence,
                is_already_installed: rec.is_already_installed,
            },
        )
        .collect()
}

fn all_tools_for_commands() -> allthecodes_engine::types::tool::Tools {
    allthecodes_tools::registry::get_all_tools()
}

fn tool_policy_names_for_commands(
    policy: allthecodes_commands::runtime::CommandToolPolicy,
) -> Vec<String> {
    let root_policy = match policy {
        allthecodes_commands::runtime::CommandToolPolicy::DefaultAgent => {
            allthecodes_tools::registry::ToolPolicy::DefaultAgent
        }
        allthecodes_commands::runtime::CommandToolPolicy::Coordinator => {
            allthecodes_tools::registry::ToolPolicy::Coordinator
        }
    };
    allthecodes_tools::registry::get_tools_for_policy(root_policy)
        .iter()
        .map(|tool| tool.name().to_string())
        .collect()
}

fn onboarding_logout_clear_for_commands() -> allthecodes_commands::logout::StepStatus {
    let store = allthecodes_services::onboarding::OnboardingStore::open_default();
    let had_state_before = match store.load() {
        Ok(state) => !state.is_first_run() || store.path().exists(),
        Err(_) => store.path().exists(),
    };
    if !had_state_before {
        return allthecodes_commands::logout::StepStatus::NoOp;
    }
    match store.update(|state| state.reset_for_logout()) {
        Ok(_) => allthecodes_commands::logout::StepStatus::Cleared,
        Err(error) => allthecodes_commands::logout::StepStatus::Failed(error.to_string()),
    }
}

fn fork_runner_for_commands(
    params: allthecodes_commands::runtime::CommandForkParams,
) -> std::pin::Pin<
    Box<
        dyn std::future::Future<
                Output = anyhow::Result<allthecodes_commands::runtime::CommandForkOutcome>,
            > + Send
            + 'static,
    >,
> {
    Box::pin(async move {
        let outcome = allthecodes_engine::agent::fork::run_fork(
            allthecodes_engine::agent::fork::ForkParams {
                agent_id: None,
                prompt: params.prompt,
                cwd: params.cwd,
                model: params.model,
                fallback_model: params.fallback_model,
                tools: params.tools,
                max_turns: params.max_turns,
                parent_messages: params.parent_messages,
                append_system_prompt: params.append_system_prompt,
                custom_system_prompt: params.custom_system_prompt,
                hook_runner: Arc::new(allthecodes_types::hooks::NoopHookRunner::new()),
                command_dispatcher: Arc::new(
                    allthecodes_types::commands::NoopCommandDispatcher::new(),
                ),
            },
        )
        .await?;

        Ok(allthecodes_commands::runtime::CommandForkOutcome {
            text: outcome.text,
            had_error: outcome.had_error,
            duration_ms: outcome.duration_ms,
            agent_id: outcome.agent_id,
        })
    })
}

fn reload_plugins_for_commands() -> allthecodes_commands::reload_plugins_cmd::ReloadReport {
    let report = allthecodes_plugins::reload_plugins();
    allthecodes_commands::reload_plugins_cmd::ReloadReport {
        count: report.count,
        error_count: report.error_count,
        errors: report.errors,
        global_errors: report.global_errors,
        duration_ms: report.duration_ms,
    }
}

fn discover_plugin_skills_for_commands() -> Vec<allthecodes_skills::SkillDefinition> {
    let mut out = Vec::new();

    for contributed in allthecodes_plugins::discover_plugin_skill_definitions() {
        let source = allthecodes_skills::SkillSource::Plugin(contributed.plugin_id.clone());
        let mut skill = match allthecodes_skills::loader::load_skill_from_file_path(
            &contributed.path,
            source,
        ) {
            Some(skill) => skill,
            None => {
                tracing::warn!(
                    plugin = %contributed.plugin_id,
                    path = %contributed.path.display(),
                    "Plugin: failed to load contributed skill file"
                );
                continue;
            }
        };

        skill.name = contributed.name;
        if let Some(desc) = contributed.description {
            if !desc.trim().is_empty() {
                skill.frontmatter.description = desc;
            }
        }
        out.push(skill);
    }

    out
}

fn emit_plugin_event_external_for_commands(
    event: allthecodes_ipc_protocol::subsystem_events::SubsystemEvent,
) {
    use allthecodes_ipc_protocol::subsystem_events::{PluginEvent, SubsystemEvent};

    let SubsystemEvent::Plugin(event) = event else {
        return;
    };
    let adapted = match event {
        PluginEvent::Reloaded { count, had_error } => {
            allthecodes_plugins::PluginSubsystemEvent::Reloaded { count, had_error }
        }
        PluginEvent::RefreshNeeded { reason } => {
            allthecodes_plugins::PluginSubsystemEvent::RefreshNeeded { reason }
        }
        PluginEvent::StatusChanged {
            plugin_id,
            name,
            status,
            error,
        } => allthecodes_plugins::PluginSubsystemEvent::StatusChanged {
            plugin_id,
            name,
            status,
            error,
        },
        PluginEvent::PluginList { .. } => return,
        PluginEvent::Installed {
            plugin_id,
            name,
            version,
        } => allthecodes_plugins::PluginSubsystemEvent::Installed {
            plugin_id,
            name,
            version,
        },
        PluginEvent::Updated {
            plugin_id,
            name,
            version,
        } => allthecodes_plugins::PluginSubsystemEvent::Updated {
            plugin_id,
            name,
            old_version: "unknown".to_string(),
            new_version: version,
        },
        PluginEvent::Uninstalled { plugin_id, name } => {
            allthecodes_plugins::PluginSubsystemEvent::Uninstalled { plugin_id, name }
        }
        PluginEvent::ValidationFailed {
            plugin_id,
            name: _,
            errors,
        } => allthecodes_plugins::PluginSubsystemEvent::ValidationFailed { plugin_id, errors },
        PluginEvent::ConfigChanged { plugin_id, name: _ } => {
            allthecodes_plugins::PluginSubsystemEvent::ConfigChanged { plugin_id }
        }
    };
    allthecodes_plugins::emit_event_external(adapted);
}

fn daemon_status_snapshot_for_commands(
) -> anyhow::Result<allthecodes_commands::daemon_cmd::DaemonStatusSnapshot> {
    Ok(
        match allthecodes_daemon::process_state::status_snapshot()? {
            allthecodes_daemon::process_state::DaemonStatusSnapshot::Running(state) => {
                allthecodes_commands::daemon_cmd::DaemonStatusSnapshot::Running(map_daemon_state(
                    state,
                ))
            }
            allthecodes_daemon::process_state::DaemonStatusSnapshot::Stale(state) => {
                allthecodes_commands::daemon_cmd::DaemonStatusSnapshot::Stale(map_daemon_state(
                    state,
                ))
            }
            allthecodes_daemon::process_state::DaemonStatusSnapshot::Stopped => {
                allthecodes_commands::daemon_cmd::DaemonStatusSnapshot::Stopped
            }
        },
    )
}

fn map_daemon_state(
    state: allthecodes_daemon::process_state::DaemonProcessState,
) -> allthecodes_commands::daemon_cmd::DaemonProcessState {
    allthecodes_commands::daemon_cmd::DaemonProcessState {
        pid: state.pid,
        health_url: state.health_url,
        workers: state
            .workers
            .into_iter()
            .map(
                |worker| allthecodes_commands::daemon_cmd::DaemonWorkerSummary {
                    worker_id: worker.worker_id,
                    kind: worker.kind,
                    pid: worker.pid,
                    status: worker.status,
                    updated_at: worker.updated_at,
                },
            )
            .collect(),
    }
}

fn sleep_state_for_commands(
    duration_seconds: u64,
    reason: &str,
) -> anyhow::Result<allthecodes_commands::sleep_cmd::DaemonSleepState> {
    let state = allthecodes_daemon::process_state::write_sleep_state(duration_seconds, reason)?;
    Ok(allthecodes_commands::sleep_cmd::DaemonSleepState {
        sleeping_until: state.sleeping_until,
    })
}

fn remote_daemon_status_for_commands(
) -> Result<allthecodes_commands::remote_cmd::LocalGatewayDaemonStatus, String> {
    allthecodes_daemon::gateway_client::LocalGatewayClient::daemon_status()
        .map(map_remote_daemon_status)
        .map_err(|error| error.to_string())
}

fn map_remote_daemon_status(
    status: allthecodes_daemon::gateway_client::LocalGatewayDaemonStatus,
) -> allthecodes_commands::remote_cmd::LocalGatewayDaemonStatus {
    match status {
        allthecodes_daemon::gateway_client::LocalGatewayDaemonStatus::Running {
            pid,
            base_url,
            health_url,
            ..
        } => allthecodes_commands::remote_cmd::LocalGatewayDaemonStatus::Running {
            pid,
            base_url,
            health_url,
        },
        allthecodes_daemon::gateway_client::LocalGatewayDaemonStatus::Stale { pid, .. } => {
            allthecodes_commands::remote_cmd::LocalGatewayDaemonStatus::Stale { pid }
        }
        allthecodes_daemon::gateway_client::LocalGatewayDaemonStatus::Stopped => {
            allthecodes_commands::remote_cmd::LocalGatewayDaemonStatus::Stopped
        }
    }
}

fn remote_capabilities_for_commands() -> allthecodes_commands::remote_cmd::RemoteFuture<
    allthecodes_commands::remote_cmd::GatewayCapabilitiesSnapshot,
> {
    Box::pin(async {
        let client = allthecodes_daemon::gateway_client::LocalGatewayClient::from_running_daemon()?;
        let cap = client.capabilities().await?;
        Ok(
            allthecodes_commands::remote_cmd::GatewayCapabilitiesSnapshot {
                version: cap.version,
                auth_mode: cap.auth_mode,
                supports_steer: cap.supports_steer,
                max_running: cap.max_running,
                max_queued: cap.max_queued,
                endpoints: cap.endpoints,
            },
        )
    })
}

fn remote_adapters_for_commands(
) -> allthecodes_commands::remote_cmd::RemoteFuture<Vec<AdapterStatus>> {
    Box::pin(async {
        let client = allthecodes_daemon::gateway_client::LocalGatewayClient::from_running_daemon()?;
        client.adapters().await
    })
}

fn remote_connect_adapter_for_commands(
    provider: AdapterProvider,
) -> allthecodes_commands::remote_cmd::RemoteFuture<AdapterStatus> {
    Box::pin(async move {
        let client = allthecodes_daemon::gateway_client::LocalGatewayClient::from_running_daemon()?;
        client.connect_adapter(provider).await
    })
}

fn remote_test_adapter_message_for_commands(
    provider: AdapterProvider,
    target: String,
    text: String,
) -> allthecodes_commands::remote_cmd::RemoteFuture<AdapterStatus> {
    Box::pin(async move {
        let client = allthecodes_daemon::gateway_client::LocalGatewayClient::from_running_daemon()?;
        client.test_adapter_message(provider, target, text).await
    })
}

fn remote_show_run_for_commands(
    run_id: RunId,
) -> allthecodes_commands::remote_cmd::RemoteFuture<RunMeta> {
    Box::pin(async move {
        let client = allthecodes_daemon::gateway_client::LocalGatewayClient::from_running_daemon()?;
        client.show_run(&run_id).await
    })
}

fn remote_run_output_for_commands(
    run_id: RunId,
) -> allthecodes_commands::remote_cmd::RemoteFuture<OutputReadBatch> {
    Box::pin(async move {
        let client = allthecodes_daemon::gateway_client::LocalGatewayClient::from_running_daemon()?;
        client.run_output(&run_id).await
    })
}

fn remote_run_timeline_for_commands(
    run_id: RunId,
) -> allthecodes_commands::remote_cmd::RemoteFuture<Vec<RunEvent>> {
    Box::pin(async move {
        let client = allthecodes_daemon::gateway_client::LocalGatewayClient::from_running_daemon()?;
        client.run_timeline(&run_id).await
    })
}

fn remote_stop_run_for_commands(
    run_id: RunId,
) -> allthecodes_commands::remote_cmd::RemoteFuture<
    allthecodes_commands::remote_cmd::GatewayRunActionResponse,
> {
    Box::pin(async move {
        let client = allthecodes_daemon::gateway_client::LocalGatewayClient::from_running_daemon()?;
        let response = client.stop_run(&run_id).await?;
        Ok(allthecodes_commands::remote_cmd::GatewayRunActionResponse {
            run_id: response.run_id,
            status: response.status,
            action: response.action,
            diagnostic: response.diagnostic,
        })
    })
}

// ---------------------------------------------------------------------------
// PluginCommandRuntime marketplace/installation implementations (Phase 2,
// Serial Integration Lane) — replaces previous stubs with real wiring to
// cc-plugins APIs.
// ---------------------------------------------------------------------------

fn install_plugin_for_commands(
    source: &str,
    _version: Option<&str>,
) -> Result<String, anyhow::Error> {
    let engine_version = Some(env!("CARGO_PKG_VERSION"));
    let policy = managed_policy_for_commands();
    let (available_plugins, all_manifests) = plugin_dependency_context();
    let source = source.to_string();

    let result = block_on_in_worker(async move {
        allthecodes_plugins::installation::install_plugin(
            &source,
            None,
            engine_version,
            policy.as_ref(),
            &available_plugins,
            &all_manifests,
        )
        .await
        .map_err(|e| anyhow::anyhow!("{}", e))
    })?;
    Ok(format!(
        "Installed {} v{}",
        result.plugin.name, result.plugin.version
    ))
}

fn list_marketplace_for_commands(query: &str) -> Result<Vec<String>, anyhow::Error> {
    let all = if query.trim().is_empty() {
        allthecodes_plugins::marketplace::list_all_marketplaces()
    } else {
        allthecodes_plugins::marketplace::search_marketplace(query.trim())
    };
    if all.is_empty() {
        // No marketplace entries cached yet; return empty list without error
        // so the caller can distinguish "not implemented" from "nothing found".
        return Ok(Vec::new());
    }
    let lines: Vec<String> = all
        .iter()
        .map(|entry| {
            format!(
                "{} v{} — {} ({})",
                entry.name, entry.version, entry.description, entry.source_name
            )
        })
        .collect();
    Ok(lines)
}

fn refresh_marketplace_cache_for_commands() -> Result<String, anyhow::Error> {
    let path = allthecodes_plugins::marketplaces_dir().join("known_marketplaces.json");
    let idx = &*allthecodes_plugins::marketplace::GLOBAL_MARKETPLACE_INDEX;
    if path.exists() {
        idx.load_from_file(&path)?;
        let count = block_on_in_worker(async {
            allthecodes_plugins::marketplace::refresh_all_marketplaces().await
        })?;
        Ok(format!(
            "Marketplace cache refreshed: {} plugin entries",
            count
        ))
    } else {
        Ok("No known marketplaces file found; cache is empty".to_string())
    }
}

fn update_plugin_for_commands(plugin_id: &str) -> Result<String, anyhow::Error> {
    let engine_version = Some(env!("CARGO_PKG_VERSION"));
    let policy = managed_policy_for_commands();
    let (available_plugins, all_manifests) = plugin_dependency_context();
    let plugin_id = plugin_id.to_string();

    let result = block_on_in_worker(async move {
        allthecodes_plugins::installation::update_plugin(
            &plugin_id,
            engine_version,
            policy.as_ref(),
            &available_plugins,
            &all_manifests,
        )
        .await
        .map_err(|e| anyhow::anyhow!("{}", e))
    })?;
    Ok(format!(
        "Updated {} to v{}",
        result.plugin.name, result.plugin.version
    ))
}

fn validate_plugin_for_commands(plugin_id: &str) -> Result<Vec<String>, anyhow::Error> {
    let plugin = allthecodes_plugins::find_plugin(plugin_id)
        .ok_or_else(|| anyhow::anyhow!("Plugin '{}' not found", plugin_id))?;
    let cache_path = plugin
        .cache_path
        .ok_or_else(|| anyhow::anyhow!("Plugin '{}' has no cache path", plugin_id))?;
    let errors = allthecodes_plugins::validation::PluginValidator::validate_plugin(&cache_path);
    let messages: Vec<String> = errors
        .iter()
        .map(|e| format!("[{:?}] {}: {}", e.severity, e.field, e.message))
        .collect();
    if messages.is_empty() {
        Ok(Vec::new())
    } else {
        Ok(messages)
    }
}

fn get_plugin_info_for_commands(plugin_id: &str) -> Result<String, anyhow::Error> {
    let plugin = allthecodes_plugins::find_plugin(plugin_id)
        .ok_or_else(|| anyhow::anyhow!("Plugin '{}' not found", plugin_id))?;
    let info = serde_json::to_string_pretty(&serde_json::json!({
        "id": plugin.id,
        "name": plugin.name,
        "version": plugin.version,
        "description": plugin.description,
        "status": format!("{:?}", plugin.status),
        "source": format!("{:?}", plugin.source),
        "marketplace": plugin.marketplace,
        "tools": plugin.tools,
        "skills": plugin.skills,
        "mcp_servers": plugin.mcp_servers,
        "installed_at": plugin.installed_at,
    }))?;
    Ok(info)
}

pub(crate) fn block_on_in_worker<F, T>(future: F) -> anyhow::Result<T>
where
    F: Future<Output = anyhow::Result<T>> + Send + 'static,
    T: Send + 'static,
{
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| anyhow::anyhow!("failed to create tokio runtime: {}", e))?;
        rt.block_on(future)
    })
    .join()
    .map_err(|_| anyhow::anyhow!("plugin worker thread panicked"))?
}

pub(crate) fn managed_policy_for_commands() -> Option<allthecodes_config::mdm::ManagedPolicy> {
    allthecodes_config::mdm::load_managed_settings_policy()
        .ok()
        .and_then(|cfg| cfg.managed)
        .and_then(|managed| managed.policy)
}

pub(crate) fn plugin_dependency_context() -> (
    std::collections::HashMap<String, String>,
    std::collections::HashMap<String, allthecodes_plugins::manifest::PluginManifest>,
) {
    let installed = allthecodes_plugins::loader::load_installed_plugins();
    let mut available = std::collections::HashMap::new();
    let mut manifests = std::collections::HashMap::new();

    for plugin in installed {
        available.insert(plugin.id.clone(), plugin.version.clone());
        available.insert(plugin.name.clone(), plugin.version.clone());
        if let Some(cache_path) = plugin.cache_path.as_ref() {
            if let Ok(manifest) = allthecodes_plugins::manifest::load_manifest(cache_path) {
                manifests.insert(manifest.name.clone(), manifest.clone());
                manifests.insert(plugin.id.clone(), manifest);
            }
        }
    }

    (available, manifests)
}

fn mcp_discovery_rows_for_tools() -> Vec<allthecodes_tools::discovery_search::DiscoverySearchResult>
{
    let mut rows = Vec::new();
    rows.extend(configured_mcp_discovery_rows_for_tools());
    if let Some(manager) = allthecodes_mcp::runtime::current_manager() {
        match manager.try_lock() {
            Ok(manager) => rows.extend(manager.discovery_search_results()),
            Err(error) => {
                tracing::debug!(
                    error = %error,
                    "MCP discovery search skipped runtime rows because manager is busy"
                );
            }
        }
    }
    if allthecodes_config::features::enabled(allthecodes_config::features::Feature::McpSkills) {
        rows.extend(mcp_skill_discovery_rows_for_tools());
    }
    rows
}

fn configured_mcp_discovery_rows_for_tools(
) -> Vec<allthecodes_tools::discovery_search::DiscoverySearchResult> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let scoped = match allthecodes_mcp::discovery::discover_mcp_servers_scoped(&cwd) {
        Ok(scoped) => scoped,
        Err(error) => {
            tracing::warn!(
                error = %error,
                "MCP discovery search could not read configured servers"
            );
            return Vec::new();
        }
    };

    scoped
        .into_iter()
        .map(|entry| {
            let source = mcp_discovery_scope_source(&entry.scope);
            let mut status = if let Some(error) = entry.error {
                allthecodes_tools::discovery_search::DiscoveryStatusSummary::new("error")
                    .with_detail(error)
            } else if entry.config.disabled.unwrap_or(false) {
                allthecodes_tools::discovery_search::DiscoveryStatusSummary::new("disabled")
            } else {
                allthecodes_tools::discovery_search::DiscoveryStatusSummary::new("configured")
            };
            if status.state == "configured"
                && entry.config.command.is_none()
                && entry.config.url.is_none()
            {
                status = status.with_detail("No command or URL configured");
            }

            allthecodes_tools::discovery_search::DiscoverySearchResult::new(
                allthecodes_tools::discovery_search::DiscoveryResultKind::McpServer,
                entry.config.name.clone(),
            )
            .with_id(format!("{}:{}", source, entry.config.name))
            .with_source(source)
            .with_server_name(entry.config.name.clone())
            .with_description(mcp_config_description(&entry.config))
            .with_status(status)
            .with_capabilities(mcp_config_capabilities(&entry.config))
            .with_next_action(
                allthecodes_tools::discovery_search::DiscoveryNextAction::new(
                    "Inspect MCP server",
                    format!("/mcp status {}", entry.config.name),
                ),
            )
            .with_signal(allthecodes_tools::discovery_search::DiscoverySignal::ExplicitSearch)
        })
        .collect()
}

fn mcp_discovery_scope_source(scope: &allthecodes_mcp::discovery::DiscoveryScope) -> &'static str {
    match scope {
        allthecodes_mcp::discovery::DiscoveryScope::User => "user",
        allthecodes_mcp::discovery::DiscoveryScope::Project => "project",
        allthecodes_mcp::discovery::DiscoveryScope::Plugin(_)
        | allthecodes_mcp::discovery::DiscoveryScope::Ide(_) => "runtime",
    }
}

fn mcp_config_description(config: &allthecodes_mcp::McpServerConfig) -> String {
    match config.transport.as_str() {
        "stdio" => match (&config.command, &config.args) {
            (Some(command), Some(args)) if !args.is_empty() => {
                format!("stdio MCP server: {} {}", command, args.join(" "))
            }
            (Some(command), _) => format!("stdio MCP server: {command}"),
            _ => "stdio MCP server".to_string(),
        },
        "sse" | "streamable-http" => config
            .url
            .as_ref()
            .map(|url| format!("{} MCP server: {}", config.transport, url))
            .unwrap_or_else(|| format!("{} MCP server", config.transport)),
        other => format!("{other} MCP server"),
    }
}

fn mcp_config_capabilities(config: &allthecodes_mcp::McpServerConfig) -> Vec<String> {
    let mut capabilities = vec![config.transport.clone()];
    if config.command.is_some() {
        capabilities.push("command".to_string());
    }
    if config.url.is_some() {
        capabilities.push("url".to_string());
    }
    if config.oauth.is_some() {
        capabilities.push("oauth".to_string());
    }
    if config.browser_mcp.unwrap_or(false) {
        capabilities.push("browser".to_string());
    }
    capabilities
}

fn mcp_skill_discovery_rows_for_tools(
) -> Vec<allthecodes_tools::discovery_search::DiscoverySearchResult> {
    allthecodes_skills::get_all_skills()
        .into_iter()
        .filter_map(|skill| {
            let allthecodes_skills::SkillSource::Mcp(server_name) = &skill.source else {
                return None;
            };
            let mut row = allthecodes_tools::discovery_search::DiscoverySearchResult::new(
                allthecodes_tools::discovery_search::DiscoveryResultKind::McpSkill,
                skill.display_name().to_string(),
            )
            .with_id(skill.name.clone())
            .with_source("runtime")
            .with_server_name(server_name.clone())
            .with_status(
                allthecodes_tools::discovery_search::DiscoveryStatusSummary::new("available"),
            )
            .with_invocation_flags(skill.is_user_invocable(), skill.is_model_invocable())
            .with_skill_summaries(
                [allthecodes_tools::discovery_search::DiscoverySkillSummary {
                    name: skill.name.clone(),
                    description: (!skill.frontmatter.description.trim().is_empty())
                        .then(|| skill.frontmatter.description.clone()),
                    source: Some(format!("mcp:{server_name}")),
                }],
            )
            .with_next_action(
                allthecodes_tools::discovery_search::DiscoveryNextAction::new(
                    "Use skill",
                    format!("/skills {}", skill.name),
                ),
            )
            .with_signal(
                allthecodes_tools::discovery_search::DiscoverySignal::McpResourceDiscovery,
            );
            if !skill.frontmatter.description.trim().is_empty() {
                row = row.with_description(skill.frontmatter.description.clone());
            }
            if let Some(when_to_use) = skill
                .frontmatter
                .when_to_use
                .as_ref()
                .filter(|value| !value.trim().is_empty())
            {
                row = row.with_when_to_use(when_to_use.clone());
            }
            Some(row)
        })
        .collect()
}

fn plugin_discovery_rows_for_tools(
) -> Vec<allthecodes_tools::discovery_search::DiscoverySearchResult> {
    plugin_discovery_rows_from_sources(
        allthecodes_plugins::loader::load_installed_plugins(),
        allthecodes_plugins::get_enabled_plugins(),
        allthecodes_plugins::marketplace::list_all_marketplaces(),
    )
}

fn plugin_discovery_rows_from_sources(
    installed: Vec<allthecodes_plugins::PluginEntry>,
    active: Vec<allthecodes_plugins::PluginEntry>,
    marketplace_entries: Vec<allthecodes_plugins::marketplace::MarketplacePluginEntry>,
) -> Vec<allthecodes_tools::discovery_search::DiscoverySearchResult> {
    let mut rows = Vec::new();
    rows.extend(
        installed
            .into_iter()
            .map(|plugin| plugin_discovery_row(plugin, "installed")),
    );
    rows.extend(
        active
            .into_iter()
            .map(|plugin| plugin_discovery_row(plugin, "active")),
    );
    rows.extend(
        marketplace_entries
            .into_iter()
            .map(marketplace_plugin_discovery_row),
    );
    rows
}

fn plugin_discovery_row(
    plugin: allthecodes_plugins::PluginEntry,
    source: &'static str,
) -> allthecodes_tools::discovery_search::DiscoverySearchResult {
    let status = plugin_status_summary(&plugin.status, source);
    let mut row = allthecodes_tools::discovery_search::DiscoverySearchResult::new(
        allthecodes_tools::discovery_search::DiscoveryResultKind::Plugin,
        plugin.name.clone(),
    )
    .with_id(plugin.id.clone())
    .with_source(source)
    .with_version(plugin.version.clone())
    .with_status(status)
    .with_skills(plugin.skills.clone())
    .with_tools(plugin.tools.clone())
    .with_mcp_servers(plugin.mcp_servers.clone())
    .with_next_action(
        allthecodes_tools::discovery_search::DiscoveryNextAction::new(
            "Inspect plugin",
            format!("/plugin info {}", plugin.id),
        ),
    )
    .with_signal(allthecodes_tools::discovery_search::DiscoverySignal::PluginMarketplaceCache);
    if !plugin.description.trim().is_empty() {
        row = row.with_description(plugin.description.clone());
    }
    if let Some(marketplace) = plugin
        .marketplace
        .as_ref()
        .filter(|marketplace| !marketplace.trim().is_empty())
    {
        row = row.with_marketplace(marketplace.clone());
    }
    row
}

fn marketplace_plugin_discovery_row(
    entry: allthecodes_plugins::marketplace::MarketplacePluginEntry,
) -> allthecodes_tools::discovery_search::DiscoverySearchResult {
    let mut row = allthecodes_tools::discovery_search::DiscoverySearchResult::new(
        allthecodes_tools::discovery_search::DiscoveryResultKind::Plugin,
        entry.name.clone(),
    )
    .with_id(entry.id.clone())
    .with_source("marketplace_cache")
    .with_version(entry.version.clone())
    .with_status(allthecodes_tools::discovery_search::DiscoveryStatusSummary::new("available"))
    .with_marketplace(entry.source_name.clone())
    .with_next_action(
        allthecodes_tools::discovery_search::DiscoveryNextAction::new(
            "Inspect marketplace plugin",
            format!("/plugin marketplace search {}", entry.id),
        ),
    )
    .with_signal(allthecodes_tools::discovery_search::DiscoverySignal::PluginMarketplaceCache)
    .with_remote_state_placeholder();
    if !entry.description.trim().is_empty() {
        row = row.with_description(entry.description.clone());
    }
    if !entry.tags.is_empty() {
        row = row.with_capabilities(entry.tags.clone());
    }
    row
}

fn plugin_status_summary(
    status: &allthecodes_plugins::PluginStatus,
    source: &str,
) -> allthecodes_tools::discovery_search::DiscoveryStatusSummary {
    match status {
        allthecodes_plugins::PluginStatus::NotInstalled => {
            allthecodes_tools::discovery_search::DiscoveryStatusSummary::new("not_installed")
        }
        allthecodes_plugins::PluginStatus::Installed if source == "active" => {
            allthecodes_tools::discovery_search::DiscoveryStatusSummary::new("active")
        }
        allthecodes_plugins::PluginStatus::Installed => {
            allthecodes_tools::discovery_search::DiscoveryStatusSummary::new("installed")
        }
        allthecodes_plugins::PluginStatus::Disabled => {
            allthecodes_tools::discovery_search::DiscoveryStatusSummary::new("disabled")
        }
        allthecodes_plugins::PluginStatus::Error(message) => {
            allthecodes_tools::discovery_search::DiscoveryStatusSummary::new("error")
                .with_detail(message.clone())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_plugins::marketplace::MarketplacePluginEntry;
    use allthecodes_plugins::{PluginEntry, PluginSource, PluginStatus};
    use allthecodes_tools::discovery_search::DiscoveryResultKind;

    fn plugin_entry(id: &str, status: PluginStatus) -> PluginEntry {
        PluginEntry {
            id: id.to_string(),
            name: id.to_string(),
            version: "1.2.3".to_string(),
            description: format!("{id} plugin"),
            source: PluginSource::Local {
                path: format!("/tmp/{id}"),
            },
            status,
            marketplace: Some("local-marketplace".to_string()),
            cache_path: None,
            installed_version: Some("1.2.3".to_string()),
            official: false,
            download_url: None,
            homepage: None,
            sha256: None,
            tools: vec![format!("{id}-tool")],
            skills: vec![format!("{id}-skill")],
            mcp_servers: vec![format!("{id}-mcp")],
            installed_at: Some(1),
            updated_at: None,
        }
    }

    fn marketplace_entry(id: &str) -> MarketplacePluginEntry {
        MarketplacePluginEntry {
            id: id.to_string(),
            name: id.to_string(),
            description: format!("{id} marketplace plugin"),
            version: "2.0.0".to_string(),
            author: Some("allthecodes".to_string()),
            source_name: "cached-marketplace".to_string(),
            download_url: None,
            checksum: None,
            sha256: None,
            tags: vec!["rust".to_string()],
            homepage: None,
            license: None,
        }
    }

    #[test]
    fn plugin_discovery_rows_cover_installed_active_and_marketplace_cache() {
        let rows = plugin_discovery_rows_from_sources(
            vec![plugin_entry("disabled-tools", PluginStatus::Disabled)],
            vec![plugin_entry("rust-tools", PluginStatus::Installed)],
            vec![marketplace_entry("future-tools")],
        );

        let disabled = rows
            .iter()
            .find(|row| row.id.as_deref() == Some("disabled-tools"))
            .expect("installed row");
        assert_eq!(disabled.kind, DiscoveryResultKind::Plugin);
        assert_eq!(disabled.source.as_deref(), Some("installed"));
        assert_eq!(
            disabled
                .status_summary
                .as_ref()
                .map(|status| status.state.as_str()),
            Some("disabled")
        );

        let active = rows
            .iter()
            .find(|row| row.id.as_deref() == Some("rust-tools"))
            .expect("active row");
        assert_eq!(active.source.as_deref(), Some("active"));
        assert_eq!(active.skills, vec!["rust-tools-skill"]);
        assert_eq!(active.tools, vec!["rust-tools-tool"]);
        assert_eq!(active.mcp_servers, vec!["rust-tools-mcp"]);

        let marketplace = rows
            .iter()
            .find(|row| row.id.as_deref() == Some("future-tools"))
            .expect("marketplace cache row");
        assert_eq!(marketplace.source.as_deref(), Some("marketplace_cache"));
        assert_eq!(
            marketplace.marketplace.as_deref(),
            Some("cached-marketplace")
        );
        assert!(marketplace.remote_url_todo);
        assert_eq!(marketplace.remote_source.as_deref(), Some("deferred"));
    }
}
