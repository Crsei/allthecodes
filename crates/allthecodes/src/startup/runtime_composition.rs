use std::sync::Arc;

use allthecodes_config::settings;
use allthecodes_engine::lifecycle::QueryEngine;
use allthecodes_engine::runtime_services::RuntimeServices;
use allthecodes_engine::types::app_state::AppState;
use allthecodes_engine::types::tool::Tools;

use crate::cli::Cli;
use crate::startup::app_state_factory::{AppStateFactory, AppStateRuntime};
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
    pub(crate) runtime_services: Arc<RuntimeServices>,
    pub(crate) engine: Arc<QueryEngine>,
}

impl RuntimeComposition {
    pub(crate) async fn build(mut startup: StartupContext) -> anyhow::Result<Self> {
        let settings = SettingsRuntimeBuilder::build(&mut startup).await?;
        let plugins = PluginRuntimeBuilder::build(&startup, &settings).await?;
        let tool_catalog = ToolCatalogBuilder::build(&startup, &settings, &plugins).await?;
        let mcp = McpRuntimeBuilder::build(&startup, &settings, tool_catalog).await?;
        let model = ModelRuntimeBuilder::build(&startup, &settings).await?;
        let app_state = match AppStateFactory::build(&startup, &settings, &mcp, &model).await? {
            AppStateRuntime::InitOnly => return Ok(Self::InitOnly),
            AppStateRuntime::Ready(app_state) => app_state,
        };
        let runtime_services = EngineFactory::runtime_services(mcp.tools.clone());
        let ready =
            EngineFactory::build(startup, settings, mcp, model, app_state, runtime_services)
                .await?;

        Ok(Self::Ready(Box::new(ready)))
    }
}
