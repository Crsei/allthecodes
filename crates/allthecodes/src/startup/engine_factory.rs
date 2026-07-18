use std::sync::Arc;

use allthecodes_engine::lifecycle::QueryEngine;
use allthecodes_engine::runtime_services::{
    CommandDispatcherService, HookRunnerService, ModelClientFactoryService,
    PermissionMessageResolver, RuntimeServices, ToolRegistryService,
};
use allthecodes_engine::types::app_state::AppState;
use allthecodes_engine::types::config::QueryEngineConfig;
use allthecodes_engine::types::tool::Tools;
use tracing::{debug, info, warn};

use crate::classifier_model;
use crate::startup::diagnostics::{deduplicate, StartupDiagnostic, StartupDiagnosticSource};
use crate::startup::mcp_runtime::McpRuntime;
use crate::startup::model_runtime::ModelRuntime;
use crate::startup::runtime_composition::RuntimeReady;
use crate::startup::settings_runtime::SettingsRuntime;
use crate::startup::startup_context::StartupContext;

pub(crate) struct EngineFactory;

struct StartupToolRegistryService {
    tools: Tools,
}

impl ToolRegistryService for StartupToolRegistryService {
    fn active_tools(&self) -> Tools {
        self.tools.clone()
    }
}

struct RootPermissionMessageResolver;

impl PermissionMessageResolver for RootPermissionMessageResolver {
    fn resolve_permission_message(&self, tool_name: &str) -> Option<String> {
        crate::resolve_descriptive_permission_message(tool_name)
    }
}

struct ShellHookRunnerService;

impl HookRunnerService for ShellHookRunnerService {
    fn hook_runner(&self) -> Arc<dyn allthecodes_types::hooks::HookRunner> {
        Arc::new(allthecodes_tools::hooks::ShellHookRunner::new())
    }
}

struct FullCommandDispatcherService;

impl CommandDispatcherService for FullCommandDispatcherService {
    fn command_dispatcher(&self) -> Arc<dyn allthecodes_types::commands::CommandDispatcher> {
        Arc::new(allthecodes_commands::DefaultCommandDispatcher::for_full_registry())
    }
}

struct ApiClientFactoryService;

impl ModelClientFactoryService for ApiClientFactoryService {
    fn client_for_backend(
        &self,
        backend_name: Option<&str>,
    ) -> Option<Arc<allthecodes_api::api::client::ApiClient>> {
        allthecodes_api::api::client::ApiClient::from_backend(backend_name).map(Arc::new)
    }
}

impl EngineFactory {
    pub(crate) fn runtime_services(tools: Tools) -> Arc<RuntimeServices> {
        Arc::new(RuntimeServices {
            tool_registry: Arc::new(StartupToolRegistryService { tools }),
            permission_message_resolver: Arc::new(RootPermissionMessageResolver),
            hook_runner: Arc::new(ShellHookRunnerService),
            command_dispatcher: Arc::new(FullCommandDispatcherService),
            model_client_factory: Arc::new(ApiClientFactoryService),
        })
    }

    pub(crate) async fn build(
        startup: StartupContext,
        settings: SettingsRuntime,
        mcp: McpRuntime,
        model: ModelRuntime,
        mut app_state: AppState,
        runtime_services: Arc<RuntimeServices>,
    ) -> anyhow::Result<RuntimeReady> {
        let mut startup_diagnostics = model.diagnostics;
        startup_diagnostics.extend(mcp.diagnostics);
        let StartupContext {
            cli,
            cwd,
            initial_prompt,
            ..
        } = startup;
        let cwd = cwd.to_string_lossy().into_owned();
        let merged_config = settings.merged_config;
        let tools = mcp.tools;

        let (resume_messages, resumed_session_id): (
            Option<Vec<allthecodes_types::message::Message>>,
            Option<String>,
        ) = if cli.resume {
            match allthecodes_session::resume::get_last_session(std::path::Path::new(&cwd)) {
                Ok(Some(info)) => {
                    info!(session = %info.session_id, "resuming last session");
                    match allthecodes_session::resume::resume_session(&info.session_id) {
                        Ok(msgs) => {
                            info!(count = msgs.len(), "loaded messages from previous session");
                            (Some(msgs), Some(info.session_id))
                        }
                        Err(e) => {
                            warn!(error = %e, "failed to load session messages");
                            startup_diagnostics.push(StartupDiagnostic::warning(
                                "history-load",
                                StartupDiagnosticSource::History,
                                "Failed to load the requested session history",
                                Some(e.to_string()),
                                Some("Start a new session or check the session store.".to_string()),
                            ));
                            (None, None)
                        }
                    }
                }
                Ok(None) => {
                    warn!("no session to resume");
                    startup_diagnostics.push(StartupDiagnostic::error(
                        "history-resume-missing",
                        StartupDiagnosticSource::History,
                        "No resumable session was found",
                        None,
                        Some("Start a new session without --resume.".to_string()),
                    ));
                    (None, None)
                }
                Err(e) => {
                    warn!(error = %e, "failed to find session to resume");
                    startup_diagnostics.push(StartupDiagnostic::warning(
                        "history-resume-lookup",
                        StartupDiagnosticSource::History,
                        "Could not find the session to resume",
                        Some(e.to_string()),
                        Some("Check the session store or start a new session.".to_string()),
                    ));
                    (None, None)
                }
            }
        } else if let Some(ref session_id) = cli.continue_session {
            info!(session = %session_id, "continuing session");
            match allthecodes_session::resume::resume_session(session_id) {
                Ok(msgs) => {
                    info!(count = msgs.len(), "loaded messages for --continue");
                    (Some(msgs), Some(session_id.clone()))
                }
                Err(e) => {
                    warn!(error = %e, "failed to load session {}", session_id);
                    startup_diagnostics.push(StartupDiagnostic::warning(
                        "history-continue-load",
                        StartupDiagnosticSource::History,
                        "Failed to load the requested continued session",
                        Some(e.to_string()),
                        Some("Check the session id or start a new session.".to_string()),
                    ));
                    (None, None)
                }
            }
        } else {
            (None, None)
        };

        if let Some(session_id) = resumed_session_id.as_deref() {
            match allthecodes_teams::reconnection::restore_team_context_for_session(session_id) {
                Ok(Some(team_context)) => {
                    app_state.team_context = Some(team_context);
                }
                Ok(None) => {}
                Err(err) => {
                    warn!(
                        session_id,
                        error = %err,
                        "failed to restore team context for resumed session"
                    );
                    startup_diagnostics.push(StartupDiagnostic::warning(
                        "history-team-context",
                        StartupDiagnosticSource::History,
                        "Resumed team context could not be restored",
                        Some(err.to_string()),
                        Some("Review the resumed session's team state.".to_string()),
                    ));
                }
            }
        }

        crate::app_runtime_adapters::ensure_installed();

        let engine_config = QueryEngineConfig {
            cwd: cwd.clone(),
            tools: tools.clone(),
            custom_system_prompt: cli.system_prompt.clone(),
            append_system_prompt: cli.append_system_prompt.clone(),
            user_specified_model: cli.model.clone(),
            fallback_model: Some(model.fallback_model.clone()),
            max_turns: cli.max_turns,
            max_budget_usd: cli.max_budget,
            task_budget: None,
            verification_policy: None,
            verbose: cli.verbose,
            initial_messages: resume_messages,
            commands: allthecodes_commands::get_all_commands()
                .iter()
                .map(|c| c.name.clone())
                .collect(),
            thinking_config: None,
            json_schema: None,
            replay_user_messages: false,
            persist_session: true,
            resolved_model: Some(model.model.clone()),
            auto_save_session: true,
            agent_context: None,
        };

        let engine = {
            let mut e = QueryEngine::new_with_services(engine_config, runtime_services.clone());

            if let Some(ref client) = model.detected_client {
                let auto_mode_policy = Arc::new(
                    merged_config
                        .permissions
                        .auto_mode
                        .clone()
                        .unwrap_or_default(),
                );
                let classifier_model = Arc::new(classifier_model::ApiClientClassifierModel {
                    client: client.clone(),
                    model: model.model.clone(),
                });
                let shared_classifier = Arc::new(
                    allthecodes_safety::classifier::SharedSafetyClassifier::new(classifier_model),
                );

                e.set_auto_classifier_fn(Some(Arc::new(
                    move |tool_name: String,
                          tool_input: serde_json::Value,
                          tool_classifier_input: serde_json::Value,
                          messages: Vec<allthecodes_engine::types::message::Message>,
                          cwd: String| {
                        let classifier = shared_classifier.clone();
                        let auto_mode_policy = auto_mode_policy.clone();
                        Box::pin(async move {
                            use allthecodes_safety::classifier::{
                                AutoModeToolClassifierInput, SafetyClassifierRequest,
                            };
                            let request =
                                SafetyClassifierRequest::auto_mode_tool_with_classifier_input(
                                    AutoModeToolClassifierInput {
                                        tool_name,
                                        tool_input,
                                        tool_classifier_input,
                                        transcript: messages,
                                        cwd: std::path::PathBuf::from(cwd),
                                        permission_mode:
                                            allthecodes_types::permissions::PermissionMode::Auto,
                                        sandbox_mode: None,
                                        auto_mode_policy: auto_mode_policy.as_ref().clone(),
                                    },
                                );
                            Some(classifier.classify(&request).await)
                        })
                    },
                )));
            }

            Arc::new(e)
        };
        info!(session = %engine.session_id, "QueryEngine created");
        crate::dashboard::init_session_id(engine.session_id.as_str());

        engine.update_app_state(|s| *s = app_state.clone());

        {
            let start_configs =
                allthecodes_types::hooks::load_hook_configs(&merged_config.hooks, "SessionStart");
            if !start_configs.is_empty() {
                let payload = serde_json::json!({
                    "session_id": engine.session_id.as_str(),
                    "cwd": std::env::current_dir().unwrap_or_default().to_string_lossy(),
                });
                let _ = allthecodes_tools::hooks::run_event_hooks(
                    "SessionStart",
                    &payload,
                    &start_configs,
                )
                .await;
            }
        }

        {
            use allthecodes_observability::{
                AuditConfig, AuditContext, AuditSink, EventKind, Outcome, SessionMeta, Stage,
            };

            let audit_config = AuditConfig::from_env();
            let source_mode = if cli.headless {
                "headless"
            } else if cli.acp {
                "acp"
            } else if cli.daemon {
                "daemon"
            } else {
                "tui"
            };

            let meta = SessionMeta {
                session_id: engine.session_id.as_str().to_string(),
                started_at: chrono::Utc::now(),
                cwd: cwd.clone(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                platform: std::env::consts::OS.to_string(),
                source: source_mode.to_string(),
            };

            let runs_dir = allthecodes_config::paths::runs_dir(engine.session_id.as_str());
            match AuditSink::init(engine.session_id.as_str(), runs_dir, &meta, audit_config) {
                Ok(sink) => {
                    let ctx = AuditContext::new(engine.session_id.as_str(), source_mode, sink);
                    ctx.emit_simple(EventKind::SessionStart, Stage::Session, Outcome::Started);
                    engine.set_audit_context(ctx);
                    debug!("audit sink initialized for session {}", engine.session_id);
                }
                Err(e) => {
                    warn!(error = %e, "failed to initialize audit sink, continuing without audit logging");
                    startup_diagnostics.push(StartupDiagnostic::error(
                        "audit-startup",
                        StartupDiagnosticSource::Other,
                        "Audit logging could not be initialized",
                        Some(e.to_string()),
                        Some("Check the runs directory permissions.".to_string()),
                    ));
                }
            }
        }

        let cwd_path = std::path::PathBuf::from(&cwd);
        let project_root =
            allthecodes_utils::git::find_git_root(&cwd_path).unwrap_or_else(|| cwd_path.clone());
        allthecodes_bootstrap::init_process_state(
            cwd_path,
            project_root,
            engine.session_id.clone(),
            !cli.print,
            Some(model.model.clone()),
        );

        Ok(RuntimeReady {
            cli,
            cwd,
            initial_prompt,
            model: model.model,
            tools,
            app_state,
            merged_config,
            runtime_services,
            engine,
            startup_diagnostics: deduplicate(startup_diagnostics),
        })
    }
}
