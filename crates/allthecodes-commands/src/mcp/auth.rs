use anyhow::Result;

use super::flags::parse_auth_complete_args;
use super::{CommandContext, CommandResult};
use allthecodes_mcp::oauth_login::McpOAuthLoginHandle;
use allthecodes_mcp::McpServerConfig;

pub(super) async fn handle_auth(rest: &[&str], ctx: &CommandContext) -> Result<CommandResult> {
    match rest.first().copied() {
        Some("start") => {
            let Some(name) = rest.get(1) else {
                return Ok(CommandResult::Output(
                    "Usage: /mcp auth start <name>".to_string(),
                ));
            };
            let config = match find_mcp_config(&ctx.cwd, name) {
                Ok(config) => config,
                Err(message) => return Ok(CommandResult::Output(message)),
            };
            match allthecodes_mcp::oauth_login::start_auto_authorization(&config).await {
                Ok((handle, start)) => {
                    spawn_oauth_wait_task(config.name.clone(), handle);
                    Ok(CommandResult::Output(format!(
                    "OAuth authorization started for MCP server `{}`.\n\
                     Open this URL in a browser:\n{}\n\n\
                     Redirect URI: {}\n\n\
                     allthecodes is waiting for the browser callback and will store credentials automatically.\n\
                     If the callback cannot be received but the provider shows a code, run:\n\
                     /mcp auth complete {} --code=<code> --state={}\n\
                     Token store: {}",
                    config.name,
                    start.authorization_url,
                    start.redirect_uri,
                    config.name,
                    start.state,
                    start.token_store_path.display()
                    )))
                }
                Err(err) => Ok(CommandResult::Output(format!(
                    "Failed to start OAuth for MCP server `{}`: {}",
                    config.name, err
                ))),
            }
        }
        Some("complete") => {
            let Some(name) = rest.get(1) else {
                return Ok(CommandResult::Output(
                    "Usage: /mcp auth complete <name> --code=<code> [--state=<state>]".to_string(),
                ));
            };
            let parsed = parse_auth_complete_args(&rest[2..]);
            if let Some(error) = parsed.error {
                return Ok(CommandResult::Output(format!(
                    "{}\n\nUsage: /mcp auth complete <name> --code=<code> [--state=<state>]",
                    error
                )));
            }
            let Some(code) = parsed.code else {
                return Ok(CommandResult::Output(
                    "Usage: /mcp auth complete <name> --code=<code> [--state=<state>]".to_string(),
                ));
            };
            let config = match find_mcp_config(&ctx.cwd, name) {
                Ok(config) => config,
                Err(message) => return Ok(CommandResult::Output(message)),
            };
            match allthecodes_mcp::auth::complete_authorization(
                &config,
                &code,
                parsed.state.as_deref(),
            )
            .await
            {
                Ok(_) => Ok(CommandResult::Output(format!(
                    "Stored OAuth credentials for MCP server `{}` in {}. Access token: {}",
                    config.name,
                    allthecodes_mcp::auth::token_store_path().display(),
                    allthecodes_mcp::auth::redact_secret(&code)
                ))),
                Err(err) => Ok(CommandResult::Output(format!(
                    "Failed to complete OAuth for MCP server `{}`: {}",
                    config.name, err
                ))),
            }
        }
        Some("clear") => {
            let Some(name) = rest.get(1) else {
                return Ok(CommandResult::Output(
                    "Usage: /mcp auth clear <name>".to_string(),
                ));
            };
            let config = match find_mcp_config(&ctx.cwd, name) {
                Ok(config) => config,
                Err(message) => return Ok(CommandResult::Output(message)),
            };
            match allthecodes_mcp::auth::clear_stored_token(&config) {
                Ok(true) => Ok(CommandResult::Output(format!(
                    "Cleared OAuth credentials for MCP server `{}`.",
                    config.name
                ))),
                Ok(false) => Ok(CommandResult::Output(format!(
                    "No stored OAuth credentials found for MCP server `{}`.",
                    config.name
                ))),
                Err(err) => Ok(CommandResult::Output(format!(
                    "Failed to clear OAuth credentials for MCP server `{}`: {}",
                    config.name, err
                ))),
            }
        }
        Some("status") => {
            let Some(name) = rest.get(1) else {
                return Ok(CommandResult::Output(
                    "Usage: /mcp auth status <name>".to_string(),
                ));
            };
            let config = match find_mcp_config(&ctx.cwd, name) {
                Ok(config) => config,
                Err(message) => return Ok(CommandResult::Output(message)),
            };
            match allthecodes_mcp::auth::credential_status(&config) {
                Ok(status) => Ok(CommandResult::Output(format!(
                    "OAuth status for MCP server `{}`: status={} configured={} authorized={} expired={} refreshable={}{}\nToken store: {}",
                    config.name,
                    status.status.as_str(),
                    status.configured,
                    status.authorized,
                    status.expired,
                    status.can_refresh,
                    status
                        .message
                        .as_deref()
                        .map(|message| format!(" message={message}"))
                        .unwrap_or_default(),
                    status.token_store_path.display()
                ))),
                Err(err) => Ok(CommandResult::Output(format!(
                    "Failed to read OAuth status for MCP server `{}`: {}",
                    config.name, err
                ))),
            }
        }
        _ => Ok(CommandResult::Output(
            "Usage: /mcp auth start|complete|status|clear <name>".to_string(),
        )),
    }
}

fn find_mcp_config(cwd: &std::path::Path, server_name: &str) -> Result<McpServerConfig, String> {
    let configs = allthecodes_mcp::discovery::discover_mcp_servers(cwd)
        .map_err(|err| format!("Failed to discover MCP servers: {err}"))?;
    configs
        .into_iter()
        .find(|cfg| cfg.name == server_name)
        .ok_or_else(|| format!("No MCP server named `{}` found.", server_name))
}

fn spawn_oauth_wait_task(server_name: String, handle: McpOAuthLoginHandle) {
    tokio::spawn(async move {
        match handle.wait().await {
            Ok(_) => tracing::info!(
                server = %server_name,
                "MCP OAuth callback completed and credentials were stored"
            ),
            Err(err) => tracing::warn!(
                server = %server_name,
                error = %err,
                "MCP OAuth callback did not complete"
            ),
        }
    });
}
