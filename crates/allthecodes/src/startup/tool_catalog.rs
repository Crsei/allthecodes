use allthecodes_engine::types::tool::Tools;
use tracing::{debug, info, warn};

use crate::startup::plugin_runtime::PluginRuntime;
use crate::startup::settings_runtime::SettingsRuntime;
use crate::startup::startup_context::StartupContext;
use crate::startup_skills::{
    discover_plugin_skills_for_root, log_skill_report, register_user_invocable_skill_commands,
};

pub(crate) struct ToolCatalog {
    pub(crate) tools: Tools,
}

pub(crate) struct ToolCatalogBuilder;

impl ToolCatalogBuilder {
    pub(crate) async fn build(
        startup: &StartupContext,
        _settings: &SettingsRuntime,
        plugins: &PluginRuntime,
    ) -> anyhow::Result<ToolCatalog> {
        debug!(
            plugins = plugins.all_plugins.len(),
            "building startup tool catalog after plugin runtime"
        );
        let tools = allthecodes_startup::tool_registry::get_tools_for_active_session();

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
            Some(startup.cwd.as_path()),
            plugin_skills,
            allthecodes_skills::SkillLoadOptions::for_app_version(env!("CARGO_PKG_VERSION")),
        );
        log_skill_report("startup", &skill_report);
        register_user_invocable_skill_commands();

        {
            use allthecodes_browser::session::ChromeSession;

            let session = ChromeSession::new(startup.chrome_enablement);
            if let Err(e) = session.start() {
                warn!(error = %e, "Chrome subsystem startup failed");
            }
            if allthecodes_browser::state::is_enabled() {
                info!("Claude in Chrome subsystem active - use /chrome for status");
            }
        }

        Ok(ToolCatalog { tools })
    }
}
