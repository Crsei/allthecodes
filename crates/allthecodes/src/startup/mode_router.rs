use std::io::{IsTerminal, Read};
use std::process::ExitCode;

use anyhow::Context;
use tracing::{error, warn};

use crate::startup::diagnostics::{
    StartupDiagnostic, StartupDiagnosticSeverity, StartupDiagnosticSource,
};
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
#[allow(clippy::items_after_test_module)]
mod tests {
    use clap::Parser;

    use super::{
        daemon_allowed_by_features, non_interactive_history_failure,
        resolve_non_interactive_prompt, startup_failure_result,
    };
    use crate::cli::Cli;
    use crate::startup::diagnostics::{StartupDiagnostic, StartupDiagnosticSource};

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

    #[test]
    fn positional_prompt_wins_without_reading_stdin() {
        let mut stdin = std::io::Cursor::new("ignored stdin");
        assert_eq!(
            resolve_non_interactive_prompt(&["positional".to_string()], &mut stdin, false).unwrap(),
            "positional"
        );
        assert_eq!(stdin.position(), 0);
    }

    #[test]
    fn non_tty_stdin_is_trimmed_for_print_and_json_modes() {
        let mut stdin = std::io::Cursor::new("  from stdin\n");
        assert_eq!(
            resolve_non_interactive_prompt(&[], &mut stdin, false).unwrap(),
            "from stdin"
        );
    }

    #[test]
    fn tty_and_empty_stdin_fail_immediately() {
        let mut tty = std::io::Cursor::new("ignored");
        assert!(resolve_non_interactive_prompt(&[], &mut tty, true)
            .unwrap_err()
            .contains("non-TTY stdin"));
        let mut empty = std::io::Cursor::new(" \n\t");
        assert!(resolve_non_interactive_prompt(&[], &mut empty, false)
            .unwrap_err()
            .contains("stdin is empty"));
    }

    #[test]
    fn non_interactive_resume_failure_is_terminal() {
        let cli = Cli::parse_from(["allthecodes", "--resume", "-p", "continue"]);
        let diagnostic = StartupDiagnostic::error(
            "history-load",
            StartupDiagnosticSource::History,
            "Failed to load the requested session history",
            Some("corrupt rollout".to_string()),
            None,
        );

        let failure = non_interactive_history_failure(&cli, &[diagnostic])
            .expect("resume failure must stop non-interactive mode");
        let result = startup_failure_result("session-new".to_string(), failure);

        assert!(result.is_error);
        assert_eq!(
            result.subtype,
            allthecodes_types::sdk::ResultSubtype::ErrorDuringExecution
        );
        assert_eq!(result.stop_reason.as_deref(), Some("history_load_error"));
        assert!(result.result.contains("corrupt rollout"));
    }

    #[test]
    fn interactive_resume_keeps_diagnostic_recovery_path() {
        let cli = Cli::parse_from(["allthecodes", "--resume"]);
        let diagnostic = StartupDiagnostic::error(
            "history-resume-missing",
            StartupDiagnosticSource::History,
            "No resumable session was found",
            None,
            None,
        );

        assert!(non_interactive_history_failure(&cli, &[diagnostic]).is_none());
    }

    #[test]
    fn non_history_warning_does_not_block_non_interactive_resume() {
        let cli = Cli::parse_from(["allthecodes", "--resume", "-p", "continue"]);
        let diagnostic = StartupDiagnostic::warning(
            "history-team-context",
            StartupDiagnosticSource::History,
            "Resumed team context could not be restored",
            None,
            None,
        );

        assert!(non_interactive_history_failure(&cli, &[diagnostic]).is_none());
    }
}

async fn run_ready_runtime(runtime: RuntimeReady) -> anyhow::Result<ExitCode> {
    let cli = &runtime.cli;

    if let Some(error) = non_interactive_history_failure(cli, &runtime.startup_diagnostics) {
        let result = startup_failure_result(runtime.engine.current_session_id().to_string(), error);
        return allthecodes_startup::modes::emit_startup_failure(
            cli.output_format.as_deref() == Some("json"),
            result,
        );
    }

    // JSON output mode takes priority (SDK sends both -p and --output-format json).
    if cli.output_format.as_deref() == Some("json") {
        let mut stdin = std::io::stdin();
        let prompt = match resolve_non_interactive_prompt(
            &cli.prompt,
            &mut stdin,
            std::io::stdin().is_terminal(),
        ) {
            Ok(prompt) => prompt,
            Err(message) => {
                eprintln!("error: {message}");
                return Ok(ExitCode::FAILURE);
            }
        };
        return allthecodes_startup::modes::run_json_mode(&runtime.engine, &prompt).await;
    }

    if cli.print {
        let mut stdin = std::io::stdin();
        let prompt = match resolve_non_interactive_prompt(
            &cli.prompt,
            &mut stdin,
            std::io::stdin().is_terminal(),
        ) {
            Ok(prompt) => prompt,
            Err(message) => {
                eprintln!("error: {message}");
                return Ok(ExitCode::FAILURE);
            }
        };
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
        ) && !daemon_allowed_by_features()
        {
            eprintln!("error: --daemon requires FEATURE_KAIROS=1 or FEATURE_PROACTIVE=1");
            return Ok(ExitCode::FAILURE);
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
    let mut startup_diagnostics = runtime.startup_diagnostics.clone();

    let mut dashboard_companion = if subagent_dashboard_companion_enabled() {
        let dashboard = async {
            let config = crate::dashboard::DashboardConfig::try_default()?;
            crate::dashboard::DashboardCompanion::spawn(config).await
        };
        match dashboard.await {
            Ok(child) => Some(child),
            Err(e) => {
                warn!(error = %e, "failed to start subagent dashboard companion");
                startup_diagnostics.push(StartupDiagnostic::warning(
                    "dashboard-companion",
                    StartupDiagnosticSource::Dashboard,
                    "Subagent dashboard companion failed to start",
                    Some(e.to_string()),
                    Some(
                        "Use the TUI without the dashboard or check its configuration.".to_string(),
                    ),
                ));
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
        startup_diagnostics,
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

fn non_interactive_history_failure(
    cli: &crate::cli::Cli,
    diagnostics: &[StartupDiagnostic],
) -> Option<String> {
    let non_interactive = cli.print || cli.output_format.as_deref() == Some("json");
    let history_requested = cli.resume || cli.continue_session.is_some();
    if !non_interactive || !history_requested {
        return None;
    }

    diagnostics
        .iter()
        .find(|diagnostic| {
            diagnostic.source == StartupDiagnosticSource::History
                && diagnostic.severity == StartupDiagnosticSeverity::Error
        })
        .map(StartupDiagnostic::display_text)
}

fn startup_failure_result(
    session_id: String,
    error: String,
) -> allthecodes_types::sdk::SdkResult {
    allthecodes_types::sdk::SdkResult {
        subtype: allthecodes_types::sdk::ResultSubtype::ErrorDuringExecution,
        is_error: true,
        duration_ms: 0,
        duration_api_ms: 0,
        num_turns: 0,
        result: error.clone(),
        stop_reason: Some("history_load_error".to_string()),
        session_id,
        total_cost_usd: 0.0,
        usage: allthecodes_types::sdk::UsageTracking::default(),
        permission_denials: Vec::new(),
        structured_output: None,
        uuid: uuid::Uuid::new_v4(),
        errors: vec![error],
    }
}

fn resolve_non_interactive_prompt<R: Read>(
    positional: &[String],
    stdin: &mut R,
    stdin_is_tty: bool,
) -> Result<String, String> {
    if !positional.is_empty() {
        let prompt = positional.join(" ");
        if prompt.trim().is_empty() {
            return Err("non-interactive positional prompt is empty".to_string());
        }
        return Ok(prompt);
    }
    if stdin_is_tty {
        return Err(
            "non-interactive mode requires a positional prompt or non-TTY stdin".to_string(),
        );
    }
    let mut input = String::new();
    stdin
        .read_to_string(&mut input)
        .map_err(|error| format!("failed to read prompt from stdin: {error}"))?;
    let input = input.trim();
    if input.is_empty() {
        return Err("non-interactive prompt from stdin is empty".to_string());
    }
    Ok(input.to_string())
}

fn subagent_dashboard_companion_enabled() -> bool {
    std::env::var("FEATURE_SUBAGENT_DASHBOARD_COMPANION")
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes"
            )
        })
        .unwrap_or(false)
}
