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
