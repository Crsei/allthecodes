//! `/effort` command -- set the thinking effort level.
//!
//! Controls the reasoning depth for the model by mapping a label or numeric
//! override to the `thinking.budget_tokens` value sent on the next request.

use allthecodes_config::settings::{self, RawSettings};
use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandHandler, CommandResult};
use allthecodes_engine::effort::{
    effort_to_budget_tokens, normalize_effort_value, normalize_output_effort_value,
    DEFAULT_THINKING_BUDGET, MAX_THINKING_BUDGET,
};

pub struct EffortHandler;

fn budget_summary(value: Option<&str>) -> String {
    match value {
        None => format!(
            "(not set - thinking falls back to {} tokens when enabled)",
            DEFAULT_THINKING_BUDGET
        ),
        Some("auto") => format!(
            "auto (model default; {} thinking tokens in the fixed-budget path)",
            DEFAULT_THINKING_BUDGET
        ),
        Some("max") => format!(
            "max (highest fixed budget in this fork: {} thinking tokens)",
            MAX_THINKING_BUDGET
        ),
        Some("xhigh") => "xhigh (extra high reasoning)".to_string(),
        Some("minimal") => "minimal (minimal reasoning)".to_string(),
        Some("none") => "none (reasoning disabled)".to_string(),
        Some(s) => match effort_to_budget_tokens(s) {
            Some(tokens) => format!("{} ({} thinking tokens)", s, tokens),
            None => format!("{} (unrecognized - will use default budget)", s),
        },
    }
}

#[async_trait]
impl CommandHandler for EffortHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let arg = args.trim().to_string();
        let Some(capability) = current_model_capability(ctx) else {
            return Ok(CommandResult::Output(
                "Current profile has no configured reasoning levels for this model. Use /login and /model to select a configured profile model.".to_string(),
            ));
        };
        let supported = capability.supported_reasoning_levels.clone();
        if supported.is_empty() {
            return Ok(CommandResult::Output(
                "Current profile has no configured reasoning levels for this model.".to_string(),
            ));
        }
        let default_reasoning = capability
            .default_reasoning_level
            .as_deref()
            .filter(|level| supported.iter().any(|supported| supported == level))
            .unwrap_or_else(|| supported[0].as_str());

        if arg.is_empty() {
            return Ok(CommandResult::Output(format!(
                "Current effort: {}\n\n\
                 Model: {}\n\
                 Usage: /effort <auto|{}>\n\
                 auto uses the model default: {}.",
                budget_summary(
                    ctx.app_state.effort_value.as_deref().or(ctx
                        .app_state
                        .settings
                        .model_reasoning_effort
                        .as_deref())
                ),
                ctx.app_state.main_loop_model,
                supported.join("|"),
                default_reasoning,
            )));
        }

        let stored = if arg.eq_ignore_ascii_case("auto") {
            default_reasoning.to_string()
        } else {
            match normalize_effort_value(&arg) {
                Some(value) if supported.iter().any(|supported| supported == &value) => value,
                _ => {
                    return Ok(CommandResult::Output(format!(
                        "Invalid effort for {}: '{}'\nSupported levels: auto, {}",
                        ctx.app_state.main_loop_model,
                        arg,
                        supported.join(", ")
                    )));
                }
            }
        };

        if !supported.iter().any(|level| level == &stored) {
            return Ok(CommandResult::Output(format!(
                "Invalid effort for {}: '{}'\nSupported levels: auto, {}",
                ctx.app_state.main_loop_model,
                arg,
                supported.join(", ")
            )));
        }

        ctx.app_state.effort_value = Some(stored.clone());
        ctx.app_state.settings.model_reasoning_effort = Some(stored.clone());
        set_output_config_effort(&mut ctx.app_state.settings, &stored);
        if let Some(active) = ctx.app_state.settings.active_auth_profile.clone() {
            if let Some(profile) = ctx.app_state.settings.auth_profiles.get_mut(&active) {
                profile.model_reasoning_effort = Some(stored.clone());
            }
        }
        ctx.app_state.settings.sources.insert(
            "model_reasoning_effort".to_string(),
            settings::SettingsSource::User,
        );
        ctx.app_state
            .settings
            .sources
            .insert("output_config".to_string(), settings::SettingsSource::User);

        let persist_msg = persist_user_profile_reasoning_effort(&ctx.app_state.settings, &stored);
        Ok(CommandResult::Output(format!(
            "Effort set to: {}\n{}",
            budget_summary(Some(&stored)),
            persist_msg
        )))
    }
}

fn set_output_config_effort(
    runtime_settings: &mut allthecodes_config::runtime_settings::SettingsJson,
    value: &str,
) {
    let Some(output_effort) = normalize_output_effort_value(value) else {
        return;
    };
    let mut output_config = runtime_settings
        .output_config
        .take()
        .and_then(|value| match value {
            serde_json::Value::Object(map) => Some(map),
            _ => None,
        })
        .unwrap_or_default();
    output_config.insert(
        "effort".to_string(),
        serde_json::Value::String(output_effort),
    );
    runtime_settings.output_config = Some(serde_json::Value::Object(output_config));
}

fn current_model_capability(
    ctx: &CommandContext,
) -> Option<allthecodes_config::settings::ModelCapabilitySettings> {
    ctx.app_state
        .settings
        .model_capabilities
        .get(&ctx.app_state.main_loop_model)
        .cloned()
        .or_else(|| {
            let active = ctx.app_state.settings.active_auth_profile.as_deref()?;
            ctx.app_state
                .settings
                .auth_profiles
                .get(active)?
                .model_capabilities
                .as_ref()?
                .get(&ctx.app_state.main_loop_model)
                .cloned()
        })
}

fn persist_user_profile_reasoning_effort(
    runtime_settings: &allthecodes_config::runtime_settings::SettingsJson,
    value: &str,
) -> String {
    let Some(active) = runtime_settings
        .active_auth_profile
        .as_deref()
        .map(str::trim)
        .filter(|profile| !profile.is_empty())
    else {
        return "Effort updated for this session; no active auth profile to persist.".to_string();
    };

    let path = settings::user_settings_path();
    let mut raw = if path.exists() {
        match std::fs::read_to_string(&path)
            .map_err(anyhow::Error::from)
            .and_then(|txt| serde_json::from_str::<RawSettings>(&txt).map_err(anyhow::Error::from))
        {
            Ok(raw) => raw,
            Err(error) => {
                return format!(
                    "Effort updated for this session, but user settings were not updated: {}",
                    error
                );
            }
        }
    } else {
        RawSettings::default()
    };

    let mut profile = raw
        .auth_profiles
        .as_ref()
        .and_then(|profiles| profiles.get(active))
        .cloned()
        .or_else(|| runtime_settings.auth_profiles.get(active).cloned())
        .unwrap_or_default();
    profile.model_reasoning_effort = Some(value.to_string());
    if profile.model_capabilities.is_none() && !runtime_settings.model_capabilities.is_empty() {
        profile.model_capabilities = Some(runtime_settings.model_capabilities.clone());
    }
    if profile.available_models.is_none() && !runtime_settings.available_models.is_empty() {
        profile.available_models = Some(runtime_settings.available_models.clone());
    }
    settings::upsert_auth_profile(&mut raw, active, profile, true);
    let output_effort = set_raw_output_config_effort(&mut raw, value);

    match settings::write_user_settings(&raw) {
        Ok(path) => format!(
            "-> persisted output_config.effort={} and authProfiles.{active}.modelReasoningEffort={} to {}",
            output_effort.unwrap_or_else(|| "unset".to_string()),
            value,
            path.display()
        ),
        Err(error) => format!(
            "Effort updated for this session, but user settings were not updated: {}",
            error
        ),
    }
}

fn set_raw_output_config_effort(raw: &mut RawSettings, value: &str) -> Option<String> {
    let output_effort = normalize_output_effort_value(value)?;
    let mut output_config = raw
        .output_config
        .take()
        .and_then(|value| match value {
            serde_json::Value::Object(map) => Some(map),
            _ => None,
        })
        .unwrap_or_default();
    output_config.insert(
        "effort".to_string(),
        serde_json::Value::String(output_effort.clone()),
    );
    raw.output_config = Some(serde_json::Value::Object(output_config));
    Some(output_effort)
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
    use allthecodes_engine::types::app_state::AppState;
    use std::path::PathBuf;

    struct HomeGuard {
        previous: Option<String>,
    }

    impl HomeGuard {
        fn temp() -> (tempfile::TempDir, Self) {
            let dir = tempfile::tempdir().unwrap();
            let previous = std::env::var("ALLTHECODES_HOME").ok();
            std::env::set_var("ALLTHECODES_HOME", dir.path());
            (dir, Self { previous })
        }
    }

    impl Drop for HomeGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var("ALLTHECODES_HOME", value),
                None => std::env::remove_var("ALLTHECODES_HOME"),
            }
        }
    }

    fn test_ctx() -> CommandContext {
        CommandContext {
            messages: Vec::new(),
            cwd: PathBuf::from("/test"),
            app_state: AppState::default(),
            session_id: SessionId::from_string("test-session"),
        }
    }

    fn add_codex_profile(ctx: &mut CommandContext) {
        let mut profile = allthecodes_config::settings::ProviderProfileSettings {
            backend: Some("codex".to_string()),
            api_provider: Some("openai-codex".to_string()),
            model: Some("gpt-5.5".to_string()),
            available_models: Some(allthecodes_config::settings::codex_model_ids()),
            model_capabilities: Some(allthecodes_config::settings::codex_model_capabilities()),
            ..Default::default()
        };
        profile.model_reasoning_effort = None;
        ctx.app_state.main_loop_model = "gpt-5.5".to_string();
        ctx.app_state.settings.active_auth_profile = Some("codex".to_string());
        ctx.app_state
            .settings
            .auth_profiles
            .insert("codex".to_string(), profile);
        ctx.app_state.settings.available_models = allthecodes_config::settings::codex_model_ids();
        ctx.app_state.settings.model_capabilities =
            allthecodes_config::settings::codex_model_capabilities();
    }

    #[tokio::test]
    async fn test_effort_no_args_shows_current() {
        let handler = EffortHandler;
        let mut ctx = test_ctx();
        add_codex_profile(&mut ctx);
        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Current effort"));
                assert!(text.contains("not set"));
                assert!(text.contains("auto"));
                assert!(text.contains("xhigh"));
            }
            _ => panic!("Expected Output"),
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_effort_set_numeric_override() {
        let (_dir, _guard) = HomeGuard::temp();
        let handler = EffortHandler;
        let mut ctx = test_ctx();
        add_codex_profile(&mut ctx);
        let result = handler.execute("12000", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => assert!(text.contains("Invalid effort")),
            _ => panic!("Expected Output"),
        }
        assert!(ctx.app_state.effort_value.is_none());
    }

    #[tokio::test]
    async fn test_effort_show_includes_resolved_budget() {
        let handler = EffortHandler;
        let mut ctx = test_ctx();
        add_codex_profile(&mut ctx);
        ctx.app_state.effort_value = Some("high".into());
        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("high"));
                assert!(text.contains("24576"), "expected high -> 24576: {}", text);
            }
            _ => panic!("Expected Output"),
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_effort_set_valid_level() {
        let (dir, _guard) = HomeGuard::temp();
        let handler = EffortHandler;
        let mut ctx = test_ctx();
        add_codex_profile(&mut ctx);
        let result = handler.execute("high", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => assert!(text.contains("high")),
            _ => panic!("Expected Output"),
        }
        assert_eq!(ctx.app_state.effort_value.as_deref(), Some("high"));
        let settings: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            settings["authProfiles"]["codex"]["modelReasoningEffort"],
            "high"
        );
        assert_eq!(settings["output_config"]["effort"], "high");
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_effort_persists_output_config_compat_value() {
        let (dir, _guard) = HomeGuard::temp();
        let handler = EffortHandler;
        let mut ctx = test_ctx();
        add_codex_profile(&mut ctx);
        let result = handler.execute("low", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => assert!(text.contains("output_config.effort=high")),
            _ => panic!("Expected Output"),
        }
        assert_eq!(ctx.app_state.effort_value.as_deref(), Some("low"));
        assert_eq!(
            ctx.app_state
                .settings
                .output_config
                .as_ref()
                .and_then(|value| value.get("effort"))
                .and_then(serde_json::Value::as_str),
            Some("high")
        );
        let settings: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            settings["authProfiles"]["codex"]["modelReasoningEffort"],
            "low"
        );
        assert_eq!(settings["output_config"]["effort"], "high");

        let result = handler.execute("xhigh", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => assert!(text.contains("output_config.effort=max")),
            _ => panic!("Expected Output"),
        }
        assert_eq!(ctx.app_state.effort_value.as_deref(), Some("xhigh"));
        assert_eq!(
            ctx.app_state
                .settings
                .output_config
                .as_ref()
                .and_then(|value| value.get("effort"))
                .and_then(serde_json::Value::as_str),
            Some("max")
        );
        let settings: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            settings["authProfiles"]["codex"]["modelReasoningEffort"],
            "xhigh"
        );
        assert_eq!(settings["output_config"]["effort"], "max");
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_effort_set_auto() {
        let (_dir, _guard) = HomeGuard::temp();
        let handler = EffortHandler;
        let mut ctx = test_ctx();
        add_codex_profile(&mut ctx);
        let result = handler.execute("auto", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("medium"));
                assert!(text.contains(&DEFAULT_THINKING_BUDGET.to_string()));
            }
            _ => panic!("Expected Output"),
        }
        assert_eq!(ctx.app_state.effort_value.as_deref(), Some("medium"));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_effort_set_max_rejected_for_legacy_model() {
        let (_dir, _guard) = HomeGuard::temp();
        let handler = EffortHandler;
        let mut ctx = test_ctx();
        add_codex_profile(&mut ctx);
        let result = handler.execute("MAX", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Invalid effort"));
            }
            _ => panic!("Expected Output"),
        }
        assert!(ctx.app_state.effort_value.is_none());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_effort_set_max_for_gpt_5_6() {
        let (dir, _guard) = HomeGuard::temp();
        let handler = EffortHandler;
        let mut ctx = test_ctx();
        add_codex_profile(&mut ctx);
        ctx.app_state.main_loop_model = "gpt-5.6-sol".to_string();
        if let Some(profile) = ctx.app_state.settings.auth_profiles.get_mut("codex") {
            profile.model = Some("gpt-5.6-sol".to_string());
        }

        let result = handler.execute("MAX", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => assert!(text.contains("max")),
            _ => panic!("Expected Output"),
        }
        assert_eq!(ctx.app_state.effort_value.as_deref(), Some("max"));
        assert_eq!(
            ctx.app_state.settings.model_reasoning_effort.as_deref(),
            Some("max")
        );
        let settings: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            settings["authProfiles"]["codex"]["modelReasoningEffort"],
            "max"
        );
    }

    #[tokio::test]
    async fn test_effort_invalid_level() {
        let handler = EffortHandler;
        let mut ctx = test_ctx();
        add_codex_profile(&mut ctx);
        let result = handler.execute("ultra", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Invalid"));
                assert!(text.contains("ultra"));
            }
            _ => panic!("Expected Output"),
        }
        assert!(ctx.app_state.effort_value.is_none());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_effort_case_insensitive() {
        let (_dir, _guard) = HomeGuard::temp();
        let handler = EffortHandler;
        let mut ctx = test_ctx();
        add_codex_profile(&mut ctx);
        let _ = handler.execute("HIGH", &mut ctx).await.unwrap();
        assert_eq!(ctx.app_state.effort_value.as_deref(), Some("high"));
    }
}
