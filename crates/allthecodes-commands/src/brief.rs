//! `/brief` command -- toggle Brief output mode (KAIROS).
//!
//! Brief mode routes all output through the BriefTool, producing
//! concise summaries instead of verbose responses.
//!
//! Requires `FEATURE_KAIROS_BRIEF=1` (which itself requires `FEATURE_KAIROS=1`).

use anyhow::Result;
use async_trait::async_trait;
use std::sync::{OnceLock, RwLock};

use crate::{CommandContext, CommandHandler, CommandResult};
use allthecodes_config::features::{self, Feature};

#[derive(Clone, Copy)]
pub struct BriefCommandRuntime {
    pub clear_prompt_cache: fn(),
}

static BRIEF_RUNTIME: OnceLock<RwLock<Option<BriefCommandRuntime>>> = OnceLock::new();

pub fn set_brief_command_runtime(runtime: BriefCommandRuntime) {
    let slot = BRIEF_RUNTIME.get_or_init(|| RwLock::new(None));
    if let Ok(mut guard) = slot.write() {
        *guard = Some(runtime);
    }
}

fn clear_prompt_cache() -> Result<()> {
    let Some(slot) = BRIEF_RUNTIME.get() else {
        anyhow::bail!("Brief command runtime is unavailable; install BriefCommandRuntime adapter");
    };
    let Ok(guard) = slot.read() else {
        anyhow::bail!("Brief command runtime lock is poisoned");
    };
    let Some(runtime) = *guard else {
        anyhow::bail!("Brief command runtime is unavailable; install BriefCommandRuntime adapter");
    };
    (runtime.clear_prompt_cache)();
    Ok(())
}

pub struct BriefHandler;

#[async_trait]
impl CommandHandler for BriefHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        if !features::enabled(Feature::KairosBrief) {
            return Ok(CommandResult::Output(
                "Brief mode requires FEATURE_KAIROS_BRIEF=1".into(),
            ));
        }

        match args.trim().to_lowercase().as_str() {
            "on" | "enable" => {
                ctx.app_state.is_brief_only = true;
                clear_prompt_cache()?;
                Ok(CommandResult::Output("Brief mode enabled.".into()))
            }
            "off" | "disable" => {
                ctx.app_state.is_brief_only = false;
                clear_prompt_cache()?;
                Ok(CommandResult::Output("Brief mode disabled.".into()))
            }
            "status" => Ok(CommandResult::Output(format!(
                "Brief mode: {}",
                if ctx.app_state.is_brief_only {
                    "ON"
                } else {
                    "OFF"
                }
            ))),
            "" => {
                ctx.app_state.is_brief_only = !ctx.app_state.is_brief_only;
                clear_prompt_cache()?;
                Ok(CommandResult::Output(format!(
                    "Brief mode {}.",
                    if ctx.app_state.is_brief_only {
                        "enabled"
                    } else {
                        "disabled"
                    }
                )))
            }
            _ => Ok(CommandResult::Output(
                "Usage: /brief [on|off|status]".into(),
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
    use std::path::PathBuf;

    fn test_ctx() -> CommandContext {
        CommandContext {
            messages: Vec::new(),
            cwd: PathBuf::from("/test"),
            app_state: Default::default(),
            session_id: SessionId::from_string("test-session"),
        }
    }

    #[tokio::test]
    async fn test_feature_gate() {
        // By default, FEATURE_KAIROS_BRIEF is not set, so the command should reject.
        let handler = BriefHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("on", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => assert!(text.contains("FEATURE_KAIROS_BRIEF")),
            _ => panic!("Expected Output"),
        }
    }

    #[tokio::test]
    async fn test_unknown_argument_gated() {
        // Feature is not enabled in test env, so even unknown args hit the gate.
        let handler = BriefHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("turbo", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => assert!(text.contains("FEATURE_KAIROS_BRIEF")),
            _ => panic!("Expected Output"),
        }
    }
}
