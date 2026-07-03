use std::sync::Arc;

use allthecodes_config::settings;
use allthecodes_engine::lifecycle::QueryEngine;
use allthecodes_engine::types::app_state::AppState;
use allthecodes_engine::types::tool::Tools;

use crate::cli::Cli;
use crate::startup::app_state_factory::AppStateFactory;
use crate::startup::engine_factory::EngineFactory;
use crate::startup::mcp_runtime::McpRuntimeBuilder;
use crate::startup::model_runtime::ModelRuntimeBuilder;
use crate::startup::plugin_runtime::PluginRuntimeBuilder;
use crate::startup::settings_runtime::SettingsRuntimeBuilder;
use crate::startup::startup_context::StartupContext;
use crate::startup::tool_catalog::ToolCatalogBuilder;

pub(crate) enum RuntimeComposition {
    InitOnly,
    Ready(Box<RuntimeReady>),
}

pub(crate) struct RuntimeReady {
    pub(crate) cli: Cli,
    pub(crate) cwd: String,
    pub(crate) initial_prompt: Option<String>,
    pub(crate) model: String,
    pub(crate) tools: Tools,
    pub(crate) app_state: AppState,
    pub(crate) merged_config: settings::EffectiveSettings,
    pub(crate) engine: Arc<QueryEngine>,
}

impl RuntimeComposition {
    pub(crate) async fn build(startup: StartupContext) -> anyhow::Result<Self> {
        let _startup_decisions = (
            startup.mode,
            startup.chrome_enabled,
            startup.computer_use_enabled,
        );

        SettingsRuntimeBuilder::build(&startup).await?;
        PluginRuntimeBuilder::build(&startup).await?;
        ToolCatalogBuilder::build(&startup).await?;
        McpRuntimeBuilder::build(&startup).await?;
        ModelRuntimeBuilder::build(&startup).await?;
        AppStateFactory::build(&startup).await?;
        EngineFactory::build(&startup).await?;

        crate::full_init::build_runtime_composition(startup).await
    }
}
