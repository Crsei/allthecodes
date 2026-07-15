//! `/mcp` - MCP server management (issue #44).
//!
//! The subcommand implementations live in focused sibling modules so the command
//! surface stays limited to dispatch and `/help` routing.

mod auth;
mod bindings;
mod config;
mod flags;
mod help;
mod list_status;
mod runtime;
mod search;
mod settings;

#[cfg(test)]
mod tests;

use anyhow::Result;
use async_trait::async_trait;

use super::{CommandContext, CommandHandler, CommandResult};
use auth::handle_auth;
use bindings::{handle_bind, handle_bindings, handle_unbind};
use config::{handle_add, handle_edit, handle_mcpjson_decision, handle_remove};
#[cfg(test)]
use flags::parse_flags;
use help::help_text;
use list_status::{handle_list, handle_status};
use runtime::{handle_connect, handle_disconnect, handle_reconnect};
use search::handle_search;

/// Handler for the `/mcp` slash command.
pub struct McpHandler;

#[async_trait]
impl CommandHandler for McpHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let parts: Vec<&str> = args.split_whitespace().collect();
        match parts.first().copied() {
            None => Ok(CommandResult::Output(help_text())),
            Some("help") | Some("-h") | Some("--help") => Ok(CommandResult::Output(help_text())),
            Some("list") | Some("ls") => handle_list(ctx).await,
            Some("status") => handle_status(ctx).await,
            Some("search") => handle_search(args, &ctx.cwd),
            Some("add") => handle_add(&parts[1..], ctx),
            Some("edit") | Some("update") => handle_edit(&parts[1..], ctx),
            Some("remove") | Some("rm") | Some("delete") => handle_remove(&parts[1..], ctx),
            Some("bindings") => handle_bindings(ctx),
            Some("bind") => handle_bind(&parts[1..], ctx),
            Some("unbind") => handle_unbind(&parts[1..], ctx),
            Some("approve") | Some("enable") => handle_mcpjson_decision(&parts[1..], ctx, true),
            Some("reject") | Some("disable") => handle_mcpjson_decision(&parts[1..], ctx, false),
            Some("connect") => handle_connect(&parts[1..], ctx).await,
            Some("disconnect") => handle_disconnect(&parts[1..], ctx).await,
            Some("reconnect") => handle_reconnect(&parts[1..], ctx).await,
            Some("auth") => handle_auth(&parts[1..], ctx).await,
            Some(sub) => Ok(CommandResult::Output(format!(
                "Unknown mcp subcommand: '{}'.\n\n{}",
                sub,
                help_text()
            ))),
        }
    }
}
