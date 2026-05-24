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
// Keep main.rs focused on orchestration: fast-path routing, Phase B
// sequencing, and handing control off to the selected mode (daemon / web /
// headless / TUI / print / json).
// ============================================================================

// Core modules
mod app_runtime_adapters;
mod app_subsystem_handlers;
mod classifier_model;
mod cli;
mod command_runtime_bridge;
mod ui;

// Plugin system
mod plan_workflow;

// LSP service layer

// Phase I: Shutdown and cleanup
mod shutdown;

mod dashboard;

use std::process::ExitCode;
use std::sync::Arc;

use allthecodes_startup as startup;
use allthecodes_web as web;
use anyhow::Context;
use clap::Parser;
use serde_json::Value;
use tracing::{debug, error, info, warn};

use crate::cli::Cli;
use startup::runtime_config::StartupCli;

fn resolve_startup_model(
    requested: Option<&str>,
    provider_default: Option<&str>,
    hardcoded_default: &str,
    available: &[String],
    settings: &settings::EffectiveSettings,
) -> String {
    for candidate in [requested, provider_default, Some(hardcoded_default)] {
        let Some(candidate) = candidate else { continue };
        if allthecodes_commands::model::is_removed_legacy_model_alias(candidate) {
            warn!(
                model = %candidate,
                replacement = ?allthecodes_models::replacement_for_removed_legacy_alias(candidate),
                "legacy model alias ignored during startup"
            );
            continue;
        }
        let model = resolve_model_alias_for_effective_settings(candidate, settings);
        if check_startup_available(&model, available, settings).is_ok() {
            return model;
        }
    }

    if let Some(first_allowed) = available
        .iter()
        .find_map(|entry| resolve_startup_model_list_entry(entry, settings))
    {
        warn!(
            fallback = %first_allowed,
            "no requested/default model satisfied availableModels; falling back to the first allowed entry"
        );
        return first_allowed;
    }

    if !available.is_empty() {
        warn!(
            "availableModels contained no usable model entries; using the hardcoded default model"
        );
    }
    resolve_model_alias_for_effective_settings(hardcoded_default, settings)
}

fn resolve_startup_model_list_entry(
    entry: &str,
    settings: &settings::EffectiveSettings,
) -> Option<String> {
    let trimmed = entry.trim();
    if trimmed.is_empty() || allthecodes_commands::model::is_removed_legacy_model_alias(trimmed) {
        None
    } else {
        Some(resolve_model_alias_for_effective_settings(
            trimmed, settings,
        ))
    }
}

fn resolve_model_alias_for_effective_settings(
    name: &str,
    settings: &settings::EffectiveSettings,
) -> String {
    let trimmed = name.trim();
    match trimmed.to_ascii_uppercase().as_str() {
        "SOTA" => settings
            .sota_model
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| allthecodes_commands::model::resolve_model_alias(trimmed)),
        "MOTA" => settings
            .mota_model
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| allthecodes_commands::model::resolve_model_alias(trimmed)),
        "FOTA" => settings
            .fota_model
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| allthecodes_commands::model::resolve_model_alias(trimmed)),
        _ => allthecodes_commands::model::resolve_model_alias(trimmed),
    }
}

fn settings_thinking_enabled(settings: &settings::EffectiveSettings) -> Option<bool> {
    let thinking = settings.thinking.as_ref()?;
    if let Some(enabled) = thinking.as_bool() {
        return Some(enabled);
    }
    let kind = thinking
        .get("type")
        .and_then(Value::as_str)
        .or_else(|| thinking.as_str())?
        .trim()
        .to_ascii_lowercase();
    match kind.as_str() {
        "enabled" | "adaptive" => Some(true),
        "disabled" => Some(false),
        _ => None,
    }
}

fn output_config_effort(output_config: Option<&Value>) -> Option<String> {
    output_config?
        .get("effort")
        .and_then(allthecodes_engine::effort::normalize_output_effort_json)
}

fn settings_effort_value(settings: &settings::EffectiveSettings) -> Option<String> {
    output_config_effort(settings.output_config.as_ref()).or_else(|| settings.effort_level.clone())
}

fn check_startup_available(
    model: &str,
    available: &[String],
    settings: &settings::EffectiveSettings,
) -> Result<(), String> {
    if available.is_empty() {
        return Ok(());
    }
    if available.iter().any(|entry| {
        resolve_startup_model_list_entry(entry, settings)
            .as_deref()
            .is_some_and(|allowed| allowed == model)
    }) {
        return Ok(());
    }
    Err(format!("Model '{model}' is not in availableModels."))
}

fn log_skill_report(scope: &str, report: &allthecodes_skills::SkillLoadReport) {
    info!(
        scope,
        loaded = report.loaded,
        skipped = report.skipped,
        revision = report.revision,
        warnings = report.warning_count(),
        errors = report.error_count(),
        "skills loaded"
    );

    for diagnostic in &report.diagnostics {
        match diagnostic.severity {
            allthecodes_skills::SkillDiagnosticSeverity::Error => warn!(
                scope,
                code = %diagnostic.code,
                skill = ?diagnostic.skill,
                source = ?diagnostic.source,
                path = ?diagnostic.path,
                message = %diagnostic.message,
                "skill load error"
            ),
            allthecodes_skills::SkillDiagnosticSeverity::Warning => debug!(
                scope,
                code = %diagnostic.code,
                skill = ?diagnostic.skill,
                source = ?diagnostic.source,
                path = ?diagnostic.path,
                message = %diagnostic.message,
                "skill load warning"
            ),
        }
    }
}

impl StartupCli for Cli {
    fn cwd(&self) -> Option<&str> {
        self.cwd.as_deref()
    }

    fn chrome(&self) -> bool {
        self.chrome
    }

    fn no_chrome(&self) -> bool {
        self.no_chrome
    }
}

impl startup::fast_paths::DumpSystemPromptCli for Cli {
    fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }

    fn system_prompt(&self) -> Option<&str> {
        self.system_prompt.as_deref()
    }

    fn append_system_prompt(&self) -> Option<&str> {
        self.append_system_prompt.as_deref()
    }
}

struct RootDashboardEmitter;

impl allthecodes_engine::agent_runtime::DashboardEmitter for RootDashboardEmitter {
    fn emit_subagent_event(
        &self,
        kind: &str,
        agent_id: &str,
        parent_agent_id: Option<&str>,
        description: Option<&str>,
        model: Option<&str>,
        depth: usize,
        background: bool,
        payload: Option<Value>,
    ) -> anyhow::Result<()> {
        crate::dashboard::emit_subagent_event(
            kind,
            agent_id,
            parent_agent_id,
            description,
            model,
            depth,
            background,
            payload,
        )
    }
}

struct RootAgentToolRegistry;

impl allthecodes_engine::agent_runtime::AgentToolRegistry for RootAgentToolRegistry {
    fn get_all_tools(&self) -> Vec<Arc<dyn allthecodes_engine::types::tool::Tool>> {
        registry::get_all_tools()
    }
}
use crate::ui::tui;
use allthecodes_config::settings;
use allthecodes_engine::lifecycle::QueryEngine;
use allthecodes_engine::types::app_state::{AppState, SettingsJson};
use allthecodes_engine::types::config::QueryEngineConfig;
use startup::runtime_config::{
    build_tool_permission_context, chrome_cli_override, resolve_cwd, resolve_permission_mode,
};
use startup::tool_registry as registry;

fn install_daemon_runtime_adapters() {
    allthecodes_daemon::runtime::set_runtime_adapters(
        allthecodes_daemon::runtime::DaemonRuntimeAdapters {
            init_plugins: allthecodes_plugins::init_plugins,
            active_tools: registry::get_tools_for_active_session,
            commands: allthecodes_commands::get_all_commands,
            command_dispatcher: daemon_command_dispatcher,
            command_executor: daemon_command_executor,
            route_github_pr_activity: daemon_route_github_pr_activity,
        },
    );
}

fn discover_plugin_skills_for_root() -> Vec<allthecodes_skills::SkillDefinition> {
    let mut out = Vec::new();

    for contributed in allthecodes_plugins::discover_plugin_skill_definitions() {
        let source = allthecodes_skills::SkillSource::Plugin(contributed.plugin_id.clone());
        let mut skill = match allthecodes_skills::loader::load_skill_from_file_path(
            &contributed.path,
            source,
        ) {
            Some(skill) => skill,
            None => {
                warn!(
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

fn daemon_command_dispatcher() -> Arc<dyn allthecodes_types::commands::CommandDispatcher> {
    Arc::new(allthecodes_commands::DefaultCommandDispatcher::for_full_registry())
}

fn daemon_command_executor() -> Arc<dyn allthecodes_engine::command_runtime::CommandExecutor> {
    Arc::new(allthecodes_commands::EngineCommandExecutor)
}

fn daemon_route_github_pr_activity(
    payload: &serde_json::Value,
    event: Option<&str>,
    delivery_id: Option<&str>,
) -> anyhow::Result<Option<allthecodes_daemon::runtime::GithubPrActivityRouteOutcome>> {
    let Some(activity) =
        allthecodes_teams::pr_activity::parse_github_pr_activity(payload, event, delivery_id)
    else {
        return Ok(None);
    };
    let result = allthecodes_teams::pr_activity::route_github_pr_activity(&activity)?;
    Ok(Some(
        allthecodes_daemon::runtime::GithubPrActivityRouteOutcome {
            matched: result.matched,
            delivered: result.delivered,
        },
    ))
}

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

fn main() -> ExitCode {
    startup::load_env_files();
    allthecodes_tools::registry::install_tool_registry_providers(
        registry::root_tool_registry_providers(),
    );
    startup::engine_runtime::install(
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
        return startup::fast_paths::run_chrome_native_host();
    }

    // Fast path: --claude-in-chrome-mcp
    // Spawned as a stdio MCP subprocess by the allthecodes MCP manager when
    // --chrome is active. Bridges MCP <-> native-host socket.
    if cli.claude_in_chrome_mcp {
        return startup::fast_paths::run_claude_in_chrome_mcp();
    }

    if let Some(output_dir) = cli.export_ui_snapshots.as_deref() {
        return startup::fast_paths::run_export_ui_snapshots(output_dir, |dir| {
            crate::ui::snapshot_export::export_ui_snapshots(dir)
                .map(|report| startup::fast_paths::SnapshotExportReport {
                    output_dir: report.output_dir,
                    index_path: report.index_path,
                    snapshot_count: report.snapshot_count,
                })
                .map_err(Into::into)
        });
    }

    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    let _tracing_guard = {
        let _enter = rt.enter();
        startup::logging::init_tracing(cli.verbose)
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
        return startup::fast_paths::run_dump_system_prompt(&cli, &tools);
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

// ---------------------------------------------------------------------------
// Phase B: Full initialization and REPL
// ---------------------------------------------------------------------------

async fn run_full_init(cli: Cli) -> anyhow::Result<ExitCode> {
    let cwd = resolve_cwd(&cli);

    // If -C / --cwd was given, switch the process working directory so that
    // all tools (Bash, Glob, Grep, etc.) operate in the target workspace.
    if cli.cwd.is_some() {
        let target = std::path::Path::new(&cwd);
        if target.is_dir() {
            std::env::set_current_dir(target)
                .with_context(|| format!("failed to set working directory to {}", cwd))?;
            info!(cwd = %cwd, "working directory changed via --cwd");
        } else {
            anyhow::bail!("--cwd path does not exist or is not a directory: {}", cwd);
        }
    }

    // First-run initialization: if no settings.json exists, seed from template.
    let first_run_initialized = match allthecodes_config::settings::initialize_first_run() {
        Ok(created) => created,
        Err(e) => {
            warn!(error = %e, "first-run initialization failed; continuing with defaults");
            false
        }
    };
    if first_run_initialized {
        info!("first-run initialization complete");
        let store = allthecodes_services::onboarding::OnboardingStore::open_default();
        if let Err(e) = store.update(|_| {}) {
            warn!(error = %e, "failed to initialize onboarding state");
        }
    }

    // B.1: Load layered settings (managed/user/project/local + env).
    let mut loaded_settings = settings::load_effective(std::path::Path::new(&cwd))?;
    let env_report = settings::apply_startup_runtime_env(&loaded_settings.effective.env)?;
    if env_report.applied > 0 || env_report.skipped > 0 || env_report.overridden > 0 {
        debug!(
            applied = env_report.applied,
            skipped = env_report.skipped,
            overridden = env_report.overridden,
            "settings.env runtime environment processed",
        );
    }
    settings::refresh_process_env_overrides(&mut loaded_settings);
    let merged_config = loaded_settings.effective.clone();
    debug!(
        model = ?merged_config.model,
        permission_mode = ?merged_config.permission_mode,
        backend = ?merged_config.backend,
        layers = loaded_settings.loaded_paths.len(),
        "settings loaded",
    );
    if !loaded_settings.loaded_paths.is_empty() {
        for (src, path) in &loaded_settings.loaded_paths {
            debug!(source = src.as_str(), path = %path.display(), "settings layer");
        }
    }
    let backend =
        allthecodes_engine::codex_exec::normalize_backend(merged_config.backend.as_deref());

    // B.2: Determine permission mode
    let permission_mode = resolve_permission_mode(
        cli.permission_mode.as_deref(),
        merged_config.permission_mode.as_deref(),
    )?;
    let chrome_enablement = allthecodes_browser::session::resolve_enablement(
        chrome_cli_override(&cli),
        merged_config.claude_in_chrome_default_enabled,
    );
    let chrome_wanted = matches!(
        chrome_enablement,
        allthecodes_browser::session::ChromeEnablement::Enabled
    );

    // B.3: Initialize plugins, tools, and skills
    allthecodes_plugins::init_plugins();
    let all_plugins = allthecodes_plugins::get_all_plugins();
    if !all_plugins.is_empty() {
        info!(count = all_plugins.len(), "plugins loaded");
    }

    // B.3a-i: Wire plugin LSP declarations into the LSP config provider
    // (Phase 2 integration: Serial Integration Lane)
    allthecodes_lsp_service::set_recommendation_engine(
        allthecodes_lsp_service::recommendation::RecommendationEngine::from_builtin(),
    );
    {
        let enabled_plugins = allthecodes_plugins::get_enabled_plugins();
        let lsp_decls = allthecodes_plugins::lsp::collect_plugin_lsp_declarations(&enabled_plugins);
        if !lsp_decls.is_empty() {
            let provider_configs: Vec<allthecodes_lsp_service::LspServerConfig> = lsp_decls
                .into_iter()
                .map(|decl| allthecodes_lsp_service::LspServerConfig {
                    name: Some(format!("{}:{}", decl.plugin_id, decl.language)),
                    language_id: decl.language,
                    extensions: decl.extensions,
                    extension_to_language: std::collections::HashMap::new(),
                    command: decl.server_command,
                    args: decl.args,
                    env: std::collections::HashMap::new(),
                    workspace_folder: None,
                    init_options: decl.config,
                    source: Some(format!("plugin:{}", decl.plugin_id)),
                })
                .collect();
            if !provider_configs.is_empty() {
                allthecodes_lsp_service::set_config_provider(Some(std::sync::Arc::new(
                    move || provider_configs.clone(),
                )));
            }
        }
    }

    // B.3a-ii: Register plugin commands in Lane C's DynamicRegistry
    // (Phase 2 integration: Serial Integration Lane)
    {
        use allthecodes_commands::dynamic_registry::{CommandSource, DynamicCommandEntry};
        for plugin in &all_plugins {
            if !matches!(plugin.status, allthecodes_plugins::PluginStatus::Installed) {
                continue;
            }
            if let Some(ref cache_path) = plugin.cache_path {
                if let Ok(manifest) = allthecodes_plugins::manifest::load_manifest(cache_path) {
                    for cmd in manifest.commands {
                        allthecodes_commands::DYNAMIC_REGISTRY
                            .lock()
                            .register(DynamicCommandEntry {
                            name: cmd.name,
                            aliases: cmd.aliases,
                            description: cmd.description,
                            source: CommandSource::Plugin,
                            plugin_id: Some(plugin.id.clone()),
                            hidden: false,
                            usage_score: 0.0,
                            execution_strategy:
                                allthecodes_commands::dynamic_registry::ExecutionStrategy::Plugin,
                        });
                    }
                }
            }
        }
    }

    // B.3a-iii: Initialize telemetry subsystem
    // (Phase 2 integration: Serial Integration Lane)
    #[cfg(feature = "telemetry")]
    {
        use allthecodes_engine::telemetry_bridge::{self, EngineTelemetry, SpanId};
        use allthecodes_services::telemetry::{
            init_telemetry, TelemetryConfig, TelemetryExporter, TelemetryHandle, TelemetryRedaction,
        };
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::sync::Mutex;

        let telemetry_config = TelemetryConfig {
            enabled: true,
            exporter: TelemetryExporter::Log,
            sampling_rate: 1.0,
            redaction: TelemetryRedaction::default(),
        };
        let telemetry_handle = init_telemetry(telemetry_config);
        info!("telemetry subsystem initialized");

        // Wrap the handle in an EngineTelemetry bridge so submit_message.rs
        // can start/end InteractionSpan and HookSpan via the trait.
        struct EngineTelemetryBridge {
            handle: TelemetryHandle,
            span_counter: AtomicU64,
            // Live spans keyed by SpanId so finish() can find them.
            active_spans: Mutex<
                std::collections::HashMap<SpanId, allthecodes_services::telemetry::InteractionSpan>,
            >,
            // Live hook spans
            active_hooks:
                Mutex<std::collections::HashMap<SpanId, allthecodes_services::telemetry::HookSpan>>,
        }

        impl EngineTelemetry for EngineTelemetryBridge {
            fn start_submit(&self, session_id: &str, submit_id: &str) -> SpanId {
                let id = self.span_counter.fetch_add(1, Ordering::Relaxed);
                let span = self
                    .handle
                    .start_interaction(session_id.to_string(), submit_id.to_string());
                self.active_spans.lock().unwrap().insert(id, span);
                id
            }

            fn end_submit(
                &self,
                span_id: SpanId,
                model: &str,
                input_tokens: u64,
                output_tokens: u64,
            ) {
                if let Some(mut span) = self.active_spans.lock().unwrap().remove(&span_id) {
                    span.finish(model, input_tokens as u32, output_tokens as u32);
                }
            }

            fn start_hook(&self, hook_name: &str) -> SpanId {
                let id = self.span_counter.fetch_add(1, Ordering::Relaxed);
                let span = allthecodes_services::telemetry::HookSpan::start(
                    hook_name.to_string(),
                    self.handle.clone(),
                );
                self.active_hooks.lock().unwrap().insert(id, span);
                id
            }

            fn end_hook(&self, span_id: SpanId, _result: &str) {
                if let Some(mut span) = self.active_hooks.lock().unwrap().remove(&span_id) {
                    if _result == "error" {
                        span.record_error("hook returned error");
                    } else {
                        span.finish();
                    }
                }
            }
        }

        let bridge = EngineTelemetryBridge {
            handle: telemetry_handle.clone(),
            span_counter: AtomicU64::new(1),
            active_spans: Mutex::new(std::collections::HashMap::new()),
            active_hooks: Mutex::new(std::collections::HashMap::new()),
        };
        telemetry_bridge::install(Box::new(bridge));
        info!("telemetry bridge installed");
    }

    let mut tools = registry::get_tools_for_active_session();

    // B.3c: Initialize skills (bundled/user/project + plugin)
    let skill_usage_path = allthecodes_config::paths::skill_usage_path();
    if let Err(error) = allthecodes_skills::load_skill_usage(&skill_usage_path) {
        warn!(
            error = %error,
            path = %skill_usage_path.display(),
            "failed to load persisted skill usage"
        );
    }

    let plugin_skills = discover_plugin_skills_for_root();
    if !plugin_skills.is_empty() {
        info!(
            count = plugin_skills.len(),
            "Skills: loading plugin-contributed skills"
        );
    }
    let skill_report = allthecodes_skills::reload_skills_with_extra(
        &allthecodes_config::paths::skills_dir_global(),
        Some(std::path::Path::new(&cwd)),
        plugin_skills,
        allthecodes_skills::SkillLoadOptions::for_app_version(env!("CARGO_PKG_VERSION")),
    );
    log_skill_report("startup", &skill_report);
    register_user_invocable_skill_commands();

    // Start Chrome setup before registering the synthetic MCP bridge so the
    // manifest/shims are in place before the bridge begins serving requests.
    {
        use allthecodes_browser::session::ChromeSession;

        let session = ChromeSession::new(chrome_enablement);
        if let Err(e) = session.start() {
            warn!(error = %e, "Chrome subsystem startup failed");
        }
        if allthecodes_browser::state::is_enabled() {
            info!("Claude in Chrome subsystem active - use /chrome for status");
        }
    }

    // B.3d: Discover and connect MCP servers
    let _mcp_manager = {
        use allthecodes_engine::mcp_tool_adapter::mcp_tools_to_tools;
        use allthecodes_mcp::discovery::discover_mcp_servers;
        use allthecodes_mcp::manager::McpManager;

        let cwd_path = std::path::Path::new(&cwd);
        let mut server_configs = match discover_mcp_servers(cwd_path) {
            Ok(configs) => configs,
            Err(err) => {
                warn!(error = %err, "MCP server discovery failed");
                Vec::new()
            }
        };
        let mcp_manager = Arc::new(tokio::sync::Mutex::new(McpManager::new()));

        // First-party Chrome integration: when --chrome is on (or env opts in),
        // register a synthetic `claude-in-chrome` MCP server that points back
        // at this same binary in `--claude-in-chrome-mcp` mode. The MCP
        // manager launches it as a stdio subprocess and talks to it like any
        // other MCP server; the bridge internally forwards over the native
        // host socket.
        if chrome_wanted {
            if let Ok(exe) = std::env::current_exe() {
                // De-dupe: if the user also put `claude-in-chrome` in
                // settings.json for some reason, the explicit config wins.
                let name = allthecodes_browser::common::CLAUDE_IN_CHROME_MCP_SERVER_NAME;
                if !server_configs.iter().any(|c| c.name == name) {
                    server_configs.push(allthecodes_mcp::McpServerConfig {
                        name: name.to_string(),
                        transport: "stdio".to_string(),
                        command: Some(exe.to_string_lossy().into_owned()),
                        args: Some(vec!["--claude-in-chrome-mcp".to_string()]),
                        url: None,
                        headers: None,
                        oauth: None,
                        env: None,
                        browser_mcp: Some(true),
                        disabled: None,
                    });
                    info!(
                        "MCP: registered first-party claude-in-chrome bridge (spawns --claude-in-chrome-mcp subprocess)"
                    );
                }
            }
        }

        // Keep a copy of the configs so we can feed them to browser detection
        // alongside the registered tools; config flags (browserMcp: true) are
        // authoritative even if the server fails to list any recognized
        // browser-shaped tools.
        let configs_for_browser = server_configs.clone();

        if !server_configs.is_empty() {
            info!(
                count = server_configs.len(),
                "MCP: connecting to configured servers"
            );
            let mut mgr = mcp_manager.lock().await;
            if let Err(e) = mgr.connect_all(server_configs).await {
                warn!(error = %e, "MCP: some servers failed to connect");
            }

            // Merge MCP tools with base tools
            let mcp_tool_defs = mgr.all_tools();
            if !mcp_tool_defs.is_empty() {
                let mcp_tools = mcp_tools_to_tools(mcp_tool_defs, mcp_manager.clone());
                info!(
                    count = mcp_tools.len(),
                    "MCP: discovered tools, merging with base tools"
                );
                tools.extend(mcp_tools);
            }

            let (mcp_skills, mcp_skill_diagnostics) =
                allthecodes_engine::mcp_tool_adapter::discover_mcp_skill_resources(&mgr).await;
            if !mcp_skills.is_empty() || !mcp_skill_diagnostics.is_empty() {
                let report = allthecodes_skills::register_skills_resolved_with_diagnostics(
                    mcp_skills,
                    mcp_skill_diagnostics,
                    allthecodes_skills::SkillLoadOptions::for_app_version(env!(
                        "CARGO_PKG_VERSION"
                    )),
                );
                log_skill_report("mcp", &report);
            }
        }

        // Install the browser MCP server registry exactly once after MCP tools
        // are folded into the tool list. Used by system-prompt injection,
        // permission prompts, and `/mcp list` styling.
        let tool_names = tools
            .iter()
            .map(|tool| tool.user_facing_name(None))
            .collect::<Vec<_>>();
        let mut browser_servers =
            allthecodes_browser::detection::detect_browser_servers_from_tool_names(
                configs_for_browser
                    .iter()
                    .map(|config| (config.name.as_str(), config.browser_mcp.unwrap_or(false))),
                tool_names.iter().map(String::as_str),
            );
        // Pre-register the first-party Chrome MCP server name when --chrome
        // (or equivalent) is on. The actual tools come online via #5; doing
        // this early means the system prompt, permissions, and /mcp list all
        // already know the capability is expected.
        if chrome_wanted {
            browser_servers
                .insert(allthecodes_browser::common::CLAUDE_IN_CHROME_MCP_SERVER_NAME.to_string());
        }
        if !browser_servers.is_empty() {
            info!(
                count = browser_servers.len(),
                "Browser MCP: detected browser-shaped MCP server(s)"
            );
        }
        allthecodes_browser::detection::install_browser_servers(browser_servers);

        allthecodes_mcp::runtime::install_manager(mcp_manager.clone());
        mcp_manager
    };

    // B.3e: Register native Computer Use tools (if --computer-use)
    if cli.computer_use {
        let cu_tools = allthecodes_computer_use::setup::register_cu_tools();
        info!(
            count = cu_tools.len(),
            "Computer Use: registered native desktop control tools"
        );
        tools.extend(cu_tools);
    }

    allthecodes_tools::tool_search::install_runtime_tool_catalog(&tools);

    // B.4: Create AppState
    // Resolve model: CLI arg > config > provider default > hardcoded fallback
    let is_codex_backend = allthecodes_engine::codex_exec::is_codex_backend(&backend);
    let detected_client =
        allthecodes_api::api::client::ApiClient::from_backend_result(Some(&backend))
            .context("invalid API provider configuration")?
            .map(Arc::new);
    let provider_default_model = detected_client
        .as_ref()
        .map(|client| client.config().default_model.clone());

    if detected_client.is_none() {
        if is_codex_backend {
            warn!("No OpenAI Codex auth detected. Set OPENAI_CODEX_AUTH_TOKEN.");
            eprintln!(
                "\x1b[33m- No OpenAI Codex auth detected.\x1b[0m\n  \
                 Set:\n  \
                 - OPENAI_CODEX_AUTH_TOKEN (required)\n  \
                 - OPENAI_CODEX_BASE_URL (optional, default: https://chatgpt.com/backend-api)\n  \
                 - OPENAI_CODEX_MODEL (optional, default: gpt-5.5)"
            );
        } else {
            warn!("No API provider detected. Set an API key in .env, environment, or use /login.");
            eprintln!(
                "\x1b[33m- No API provider detected.\x1b[0m\n  \
                 Set an API key via:\n  \
                 - .env file (ANTHROPIC_API_KEY, AZURE_API_KEY, OPENAI_API_KEY, ...)\n  \
                 - Environment variable\n  \
                 - /login command in the REPL"
            );
        }
    }

    let hardcoded_default = if let Some(default_model) = merged_config.default_model.as_deref() {
        default_model.to_string()
    } else if is_codex_backend {
        allthecodes_engine::codex_exec::DEFAULT_CODEX_MODEL.to_string()
    } else {
        allthecodes_models::DEFAULT_MODEL_ALIAS.to_string()
    };
    let requested_model = cli.model.clone().or(merged_config.model.clone());
    let model = resolve_startup_model(
        requested_model.as_deref(),
        provider_default_model.as_deref(),
        &hardcoded_default,
        &merged_config.available_models,
        &merged_config,
    );
    let fallback_model = merged_config
        .fallback_model
        .as_deref()
        .map(|model| resolve_model_alias_for_effective_settings(model, &merged_config))
        .unwrap_or_else(|| {
            let is_anthropic_compatible = detected_client.as_ref().is_some_and(|client| {
                matches!(
                    client.config().provider.endpoint_kind(),
                    Some(
                        allthecodes_api::api::providers::AnthropicEndpointKind::CompatibleAnthropic
                    )
                )
            });
            if is_anthropic_compatible {
                model.clone()
            } else {
                resolve_model_alias_for_effective_settings(
                    allthecodes_models::DEFAULT_FALLBACK_MODEL_ALIAS,
                    &merged_config,
                )
            }
        });
    let persisted_plan_workflow =
        match allthecodes_commands::plan_workflow::load(std::path::Path::new(&cwd)) {
            Ok(record) => record,
            Err(e) => {
                warn!(error = %e, "failed to load persisted plan workflow");
                None
            }
        };

    // Mark CLI overrides (model / verbose) in the source map so /config show
    // reports them correctly.
    let mut sources = loaded_settings.sources.clone();
    if cli.model.is_some() {
        sources.insert("model".into(), settings::SettingsSource::Cli);
    }
    if cli.verbose {
        sources.insert("verbose".into(), settings::SettingsSource::Cli);
    }
    if cli.permission_mode.is_some() {
        sources.insert("permissionMode".into(), settings::SettingsSource::Cli);
    }
    // --no-network is a CLI-level override that forces network.disabled=true
    // on the sandbox section for the remainder of the session.
    let mut effective_sandbox = merged_config.sandbox.clone();
    if cli.no_network {
        effective_sandbox.network.disabled = Some(true);
        sources.insert("sandbox".into(), settings::SettingsSource::Cli);
    }

    let mut app_state = AppState {
        settings: SettingsJson {
            model: Some(model.clone()),
            backend: Some(backend.clone()),
            api_provider: merged_config.api_provider.clone(),
            active_auth_profile: merged_config.active_auth_profile.clone(),
            auth_profiles: merged_config.auth_profiles.clone(),
            theme: merged_config.theme.clone(),
            verbose: Some(cli.verbose),
            permission_mode: merged_config.permission_mode.clone(),
            permissions: merged_config.permissions.clone(),
            sandbox: effective_sandbox,
            status_line: merged_config.status_line.clone(),
            spinner_tips: merged_config.spinner_tips.clone(),
            output_style: merged_config.output_style.clone(),
            language: merged_config.language.clone(),
            voice_enabled: merged_config.voice_enabled,
            editor_mode: merged_config.editor_mode.clone(),
            view_mode: merged_config.view_mode.clone(),
            terminal_progress_bar_enabled: merged_config.terminal_progress_bar_enabled,
            thinking: merged_config.thinking.clone(),
            output_config: merged_config.output_config.clone(),
            default_model: merged_config.default_model.clone(),
            fallback_model: merged_config.fallback_model.clone(),
            fast_model: merged_config.fast_model.clone(),
            sota_model: merged_config.sota_model.clone(),
            mota_model: merged_config.mota_model.clone(),
            fota_model: merged_config.fota_model.clone(),
            available_models: merged_config.available_models.clone(),
            model_capabilities: merged_config.model_capabilities.clone(),
            effort_level: merged_config.effort_level.clone(),
            model_reasoning_effort: merged_config.model_reasoning_effort.clone(),
            fast_mode: merged_config.fast_mode,
            fast_mode_per_session_opt_in: merged_config.fast_mode_per_session_opt_in,
            teammate_mode: merged_config.teammate_mode,
            claude_in_chrome_default_enabled: merged_config.claude_in_chrome_default_enabled,
            auto_memory_enabled: merged_config.auto_memory_enabled,
            advisor_model: merged_config.advisor_model.clone(),
            sources,
        },
        verbose: cli.verbose,
        main_loop_model: model.clone(),
        main_loop_backend: backend.clone(),
        advisor_model: merged_config.advisor_model.clone(),
        tool_permission_context: build_tool_permission_context(
            permission_mode.clone(),
            &loaded_settings,
        ),
        thinking_enabled: settings_thinking_enabled(&merged_config),
        fast_mode: merged_config.fast_mode.unwrap_or(false),
        effort_value: settings_effort_value(&merged_config),
        team_context: None,
        hooks: merged_config.hooks.clone(),
        plan_workflow: persisted_plan_workflow,
        surfaced_memory_keys: std::collections::HashSet::new(),
        kairos_active: false,
        is_brief_only: false,
        is_assistant_mode: false,
        autonomous_tick_ms: None,
        terminal_focus: true,
        keybindings: allthecodes_keybindings::KeybindingRegistry::with_user_path(Some(
            allthecodes_config::paths::keybindings_path(),
        )),
        status_line_runner: crate::ui::status_line::StatusLineRunner::new(),
    };

    // B.5: Init-only fast path
    if cli.init_only {
        info!("init-only mode: initialization complete");
        return Ok(ExitCode::SUCCESS);
    }

    // B.6: Handle session resume (before engine creation)
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
                        (None, None)
                    }
                }
            }
            Ok(None) => {
                warn!("no session to resume");
                (None, None)
            }
            Err(e) => {
                warn!(error = %e, "failed to find session to resume");
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
            }
        }
    }

    // Install root runtime adapters before QueryEngine can spawn agents. This
    // keeps cc-engine free of direct cc-ipc dependencies while preserving the
    // shared IPC agent tree used by headless/TUI status surfaces.
    crate::app_runtime_adapters::ensure_installed();

    // B.7: Build QueryEngineConfig
    let engine_config = QueryEngineConfig {
        cwd: cwd.clone(),
        tools: tools.clone(),
        custom_system_prompt: cli.system_prompt.clone(),
        append_system_prompt: cli.append_system_prompt.clone(),
        user_specified_model: cli.model.clone(),
        fallback_model: Some(fallback_model),
        max_turns: cli.max_turns,
        max_budget_usd: cli.max_budget,
        task_budget: None,
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
        resolved_model: Some(model.clone()),
        auto_save_session: true,
        agent_context: None,
    };

    // B.8: Create QueryEngine
    let engine = {
        let mut e = QueryEngine::new(engine_config);
        e.set_hook_runner(Arc::new(allthecodes_tools::hooks::ShellHookRunner::new()));
        e.set_command_dispatcher(Arc::new(
            allthecodes_commands::DefaultCommandDispatcher::for_full_registry(),
        ));

        // Wire auto-mode classifier if an API client is available.
        if let Some(ref client) = detected_client {
            let auto_mode_policy = Arc::new(
                merged_config
                    .permissions
                    .auto_mode
                    .clone()
                    .unwrap_or_default(),
            );
            let classifier_model = Arc::new(classifier_model::ApiClientClassifierModel {
                client: client.clone(),
                model: model.clone(),
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
                        use allthecodes_safety::classifier::SafetyClassifierRequest;
                        let request = SafetyClassifierRequest::auto_mode_tool_with_classifier_input(
                            tool_name,
                            tool_input,
                            tool_classifier_input,
                            messages,
                            std::path::PathBuf::from(cwd),
                            allthecodes_types::permissions::PermissionMode::Auto,
                            None,
                            auto_mode_policy.as_ref().clone(),
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

    // Apply the fully-resolved AppState (with hooks, permissions, etc.)
    engine.update_app_state(|s| *s = app_state);

    // B.8a: Fire SessionStart hook (fire-and-forget)
    {
        let start_configs =
            allthecodes_types::hooks::load_hook_configs(&merged_config.hooks, "SessionStart");
        if !start_configs.is_empty() {
            let payload = serde_json::json!({
                "session_id": engine.session_id.as_str(),
                "cwd": std::env::current_dir().unwrap_or_default().to_string_lossy(),
            });
            let _ =
                allthecodes_tools::hooks::run_event_hooks("SessionStart", &payload, &start_configs)
                    .await;
        }
    }

    // B.8b: Initialize audit sink
    {
        use allthecodes_observability::{
            AuditConfig, AuditContext, AuditSink, EventKind, Outcome, SessionMeta, Stage,
        };

        let audit_config = AuditConfig::from_env();
        let source_mode = if cli.headless {
            "headless"
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
                // Emit session.start
                ctx.emit_simple(EventKind::SessionStart, Stage::Session, Outcome::Started);
                engine.set_audit_context(ctx);
                debug!("audit sink initialized for session {}", engine.session_id);
            }
            Err(e) => {
                warn!(error = %e, "failed to initialize audit sink, continuing without audit logging");
                // Engine keeps the noop context from construction
            }
        }
    }

    // B.8.1: Initialize global ProcessState
    let cwd_path = std::path::PathBuf::from(&cwd);
    let project_root =
        allthecodes_utils::git::find_git_root(&cwd_path).unwrap_or_else(|| cwd_path.clone());
    allthecodes_bootstrap::init_process_state(
        cwd_path,
        project_root,
        engine.session_id.clone(),
        !cli.print,
        Some(model.clone()),
    );

    // B.9: Non-interactive output modes
    // JSON output mode takes priority (SDK sends both -p and --output-format json)
    if cli.output_format.as_deref() == Some("json") {
        let prompt = cli.prompt.join(" ");
        if prompt.is_empty() {
            // Read prompt from stdin (SDK pipes it)
            use std::io::Read;
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf)?;
            return startup::modes::run_json_mode(&engine, buf.trim()).await;
        }
        return startup::modes::run_json_mode(&engine, &prompt).await;
    }

    // Plain text print mode (-p without --output-format json)
    if cli.print {
        let prompt = cli.prompt.join(" ");
        if prompt.is_empty() {
            error!("print mode requires a prompt argument");
            return Ok(ExitCode::FAILURE);
        }
        return startup::modes::run_print_mode(&engine, &prompt).await;
    }

    // B.10: Web UI mode
    if cli.web {
        web::handlers::set_command_provider(allthecodes_commands::get_all_commands);
        let web_state = web::state::WebState::new(
            engine.clone(),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        return match web::start_server(web_state, cli.web_port, cli.no_open).await {
            Ok(()) => Ok(ExitCode::SUCCESS),
            Err(e) => {
                error!("Web server error: {:#}", e);
                Ok(ExitCode::FAILURE)
            }
        };
    }

    // B.11: Check for inline prompt
    let initial_prompt = if !cli.prompt.is_empty() {
        Some(cli.prompt.join(" "))
    } else {
        None
    };

    // Daemon mode
    if cli.daemon {
        use allthecodes_config::features::{self, Feature};
        if !features::enabled(Feature::Kairos) {
            eprintln!("error: --daemon requires FEATURE_KAIROS=1");
            return Ok(ExitCode::FAILURE);
        }
        allthecodes_daemon::process_state::write_started(cli.port, std::path::Path::new(&cwd))?;

        // Set KAIROS state
        engine.update_app_state(|app| {
            app.kairos_active = true;
            app.is_assistant_mode = true;
            app.autonomous_tick_ms = Some(30_000);
        });

        let mut daemon_state = allthecodes_daemon::state::DaemonState::new(
            engine.clone(),
            Arc::new(features::FLAGS.clone()),
            cli.port,
        );

        // Spawn team-memory-server if feature is enabled.
        let _team_memory_child = if features::enabled(Feature::TeamMemory) {
            match allthecodes_daemon::team_memory_proxy::spawn_team_memory_server(
                cli.port,
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

        let http_state = daemon_state.clone();
        let tick_state = daemon_state.clone();
        let tick_enabled = features::enabled(Feature::Proactive);
        let supervisor_cwd = std::path::PathBuf::from(cwd.clone());

        let daemon_result = tokio::select! {
            result = allthecodes_daemon::server::serve_http(http_state, cli.port) => {
                result.map(|()| ExitCode::SUCCESS)
            }
            _ = allthecodes_daemon::tick::tick_loop(tick_state), if tick_enabled => {
                Ok(ExitCode::SUCCESS)
            }
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("daemon shutting down");
                Ok(ExitCode::SUCCESS)
            }
            result = allthecodes_daemon::supervisor::run_supervisor_loop(supervisor_cwd, cli.port) => {
                result.map(|()| ExitCode::SUCCESS)
            }
        };
        if let Err(err) = allthecodes_daemon::supervisor::terminate_known_workers() {
            warn!(error = %err, "failed to terminate daemon workers");
        }
        if let Err(err) =
            allthecodes_daemon::process_state::write_stopped(cli.port, std::path::Path::new(&cwd))
        {
            warn!(error = %err, "failed to write daemon stopped state");
        }
        persist_skill_usage();
        return daemon_result;
    }

    // B.12: Enter TUI or headless mode
    if cli.headless {
        let result = allthecodes_ipc::headless::run_headless(
            crate::app_runtime_adapters::headless_config(engine, model),
        )
        .await
        .map(|()| ExitCode::SUCCESS);
        persist_skill_usage();
        return result;
    }

    // Register shutdown handler
    let shutdown_token = shutdown::register_shutdown_handler();

    let mut dashboard_companion = if allthecodes_config::features::enabled(
        allthecodes_config::features::Feature::SubagentDashboard,
    ) {
        match dashboard::DashboardCompanion::spawn(dashboard::DashboardConfig::default()).await {
            Ok(child) => Some(child),
            Err(e) => {
                warn!(error = %e, "failed to start subagent dashboard companion");
                None
            }
        }
    } else {
        None
    };

    let tui_result = tui::run_tui(engine.clone(), initial_prompt, &model, shutdown_token).await;

    // Phase I: Shutdown and cleanup
    shutdown::graceful_shutdown(&engine).await;
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

fn register_user_invocable_skill_commands() {
    use allthecodes_commands::dynamic_registry::{
        CommandSource, DynamicCommandEntry, ExecutionStrategy,
    };

    let skills = allthecodes_skills::get_user_invocable_skills();
    let mut registry = allthecodes_commands::DYNAMIC_REGISTRY.lock();
    let stale_skill_names: Vec<String> = registry
        .list_all()
        .into_iter()
        .filter(|entry| entry.source == CommandSource::Skill)
        .map(|entry| entry.name.clone())
        .collect();
    for name in stale_skill_names {
        registry.unregister(&name, CommandSource::Skill);
    }

    for skill in skills {
        registry.register(DynamicCommandEntry {
            name: skill.name.clone(),
            aliases: Vec::new(),
            description: skill.frontmatter.description.clone(),
            source: CommandSource::Skill,
            plugin_id: None,
            hidden: false,
            usage_score: allthecodes_skills::skill_usage_score(&skill.name),
            execution_strategy: ExecutionStrategy::Skill,
        });
    }
}

fn persist_skill_usage() {
    let path = allthecodes_config::paths::skill_usage_path();
    if let Err(error) = allthecodes_skills::save_skill_usage(&path) {
        warn!(
            error = %error,
            path = %path.display(),
            "failed to persist skill usage"
        );
    }
}
