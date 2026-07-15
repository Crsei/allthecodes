//! `/kairos` command -- configure and control the local KAIROS runtime.

use allthecodes_types::kairos::{
    KairosConfigScope, KairosControlAction, KairosControlRequest, KairosFeatureProfilePatch,
};
use anyhow::{Context, Result};
use async_trait::async_trait;

use crate::daemon_cmd::{
    configure_kairos, control_kairos, format_kairos_snapshot, kairos_snapshot, DaemonCmdHandler,
};
use crate::{CommandContext, CommandHandler, CommandResult};

pub struct KairosCmdHandler;

#[async_trait]
impl CommandHandler for KairosCmdHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let parts: Vec<&str> = args.split_whitespace().collect();
        match parts.as_slice() {
            [] | ["status"] => status(ctx),
            ["start"] => control(KairosControlAction::Start, ctx).await,
            ["stop"] => control(KairosControlAction::Stop, ctx).await,
            ["restart"] => control(KairosControlAction::Restart, ctx).await,
            ["reconcile"] | ["apply"] => control(KairosControlAction::Reconcile, ctx).await,
            ["enable", rest @ ..] => configure_enabled(true, rest, ctx).await,
            ["disable", rest @ ..] => configure_enabled(false, rest, ctx).await,
            ["feature", feature, state, rest @ ..] => {
                configure_feature(feature, state, rest, ctx).await
            }
            ["bridge", rest @ ..] => {
                let args = format!("bridge {}", rest.join(" "));
                DaemonCmdHandler.execute(args.trim(), ctx).await
            }
            ["help"] | ["--help"] | ["-h"] => Ok(CommandResult::Output(usage().to_string())),
            _ => Ok(CommandResult::Output(format!(
                "Unknown KAIROS command: '{}'\n{}",
                args.trim(),
                usage()
            ))),
        }
    }
}

fn status(ctx: &CommandContext) -> Result<CommandResult> {
    let snapshot = kairos_snapshot(&ctx.cwd)?;
    Ok(CommandResult::Output(format_kairos_snapshot(&snapshot)))
}

async fn control(action: KairosControlAction, ctx: &CommandContext) -> Result<CommandResult> {
    let result = control_kairos(KairosControlRequest {
        action,
        cwd: Some(ctx.cwd.display().to_string()),
        ..Default::default()
    })
    .await?;
    Ok(CommandResult::Output(format!(
        "KAIROS {:?}: {}\n{}",
        result.action,
        if result.changed {
            "changed"
        } else {
            "unchanged"
        },
        format_kairos_snapshot(&result.snapshot)
    )))
}

async fn configure_enabled(
    enabled: bool,
    args: &[&str],
    ctx: &CommandContext,
) -> Result<CommandResult> {
    let (scope, apply) = parse_options(args, if enabled { "--start" } else { "--stop" })?;
    let snapshot = configure_kairos(
        &ctx.cwd,
        scope,
        &KairosFeatureProfilePatch {
            enabled: Some(enabled),
            ..Default::default()
        },
    )?;
    if apply {
        let action = if enabled {
            KairosControlAction::Reconcile
        } else {
            KairosControlAction::Stop
        };
        return control(action, ctx)
            .await
            .context("KAIROS configuration was saved, but applying it failed");
    }
    Ok(CommandResult::Output(format!(
        "KAIROS configuration saved ({scope:?}).\n{}",
        format_kairos_snapshot(&snapshot)
    )))
}

async fn configure_feature(
    feature: &str,
    state: &str,
    args: &[&str],
    ctx: &CommandContext,
) -> Result<CommandResult> {
    let enabled = parse_on_off(state)?;
    let (scope, apply) = parse_options(args, "--apply")?;
    let mut patch = KairosFeatureProfilePatch::default();
    match feature.to_ascii_lowercase().as_str() {
        "brief" => patch.brief = Some(enabled),
        "channels" => patch.channels = Some(enabled),
        "push" | "push-notifications" | "push_notifications" => {
            patch.push_notifications = Some(enabled)
        }
        "github" | "github-webhooks" | "github_webhooks" => {
            patch.github_webhooks = Some(enabled)
        }
        "proactive" => patch.proactive = Some(enabled),
        other => anyhow::bail!(
            "unknown KAIROS feature '{other}'; expected brief, channels, push-notifications, github-webhooks, or proactive"
        ),
    }
    let snapshot = configure_kairos(&ctx.cwd, scope, &patch)?;
    if apply {
        return control(KairosControlAction::Reconcile, ctx)
            .await
            .context("KAIROS feature was saved, but applying it failed");
    }
    Ok(CommandResult::Output(format!(
        "KAIROS feature {feature}={} saved ({scope:?}).\n{}",
        if enabled { "on" } else { "off" },
        format_kairos_snapshot(&snapshot)
    )))
}

fn parse_options(args: &[&str], apply_flag: &str) -> Result<(KairosConfigScope, bool)> {
    let mut scope = KairosConfigScope::Local;
    let mut apply = false;
    let mut index = 0;
    while index < args.len() {
        match args[index] {
            value if value == apply_flag => apply = true,
            "--scope" => {
                index += 1;
                let value = args.get(index).context("--scope requires a value")?;
                scope = parse_scope(value)?;
            }
            value if value.starts_with("--scope=") => {
                scope = parse_scope(value.trim_start_matches("--scope="))?;
            }
            other => anyhow::bail!("unknown KAIROS option '{other}'"),
        }
        index += 1;
    }
    Ok((scope, apply))
}

fn parse_scope(value: &str) -> Result<KairosConfigScope> {
    match value.to_ascii_lowercase().as_str() {
        "local" => Ok(KairosConfigScope::Local),
        "project" => Ok(KairosConfigScope::Project),
        "user" => Ok(KairosConfigScope::User),
        _ => anyhow::bail!("invalid KAIROS scope '{value}'; expected local, project, or user"),
    }
}

fn parse_on_off(value: &str) -> Result<bool> {
    match value.to_ascii_lowercase().as_str() {
        "on" | "true" | "enable" | "enabled" => Ok(true),
        "off" | "false" | "disable" | "disabled" => Ok(false),
        _ => anyhow::bail!("invalid feature state '{value}'; expected on or off"),
    }
}

fn usage() -> &'static str {
    "Usage:\n  \
       /kairos status\n  \
       /kairos enable [--scope local|project|user] [--start]\n  \
       /kairos disable [--scope local|project|user] [--stop]\n  \
       /kairos start|stop|restart|reconcile\n  \
       /kairos feature <brief|channels|push-notifications|github-webhooks|proactive> <on|off> [--scope ...] [--apply]\n  \
       /kairos bridge <sessions|status|resume|new|release>"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_scope_and_apply_options() {
        assert_eq!(
            parse_options(&["--scope", "project", "--start"], "--start").unwrap(),
            (KairosConfigScope::Project, true)
        );
        assert_eq!(
            parse_options(&["--scope=user"], "--start").unwrap(),
            (KairosConfigScope::User, false)
        );
    }

    #[test]
    fn explicit_off_is_supported() {
        assert!(!parse_on_off("off").unwrap());
        assert!(parse_on_off("on").unwrap());
    }

    #[test]
    fn usage_lists_direct_start_and_feature_controls() {
        let text = usage();
        assert!(text.contains("/kairos start|stop|restart"));
        assert!(text.contains("push-notifications"));
    }
}
