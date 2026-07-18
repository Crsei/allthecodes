use std::sync::Arc;

use allthecodes_config::settings;
use allthecodes_engine::lifecycle::QueryEngine;
use allthecodes_engine::runtime_services::RuntimeServices;
use allthecodes_engine::types::app_state::AppState;
use allthecodes_engine::types::tool::Tools;

use crate::cli::Cli;
use crate::startup::app_state_factory::{AppStateFactory, AppStateRuntime};
use crate::startup::diagnostics::StartupDiagnostic;
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
    pub(crate) startup_diagnostics: Vec<StartupDiagnostic>,
}

impl RuntimeComposition {
    pub(crate) async fn build(mut startup: StartupContext) -> anyhow::Result<Self> {
        let settings = SettingsRuntimeBuilder::build(&mut startup).await?;
        let plugins = PluginRuntimeBuilder::build(&startup, &settings).await?;
        activate_startup_proactive_if_requested(&startup.cli);
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

pub(crate) fn activate_startup_proactive_if_requested(cli: &crate::cli::Cli) {
    if !startup_proactive_requested(cli) {
        return;
    }

    let mut flags = allthecodes_config::features::current();
    flags.proactive = true;
    allthecodes_config::features::set_runtime_override(flags);
    let controller = allthecodes_services::proactive::global_controller();
    controller.activate("startup");
    if explicit_startup_proactive_requested(cli) {
        let snapshot = controller.snapshot();
        if let Err(error) =
            allthecodes_services::proactive::write_durable_state(true, snapshot.next_tick_at)
        {
            tracing::warn!(%error, "failed to persist startup proactive state");
        }
    }
}

fn startup_proactive_requested(cli: &crate::cli::Cli) -> bool {
    explicit_startup_proactive_requested(cli)
        || allthecodes_config::features::enabled(allthecodes_config::features::Feature::Proactive)
}

fn explicit_startup_proactive_requested(cli: &crate::cli::Cli) -> bool {
    cli.proactive || proactive_env_requested()
}

fn proactive_env_requested() -> bool {
    std::env::var("ALLTHECODES_PROACTIVE")
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes"
            )
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use allthecodes_config::features::{self, Feature, FeatureFlags};
    use allthecodes_services::proactive::ProactiveStatus;
    use clap::Parser;

    use crate::cli::Cli;

    use super::activate_startup_proactive_if_requested;

    struct FeatureOverrideGuard;

    impl FeatureOverrideGuard {
        fn set(flags: FeatureFlags) -> Self {
            features::set_runtime_override(flags);
            Self
        }
    }

    impl Drop for FeatureOverrideGuard {
        fn drop(&mut self) {
            features::clear_runtime_override();
        }
    }

    struct ProactiveControllerGuard;

    impl ProactiveControllerGuard {
        fn inactive() -> Self {
            allthecodes_services::proactive::global_controller().deactivate("test_setup");
            Self
        }
    }

    impl Drop for ProactiveControllerGuard {
        fn drop(&mut self) {
            allthecodes_services::proactive::global_controller().deactivate("test_cleanup");
        }
    }

    struct EnvGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, previous }
        }

        fn set_path(key: &'static str, value: &std::path::Path) -> Self {
            let previous = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(previous) = self.previous.as_ref() {
                std::env::set_var(self.key, previous);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    #[test]
    #[serial_test::serial]
    fn startup_proactive_cli_sets_runtime_feature_override() {
        let _features = FeatureOverrideGuard::set(FeatureFlags::all_disabled());
        let _controller = ProactiveControllerGuard::inactive();
        let cli = Cli::parse_from(["claude", "--proactive"]);

        activate_startup_proactive_if_requested(&cli);

        assert!(features::enabled(Feature::Proactive));
        let snapshot = allthecodes_services::proactive::global_controller().snapshot();
        assert_eq!(snapshot.status, ProactiveStatus::Active);
        assert_eq!(snapshot.source.as_deref(), Some("startup"));
    }

    #[test]
    #[serial_test::serial]
    fn startup_proactive_cli_persists_worker_visible_tick_state() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set_path("ALLTHECODES_HOME", home.path());
        let _features = FeatureOverrideGuard::set(FeatureFlags::all_disabled());
        let controller = allthecodes_services::proactive::global_controller();
        controller.deactivate("test_setup");
        let cli = Cli::parse_from(["claude", "--proactive"]);

        activate_startup_proactive_if_requested(&cli);

        features::clear_runtime_override();
        controller.deactivate("simulate_worker_process");
        let command =
            allthecodes_daemon::tick::enqueue_proactive_tick_once(chrono::Local::now(), false)
                .unwrap();
        assert!(command.is_some());

        controller.deactivate("test_cleanup");
    }

    #[test]
    #[serial_test::serial]
    fn startup_proactive_env_sets_runtime_feature_override() {
        let _guard = EnvGuard::set("ALLTHECODES_PROACTIVE", "yes");
        let _features = FeatureOverrideGuard::set(FeatureFlags::all_disabled());
        let _controller = ProactiveControllerGuard::inactive();
        let cli = Cli::parse_from(["claude"]);

        activate_startup_proactive_if_requested(&cli);

        assert!(features::enabled(Feature::Proactive));
        let snapshot = allthecodes_services::proactive::global_controller().snapshot();
        assert_eq!(snapshot.status, ProactiveStatus::Active);
        assert_eq!(snapshot.source.as_deref(), Some("startup"));
    }

    #[test]
    #[serial_test::serial]
    fn startup_proactive_env_persists_worker_visible_tick_state() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set_path("ALLTHECODES_HOME", home.path());
        let _env = EnvGuard::set("ALLTHECODES_PROACTIVE", "1");
        let _features = FeatureOverrideGuard::set(FeatureFlags::all_disabled());
        let controller = allthecodes_services::proactive::global_controller();
        controller.deactivate("test_setup");
        let cli = Cli::parse_from(["claude"]);

        activate_startup_proactive_if_requested(&cli);

        features::clear_runtime_override();
        controller.deactivate("simulate_worker_process");
        let command =
            allthecodes_daemon::tick::enqueue_proactive_tick_once(chrono::Local::now(), false)
                .unwrap();
        assert!(command.is_some());

        controller.deactivate("test_cleanup");
    }

    #[test]
    #[serial_test::serial]
    fn startup_proactive_feature_gate_activates_controller() {
        let _features = FeatureOverrideGuard::set(FeatureFlags::from_env_iter([(
            "FEATURE_PROACTIVE".to_string(),
            "1".to_string(),
        )]));
        let _controller = ProactiveControllerGuard::inactive();
        let cli = Cli::parse_from(["claude"]);

        activate_startup_proactive_if_requested(&cli);

        assert!(features::enabled(Feature::Proactive));
        let snapshot = allthecodes_services::proactive::global_controller().snapshot();
        assert_eq!(snapshot.status, ProactiveStatus::Active);
        assert_eq!(snapshot.source.as_deref(), Some("startup"));
    }

    #[test]
    #[serial_test::serial]
    fn startup_kairos_implied_proactive_activates_controller() {
        let _features = FeatureOverrideGuard::set(FeatureFlags::from_env_iter([(
            "FEATURE_KAIROS".to_string(),
            "1".to_string(),
        )]));
        let _controller = ProactiveControllerGuard::inactive();
        let cli = Cli::parse_from(["claude"]);

        activate_startup_proactive_if_requested(&cli);

        assert!(features::enabled(Feature::Kairos));
        assert!(features::enabled(Feature::Proactive));
        let snapshot = allthecodes_services::proactive::global_controller().snapshot();
        assert_eq!(snapshot.status, ProactiveStatus::Active);
        assert_eq!(snapshot.source.as_deref(), Some("startup"));
    }
}
