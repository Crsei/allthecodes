//! `/effort` command -- set the reasoning effort level per active provider.
//!
//! Behavior is provider-aware:
//!   - Codex / OpenAI Responses transport: persist to the active auth
//!     profile's `modelReasoningEffort` and emit `reasoning.effort` on the
//!     wire. The display never mentions a local thinking-token budget,
//!     because the upstream reasoning token count is server-controlled.
//!   - Anthropic transport: persist to `output_config.effort` (the Claude
//!     compat wire field) and show the local request budget as a hint.
//!   - Unknown / passthrough providers: refuse to set or display effort and
//!     tell the user that provider capability metadata is required.
//!
//! The historical unified path that simultaneously wrote `output_config.effort`
//! (with the Claude compatibility collapsing map) and the Codex profile
//! `modelReasoningEffort` is intentionally gone: that path caused the documented
//! "TUI shows high, wire sends low" drift (see
//! `development/tui/2026-07-18-model-effort-provider-semantics-fix-plan.md`
//! §2.2).

use allthecodes_config::settings::{self, RawSettings};
use allthecodes_engine::effort::{
    effort_to_budget_tokens, normalize_effort_value, normalize_output_effort_value,
    EffortTransport, DEFAULT_THINKING_BUDGET, MAX_THINKING_BUDGET,
};
use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandHandler, CommandResult};

pub struct EffortHandler;

/// Identify the active provider's wire transport from the live command
/// context. Falls back to `Passthrough` when no active profile is set or the
/// profile does not declare an `api_provider`.
fn active_transport(ctx: &CommandContext) -> EffortTransport {
    let api_provider = ctx
        .app_state
        .settings
        .active_auth_profile
        .as_deref()
        .and_then(|active| ctx.app_state.settings.auth_profiles.get(active))
        .and_then(|profile| profile.api_provider.as_deref())
        .or(ctx.app_state.settings.api_provider.as_deref());
    EffortTransport::from_api_provider(api_provider)
}

/// Capability lookup for the active model. Bundled catalog takes priority for
/// known bundled ids; a user override is consulted only when the model is not
/// in the bundled catalog.
fn current_model_capability(
    ctx: &CommandContext,
) -> Option<allthecodes_config::settings::ModelCapabilitySettings> {
    let model = &ctx.app_state.main_loop_model;
    let bundled = settings::codex_model_capabilities();
    if bundled.contains_key(model) {
        return bundled.get(model).cloned();
    }
    ctx.app_state
        .settings
        .model_capabilities
        .get(model)
        .cloned()
        .or_else(|| {
            let active = ctx.app_state.settings.active_auth_profile.as_deref()?;
            ctx.app_state
                .settings
                .auth_profiles
                .get(active)?
                .model_capabilities
                .as_ref()?
                .get(model)
                .cloned()
        })
}

/// Display the current effort for the Codex transport. Codex never exposes a
/// local thinking-token budget; only the label and its provenance matter.
fn codex_display(value: Option<&str>, default_reasoning: &str) -> String {
    match value {
        None | Some("auto") => format!(
            "auto (bundled default: {}; model default applied by provider)",
            default_reasoning
        ),
        Some(level) => format!("{} (sent as reasoning.effort)", level),
    }
}

/// Display the current effort for the Anthropic transport. The Anthropic
/// fixed-budget path is the only place where a local token budget is owned by
/// this process, so it is the only place where we surface "tokens" in the
/// `/effort` display.
fn anthropic_display(value: Option<&str>) -> String {
    match value {
        None => format!(
            "(not set - thinking falls back to {} local request budget when enabled)",
            DEFAULT_THINKING_BUDGET
        ),
        Some("auto") => format!(
            "auto (model default; {} local request budget in the fixed-budget path)",
            DEFAULT_THINKING_BUDGET
        ),
        Some("max") => format!(
            "max (highest local request budget in this fork: {} tokens)",
            MAX_THINKING_BUDGET
        ),
        Some("xhigh") => "xhigh (extra-high local request budget)".to_string(),
        Some("minimal") => "minimal (minimal local reasoning)".to_string(),
        Some("none") => "none (reasoning disabled)".to_string(),
        Some(s) => match effort_to_budget_tokens(s) {
            Some(tokens) => format!("{} ({} local request budget tokens)", s, tokens),
            None => format!("{} (unrecognized - will use default budget)", s),
        },
    }
}

#[async_trait]
impl CommandHandler for EffortHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let arg = args.trim().to_string();
        let transport = active_transport(ctx);

        // Unknown transport: refuse to fabricate effort semantics.
        if matches!(transport, EffortTransport::Passthrough) {
            return Ok(CommandResult::Output(
                "Effort shaping is not available for the active provider. \
                 Configure a provider that declares modelCapabilities (Codex or Anthropic) \
                 to use /effort."
                    .to_string(),
            ));
        }

        let Some(capability) = current_model_capability(ctx) else {
            return Ok(CommandResult::Output(
                "Current profile has no configured reasoning levels for this model. \
                 Use /login and /model to select a configured profile model."
                    .to_string(),
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

        // The "current" displayed value is the single resolved effort, read
        // from the provider-appropriate field per transport. We deliberately
        // do NOT fall through to `output_config.effort` for the Codex
        // transport, and do NOT fall through to `modelReasoningEffort` for
        // the Anthropic transport -- this is the boundary the historical
        // unified path crossed.
        let current_value: Option<String> = match transport {
            EffortTransport::Codex => ctx
                .app_state
                .settings
                .active_auth_profile
                .as_deref()
                .and_then(|active| ctx.app_state.settings.auth_profiles.get(active))
                .and_then(|profile| profile.model_reasoning_effort.clone())
                .or_else(|| ctx.app_state.effort_value.clone()),
            EffortTransport::Anthropic => ctx
                .app_state
                .settings
                .output_config
                .as_ref()
                .and_then(|value| value.get("effort"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
                .or_else(|| ctx.app_state.effort_value.clone()),
            EffortTransport::Passthrough => None,
        };
        let display_value = current_value.as_deref();

        if arg.is_empty() {
            let summary = match transport {
                EffortTransport::Codex => codex_display(display_value, default_reasoning),
                EffortTransport::Anthropic => anthropic_display(display_value),
                EffortTransport::Passthrough => unreachable!(),
            };
            return Ok(CommandResult::Output(format!(
                "Current effort: {}\n\n\
                 Model: {}\n\
                 Usage: /effort <auto|{}>\n\
                 auto uses the model default: {}.",
                summary,
                ctx.app_state.main_loop_model,
                supported.join("|"),
                default_reasoning,
            )));
        }

        // Validate the requested level against the supported set for this
        // model + transport. `auto` is always accepted and clears the
        // provider-specific override.
        let is_auto = arg.eq_ignore_ascii_case("auto");
        let stored = if is_auto {
            default_reasoning.to_string()
        } else {
            match normalize_effort_value(&arg) {
                Some(value) if supported.iter().any(|s| s == &value) => value,
                _ => {
                    return Ok(CommandResult::Output(format!(
                        "Invalid effort for {}: '{}'\\nSupported levels: auto, {}",
                        ctx.app_state.main_loop_model,
                        arg,
                        supported.join(", ")
                    )));
                }
            }
        };

        // Apply the resolved value to the in-memory app_state per transport,
        // then persist to disk via the provider-appropriate path.
        let persist_msg = match transport {
            EffortTransport::Codex => {
                apply_codex_effort(ctx, &stored);
                persist_codex_profile_reasoning_effort(&ctx.app_state.settings, &stored, is_auto)
            }
            EffortTransport::Anthropic => {
                apply_anthropic_effort(ctx, &stored);
                persist_anthropic_output_effort(&ctx.app_state.settings, &stored, is_auto)
            }
            EffortTransport::Passthrough => unreachable!(),
        };

        let confirmation = match transport {
            EffortTransport::Codex => {
                format!("Effort set to: {} (sent as reasoning.effort)", stored)
            }
            EffortTransport::Anthropic => anthropic_display(Some(&stored)),
            EffortTransport::Passthrough => unreachable!(),
        };

        Ok(CommandResult::Output(format!(
            "{}\n{}",
            confirmation, persist_msg
        )))
    }
}

/// Apply a Codex effort to in-memory state: write the canonical
/// `modelReasoningEffort` paths and synthesize `effort_value` for the tool
/// context (which exposes a stable `effort` key to hooks/plugins). Never
/// touch `output_config.effort` -- that field belongs to the Anthropic
/// transport.
fn apply_codex_effort(ctx: &mut CommandContext, value: &str) {
    ctx.app_state.effort_value = Some(value.to_string());
    ctx.app_state.settings.model_reasoning_effort = Some(value.to_string());
    if let Some(active) = ctx.app_state.settings.active_auth_profile.clone() {
        if let Some(profile) = ctx.app_state.settings.auth_profiles.get_mut(&active) {
            profile.model_reasoning_effort = Some(value.to_string());
        }
    }
    ctx.app_state.settings.sources.insert(
        "model_reasoning_effort".to_string(),
        settings::SettingsSource::User,
    );
}

/// Apply an Anthropic effort to in-memory state: write `output_config.effort`
/// (the canonical Claude compat wire field) and synthesize `effort_value` for
/// the tool context. Never touch `model_reasoning_effort` -- that field
/// belongs to the Codex transport.
fn apply_anthropic_effort(ctx: &mut CommandContext, value: &str) {
    let Some(output_effort) = normalize_output_effort_value(value) else {
        return;
    };
    ctx.app_state.effort_value = Some(value.to_string());
    let mut object = ctx
        .app_state
        .settings
        .output_config
        .take()
        .and_then(|value| match value {
            serde_json::Value::Object(map) => Some(map),
            _ => None,
        })
        .unwrap_or_default();
    object.insert(
        "effort".to_string(),
        serde_json::Value::String(output_effort),
    );
    ctx.app_state.settings.output_config = Some(serde_json::Value::Object(object));
    ctx.app_state
        .settings
        .sources
        .insert("output_config".to_string(), settings::SettingsSource::User);
}

/// Persist a Codex effort: only update the active auth profile's
/// `modelReasoningEffort`. When `is_auto`, remove the override so the
/// provider follows its own default rather than freezing a bundled default.
fn persist_codex_profile_reasoning_effort(
    runtime_settings: &allthecodes_config::runtime_settings::SettingsJson,
    value: &str,
    is_auto: bool,
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
    let mut raw = load_or_default_raw(&path);

    if is_auto {
        // Directly clear the override -- bypass `upsert_auth_profile` because
        // the merge helper applies Some-wins semantics and would keep an
        // existing `model_reasoning_effort` instead of writing `None`.
        let profiles = raw
            .auth_profiles
            .get_or_insert_with(std::collections::HashMap::new);
        let entry = profiles.entry(active.to_string()).or_default();
        entry.model_reasoning_effort = None;
    } else {
        let mut profile = raw
            .auth_profiles
            .as_ref()
            .and_then(|profiles| profiles.get(active))
            .cloned()
            .or_else(|| runtime_settings.auth_profiles.get(active).cloned())
            .unwrap_or_default();
        profile.model_reasoning_effort = Some(value.to_string());
        settings::upsert_auth_profile(&mut raw, active, profile, false);
    }
    match settings::write_user_settings(&raw) {
        Ok(path) => {
            if is_auto {
                format!(
                    "-> cleared authProfiles.{active}.modelReasoningEffort (auto); settings: {}",
                    path.display()
                )
            } else {
                format!(
                    "-> persisted authProfiles.{active}.modelReasoningEffort={} to {}",
                    value,
                    path.display()
                )
            }
        }
        Err(error) => format!(
            "Effort updated for this session, but user settings were not updated: {}",
            error
        ),
    }
}

/// Persist an Anthropic effort: only update `output_config.effort`. When
/// `is_auto`, remove the override so the Anthropic API default applies.
fn persist_anthropic_output_effort(
    runtime_settings: &allthecodes_config::runtime_settings::SettingsJson,
    value: &str,
    is_auto: bool,
) -> String {
    let path = settings::user_settings_path();
    let mut raw = load_or_default_raw(&path);

    if is_auto {
        let mut removed = false;
        if let Some(serde_json::Value::Object(map)) = raw.output_config.as_mut() {
            if map.remove("effort").is_some() {
                removed = true;
            }
            if map.is_empty() {
                raw.output_config = None;
            }
        }
        let _ = runtime_settings; // acknowledge we did not consult it
        match settings::write_user_settings(&raw) {
            Ok(path) => format!(
                "-> cleared output_config.effort (auto); settings: {} (was {}",
                path.display(),
                if removed { "present" } else { "absent" },
            ),
            Err(error) => format!(
                "Effort updated for this session, but user settings were not updated: {}",
                error
            ),
        }
    } else {
        let Some(output_effort) = normalize_output_effort_value(value) else {
            return format!(
                "Effort '{}' is not a valid Anthropic output effort level",
                value
            );
        };
        let mut object = raw
            .output_config
            .take()
            .and_then(|value| match value {
                serde_json::Value::Object(map) => Some(map),
                _ => None,
            })
            .unwrap_or_default();
        object.insert(
            "effort".to_string(),
            serde_json::Value::String(output_effort),
        );
        raw.output_config = Some(serde_json::Value::Object(object));
        match settings::write_user_settings(&raw) {
            Ok(path) => format!(
                "-> persisted output_config.effort={} to {}",
                value,
                path.display()
            ),
            Err(error) => format!(
                "Effort updated for this session, but user settings were not updated: {}",
                error
            ),
        }
    }
}

fn load_or_default_raw(path: &std::path::Path) -> RawSettings {
    if path.exists() {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|txt| serde_json::from_str::<RawSettings>(&txt).ok())
            .unwrap_or_default()
    } else {
        RawSettings::default()
    }
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
        let profile = allthecodes_config::settings::ProviderProfileSettings {
            backend: Some("codex".to_string()),
            api_provider: Some("openai-codex".to_string()),
            model: Some("gpt-5.5".to_string()),
            available_models: Some(allthecodes_config::settings::codex_model_ids()),
            model_capabilities: Some(allthecodes_config::settings::codex_model_capabilities()),
            model_reasoning_effort: None,
            ..Default::default()
        };
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

    fn add_anthropic_profile(ctx: &mut CommandContext) {
        ctx.app_state.main_loop_model = "claude-opus-4-8".to_string();
        ctx.app_state.settings.active_auth_profile = Some("anthropic".to_string());
        let cap = allthecodes_config::settings::ModelCapabilitySettings {
            supported_reasoning_levels: vec![
                "low".to_string(),
                "medium".to_string(),
                "high".to_string(),
                "xhigh".to_string(),
                "max".to_string(),
            ],
            default_reasoning_level: Some("medium".to_string()),
            ..Default::default()
        };
        let profile = allthecodes_config::settings::ProviderProfileSettings {
            backend: Some("anthropic".to_string()),
            api_provider: Some("anthropic".to_string()),
            model: Some("claude-opus-4-8".to_string()),
            model_capabilities: Some({
                let mut map = std::collections::HashMap::new();
                map.insert("claude-opus-4-8".to_string(), cap.clone());
                map
            }),
            ..Default::default()
        };
        ctx.app_state
            .settings
            .auth_profiles
            .insert("anthropic".to_string(), profile);
        ctx.app_state
            .settings
            .model_capabilities
            .insert("claude-opus-4-8".to_string(), cap);
    }

    #[tokio::test]
    async fn codex_no_args_shows_level_only_no_token_text() {
        // Phase A failure test #1: Codex /effort no-arg must NOT show
        // "X thinking tokens" -- the upstream reasoning budget is
        // server-controlled.
        let handler = EffortHandler;
        let mut ctx = test_ctx();
        add_codex_profile(&mut ctx);
        ctx.app_state.effort_value = Some("high".into());
        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Current effort"));
                assert!(text.contains("high"));
                assert!(
                    !text.contains("24576"),
                    "Codex display must not advertise thinking-token budget: {}",
                    text
                );
                assert!(
                    !text.contains("thinking tokens"),
                    "Codex display must not use Anthropic budget language: {}",
                    text
                );
            }
            _ => panic!("Expected Output"),
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn codex_set_level_persists_only_model_reasoning_effort() {
        // Phase A failure test #2: `/effort low` with a Codex profile must
        // NOT also write `output_config.effort` (which previously collapsed
        // low->high and caused TUI/wire drift).
        let (dir, _guard) = HomeGuard::temp();
        let handler = EffortHandler;
        let mut ctx = test_ctx();
        add_codex_profile(&mut ctx);
        let result = handler.execute("low", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("low"));
                assert!(
                    !text.contains("output_config.effort"),
                    "Codex path must not report output_config.effort writes: {}",
                    text
                );
            }
            _ => panic!("Expected Output"),
        }
        assert_eq!(ctx.app_state.effort_value.as_deref(), Some("low"));
        let settings: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            settings["authProfiles"]["codex"]["modelReasoningEffort"],
            "low"
        );
        // Critical: output_config.effort must NOT have been written for the
        // Codex transport.
        assert!(
            settings["output_config"].is_null() || settings["output_config"]["effort"].is_null(),
            "Codex /effort must not write output_config.effort: {}",
            settings["output_config"]
        );
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn codex_set_xhigh_does_not_collapse_to_max_in_settings() {
        // Phase A failure test #3: setting `xhigh` on Codex must persist
        // `xhigh`, not collapse to `max` (the historical Claude compat
        // collapse that produced TUI/wire drift).
        let (dir, _guard) = HomeGuard::temp();
        let handler = EffortHandler;
        let mut ctx = test_ctx();
        add_codex_profile(&mut ctx);
        let result = handler.execute("xhigh", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => assert!(text.contains("xhigh")),
            _ => panic!("Expected Output"),
        }
        assert_eq!(ctx.app_state.effort_value.as_deref(), Some("xhigh"));
        let settings: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            settings["authProfiles"]["codex"]["modelReasoningEffort"],
            "xhigh"
        );
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn codex_auto_clears_profile_reasoning_effort() {
        // Phase A failure test #4: `/effort auto` must NOT persist the
        // resolved bundled default; it must clear the profile override so the
        // provider follows its own default going forward.
        let (dir, _guard) = HomeGuard::temp();
        let handler = EffortHandler;
        let mut ctx = test_ctx();
        add_codex_profile(&mut ctx);
        // Pre-set a level so auto has something to clear.
        let _ = handler.execute("high", &mut ctx).await.unwrap();
        let _ = handler.execute("auto", &mut ctx).await.unwrap();
        let settings: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("settings.json")).unwrap(),
        )
        .unwrap();
        assert!(
            settings["authProfiles"]["codex"]["modelReasoningEffort"].is_null(),
            "auto must clear modelReasoningEffort, got: {}",
            settings["authProfiles"]["codex"]["modelReasoningEffort"]
        );
    }

    #[tokio::test]
    async fn codex_max_unsupported_for_legacy_model_rejected() {
        let (_dir, _guard) = HomeGuard::temp();
        let handler = EffortHandler;
        let mut ctx = test_ctx();
        add_codex_profile(&mut ctx);
        let result = handler.execute("MAX", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => assert!(text.contains("Invalid effort")),
            _ => panic!("Expected Output"),
        }
        assert!(ctx.app_state.effort_value.is_none());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn codex_max_for_gpt_5_6_persists_only_profile() {
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
        let settings: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            settings["authProfiles"]["codex"]["modelReasoningEffort"],
            "max"
        );
        assert!(
            settings["output_config"].is_null() || settings["output_config"]["effort"].is_null(),
            "max on Codex must not write output_config.effort"
        );
    }

    #[tokio::test]
    async fn codex_invalid_level_rejected() {
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
    async fn codex_numeric_override_rejected_without_capability_match() {
        // Codex transport does not accept numeric overrides (those are an
        // Anthropic fixed-budget concept).
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
    #[serial_test::serial]
    async fn anthropic_set_level_persists_only_output_config_effort() {
        // Phase A failure test #5: Anthropic `/effort high` must persist only
        // `output_config.effort` (the canonical Anthropic wire field), and
        // must NOT touch `authProfiles.<active>.modelReasoningEffort`.
        let (dir, _guard) = HomeGuard::temp();
        let handler = EffortHandler;
        let mut ctx = test_ctx();
        add_anthropic_profile(&mut ctx);
        let result = handler.execute("high", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("high"));
                assert!(
                    !text.contains("modelReasoningEffort"),
                    "Anthropic path must not mention modelReasoningEffort: {}",
                    text
                );
            }
            _ => panic!("Expected Output"),
        }
        assert_eq!(ctx.app_state.effort_value.as_deref(), Some("high"));
        let settings: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(settings["output_config"]["effort"], "high");
        assert!(
            settings["authProfiles"]["anthropic"]["modelReasoningEffort"].is_null(),
            "Anthropic /effort must not write modelReasoningEffort"
        );
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn anthropic_auto_clears_output_config_effort() {
        let (dir, _guard) = HomeGuard::temp();
        let handler = EffortHandler;
        let mut ctx = test_ctx();
        add_anthropic_profile(&mut ctx);
        let _ = handler.execute("high", &mut ctx).await.unwrap();
        let _ = handler.execute("auto", &mut ctx).await.unwrap();
        let settings: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("settings.json")).unwrap(),
        )
        .unwrap();
        assert!(
            settings["output_config"].is_null() || settings["output_config"]["effort"].is_null(),
            "auto must clear output_config.effort, got: {}",
            settings["output_config"]
        );
    }

    #[tokio::test]
    async fn anthropic_no_arg_shows_local_request_budget_label() {
        let handler = EffortHandler;
        let mut ctx = test_ctx();
        add_anthropic_profile(&mut ctx);
        ctx.app_state.effort_value = Some("high".into());
        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("high"));
                // Anthropic path *may* show local request budget language.
                assert!(
                    text.contains("local request budget") || text.contains("tokens"),
                    "Anthropic display must mention local budget, got: {}",
                    text
                );
            }
            _ => panic!("Expected Output"),
        }
    }

    #[tokio::test]
    async fn passthrough_provider_refuses_effort() {
        let mut ctx = test_ctx();
        // No active profile -> passthrough transport.
        let handler = EffortHandler;
        let result = handler.execute("high", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("not available"));
            }
            _ => panic!("Expected Output"),
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn codex_case_insensitive_normalizes() {
        let (_dir, _guard) = HomeGuard::temp();
        let handler = EffortHandler;
        let mut ctx = test_ctx();
        add_codex_profile(&mut ctx);
        let _ = handler.execute("HIGH", &mut ctx).await.unwrap();
        assert_eq!(ctx.app_state.effort_value.as_deref(), Some("high"));
    }
}
