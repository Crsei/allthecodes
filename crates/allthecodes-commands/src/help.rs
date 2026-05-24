//! /help command -- displays available commands and their descriptions.

use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandHandler, CommandMetadata, CommandResult};

/// Handler for the `/help` slash command.
pub struct HelpHandler;

#[async_trait]
impl CommandHandler for HelpHandler {
    async fn execute(&self, args: &str, _ctx: &mut CommandContext) -> Result<CommandResult> {
        let Some(commands) = command_metadata_snapshot() else {
            return Ok(CommandResult::Output(COMMAND_REGISTRY_UNAVAILABLE.into()));
        };

        // If a specific command name is given, show detailed help for that command.
        if !args.is_empty() {
            let target = args.trim();
            if let Some(cmd) = commands
                .iter()
                .find(|c| c.name == target || c.aliases.iter().any(|a| a == target))
            {
                let aliases = if cmd.aliases.is_empty() {
                    String::new()
                } else {
                    format!("  Aliases: {}", cmd.aliases.join(", "))
                };
                let usage = help_usage_for(&cmd.name);
                let mut text = format!("/{} -- {}\nUsage: {}", cmd.name, cmd.description, usage);
                if !aliases.is_empty() {
                    text.push('\n');
                    text.push_str(aliases.trim());
                }
                if let Some(tip) = help_tip_for(&cmd.name) {
                    text.push('\n');
                    text.push_str(tip);
                }
                return Ok(CommandResult::Output(text));
            } else {
                return Ok(CommandResult::Output(format!(
                    "Unknown command: '{}'. Type /help to see all commands.",
                    target
                )));
            }
        }

        // Build the full help listing.
        let mut lines: Vec<String> = vec![
            "Help V2".into(),
            "Quick surfaces: /resume recent, /session list, /session-export list, /export, /doctor summary, /keybindings status.".into(),
            "Keys: Ctrl+R history search, Ctrl+O transcript/global search, / opens command palette, Esc closes surfaces.".into(),
            String::new(),
            "Available commands:".into(),
            String::new(),
        ];

        // Filter out hidden commands from the listing.
        let visible: Vec<_> = commands
            .iter()
            .filter(|c| !crate::is_hidden_command(&c.name))
            .collect();

        // Find the longest command name for alignment.
        let max_len = visible.iter().map(|c| c.name.len()).max().unwrap_or(0);

        for cmd in &visible {
            let padding = " ".repeat(max_len - cmd.name.len() + 2);
            let mut line = format!("  /{}{}{}", cmd.name, padding, cmd.description);
            if !cmd.aliases.is_empty() {
                let alias_str = cmd
                    .aliases
                    .iter()
                    .map(|a| format!("/{}", a))
                    .collect::<Vec<_>>()
                    .join(", ");
                line.push_str(&format!(" ({})", alias_str));
            }
            lines.push(line);
        }

        lines.push(String::new());
        lines.push("Type /help <command> for more information about a specific command.".into());

        Ok(CommandResult::Output(lines.join("\n")))
    }
}

const COMMAND_REGISTRY_UNAVAILABLE: &str =
    "Command registry is unavailable. Wire a runtime command metadata provider before using /help.";

fn command_metadata_snapshot() -> Option<Vec<CommandMetadata>> {
    let runtime_commands = crate::runtime::command_metadata_snapshot();
    if !runtime_commands.is_empty() {
        return Some(runtime_commands);
    }

    #[cfg(test)]
    {
        Some(vec![
            CommandMetadata {
                name: "help".into(),
                aliases: vec!["h".into(), "?".into()],
                description: "Show help".into(),
            },
            CommandMetadata {
                name: "clear".into(),
                aliases: vec![],
                description: "Clear conversation".into(),
            },
            CommandMetadata {
                name: "commit".into(),
                aliases: vec![],
                description: "Create a git commit".into(),
            },
            CommandMetadata {
                name: "config".into(),
                aliases: vec!["settings".into()],
                description: "Configure settings".into(),
            },
            CommandMetadata {
                name: "diff".into(),
                aliases: vec![],
                description: "Show git diff".into(),
            },
        ])
    }

    #[cfg(not(test))]
    {
        None
    }
}

fn help_usage_for(name: &str) -> &'static str {
    match name {
        "doctor" => "/doctor [summary|raw]",
        "exit" => "/exit",
        "export" => "/export [list|path|session-id]",
        "help" => "/help [command]",
        "keybindings" => "/keybindings [open|status|list|reload|path]",
        "memory" => "/memory [show|path|edit|search|open]",
        "resume" => "/resume <session-id|recent>",
        "session" => "/session [list|list all]",
        "session-export" => "/session-export <list|session-id|path>",
        _ => "/<command> [args]",
    }
}

fn help_tip_for(name: &str) -> Option<&'static str> {
    match name {
        "doctor" => Some("Tip: diagnostics include keybinding warnings for core flows."),
        "help" => Some("Tip: use /help <command> for usage plus command-specific hints."),
        "keybindings" => {
            Some("Tip: run /keybindings status to see parse issues and core-flow warnings.")
        }
        "resume" => Some(
            "Tip: /resume recent reloads the newest saved session; Ctrl+R can reuse saved prompts.",
        ),
        "session-export" => {
            Some("Tip: /session-export list shows structured exports; /export writes Markdown.")
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
    use std::path::PathBuf;

    fn test_ctx() -> CommandContext {
        CommandContext {
            messages: Vec::new(),
            cwd: PathBuf::from("."),
            app_state: Default::default(),
            session_id: SessionId::from_string("test-session"),
        }
    }

    #[tokio::test]
    async fn test_help_lists_all_commands() {
        let handler = HelpHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("/help"));
                assert!(text.contains("/clear"));
                assert!(text.contains("/commit"));
                assert!(text.contains("/config"));
                assert!(text.contains("/diff"));
            }
            _ => panic!("Expected Output result"),
        }
    }

    #[tokio::test]
    async fn test_help_specific_command() {
        let handler = HelpHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("clear", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("/clear"));
                assert!(text.contains("Clear"));
            }
            _ => panic!("Expected Output result"),
        }
    }

    #[tokio::test]
    async fn test_help_unknown_command() {
        let handler = HelpHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("nonexistent", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Unknown command"));
            }
            _ => panic!("Expected Output result"),
        }
    }
}
