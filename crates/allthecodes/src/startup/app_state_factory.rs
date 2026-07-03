use allthecodes_config::settings;
use allthecodes_engine::types::app_state::{AppState, SettingsJson};
use tracing::{info, warn};

use crate::startup::mcp_runtime::McpRuntime;
use crate::startup::model_runtime::ModelRuntime;
use crate::startup::settings_runtime::SettingsRuntime;
use crate::startup::startup_context::StartupContext;
use crate::startup_model::{settings_effort_value, settings_thinking_enabled};

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

        let app_state = AppState {
            settings: SettingsJson {
                model: Some(model.model.clone()),
                backend: Some(settings.backend.clone()),
                api_provider: merged_config.api_provider.clone(),
                active_auth_profile: merged_config.active_auth_profile.clone(),
                auth_profiles: merged_config.auth_profiles.clone(),
                theme: merged_config.theme.clone(),
                verbose: Some(cli.verbose),
                extra: merged_config.extra.clone(),
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
                app_icon: merged_config.app_icon.clone(),
                auto_start: merged_config.auto_start,
                start_minimized: merged_config.start_minimized,
                minimize_to_tray: merged_config.minimize_to_tray,
                close_to_tray: merged_config.close_to_tray,
                quick_chat_hide_on_blur: merged_config.quick_chat_hide_on_blur,
                quick_chat_inject_screen: merged_config.quick_chat_inject_screen,
                quick_chat_ambient: merged_config.quick_chat_ambient,
                auto_approve_tools: merged_config.auto_approve_tools,
                analytics_enabled: merged_config.analytics_enabled,
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
                context_window: merged_config.context_window,
                max_messages: merged_config.max_messages,
                auto_title: merged_config.auto_title,
                temperature: merged_config.temperature,
                max_tokens: merged_config.max_tokens,
                streaming: merged_config.streaming,
                show_token_usage: merged_config.show_token_usage,
                show_reasoning_details: merged_config.show_reasoning_details,
                markdown_rendering: merged_config.markdown_rendering,
                single_dollar_math: merged_config.single_dollar_math,
                infographic: merged_config.infographic,
                auto_collapse_reasoning: merged_config.auto_collapse_reasoning,
                quick_reply_suggestions: merged_config.quick_reply_suggestions,
                default_tool_selection: merged_config.default_tool_selection.clone(),
                default_skill_selection: merged_config.default_skill_selection.clone(),
                sound_effects: merged_config.sound_effects,
                auto_compact: merged_config.auto_compact,
                compact_threshold: merged_config.compact_threshold,
                keep_recent_messages: merged_config.keep_recent_messages,
                hashline_mode: merged_config.hashline_mode,
                teammate_mode: merged_config.teammate_mode,
                claude_in_chrome_default_enabled: merged_config.claude_in_chrome_default_enabled,
                auto_memory_enabled: merged_config.auto_memory_enabled,
                memory_auto_retrieve: merged_config.memory_auto_retrieve,
                memory_query_rewriting: merged_config.memory_query_rewriting,
                memory_max_retrieved: merged_config.memory_max_retrieved,
                memory_similarity_threshold: merged_config.memory_similarity_threshold,
                memory_auto_summarize: merged_config.memory_auto_summarize,
                memory_nightly: merged_config.memory_nightly,
                memory_sleep_time: merged_config.memory_sleep_time.clone(),
                memory_temp_ttl: merged_config.memory_temp_ttl,
                memory_archive_retention: merged_config.memory_archive_retention,
                memory_tool_model: merged_config.memory_tool_model.clone(),
                memory_embedding_model: merged_config.memory_embedding_model.clone(),
                proxy_enabled: merged_config.proxy_enabled,
                proxy_url: merged_config.proxy_url.clone(),
                prefer_ipv4: merged_config.prefer_ipv4,
                request_timeout: merged_config.request_timeout,
                retry_attempts: merged_config.retry_attempts,
                custom_user_agent: merged_config.custom_user_agent.clone(),
                speech_enabled: merged_config.speech_enabled,
                speech_active_model: merged_config.speech_active_model.clone(),
                speech_language: merged_config.speech_language.clone(),
                tts_provider: merged_config.tts_provider.clone(),
                tts_api_key: merged_config.tts_api_key.clone(),
                tts_voice: merged_config.tts_voice.clone(),
                tts_voice_custom_id: merged_config.tts_voice_custom_id.clone(),
                tts_model: merged_config.tts_model.clone(),
                search_engine: merged_config.search_engine.clone(),
                web_search_provider: merged_config.web_search_provider.clone(),
                web_search_tavily_api_key: merged_config.web_search_tavily_api_key.clone(),
                web_search_brave_api_key: merged_config.web_search_brave_api_key.clone(),
                cloud_sync_enabled: merged_config.cloud_sync_enabled,
                cloud_sync_path: merged_config.cloud_sync_path.clone(),
                token_savings_tracking: merged_config.token_savings_tracking,
                advisor_model: merged_config.advisor_model.clone(),
                system_prompt: merged_config.system_prompt.clone(),
                sources,
            },
            verbose: cli.verbose,
            main_loop_model: model.model.clone(),
            main_loop_backend: settings.backend.clone(),
            advisor_model: merged_config.advisor_model.clone(),
            tool_permission_context:
                allthecodes_startup::runtime_config::build_tool_permission_context(
                    startup.permission_mode(),
                    loaded_settings,
                ),
            thinking_enabled: settings_thinking_enabled(merged_config),
            fast_mode: merged_config.fast_mode.unwrap_or(false),
            effort_value: settings_effort_value(merged_config),
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

        if startup.mode == crate::startup::startup_context::StartupMode::InitOnly {
            info!("init-only mode: initialization complete");
            return Ok(AppStateRuntime::InitOnly);
        }

        Ok(AppStateRuntime::Ready(app_state))
    }
}
