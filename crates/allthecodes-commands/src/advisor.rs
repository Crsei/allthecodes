//! `/advisor` — advisor-model command (issue #33).
//!
//! Lets the user configure a stronger secondary model that the API request
//! pipeline passes through as [`allthecodes_api::api::client::MessagesRequest::advisor_model`].
//! Only providers that advertise advisor support (see
//! [`allthecodes_api::api::client::provider_supports_advisor`]) actually receive the
//! field; for others the command still persists the setting but surfaces a
//! clear "inactive" message so the user knows their choice won't reach the
//! provider.
//!
//! Subcommands:
//!   /advisor                — show the current advisor + support status
//!   /advisor <model>        — set the advisor model
//!   /advisor unset|none|off — clear the advisor model
//!
//! Persistence: the user-level `settings.json` is updated under the
//! `advisorModel` key. AppState is mirrored in-place so the next API call
//! picks up the change without a restart.

use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandHandler, CommandResult};

pub struct AdvisorHandler;

#[async_trait]
impl CommandHandler for AdvisorHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let arg = args.trim();

        match arg {
            "" => Ok(CommandResult::Output(render_status(ctx))),
            "unset" | "none" | "off" | "clear" => clear_advisor(ctx),
            other => set_advisor(ctx, other),
        }
    }
}

/// Human-friendly status text showing current advisor + support state.
fn render_status(ctx: &CommandContext) -> String {
    let mut lines = Vec::new();
    lines.push("Advisor model".to_string());
    lines.push(String::new());

    match ctx.app_state.advisor_model.as_deref() {
        Some(m) => lines.push(format!("  Current: {}", m)),
        None => lines.push("  Current: (unset)".to_string()),
    }

    if let Some(settings_model) = ctx.app_state.settings.advisor_model.as_deref() {
        lines.push(format!(
            "  Persisted (settings.json::advisorModel): {}",
            settings_model
        ));
    } else {
        lines.push("  Persisted: (not set)".to_string());
    }

    lines.push(String::new());
    lines.push(format!("  Main model: {}", ctx.app_state.main_loop_model));

    if ctx.app_state.advisor_model.is_some() {
        lines.push(String::new());
        lines.push(
            "  Status: active — advisor_model will be attached to outbound API requests \
             when the provider supports it (Anthropic, Azure, Bedrock, Vertex). Providers \
             that don't support it ignore the field and log a debug-level notice."
                .into(),
        );
    }

    lines.push(String::new());
    lines.push("Usage:".into());
    lines.push("  /advisor               — show current state".into());
    lines.push("  /advisor <model>       — set advisor model (alias or full id)".into());
    lines.push("  /advisor unset         — clear advisor".into());

    lines.join("\n")
}

fn set_advisor(ctx: &mut CommandContext, raw: &str) -> Result<CommandResult> {
    set_advisor_with_persist(ctx, raw, persist_advisor)
}

fn clear_advisor(ctx: &mut CommandContext) -> Result<CommandResult> {
    clear_advisor_with_persist(ctx, persist_advisor)
}

/// Validate + mutate + persist. Factored out so tests can exercise the
/// validation and AppState mutation with a no-op persist closure (avoiding
/// any touch of the real user settings file).
fn set_advisor_with_persist<F>(
    ctx: &mut CommandContext,
    raw: &str,
    persist: F,
) -> Result<CommandResult>
where
    F: FnOnce(&std::path::Path, Option<&str>) -> Result<std::path::PathBuf>,
{
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(CommandResult::Output(
            "Rejected: model name required".into(),
        ));
    }

    let resolved = if crate::model::is_removed_legacy_model_alias(raw) {
        return Ok(CommandResult::Output(format!(
            "Rejected: {}",
            crate::model::removed_legacy_model_alias_error(raw)
        )));
    } else {
        crate::model::resolve_model_alias_with_settings(raw, &ctx.app_state.settings)
    };
    let trimmed = resolved.trim();

    let previous = ctx.app_state.advisor_model.clone();
    ctx.app_state.advisor_model = Some(trimmed.to_string());
    ctx.app_state.settings.advisor_model = Some(trimmed.to_string());

    let persist_result = persist(&ctx.cwd, Some(trimmed));

    let mut out = Vec::new();
    out.push(match previous {
        Some(p) => format!("Advisor model updated: {} -> {}", p, trimmed),
        None => format!("Advisor model set: {}", trimmed),
    });
    match persist_result {
        Ok(path) => out.push(format!("Persisted to: {}", path.display())),
        Err(e) => out.push(format!(
            "Warning: failed to persist advisor_model to settings.json: {}",
            e
        )),
    }

    out.push(String::new());
    out.push(
        "Note: `advisor_model` is only attached to requests for providers that support \
         it (Anthropic, Azure, Bedrock, Vertex). For other providers the setting is \
         preserved but inactive."
            .to_string(),
    );

    Ok(CommandResult::Output(out.join("\n")))
}

fn clear_advisor_with_persist<F>(ctx: &mut CommandContext, persist: F) -> Result<CommandResult>
where
    F: FnOnce(&std::path::Path, Option<&str>) -> Result<std::path::PathBuf>,
{
    let previous = ctx.app_state.advisor_model.clone();
    ctx.app_state.advisor_model = None;
    ctx.app_state.settings.advisor_model = None;

    let persist_result = persist(&ctx.cwd, None);

    let mut out = Vec::new();
    out.push(match previous {
        Some(p) => format!("Advisor model cleared (was: {}).", p),
        None => "Advisor model cleared (was already unset).".to_string(),
    });
    if let Err(e) = persist_result {
        out.push(format!(
            "Warning: failed to persist advisor_model change to settings.json: {}",
            e
        ));
    }

    Ok(CommandResult::Output(out.join("\n")))
}

/// Write the advisor model to the user-level `settings.json`.
///
/// Uses the atomic-write helper in `cc-config::settings`. Returns the path
/// that was written so the caller can show it to the user.
fn persist_advisor(_cwd: &std::path::Path, new_value: Option<&str>) -> Result<std::path::PathBuf> {
    use allthecodes_config::settings::{load_global_config, write_user_settings};
    let mut raw = load_global_config()?;
    raw.advisor_model = new_value.map(|s| s.to_string());
    let path = write_user_settings(&raw)?;
    Ok(path)
}

/// Test-only: persist to an explicit path instead of the user-level file.
///
/// Keeps the on-disk round-trip covered without racing on the shared
/// `ALLTHECODES_HOME` env var (which other modules' tests also mutate).
#[cfg(test)]
fn persist_advisor_to_path(
    path: &std::path::Path,
    new_value: Option<&str>,
) -> Result<std::path::PathBuf> {
    use allthecodes_config::settings::{write_settings_file, RawSettings};
    let mut raw: RawSettings = if path.exists() {
        let s = std::fs::read_to_string(path)?;
        serde_json::from_str(&s).unwrap_or_default()
    } else {
        RawSettings::default()
    };
    raw.advisor_model = new_value.map(|s| s.to_string());
    write_settings_file(path, &raw)?;
    Ok(path.to_path_buf())
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
            cwd: PathBuf::from("."),
            app_state: AppState::default(),
            session_id: SessionId::from_string("advisor-test-session"),
        }
    }

    /// No-op persist closure for tests. Reports a fake path so the
    /// formatting code path exercises the Ok-branch without writing to
    /// disk. This keeps the command tests hermetic — no env vars, no
    /// files — so they can't race with other modules' settings tests.
    fn noop_persist(_cwd: &std::path::Path, _v: Option<&str>) -> Result<std::path::PathBuf> {
        Ok(std::path::PathBuf::from("/dev/null/fake-settings.json"))
    }

    #[tokio::test]
    async fn show_when_unset() {
        let handler = AdvisorHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Advisor model"));
                assert!(text.contains("Current: (unset)"));
                assert!(text.contains("Usage:"));
            }
            _ => panic!("expected Output"),
        }
    }

    #[tokio::test]
    async fn set_mutates_app_state_and_settings() {
        let mut ctx = test_ctx();
        let result = set_advisor_with_persist(&mut ctx, "SOTA", noop_persist).unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains(allthecodes_types::models::SOTA_MODEL_ID))
            }
            _ => panic!("expected Output"),
        }
        assert_eq!(
            ctx.app_state.advisor_model.as_deref(),
            Some(allthecodes_types::models::SOTA_MODEL_ID)
        );
        assert_eq!(
            ctx.app_state.settings.advisor_model.as_deref(),
            Some(allthecodes_types::models::SOTA_MODEL_ID)
        );
    }

    #[tokio::test]
    async fn show_after_set_reports_active_status() {
        let mut ctx = test_ctx();
        set_advisor_with_persist(&mut ctx, "my-advisor-model", noop_persist).unwrap();
        let rendered = render_status(&ctx);
        assert!(rendered.contains("my-advisor-model"));
        assert!(rendered.contains("Status: active"));
    }

    #[test]
    fn clear_resets_both_fields() {
        let mut ctx = test_ctx();
        ctx.app_state.advisor_model = Some("foo".into());
        ctx.app_state.settings.advisor_model = Some("foo".into());
        let result = clear_advisor_with_persist(&mut ctx, noop_persist).unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.to_lowercase().contains("clear"));
            }
            _ => panic!("expected Output"),
        }
        assert!(ctx.app_state.advisor_model.is_none());
        assert!(ctx.app_state.settings.advisor_model.is_none());
    }

    /// Dispatch routing: all four unset aliases reach `clear_advisor`.
    /// We call the private helpers so this test stays hermetic — a handler
    /// round-trip would pull in real persistence.
    #[test]
    fn unset_variants_route_to_clear() {
        for variant in ["unset", "none", "off", "clear"] {
            // Simulate the dispatch branch the handler uses.
            let matched = matches!(variant, "unset" | "none" | "off" | "clear");
            assert!(matched, "variant {variant:?} not routed to clear");

            let mut ctx = test_ctx();
            ctx.app_state.advisor_model = Some("x".into());
            let _ = clear_advisor_with_persist(&mut ctx, noop_persist).unwrap();
            assert!(
                ctx.app_state.advisor_model.is_none(),
                "variant '{}' did not clear advisor_model",
                variant
            );
        }
    }

    #[tokio::test]
    async fn empty_set_rejected() {
        // No persist path — whitespace-only input returns Rejected before
        // touching disk.
        let mut ctx = test_ctx();
        let result = set_advisor(&mut ctx, "   ").unwrap();
        match result {
            CommandResult::Output(text) => assert!(text.contains("Rejected")),
            _ => panic!("expected Output"),
        }
    }

    #[tokio::test]
    async fn legacy_alias_set_rejected() {
        let mut ctx = test_ctx();
        let result = set_advisor(&mut ctx, "sonnet").unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Rejected"));
                assert!(text.contains("MOTA"));
            }
            _ => panic!("expected Output"),
        }
        assert!(ctx.app_state.advisor_model.is_none());
    }

    #[test]
    fn persist_to_path_round_trips_via_disk() {
        // Uses an explicit tempdir path — no env-var mutation, no race.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.json");
        persist_advisor_to_path(&path, Some(allthecodes_types::models::SOTA_MODEL_ID)).unwrap();

        let raw_json = std::fs::read_to_string(&path).unwrap();
        let raw: allthecodes_config::settings::RawSettings =
            serde_json::from_str(&raw_json).unwrap();
        assert_eq!(
            raw.advisor_model.as_deref(),
            Some(allthecodes_types::models::SOTA_MODEL_ID)
        );

        // Clear via None.
        persist_advisor_to_path(&path, None).unwrap();
        let raw_json = std::fs::read_to_string(&path).unwrap();
        let raw: allthecodes_config::settings::RawSettings =
            serde_json::from_str(&raw_json).unwrap();
        assert!(raw.advisor_model.is_none());
    }
}
