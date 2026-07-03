// ============================================================================
// Phase A+B: Process startup, fast paths, and full initialization
//
// Corresponds to: LIFECYCLE_STATE_MACHINE.md Section 2 (Phase A) and Section 3 (Phase B)
//
// Phase A: CLI arg parsing -> fast path detection -> immediate exit
// Phase B: Full initialization -> settings, permissions, tools, AppState -> REPL
// Phase I: Shutdown and cleanup (graceful_shutdown)
//
// Most of the heavy lifting lives in:
//   - `cli`           — argument shape (`Cli` struct + clap derives)
//   - `startup`       — logging, fast paths, runtime config helpers, print/json modes
//   - `full_init`     — Phase B initialization and mode dispatch
// Keep main.rs focused on orchestration: fast-path routing and handoff.
// ============================================================================

// Core modules
mod app_runtime_adapters;
mod app_subsystem_handlers;
mod classifier_model;
mod cli;
mod command_runtime_bridge;
mod full_init;
mod startup;
mod startup_daemon_adapters;
mod startup_model;
mod startup_skills;
mod startup_traits;
mod ui;

// Plugin system
mod plan_workflow;

// Phase I: Shutdown and cleanup
mod shutdown;

mod dashboard;

use std::process::ExitCode;
use std::sync::Arc;

use allthecodes_startup as startup_crate;
use clap::Parser;
use tracing::{error, info};

use crate::cli::Cli;
use crate::full_init::run_full_init;
use crate::startup_daemon_adapters::install_daemon_runtime_adapters;
use crate::startup_traits::{RootAgentToolRegistry, RootDashboardEmitter};
use startup_crate::runtime_config::resolve_cwd;
use startup_crate::tool_registry as registry;

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

fn main() -> ExitCode {
    startup_crate::load_env_files();
    allthecodes_tools::registry::install_tool_registry_providers(
        registry::root_tool_registry_providers(),
    );
    startup_crate::engine_runtime::install(
        Arc::new(RootDashboardEmitter),
        Arc::new(RootAgentToolRegistry),
    );

    // Wire cc-permissions' descriptive-prompt callbacks. cc-permissions moved
    // out of the root crate in Phase 4 (issue #73); the Computer Use and
    // browser prompt strings still live here, so we register look-ups.
    allthecodes_permissions::decision::set_cu_message_callback(|tool_name: &str| {
        let action = allthecodes_computer_use::detection::extract_cu_action(tool_name)?;
        let risk = allthecodes_computer_use::detection::classify_risk(action);
        let risk_tag = match risk {
            allthecodes_computer_use::detection::CuRiskLevel::Medium => "[medium risk]",
            allthecodes_computer_use::detection::CuRiskLevel::High => "[HIGH RISK]",
        };
        let description = match action {
            "screenshot" => "read the screen (take a screenshot)",
            "cursor_position" => "read the current cursor position",
            "left_click" => "click the left mouse button on your screen",
            "right_click" => "click the right mouse button on your screen",
            "middle_click" => "click the middle mouse button on your screen",
            "double_click" => "double-click the mouse on your screen",
            "type_text" | "type" => "type text using the keyboard",
            "key" => "press a keyboard shortcut",
            "scroll" => "scroll the mouse wheel",
            "mouse_move" => "move the mouse cursor",
            _ => {
                return Some(format!(
                    "Allow desktop control action '{}' {}?",
                    action, risk_tag
                ));
            }
        };
        Some(format!("Allow {} {}?", description, risk_tag))
    });
    allthecodes_permissions::decision::set_browser_message_callback(|tool_name: &str| {
        if let Some(m) = allthecodes_browser::permissions::browser_permission_message(tool_name) {
            return Some(m);
        }
        if let Some(rest) = tool_name.strip_prefix("mcp__") {
            if let Some((server, action)) = rest.split_once("__") {
                if allthecodes_browser::detection::is_browser_server(server) {
                    let cat = allthecodes_browser::permissions::classify_browser_action(action);
                    return Some(format!(
                        "Allow browser action '{}' via MCP server '{}' {}?",
                        action,
                        server,
                        cat.risk_tag()
                    ));
                }
            }
        }
        None
    });

    // Phase A: parse args first so fast paths can exit immediately
    let cli = Cli::parse();

    // Fast path: --version
    if cli.version {
        println!("allthecodes {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }

    // Fast path: --chrome-native-host
    // Launched by Chrome via the native-messaging manifest installed by the
    // Chrome subsystem (see src/browser/setup.rs). Skip ALL normal init:
    // no tracing to stderr (Chrome captures stderr as error logs), no
    // REPL, no HTTP server. Just bridge Chrome <-> local socket and exit
    // when Chrome closes stdin.
    if cli.chrome_native_host {
        return startup_crate::fast_paths::run_chrome_native_host();
    }

    // Fast path: --claude-in-chrome-mcp
    // Spawned as a stdio MCP subprocess by the allthecodes MCP manager when
    // --chrome is active. Bridges MCP <-> native-host socket.
    if cli.claude_in_chrome_mcp {
        return startup_crate::fast_paths::run_claude_in_chrome_mcp();
    }

    if let Some(output_dir) = cli.export_ui_snapshots.as_deref() {
        return startup_crate::fast_paths::run_export_ui_snapshots(output_dir, |dir| {
            crate::ui::snapshot_export::export_ui_snapshots(dir)
                .map(|report| startup_crate::fast_paths::SnapshotExportReport {
                    output_dir: report.output_dir,
                    index_path: report.index_path,
                    snapshot_count: report.snapshot_count,
                })
                .map_err(Into::into)
        });
    }

    let tracing_cwd = resolve_cwd(&cli);
    if let Err(error) =
        startup_crate::apply_settings_env_before_tracing(std::path::Path::new(&tracing_cwd))
    {
        eprintln!(
            "warning: failed to apply settings.env before tracing: {:#}",
            error
        );
    }

    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(error) => {
            eprintln!("error: failed to create tokio runtime: {error}");
            return ExitCode::FAILURE;
        }
    };
    let _tracing_guard = {
        let _enter = rt.enter();
        startup_crate::logging::init_tracing(cli.verbose)
    };

    info!("allthecodes v{}", env!("CARGO_PKG_VERSION"));
    command_runtime_bridge::install_command_runtime_providers();
    install_daemon_runtime_adapters();

    if let Some(worker_kind) = cli.daemon_worker.clone() {
        let worker_cwd = std::path::PathBuf::from(resolve_cwd(&cli));
        let worker_id = cli
            .worker_id
            .clone()
            .unwrap_or_else(|| format!("{}-{}", worker_kind, std::process::id()));
        let worker_result = rt.block_on(async {
            allthecodes_daemon::supervisor::run_worker_mode(&worker_kind, &worker_id, worker_cwd)
                .await
        });
        return match worker_result {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                error!("Daemon worker failed: {:#}", err);
                ExitCode::FAILURE
            }
        };
    }

    // Fast path: --dump-system-prompt
    if cli.dump_system_prompt {
        allthecodes_plugins::init_plugins();
        let tools = registry::get_tools_for_active_session();
        return startup_crate::fast_paths::run_dump_system_prompt(&cli, &tools);
    }

    if !cli.print && cli.output_format.is_none() {
        let daemon_cwd = std::path::PathBuf::from(resolve_cwd(&cli));
        if let Some(code) = allthecodes_daemon::process_state::try_run_management_command(
            &cli.prompt,
            &daemon_cwd,
            cli.port,
        ) {
            return code;
        }
    }

    let exit_code = rt.block_on(async {
        match run_full_init(cli).await {
            Ok(code) => code,
            Err(e) => {
                error!("Fatal error: {:#}", e);
                ExitCode::FAILURE
            }
        }
    });
    allthecodes_services::langfuse::shutdown_langfuse();
    exit_code
}
