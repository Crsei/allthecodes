use allthecodes_config::settings;
use tracing::{debug, info, warn};

use crate::startup::startup_context::StartupContext;

pub(crate) struct SettingsRuntime {
    pub(crate) loaded_settings: settings::LoadedSettings,
    pub(crate) merged_config: settings::EffectiveSettings,
    pub(crate) backend: String,
}

pub(crate) struct SettingsRuntimeBuilder;

impl SettingsRuntimeBuilder {
    pub(crate) async fn build(startup: &mut StartupContext) -> anyhow::Result<SettingsRuntime> {
        let cwd = startup.cwd.to_string_lossy();

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

        let mut loaded_settings = settings::load_effective(std::path::Path::new(cwd.as_ref()))?;
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
        allthecodes_config::features::set_settings_baseline(
            allthecodes_config::features::FeatureFlags::from_settings(&loaded_settings.effective),
        );
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

        startup.resolve_runtime_decisions(
            merged_config.permission_mode.as_deref(),
            merged_config.claude_in_chrome_default_enabled,
        )?;

        let backend =
            allthecodes_engine::codex_exec::normalize_backend(merged_config.backend.as_deref());

        Ok(SettingsRuntime {
            loaded_settings,
            merged_config,
            backend,
        })
    }
}
