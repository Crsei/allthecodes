use std::process::ExitCode;
use std::sync::Arc;

use allthecodes_engine::lifecycle::QueryEngine;
use allthecodes_web as web;
use anyhow::Context;
use axum::Router;
use tracing::{info, warn};

use crate::cli::Cli;
use crate::startup::{ModeRouter, RuntimeComposition, StartupContext};

// ---------------------------------------------------------------------------
// Phase B: Full initialization and REPL
// ---------------------------------------------------------------------------

pub(crate) async fn run_full_init(cli: Cli) -> anyhow::Result<ExitCode> {
    let startup = StartupContext::from_cli(cli).await?;
    let runtime = RuntimeComposition::build(startup).await?;
    ModeRouter::new(runtime).run().await
}

// ---------------------------------------------------------------------------
// Server mode helpers
// ---------------------------------------------------------------------------

/// Start one or both HTTP servers according to [`ServerMode`].
///
/// For Web-only mode this runs the server in the foreground (blocking).
/// For Daemon or All modes it delegates to [`run_daemon_with_server`] which
/// also runs background loops (tick, scheduler, supervisor).
pub(crate) async fn run_server_mode(
    server_mode: allthecodes_server::ServerMode,
    engine: Arc<QueryEngine>,
    cli: &Cli,
    _initial_prompt: Option<String>,
) -> anyhow::Result<ExitCode> {
    use std::sync::atomic::AtomicBool;

    let web_addr = match &server_mode {
        allthecodes_server::ServerMode::Web { addr } => Some(*addr),
        allthecodes_server::ServerMode::All { web_addr, .. } => Some(*web_addr),
        allthecodes_server::ServerMode::None | allthecodes_server::ServerMode::Daemon { .. } => {
            None
        }
    };
    let web_control_token = if web_addr.is_some() {
        Some(
            std::env::var("ALLTHECODES_WEB_CONTROL_TOKEN")
                .ok()
                .filter(|token| !token.is_empty())
                .context("ALLTHECODES_WEB_CONTROL_TOKEN is required for Web and All modes")?,
        )
    } else {
        None
    };
    let web_privileged_token = std::env::var("ALLTHECODES_WEB_PRIVILEGED_TOKEN")
        .ok()
        .filter(|token| !token.is_empty());

    // Validate exposure before constructing any Web state or binding a listener.
    let manager = allthecodes_server::ServerManager::new_with_web_control_secret(
        server_mode.clone(),
        web_control_token.is_some(),
    )?;

    // --- Build web router if the mode requires it ---
    let (web_router, _is_streaming) = if matches!(
        server_mode,
        allthecodes_server::ServerMode::Web { .. } | allthecodes_server::ServerMode::All { .. }
    ) {
        web::handlers::set_command_provider(allthecodes_commands::get_all_commands);
        let is_streaming = Arc::new(AtomicBool::new(false));
        let mut web_state = web::state::WebState::new_with_version(
            engine.clone(),
            is_streaming.clone(),
            env!("CARGO_PKG_VERSION"),
        )
        .with_control_token(
            web_control_token
                .as_deref()
                .context("Web mode requires a control token")?,
        )
        .with_listener_authority(
            web_addr
                .context("Web mode requires a listener address")?
                .to_string(),
        );
        if let Some(token) = web_privileged_token.as_deref() {
            web_state = web_state.with_privileged_token(token);
        }
        (Some(web::build_router(web_state)), Some(is_streaming))
    } else {
        (None, None)
    };

    // --- Build daemon router if the mode requires it ---
    let (daemon_router, daemon_state) = if matches!(
        server_mode,
        allthecodes_server::ServerMode::Daemon { .. } | allthecodes_server::ServerMode::All { .. }
    ) {
        let daemon_addr = match server_mode {
            allthecodes_server::ServerMode::Daemon { addr } => addr,
            allthecodes_server::ServerMode::All { daemon_addr, .. } => daemon_addr,
            _ => unreachable!(),
        };
        let features = Arc::new(daemon_feature_flags());
        let ds = allthecodes_daemon::state::DaemonState::new(
            engine.clone(),
            features,
            daemon_addr.port(),
        );
        let dr = allthecodes_daemon::build_router(ds.clone());
        (Some(dr), Some(ds))
    } else {
        (None, None)
    };

    match server_mode {
        allthecodes_server::ServerMode::Web { addr } => {
            if !cli.no_open {
                info!("Open http://{addr} in your browser");
            }
            let web_router = web_router.context("Web mode requires a web router")?;
            let (web_handle, daemon_handle) = manager.start(web_router, Router::new()).await?;
            wait_for_server_shutdown(&manager, web_handle, daemon_handle, false).await?;
            Ok(ExitCode::SUCCESS)
        }
        allthecodes_server::ServerMode::Daemon { .. }
        | allthecodes_server::ServerMode::All { .. } => {
            let daemon_router = daemon_router.context("Daemon mode requires a daemon router")?;
            let daemon_state = daemon_state.context("Daemon mode requires daemon state")?;
            run_daemon_with_server(
                manager,
                web_router.unwrap_or(Router::new()),
                daemon_router,
                daemon_state,
                engine,
                cli,
            )
            .await
        }
        allthecodes_server::ServerMode::None => {
            unreachable!("run_server_mode called with ServerMode::None");
        }
    }
}

fn daemon_feature_flags() -> allthecodes_config::features::FeatureFlags {
    allthecodes_config::features::current()
}

#[cfg(test)]
mod tests {
    use allthecodes_config::features::{self, FeatureFlags};
    use serial_test::serial;

    struct FeatureGuard;

    impl Drop for FeatureGuard {
        fn drop(&mut self) {
            features::clear_runtime_override();
        }
    }

    #[test]
    #[serial]
    fn daemon_feature_flags_uses_runtime_overrides() {
        let mut flags = FeatureFlags::all_disabled();
        flags.team_memory = true;
        features::set_runtime_override(flags);
        let _guard = FeatureGuard;

        let current = super::daemon_feature_flags();

        assert!(current.team_memory);
    }
}

/// Start the daemon (background loops + HTTP server(s)) and wait for shutdown.
async fn run_daemon_with_server(
    manager: allthecodes_server::ServerManager,
    web_router: Router,
    daemon_router: Router,
    mut daemon_state: allthecodes_daemon::state::DaemonState,
    engine: Arc<QueryEngine>,
    cli: &Cli,
) -> anyhow::Result<ExitCode> {
    use allthecodes_config::features::{self, Feature};

    let cwd = allthecodes_startup::runtime_config::resolve_cwd(cli);

    // --- Set KAIROS engine state ---
    engine.update_app_state(|app| {
        app.kairos_active = true;
        app.is_assistant_mode = true;
        app.autonomous_tick_ms = Some(30_000);
    });

    // --- Write process state ---
    let daemon_port = match manager.mode() {
        allthecodes_server::ServerMode::Daemon { addr } => addr.port(),
        allthecodes_server::ServerMode::All { daemon_addr, .. } => daemon_addr.port(),
        _ => cli.port,
    };
    // --- Spawn team-memory-server if feature is enabled ---
    let mut team_memory_child = if features::enabled(Feature::TeamMemory) {
        match allthecodes_daemon::team_memory_proxy::spawn_team_memory_server(
            daemon_port,
            std::path::Path::new(&cwd),
        )
        .await
        {
            Ok((child, tm_port, tm_secret)) => {
                daemon_state.team_memory_port = Some(tm_port);
                daemon_state.team_memory_secret = Some(tm_secret);
                info!(port = tm_port, "team-memory-server started");
                Some(child)
            }
            Err(e) => {
                warn!(error = %e, "failed to start team-memory-server, feature disabled");
                None
            }
        }
    } else {
        None
    };

    // --- Start background loops ---
    let supervisor_cwd = std::path::PathBuf::from(&cwd);
    let mut notification_handle = start_notification_consumer_if_enabled(&mut daemon_state);

    // --- Start servers ---
    let (web_handle, daemon_handle) = match manager.start(web_router, daemon_router).await {
        Ok(handles) => handles,
        Err(err) => {
            manager.shutdown();
            rollback_daemon_resources(&mut team_memory_child, &mut notification_handle).await;
            let _ = allthecodes_daemon::process_state::write_stopped(
                daemon_port,
                std::path::Path::new(&cwd),
            );
            return Err(err);
        }
    };
    if let Err(err) =
        allthecodes_daemon::process_state::write_started(daemon_port, std::path::Path::new(&cwd))
    {
        manager.shutdown();
        let _ = wait_remaining_servers_with_grace(
            web_handle,
            daemon_handle,
            std::time::Duration::from_secs(5),
        )
        .await;
        rollback_daemon_resources(&mut team_memory_child, &mut notification_handle).await;
        let _ = allthecodes_daemon::process_state::write_stopped(
            daemon_port,
            std::path::Path::new(&cwd),
        );
        return Err(err);
    }
    let supervisor_handle =
        match allthecodes_daemon::supervisor::start_supervisor(supervisor_cwd, daemon_port) {
            Ok(handle) => handle,
            Err(err) => {
                manager.shutdown();
                let _ = wait_remaining_servers_with_grace(
                    web_handle,
                    daemon_handle,
                    std::time::Duration::from_secs(5),
                )
                .await;
                rollback_daemon_resources(&mut team_memory_child, &mut notification_handle).await;
                let _ = allthecodes_daemon::process_state::write_stopped(
                    daemon_port,
                    std::path::Path::new(&cwd),
                );
                return Err(err);
            }
        };
    let server_result = wait_for_server_shutdown(&manager, web_handle, daemon_handle, true).await;

    supervisor_handle.cancel();
    if let Err(err) = supervisor_handle
        .shutdown(std::time::Duration::from_secs(10))
        .await
    {
        tracing::warn!(error = %err, "daemon supervisor required forced cancellation");
    }

    // --- Cleanup ---
    if let Err(err) = allthecodes_daemon::supervisor::terminate_known_workers() {
        tracing::warn!(error = %err, "failed to terminate daemon workers");
    }
    if let Err(err) =
        allthecodes_daemon::process_state::write_stopped(daemon_port, std::path::Path::new(&cwd))
    {
        tracing::warn!(error = %err, "failed to write daemon stopped state");
    }
    rollback_daemon_resources(&mut team_memory_child, &mut notification_handle).await;

    server_result?;
    Ok(ExitCode::SUCCESS)
}

async fn rollback_daemon_resources(
    team_memory_child: &mut Option<tokio::process::Child>,
    notification_handle: &mut Option<tokio::task::JoinHandle<()>>,
) {
    if let Some(handle) = notification_handle.take() {
        handle.abort();
        if tokio::time::timeout(std::time::Duration::from_secs(2), handle)
            .await
            .is_err()
        {
            tracing::warn!("notification consumer exceeded shutdown timeout");
        }
    }
    if let Some(child) = team_memory_child.take() {
        allthecodes_daemon::team_memory_proxy::stop_team_memory_server(child).await;
    }
}

fn start_notification_consumer_if_enabled(
    daemon_state: &mut allthecodes_daemon::state::DaemonState,
) -> Option<tokio::task::JoinHandle<()>> {
    use allthecodes_config::features::{self, Feature};

    if !features::enabled(Feature::KairosPushNotification) {
        return None;
    }
    let rx = daemon_state.notification_rx.lock().take()?;
    let client_state = daemon_state.clone();
    Some(tokio::spawn(async move {
        allthecodes_daemon::notification::notification_consumer(
            rx,
            allthecodes_daemon::notification::NotificationConfig::default(),
            move || client_state.has_clients(),
        )
        .await;
    }))
}

async fn wait_for_server_shutdown(
    manager: &allthecodes_server::ServerManager,
    mut web_handle: Option<allthecodes_server::ServerHandle>,
    mut daemon_handle: Option<allthecodes_server::ServerHandle>,
    watch_daemon_shutdown_request: bool,
) -> anyhow::Result<()> {
    let cancel = manager.shutdown_token();
    let mut first_server_error: Option<anyhow::Error> = None;

    tokio::select! {
        _ = cancel.cancelled() => {
            tracing::info!("server shutdown signal received");
        }
        signal = wait_for_process_shutdown_signal() => {
            tracing::info!(signal, "server shutdown requested");
            manager.shutdown();
        }
        reason = wait_for_daemon_shutdown_request(), if watch_daemon_shutdown_request => {
            tracing::info!(reason, "server shutdown requested");
            manager.shutdown();
        }
        (label, result) = wait_optional_server("web", &mut web_handle) => {
            if let Err(err) = result {
                tracing::error!(error = %err, "{label} server exited unexpectedly");
                first_server_error = Some(err.context(format!("{label} server exited unexpectedly")));
            } else {
                tracing::info!("{label} server exited");
            }
            manager.shutdown();
        }
        (label, result) = wait_optional_server("daemon", &mut daemon_handle) => {
            if let Err(err) = result {
                tracing::error!(error = %err, "{label} server exited unexpectedly");
                first_server_error = Some(err.context(format!("{label} server exited unexpectedly")));
            } else {
                tracing::info!("{label} server exited");
            }
            manager.shutdown();
        }
    }

    if let Err(err) = wait_remaining_servers_with_grace(
        web_handle,
        daemon_handle,
        std::time::Duration::from_secs(30),
    )
    .await
    {
        if first_server_error.is_none() {
            first_server_error = Some(err);
        } else {
            tracing::warn!(error = %err, "additional server shutdown error");
        }
    }

    if let Some(err) = first_server_error {
        Err(err)
    } else {
        Ok(())
    }
}

async fn wait_for_daemon_shutdown_request() -> &'static str {
    loop {
        if allthecodes_daemon::process_state::shutdown_requested() {
            return "daemon-stop";
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}

async fn wait_optional_server(
    label: &'static str,
    handle: &mut Option<allthecodes_server::ServerHandle>,
) -> (&'static str, anyhow::Result<()>) {
    match handle.as_mut() {
        Some(handle) => (label, handle.wait().await),
        None => std::future::pending().await,
    }
}

async fn wait_remaining_servers_with_grace(
    web_handle: Option<allthecodes_server::ServerHandle>,
    daemon_handle: Option<allthecodes_server::ServerHandle>,
    grace_period: std::time::Duration,
) -> anyhow::Result<()> {
    let deadline = std::time::Instant::now() + grace_period;
    let mut first_error: Option<anyhow::Error> = None;

    for (label, handle) in [("web", web_handle), ("daemon", daemon_handle)] {
        let Some(mut handle) = handle else {
            continue;
        };

        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            tracing::warn!("{label} server did not stop before grace period expired; aborting");
            handle.abort();
            continue;
        }

        match tokio::time::timeout(remaining, handle.wait()).await {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                tracing::warn!(error = %err, "{label} server returned an error during shutdown");
                if first_error.is_none() {
                    first_error = Some(err.context(format!("{label} server shutdown failed")));
                }
            }
            Err(_) => {
                tracing::warn!("{label} server did not stop within grace period; aborting");
                handle.abort();
            }
        }
    }

    if let Some(err) = first_error {
        Err(err)
    } else {
        Ok(())
    }
}

async fn wait_for_process_shutdown_signal() -> &'static str {
    #[cfg(unix)]
    {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sigterm) => {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => "ctrl-c",
                    _ = sigterm.recv() => "sigterm",
                }
            }
            Err(err) => {
                tracing::warn!(error = %err, "failed to install SIGTERM handler; falling back to Ctrl-C");
                let _ = tokio::signal::ctrl_c().await;
                "ctrl-c"
            }
        }
    }

    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
        "ctrl-c"
    }
}

// ============================================================================
// ACP runtime bridge — creates allthecodes_acp::AcpRuntimeConfig from root-crate
// initialization artifacts and implements the per-session engine factory.
// ============================================================================

pub(crate) mod acp_runtime_bridge {
    use std::sync::Arc;

    use allthecodes_engine::lifecycle::QueryEngine;
    use allthecodes_engine::runtime_services::RuntimeServices;
    use allthecodes_engine::types::config::QueryEngineConfig;
    use allthecodes_engine::types::tool::Tools;

    use crate::cli::Cli;

    pub(crate) struct AcpBridgeInputs {
        pub(crate) model: String,
        pub(crate) cwd: String,
        pub(crate) tools: Tools,
        pub(crate) app_state_template: allthecodes_engine::types::app_state::AppState,
        pub(crate) merged_config: allthecodes_config::settings::EffectiveSettings,
        pub(crate) runtime_services: Arc<RuntimeServices>,
        pub(crate) cli_overrides: AcpCliOverrides,
    }

    #[derive(Debug, Clone, Default)]
    pub(crate) struct AcpCliOverrides {
        pub(crate) max_turns: Option<usize>,
        pub(crate) model: Option<String>,
        pub(crate) system_prompt: Option<String>,
        pub(crate) append_system_prompt: Option<String>,
        pub(crate) permission_mode: Option<String>,
        pub(crate) verbose: bool,
        pub(crate) no_network: bool,
    }

    impl AcpCliOverrides {
        pub(crate) fn from_cli(cli: &Cli) -> Self {
            Self {
                max_turns: cli.max_turns,
                model: cli.model.clone(),
                system_prompt: cli.system_prompt.clone(),
                append_system_prompt: cli.append_system_prompt.clone(),
                permission_mode: cli.permission_mode.clone(),
                verbose: cli.verbose,
                no_network: cli.no_network,
            }
        }
    }

    pub(crate) fn build_acp_runtime_config(
        inputs: AcpBridgeInputs,
    ) -> allthecodes_acp::AcpRuntimeConfig {
        let factory = AcpEngineFactoryImpl {
            model: inputs.model.clone(),
            tools: inputs.tools.clone(),
            app_state_template: inputs.app_state_template.clone(),
            runtime_services: inputs.runtime_services.clone(),
            cli_overrides: inputs.cli_overrides.clone(),
        };

        allthecodes_acp::AcpRuntimeConfig {
            model: inputs.model,
            cwd: std::path::PathBuf::from(&inputs.cwd),
            tools: inputs.tools,
            app_state_template: inputs.app_state_template,
            merged_config: inputs.merged_config,
            cli_overrides: allthecodes_acp::AcpCliOverrides {
                max_turns: inputs.cli_overrides.max_turns,
                model: inputs.cli_overrides.model,
                system_prompt: inputs.cli_overrides.system_prompt,
                append_system_prompt: inputs.cli_overrides.append_system_prompt,
                permission_mode: inputs.cli_overrides.permission_mode,
                verbose: inputs.cli_overrides.verbose,
                no_network: inputs.cli_overrides.no_network,
            },
            engine_factory: Arc::new(factory),
        }
    }

    struct AcpEngineFactoryImpl {
        model: String,
        tools: Tools,
        app_state_template: allthecodes_engine::types::app_state::AppState,
        runtime_services: Arc<RuntimeServices>,
        cli_overrides: AcpCliOverrides,
    }

    impl allthecodes_acp::AcpEngineFactory for AcpEngineFactoryImpl {
        fn create_engine(
            &self,
            params: allthecodes_acp::AcpEngineParams,
        ) -> anyhow::Result<Arc<QueryEngine>> {
            let config = QueryEngineConfig {
                cwd: params.cwd.to_string_lossy().to_string(),
                tools: self.tools.clone(),
                custom_system_prompt: self.cli_overrides.system_prompt.clone(),
                append_system_prompt: self.cli_overrides.append_system_prompt.clone(),
                user_specified_model: self.cli_overrides.model.clone(),
                fallback_model: None,
                max_turns: self.cli_overrides.max_turns,
                max_budget_usd: None,
                task_budget: None,
                verification_policy: None,
                verbose: self.cli_overrides.verbose,
                initial_messages: params.initial_messages,
                commands: allthecodes_commands::get_all_commands()
                    .iter()
                    .map(|c| c.name.clone())
                    .collect(),
                thinking_config: None,
                json_schema: None,
                replay_user_messages: true,
                persist_session: true,
                resolved_model: Some(self.model.clone()),
                auto_save_session: true,
                agent_context: None,
            };

            let engine = QueryEngine::new_with_services(config, self.runtime_services.clone());

            if let Some(ref session_id) = params.session_id {
                engine.set_current_session_id(
                    allthecodes_engine::bootstrap::SessionId::from_string(session_id),
                );
            }

            let mut app_state = self.app_state_template.clone();
            app_state.main_loop_model = self
                .cli_overrides
                .model
                .clone()
                .unwrap_or_else(|| self.model.clone());
            if let Some(permission_mode) = self.cli_overrides.permission_mode.clone() {
                app_state.tool_permission_context.mode =
                    allthecodes_types::permissions::PermissionMode::parse(&permission_mode);
            }
            engine.update_app_state(|state| *state = app_state);

            for dir in &params.additional_directories {
                if dir.is_absolute() && dir.is_dir() {
                    let canonical = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.clone());
                    engine.update_app_state(|state| {
                        state
                            .tool_permission_context
                            .additional_working_directories
                            .insert(
                                format!("acp-additional-{}", canonical.display()),
                                allthecodes_types::permissions::AdditionalWorkingDirectory {
                                    path: canonical.to_string_lossy().to_string(),
                                    read_only: false,
                                },
                            );
                    });
                }
            }

            Ok(Arc::new(engine))
        }
    }
}
