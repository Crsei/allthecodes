use allthecodes_config::settings;
use allthecodes_engine::types::app_state::{AppState, SettingsJson};
use tracing::{info, warn};

use crate::startup::mcp_runtime::McpRuntime;
use crate::startup::model_runtime::ModelRuntime;
use crate::startup::settings_runtime::SettingsRuntime;
use crate::startup::startup_context::StartupContext;
use crate::startup_model::{settings_effort_value, settings_thinking_enabled};

#[allow(clippy::large_enum_variant)]
pub(crate) enum AppStateRuntime {
    InitOnly,
    Ready(AppState),
}

pub(crate) struct AppStateFactory;

impl AppStateFactory {
    pub(crate) async fn build(
        startup: &StartupContext,
        settings: &SettingsRuntime,
        _mcp: &McpRuntime,
        model: &ModelRuntime,
    ) -> anyhow::Result<AppStateRuntime> {
        let cli = &startup.cli;
        let cwd = startup.cwd.to_string_lossy();
        let loaded_settings = &settings.loaded_settings;
        let merged_config = &settings.merged_config;

        let persisted_plan_workflow =
            match allthecodes_commands::plan_workflow::load(std::path::Path::new(cwd.as_ref())) {
                Ok(record) => record,
                Err(e) => {
                    warn!(error = %e, "failed to load persisted plan workflow");
                    None
                }
            };

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
        let mut effective_sandbox = merged_config.sandbox.clone();
        if cli.no_network {
            effective_sandbox.network.disabled = Some(true);
            sources.insert("sandbox".into(), settings::SettingsSource::Cli);
        }

        let mut app_state = AppState::try_from(merged_config)?;
        app_state.settings = SettingsJson::from_effective(merged_config, sources);
        app_state.settings.model = Some(model.model.clone());
        app_state.settings.backend = Some(settings.backend.clone());
        app_state.settings.verbose = Some(cli.verbose);
        app_state.settings.sandbox = effective_sandbox;
        app_state.verbose = cli.verbose;
        app_state.main_loop_model = model.model.clone();
        app_state.main_loop_backend = settings.backend.clone();
        app_state.thinking_enabled = settings_thinking_enabled(merged_config);
        app_state.effort_value = settings_effort_value(merged_config);
        app_state.tool_permission_context =
            allthecodes_startup::runtime_config::build_tool_permission_context(
                startup.permission_mode(),
                loaded_settings,
            );
        app_state.plan_workflow = persisted_plan_workflow;
        app_state.keybindings = allthecodes_keybindings::KeybindingRegistry::with_user_path(Some(
            allthecodes_config::paths::keybindings_path(),
        ));
        app_state.status_line_runner = crate::ui::status_line::StatusLineRunner::new();

        if startup.mode == crate::startup::startup_context::StartupMode::InitOnly {
            info!("init-only mode: initialization complete");
            return Ok(AppStateRuntime::InitOnly);
        }

        Ok(AppStateRuntime::Ready(app_state))
    }
}
