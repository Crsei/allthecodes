//! `/sandbox` slash command — inspect + toggle the sandbox.
//!
//! Usage:
//!   /sandbox                  show status
//!   /sandbox status           show status (explicit)
//!   /sandbox on               set enabled=true (mode defaults to workspace)
//!   /sandbox off              set enabled=false (mode=full)
//!   /sandbox mode <name>      switch mode (read-only | workspace | full)
//!   /sandbox require          fail if the OS-level primitive is unavailable
//!   /sandbox optional         allow Rust-level checks when primitive is missing
//!   /sandbox no-network       disable all network (network.disabled=true)
//!   /sandbox network on|off   toggle network.disabled
//!
//! Changes are applied to the in-memory command context via the session-level
//! override path — they do not persist to
//! `settings.json`. Use `/config set sandbox.*` for persistent edits.

use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandHandler, CommandResult};
use allthecodes_sandbox::{policy_from_app_state, Availability, Mechanism, SandboxMode};

pub struct SandboxHandler;

#[async_trait]
impl CommandHandler for SandboxHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let args = args.trim();
        let mut parts = args.split_whitespace();
        let sub = parts.next().unwrap_or("").to_ascii_lowercase();

        match sub.as_str() {
            "" | "status" | "show" => Ok(CommandResult::Output(render_status(ctx))),
            "on" | "enable" => {
                ctx.app_state.settings.sandbox.enabled = Some(true);
                if ctx.app_state.settings.sandbox.mode.is_none() {
                    ctx.app_state.settings.sandbox.mode =
                        Some(SandboxMode::default_enabled().as_str().to_string());
                }
                Ok(CommandResult::Output(format!(
                    "Sandbox enabled (mode={}).\n\n{}",
                    ctx.app_state
                        .settings
                        .sandbox
                        .mode
                        .clone()
                        .unwrap_or_default(),
                    render_status(ctx)
                )))
            }
            "off" | "disable" => {
                ctx.app_state.settings.sandbox.enabled = Some(false);
                Ok(CommandResult::Output(format!(
                    "Sandbox disabled.\n\n{}",
                    render_status(ctx)
                )))
            }
            "mode" => {
                let name = parts.next().unwrap_or("").trim();
                if name.is_empty() {
                    return Ok(CommandResult::Output(
                        "Usage: /sandbox mode <read-only | workspace | full>".into(),
                    ));
                }
                match name.parse::<SandboxMode>() {
                    Ok(mode) => {
                        ctx.app_state.settings.sandbox.mode = Some(mode.as_str().to_string());
                        ctx.app_state.settings.sandbox.enabled = Some(mode.is_active());
                        Ok(CommandResult::Output(format!(
                            "Sandbox mode set to '{}'.\n\n{}",
                            mode.as_str(),
                            render_status(ctx)
                        )))
                    }
                    Err(e) => Ok(CommandResult::Output(format!("Error: {}", e))),
                }
            }
            "require" | "required" | "strict" | "fail-closed" => {
                ctx.app_state.settings.sandbox.enabled = Some(true);
                ctx.app_state.settings.sandbox.fail_if_unavailable = Some(true);
                if ctx.app_state.settings.sandbox.mode.is_none() {
                    ctx.app_state.settings.sandbox.mode =
                        Some(SandboxMode::default_enabled().as_str().to_string());
                }
                Ok(CommandResult::Output(format!(
                    "Sandbox now requires an OS-level primitive.\n\n{}",
                    render_status(ctx)
                )))
            }
            "optional" | "best-effort" => {
                ctx.app_state.settings.sandbox.fail_if_unavailable = Some(false);
                Ok(CommandResult::Output(format!(
                    "Sandbox may fall back to Rust-level checks when the OS-level primitive is unavailable.\n\n{}",
                    render_status(ctx)
                )))
            }
            "no-network" | "offline" => {
                ctx.app_state.settings.sandbox.network.disabled = Some(true);
                Ok(CommandResult::Output(format!(
                    "Network disabled for this session.\n\n{}",
                    render_status(ctx)
                )))
            }
            "network" => {
                let val = parts.next().unwrap_or("").to_ascii_lowercase();
                match val.as_str() {
                    "on" | "enable" => {
                        ctx.app_state.settings.sandbox.network.disabled = Some(false);
                        Ok(CommandResult::Output(format!(
                            "Network enabled.\n\n{}",
                            render_status(ctx)
                        )))
                    }
                    "off" | "disable" => {
                        ctx.app_state.settings.sandbox.network.disabled = Some(true);
                        Ok(CommandResult::Output(format!(
                            "Network disabled.\n\n{}",
                            render_status(ctx)
                        )))
                    }
                    "" => Ok(CommandResult::Output(
                        "Usage: /sandbox network <on|off>".into(),
                    )),
                    other => Ok(CommandResult::Output(format!(
                        "Unknown subcommand '{}'. Try: /sandbox network <on|off>",
                        other
                    ))),
                }
            }
            other => Ok(CommandResult::Output(format!(
                "Unknown /sandbox subcommand '{}'.\n\nUsage:\n  \
                 /sandbox                — show status\n  \
                 /sandbox on             — enable sandbox\n  \
                 /sandbox off            — disable sandbox\n  \
                 /sandbox mode <name>    — switch mode (read-only | workspace | full)\n  \
                 /sandbox require        — fail if OS-level sandbox is unavailable\n  \
                 /sandbox optional       — allow best-effort fallback\n  \
                 /sandbox no-network     — disable network\n  \
                 /sandbox network on|off — toggle network access",
                other
            ))),
        }
    }
}

/// Render a multi-line status block for display in the REPL.
fn render_status(ctx: &CommandContext) -> String {
    let policy = policy_from_app_state(
        &ctx.app_state.tool_permission_context,
        &ctx.app_state.settings.sandbox,
        ctx.cwd.clone(),
        false,
    );
    let mut out = String::new();
    out.push_str("Sandbox status\n");
    out.push_str("──────────────\n");
    out.push_str(&format!(
        "  Enabled:  {}\n",
        if policy.enabled { "yes" } else { "no" }
    ));
    out.push_str(&format!("  Mode:     {}\n", policy.mode.as_str()));
    out.push_str(&format!(
        "  Escape:   allowUnsandboxedCommands = {}\n",
        policy.allow_unsandboxed_commands
    ));
    out.push_str(&format!(
        "  Fail if unavailable: {}\n",
        policy.fail_if_unavailable
    ));
    out.push_str(&format!(
        "  Workspace: {}\n",
        policy.paths.workspace().display()
    ));

    // Platform / OS primitive
    out.push_str("\nPlatform:\n");
    match &policy.availability {
        Availability::Available(m) => {
            out.push_str(&format!(
                "  OS-level sandbox: available via {}\n",
                mechanism_label(*m)
            ));
        }
        Availability::Unavailable { platform, reason } => {
            out.push_str(&format!(
                "  OS-level sandbox: UNAVAILABLE on {}\n    {}\n",
                platform, reason
            ));
        }
    }

    // Network summary
    out.push_str("\nNetwork:\n");
    out.push_str(&format!("  Disabled:        {}\n", policy.network.disabled));
    if policy.network.allowed_domains.is_empty() {
        out.push_str("  Allowed domains: (all, no restriction)\n");
    } else {
        out.push_str("  Allowed domains:\n");
        for d in &policy.network.allowed_domains {
            out.push_str(&format!("    - {}\n", d));
        }
    }

    // Filesystem summary
    out.push_str("\nFilesystem:\n");
    render_path_list(&mut out, "allowWrite", policy.paths.allow_write_paths());
    render_path_list(&mut out, "denyWrite", policy.paths.deny_write_paths());
    render_path_list(&mut out, "allowRead", policy.paths.allow_read_paths());
    render_path_list(&mut out, "denyRead", policy.paths.deny_read_paths());

    // Excluded / allowed commands
    if !policy.excluded_commands.is_empty() {
        out.push_str("\nExcluded commands (run outside sandbox):\n");
        for c in &policy.excluded_commands {
            out.push_str(&format!("  - {}\n", c));
        }
    }

    out
}

fn render_path_list(out: &mut String, label: &str, paths: &[std::path::PathBuf]) {
    if paths.is_empty() {
        out.push_str(&format!("  {}: (none)\n", label));
        return;
    }
    out.push_str(&format!("  {}:\n", label));
    for p in paths {
        out.push_str(&format!("    - {}\n", p.display()));
    }
}

fn mechanism_label(m: Mechanism) -> &'static str {
    m.as_str()
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
    fn make_ctx() -> CommandContext {
        CommandContext {
            messages: vec![],
            cwd: std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
            app_state: Default::default(),
            session_id: SessionId::new(),
        }
    }

    #[tokio::test]
    async fn status_shows_disabled_by_default() {
        let handler = SandboxHandler;
        let mut ctx = make_ctx();
        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(s) => {
                assert!(s.contains("Enabled:  no") || s.contains("Enabled: no"));
                assert!(s.contains("Mode:"));
            }
            _ => panic!("expected Output"),
        }
    }

    #[tokio::test]
    async fn on_enables_sandbox_and_sets_workspace_default() {
        let handler = SandboxHandler;
        let mut ctx = make_ctx();
        handler.execute("on", &mut ctx).await.unwrap();
        assert_eq!(ctx.app_state.settings.sandbox.enabled, Some(true));
        assert_eq!(
            ctx.app_state.settings.sandbox.mode.as_deref(),
            Some("workspace")
        );
    }

    #[tokio::test]
    async fn mode_accepts_read_only() {
        let handler = SandboxHandler;
        let mut ctx = make_ctx();
        handler.execute("mode read-only", &mut ctx).await.unwrap();
        assert_eq!(
            ctx.app_state.settings.sandbox.mode.as_deref(),
            Some("read-only")
        );
        assert_eq!(ctx.app_state.settings.sandbox.enabled, Some(true));
    }

    #[tokio::test]
    async fn mode_full_disables_sandbox() {
        let handler = SandboxHandler;
        let mut ctx = make_ctx();
        handler.execute("mode full", &mut ctx).await.unwrap();
        assert_eq!(ctx.app_state.settings.sandbox.enabled, Some(false));
    }

    #[tokio::test]
    async fn require_enables_fail_if_unavailable() {
        let handler = SandboxHandler;
        let mut ctx = make_ctx();
        handler.execute("require", &mut ctx).await.unwrap();
        assert_eq!(ctx.app_state.settings.sandbox.enabled, Some(true));
        assert_eq!(
            ctx.app_state.settings.sandbox.mode.as_deref(),
            Some("workspace")
        );
        assert_eq!(
            ctx.app_state.settings.sandbox.fail_if_unavailable,
            Some(true)
        );
    }

    #[tokio::test]
    async fn optional_disables_fail_if_unavailable() {
        let handler = SandboxHandler;
        let mut ctx = make_ctx();
        ctx.app_state.settings.sandbox.fail_if_unavailable = Some(true);
        handler.execute("optional", &mut ctx).await.unwrap();
        assert_eq!(
            ctx.app_state.settings.sandbox.fail_if_unavailable,
            Some(false)
        );
    }

    #[tokio::test]
    async fn no_network_toggles_disabled() {
        let handler = SandboxHandler;
        let mut ctx = make_ctx();
        handler.execute("no-network", &mut ctx).await.unwrap();
        assert_eq!(ctx.app_state.settings.sandbox.network.disabled, Some(true));
    }

    #[tokio::test]
    async fn network_on_reenables() {
        let handler = SandboxHandler;
        let mut ctx = make_ctx();
        ctx.app_state.settings.sandbox.network.disabled = Some(true);
        handler.execute("network on", &mut ctx).await.unwrap();
        assert_eq!(ctx.app_state.settings.sandbox.network.disabled, Some(false));
    }

    #[tokio::test]
    async fn unknown_subcommand_returns_usage() {
        let handler = SandboxHandler;
        let mut ctx = make_ctx();
        let result = handler.execute("bogus", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(s) => {
                assert!(s.contains("Unknown /sandbox subcommand"));
                assert!(s.contains("Usage:"));
            }
            _ => panic!("expected Output"),
        }
    }
}
