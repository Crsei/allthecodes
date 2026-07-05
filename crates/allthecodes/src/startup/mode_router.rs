use std::process::ExitCode;

use anyhow::Context;
use tracing::{error, warn};

use crate::startup::runtime_composition::{RuntimeComposition, RuntimeReady};
use crate::startup_skills::persist_skill_usage;

pub(crate) struct ModeRouter {
    runtime: RuntimeComposition,
}

impl ModeRouter {
    pub(crate) fn new(runtime: RuntimeComposition) -> Self {
        Self { runtime }
    }

    pub(crate) async fn run(self) -> anyhow::Result<ExitCode> {
        match self.runtime {
            RuntimeComposition::InitOnly => Ok(ExitCode::SUCCESS),
            RuntimeComposition::Ready(runtime) => run_ready_runtime(*runtime).await,
        }
    }
}

fn daemon_allowed_by_features() -> bool {
    use allthecodes_config::features::{self, Feature};
    features::enabled(Feature::Kairos) || features::enabled(Feature::Proactive)
}

#[cfg(test)]
mod tests {
    use super::daemon_allowed_by_features;

    struct FeatureOverrideGuard;

    impl FeatureOverrideGuard {
        fn set(flags: allthecodes_config::features::FeatureFlags) -> Self {
            allthecodes_config::features::set_runtime_override(flags);
            Self
        }
    }

    impl Drop for FeatureOverrideGuard {
        fn drop(&mut self) {
            allthecodes_config::features::clear_runtime_override();
        }
    }

    #[test]
    #[serial_test::serial]
    fn daemon_mode_accepts_standalone_proactive() {
        let mut flags = allthecodes_config::features::FeatureFlags::all_disabled();
        flags.proactive = true;
        let _guard = FeatureOverrideGuard::set(flags);
        assert!(daemon_allowed_by_features());
    }
}

async fn run_ready_runtime(runtime: RuntimeReady) -> anyhow::Result<ExitCode> {
    let cli = &runtime.cli;

    // JSON output mode takes priority (SDK sends both -p and --output-format json).
    if cli.output_format.as_deref() == Some("json") {
        let prompt = cli.prompt.join(" ");
        if prompt.is_empty() {
            use std::io::Read;
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf)?;
            return allthecodes_startup::modes::run_json_mode(&runtime.engine, buf.trim()).await;
        }
        return allthecodes_startup::modes::run_json_mode(&runtime.engine, &prompt).await;
    }

    if cli.print {
        let prompt = cli.prompt.join(" ");
        if prompt.is_empty() {
            error!("print mode requires a prompt argument");
            return Ok(ExitCode::FAILURE);
        }
        return allthecodes_startup::modes::run_print_mode(&runtime.engine, &prompt).await;
    }

    let listen = cli
        .listen
        .as_deref()
        .map(allthecodes_server::ListenUrl::parse)
        .transpose()
        .with_context(|| {
            format!(
                "failed to parse --listen {}",
                cli.listen.as_deref().unwrap_or_default()
            )
        })?;
    let server_mode = allthecodes_server::ServerMode::from_listen_and_fallback(
        listen,
        cli.web,
        cli.daemon,
        cli.web_port,
        cli.port,
    )?;

    if server_mode.is_active() {
        if matches!(
            server_mode,
            allthecodes_server::ServerMode::Daemon { .. }
                | allthecodes_server::ServerMode::All { .. }
        ) {
            if !daemon_allowed_by_features() {
                eprintln!("error: --daemon requires FEATURE_KAIROS=1 or FEATURE_PROACTIVE=1");
                return Ok(ExitCode::FAILURE);
            }
        }

        let exit_code = crate::full_init::run_server_mode(
            server_mode,
            runtime.engine.clone(),
            cli,
            runtime.initial_prompt.clone(),
        )
        .await?;
        persist_skill_usage();
        return Ok(exit_code);
    }

    if cli.headless {
        let result =
            allthecodes_ipc::headless::run_headless(crate::app_runtime_adapters::headless_config(
                runtime.engine.clone(),
                runtime.model.clone(),
            ))
            .await
            .map(|()| ExitCode::SUCCESS);
        persist_skill_usage();
        return result;
    }

    if cli.acp {
        let result = allthecodes_acp::run_stdio(
            crate::full_init::acp_runtime_bridge::build_acp_runtime_config(
                crate::full_init::acp_runtime_bridge::AcpBridgeInputs {
                    model: runtime.model.clone(),
                    cwd: runtime.cwd.clone(),
                    tools: runtime.tools.clone(),
                    app_state_template: runtime.app_state.clone(),
                    merged_config: runtime.merged_config.clone(),
                    runtime_services: runtime.runtime_services.clone(),
                    cli_overrides: crate::full_init::acp_runtime_bridge::AcpCliOverrides::from_cli(
                        cli,
                    ),
                },
            ),
        )
        .await
        .map(|()| ExitCode::SUCCESS);
        persist_skill_usage();
        return result;
    }

    let shutdown_token = crate::shutdown::register_shutdown_handler();

    let mut dashboard_companion = if allthecodes_config::features::enabled(
        allthecodes_config::features::Feature::SubagentDashboard,
    ) {
        match crate::dashboard::DashboardCompanion::spawn(
            crate::dashboard::DashboardConfig::default(),
        )
        .await
        {
            Ok(child) => Some(child),
            Err(e) => {
                warn!(error = %e, "failed to start subagent dashboard companion");
                None
            }
        }
    } else {
        None
    };

    let tui_result = crate::ui::tui::run_tui(
        runtime.engine.clone(),
        runtime.initial_prompt,
        &runtime.model,
        shutdown_token,
    )
    .await;

    crate::shutdown::graceful_shutdown(&runtime.engine).await;
    if let Some(companion) = dashboard_companion.as_mut() {
        companion.kill();
    }

    match tui_result {
        Ok(()) => Ok(ExitCode::SUCCESS),
        Err(e) => {
            error!("TUI error: {:#}", e);
            Ok(ExitCode::FAILURE)
        }
    }
}
