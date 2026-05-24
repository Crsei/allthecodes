//! `/keybindings` slash command — create, open, list, or reload the user
//! keybindings file.
//!
//! The default action mirrors the Bun reference: ensure the file exists
//! (creating it from an empty template if missing) and open it in the user's
//! `$VISUAL`/`$EDITOR`. Everything else is a named subcommand so callers who
//! want the old status readout keep their flow.
//!
//! Usage:
//!   /keybindings                 ensure file exists and open in editor
//!   /keybindings status          show file path + effective binding count
//!   /keybindings open            alias for the default action
//!   /keybindings list [ctx]      list effective bindings (optionally for a
//!                                context)
//!   /keybindings reload          force a reload now
//!   /keybindings path            print the config file path
//!
//! `~/.allthecodes/keybindings.json` is created from [`EMPTY_TEMPLATE`] when
//! missing. The existing file is never overwritten.

use anyhow::Result;
use async_trait::async_trait;

use crate::browser::{ensure_and_open, format_open_outcome};
use crate::{CommandContext, CommandHandler, CommandResult};
use allthecodes_config::paths;
use allthecodes_keybindings::action::Action;
use allthecodes_keybindings::config::EMPTY_TEMPLATE;
use allthecodes_keybindings::context::Context as KbContext;
use allthecodes_keybindings::registry::KeybindingRegistry;

pub struct KeybindingsHandler;

#[async_trait]
impl CommandHandler for KeybindingsHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let args = args.trim();
        let mut parts = args.split_whitespace();
        let sub = parts.next().unwrap_or("").to_ascii_lowercase();

        // Use the registry held on AppState — shared with any other UI
        // surfaces — rather than constructing a fresh one per invocation.
        let reg = ctx.app_state.keybindings.clone();

        match sub.as_str() {
            // Default: editor-first flow. Matches the Bun reference where
            // `/keybindings` creates the template (if missing) and hands the
            // user straight to their editor rather than a status readout.
            "" | "open" | "edit" => Ok(CommandResult::Output(open_for_edit(&reg))),
            "status" | "show" => Ok(CommandResult::Output(render_status(&reg))),
            "path" => Ok(CommandResult::Output(format!(
                "{}",
                paths::keybindings_path().display()
            ))),
            "reload" | "refresh" => Ok(CommandResult::Output(reload_now(&reg))),
            "list" => {
                let filter_ctx = parts.next().and_then(|s| s.parse::<KbContext>().ok());
                Ok(CommandResult::Output(list_bindings(&reg, filter_ctx)))
            }
            other => Ok(CommandResult::Output(format!(
                "Unknown /keybindings subcommand '{}'.\n\nUsage:\n  \
                 /keybindings                 — create (if missing) and open in $EDITOR\n  \
                 /keybindings status          — show status and effective binding count\n  \
                 /keybindings list [context]  — list effective bindings\n  \
                 /keybindings reload          — force a reload\n  \
                 /keybindings path            — print config file path",
                other
            ))),
        }
    }
}

fn render_status(reg: &KeybindingRegistry) -> String {
    let path = reg.user_path().unwrap_or_else(paths::keybindings_path);
    let exists = path.exists();
    reg.refresh_if_changed();
    let issues = reg.last_issues();
    let total = reg.all_bindings().len();

    let mut out = String::new();
    out.push_str("Keybindings\n");
    out.push_str("───────────\n");
    out.push_str(&format!("  Config path: {}\n", path.display()));
    out.push_str(&format!(
        "  File:        {}\n",
        if exists { "exists" } else { "not created yet" }
    ));
    out.push_str(&format!("  Effective bindings: {}\n", total));
    if !issues.is_empty() {
        out.push_str("\nConfig issues:\n");
        for i in &issues {
            out.push_str(&format!("  - {}\n", i));
        }
    }
    let warnings = critical_binding_warnings(reg);
    if !warnings.is_empty() {
        out.push_str("\nWarnings:\n");
        for warning in warnings {
            out.push_str(&format!("  - {}\n", warning));
        }
    }
    out.push_str(
        "\nTip: run `/keybindings` to create and edit the file, or \
         `/keybindings list` to see every effective binding.\n",
    );
    out
}

fn critical_binding_warnings(reg: &KeybindingRegistry) -> Vec<String> {
    [
        (KbContext::Global, "history:search", "Ctrl+R history search"),
        (KbContext::Global, "app:exit", "exit flow"),
        (
            KbContext::HistorySearch,
            "historySearch:accept",
            "history accept",
        ),
    ]
    .into_iter()
    .filter_map(|(ctx, action, label)| {
        let present = reg
            .bindings_for(&Action::new_static(action))
            .iter()
            .any(|(binding_ctx, _)| *binding_ctx == ctx);
        (!present).then(|| format!("{label} has no {ctx} binding for {action}"))
    })
    .collect()
}

fn open_for_edit(reg: &KeybindingRegistry) -> String {
    let path = reg.user_path().unwrap_or_else(paths::keybindings_path);
    let outcome = ensure_and_open(&path, EMPTY_TEMPLATE);
    format_open_outcome(&outcome, &path)
}

fn reload_now(reg: &KeybindingRegistry) -> String {
    let path = reg.user_path().unwrap_or_else(paths::keybindings_path);
    match reg.reload() {
        Ok(()) => {
            let issues = reg.last_issues();
            if issues.is_empty() {
                format!(
                    "Reloaded {} — {} effective bindings.",
                    path.display(),
                    reg.all_bindings().len()
                )
            } else {
                let mut out = format!(
                    "Reloaded {} with {} issue(s):\n",
                    path.display(),
                    issues.len()
                );
                for i in issues {
                    out.push_str(&format!("  - {}\n", i));
                }
                out
            }
        }
        Err(e) => format!("Error: {}", e),
    }
}

fn list_bindings(reg: &KeybindingRegistry, filter: Option<KbContext>) -> String {
    reg.refresh_if_changed();
    let mut bindings = reg.all_bindings();
    if let Some(ctx) = filter {
        bindings.retain(|(c, _, _)| *c == ctx);
    }
    if bindings.is_empty() {
        return "(no bindings)".to_string();
    }
    let mut out = String::new();
    let mut current_ctx: Option<KbContext> = None;
    for (ctx, chord, action) in bindings {
        if current_ctx != Some(ctx) {
            if current_ctx.is_some() {
                out.push('\n');
            }
            out.push_str(&format!("[{}]\n", ctx.as_str()));
            current_ctx = Some(ctx);
        }
        out.push_str(&format!("  {:<20} → {}\n", chord.display(), action));
    }
    out
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
    async fn status_subcommand_renders_path_line() {
        let handler = KeybindingsHandler;
        let mut ctx = make_ctx();
        let result = handler.execute("status", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(s) => {
                assert!(s.contains("Config path:"));
                assert!(s.contains("Effective bindings:"));
            }
            _ => panic!("expected Output"),
        }
    }

    #[tokio::test]
    async fn default_action_is_open_for_edit() {
        // Default `/keybindings` should either open the file in an editor or,
        // when no editor is set, return a path-pointing message — never the
        // old status readout.
        let handler = KeybindingsHandler;
        let mut ctx = make_ctx();
        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(s) => {
                // `render_status` output contains the literal string
                // "Effective bindings:" which the editor-first flow never
                // emits. This is the discriminator we care about.
                assert!(
                    !s.contains("Effective bindings:"),
                    "default should not show status readout, got: {}",
                    s
                );
                assert!(
                    s.contains("keybindings.json") || s.contains("Opened") || s.contains("Created"),
                    "expected editor-first output, got: {}",
                    s
                );
            }
            _ => panic!("expected Output"),
        }
    }

    #[tokio::test]
    async fn path_subcommand_returns_absolute_path() {
        let handler = KeybindingsHandler;
        let mut ctx = make_ctx();
        let result = handler.execute("path", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(s) => {
                assert!(s.contains("keybindings.json"));
            }
            _ => panic!("expected Output"),
        }
    }

    #[tokio::test]
    async fn list_subcommand_emits_context_headers() {
        let handler = KeybindingsHandler;
        let mut ctx = make_ctx();
        let result = handler.execute("list Chat", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(s) => {
                assert!(s.contains("[Chat]"));
                // Chat bindings contain chat:submit
                assert!(s.contains("chat:submit"));
            }
            _ => panic!("expected Output"),
        }
    }

    #[tokio::test]
    async fn unknown_subcommand_returns_usage() {
        let handler = KeybindingsHandler;
        let mut ctx = make_ctx();
        let result = handler.execute("bogus", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(s) => {
                assert!(s.contains("Unknown /keybindings"));
                assert!(s.contains("Usage:"));
            }
            _ => panic!("expected Output"),
        }
    }
}
