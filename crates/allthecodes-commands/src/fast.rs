//! `/fast` command -- toggle fast mode.
//!
//! Fast mode uses a configured fast-capable model with faster output via
//! `speed: "fast"` API parameter + `fast-mode-2026-02-01` beta header.
//!
//! Implementation:
//! - Toggle `app_state.fast_mode`
//! - Validate model compatibility
//! - Auto-switch model if needed
//! - Show current status

use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandHandler, CommandResult};

pub struct FastHandler;

#[async_trait]
impl CommandHandler for FastHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let subcmd = args.trim().to_lowercase();

        match subcmd.as_str() {
            "on" | "enable" => enable_fast_mode(ctx),
            "off" | "disable" => disable_fast_mode(ctx),
            "status" => show_status(ctx),
            "" => toggle_fast_mode(ctx),
            _ => Ok(CommandResult::Output(format!(
                "Unknown argument: '{}'\n\
                 Usage:\n  \
                   /fast          -- toggle fast mode\n  \
                   /fast on       -- enable fast mode\n  \
                   /fast off      -- disable fast mode\n  \
                   /fast status   -- show current status",
                subcmd
            ))),
        }
    }
}

/// Check if the current model supports fast mode.
fn model_supports_fast(ctx: &CommandContext, model: &str) -> bool {
    ctx.app_state
        .settings
        .model_capabilities
        .get(model)
        .map(|capability| capability.supports_fast_mode)
        .or_else(|| {
            let active = ctx.app_state.settings.active_auth_profile.as_deref()?;
            ctx.app_state
                .settings
                .auth_profiles
                .get(active)?
                .model_capabilities
                .as_ref()?
                .get(model)
                .map(|capability| capability.supports_fast_mode)
        })
        .unwrap_or(false)
}

/// Toggle fast mode on/off.
fn toggle_fast_mode(ctx: &mut CommandContext) -> Result<CommandResult> {
    if ctx.app_state.fast_mode {
        disable_fast_mode(ctx)
    } else {
        enable_fast_mode(ctx)
    }
}

/// Enable fast mode, auto-switching model if necessary.
fn enable_fast_mode(ctx: &mut CommandContext) -> Result<CommandResult> {
    if ctx.app_state.fast_mode {
        return Ok(CommandResult::Output(
            "Fast mode is already enabled.".to_string(),
        ));
    }

    let previous_model = ctx.app_state.main_loop_model.clone();
    let mut switched_model = false;

    let fast_model = resolve_fast_model(ctx);

    // Auto-switch if current model doesn't support fast mode.
    if !model_supports_fast(ctx, &ctx.app_state.main_loop_model) {
        if !model_supports_fast(ctx, &fast_model) {
            return Ok(CommandResult::Output(format!(
                "Fast mode is not supported by the active profile for model '{}'.",
                ctx.app_state.main_loop_model
            )));
        }
        ctx.app_state.main_loop_model = fast_model.clone();
        ctx.app_state.settings.model = Some(fast_model.clone());
        switched_model = true;
    }

    ctx.app_state.fast_mode = true;

    let mut msg =
        "Fast mode enabled. Output will be generated faster using the same model.".to_string();
    if switched_model {
        msg.push_str(&format!(
            "\nModel switched from '{}' to '{}' for fast mode.",
            previous_model, fast_model
        ));
    }

    // Note: The query loop reads fast_mode to set:
    //   - speed: "fast" in API request
    //   - anthropic-beta: fast-mode-2026-02-01 header

    Ok(CommandResult::Output(msg))
}

fn resolve_fast_model(ctx: &CommandContext) -> String {
    if let Some(model) = ctx.app_state.settings.fast_model.as_deref() {
        return crate::model::resolve_model_alias_with_settings(model, &ctx.app_state.settings);
    }
    if let Some(model) = ctx
        .app_state
        .settings
        .available_models
        .iter()
        .find(|model| model_supports_fast(ctx, model))
    {
        return model.clone();
    }
    ctx.app_state
        .settings
        .model_capabilities
        .iter()
        .find(|(_, capability)| capability.supports_fast_mode)
        .map(|(model, _)| model.clone())
        .unwrap_or_else(|| {
            crate::model::resolve_model_alias_with_settings(
                allthecodes_types::models::DEFAULT_FAST_MODEL_ALIAS,
                &ctx.app_state.settings,
            )
        })
}

/// Disable fast mode (keeps the current model).
fn disable_fast_mode(ctx: &mut CommandContext) -> Result<CommandResult> {
    if !ctx.app_state.fast_mode {
        return Ok(CommandResult::Output(
            "Fast mode is already disabled.".to_string(),
        ));
    }

    ctx.app_state.fast_mode = false;
    Ok(CommandResult::Output(
        "Fast mode disabled. Normal output speed restored.".to_string(),
    ))
}

/// Show current fast mode status.
fn show_status(ctx: &CommandContext) -> Result<CommandResult> {
    let status = if ctx.app_state.fast_mode {
        "enabled"
    } else {
        "disabled"
    };
    let model = &ctx.app_state.main_loop_model;
    let compatible = model_supports_fast(ctx, model);

    Ok(CommandResult::Output(format!(
        "Fast mode: {}\n\
         Model: {}\n\
         Model compatible: {}",
        status, model, compatible
    )))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
    use allthecodes_engine::types::app_state::AppState;
    use std::path::PathBuf;

    fn test_ctx() -> CommandContext {
        CommandContext {
            messages: Vec::new(),
            cwd: PathBuf::from("/test"),
            app_state: AppState::default(),
            session_id: SessionId::from_string("test-session"),
        }
    }

    fn add_codex_capabilities(ctx: &mut CommandContext) {
        let mut profile = allthecodes_config::settings::ProviderProfileSettings {
            backend: Some("codex".to_string()),
            api_provider: Some("openai-codex".to_string()),
            available_models: Some(allthecodes_config::settings::codex_model_ids()),
            model_capabilities: Some(allthecodes_config::settings::codex_model_capabilities()),
            ..Default::default()
        };
        profile.model = Some("gpt-5.5".to_string());
        ctx.app_state.settings.active_auth_profile = Some("codex".to_string());
        ctx.app_state
            .settings
            .auth_profiles
            .insert("codex".to_string(), profile);
        ctx.app_state.settings.available_models = allthecodes_config::settings::codex_model_ids();
        ctx.app_state.settings.model_capabilities =
            allthecodes_config::settings::codex_model_capabilities();
    }

    #[test]
    fn test_model_supports_fast() {
        let mut ctx = test_ctx();
        add_codex_capabilities(&mut ctx);
        assert!(model_supports_fast(&ctx, "gpt-5.5"));
        assert!(!model_supports_fast(&ctx, "gpt-5.4-mini"));
        assert!(!model_supports_fast(&ctx, "claude-haiku-4-5"));
    }

    #[tokio::test]
    async fn test_toggle_enables_fast_mode() {
        let handler = FastHandler;
        let mut ctx = test_ctx();
        add_codex_capabilities(&mut ctx);
        assert!(!ctx.app_state.fast_mode);

        let result = handler.execute("", &mut ctx).await.unwrap();
        assert!(ctx.app_state.fast_mode);
        match result {
            CommandResult::Output(text) => assert!(text.contains("enabled")),
            _ => panic!("Expected Output"),
        }
    }

    #[tokio::test]
    async fn test_toggle_disables_fast_mode() {
        let handler = FastHandler;
        let mut ctx = test_ctx();
        add_codex_capabilities(&mut ctx);
        ctx.app_state.fast_mode = true;

        let result = handler.execute("", &mut ctx).await.unwrap();
        assert!(!ctx.app_state.fast_mode);
        match result {
            CommandResult::Output(text) => assert!(text.contains("disabled")),
            _ => panic!("Expected Output"),
        }
    }

    #[tokio::test]
    async fn test_enable_auto_switches_model() {
        let handler = FastHandler;
        let mut ctx = test_ctx();
        add_codex_capabilities(&mut ctx);
        ctx.app_state.main_loop_model = "legacy-non-fast-model".to_string();
        assert!(!model_supports_fast(&ctx, &ctx.app_state.main_loop_model));

        let result = handler.execute("on", &mut ctx).await.unwrap();
        assert!(ctx.app_state.fast_mode);
        assert_eq!(ctx.app_state.main_loop_model, "gpt-5.5");
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("switched"));
                assert!(text.contains("gpt-5.5"));
            }
            _ => panic!("Expected Output"),
        }
    }

    #[tokio::test]
    async fn test_enable_already_enabled() {
        let handler = FastHandler;
        let mut ctx = test_ctx();
        add_codex_capabilities(&mut ctx);
        ctx.app_state.fast_mode = true;

        let result = handler.execute("on", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => assert!(text.contains("already enabled")),
            _ => panic!("Expected Output"),
        }
    }

    #[tokio::test]
    async fn test_status_command() {
        let handler = FastHandler;
        let mut ctx = test_ctx();
        add_codex_capabilities(&mut ctx);

        let result = handler.execute("status", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("disabled"));
                assert!(text.contains("Model:"));
            }
            _ => panic!("Expected Output"),
        }
    }

    #[tokio::test]
    async fn test_disable_already_disabled() {
        let handler = FastHandler;
        let mut ctx = test_ctx();
        add_codex_capabilities(&mut ctx);

        let result = handler.execute("off", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => assert!(text.contains("already disabled")),
            _ => panic!("Expected Output"),
        }
    }

    #[tokio::test]
    async fn test_unknown_argument() {
        let handler = FastHandler;
        let mut ctx = test_ctx();
        add_codex_capabilities(&mut ctx);

        let result = handler.execute("turbo", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => assert!(text.contains("Unknown argument")),
            _ => panic!("Expected Output"),
        }
    }
}
