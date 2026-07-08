use anyhow::Result;

use super::{CommandContext, CommandResult};

// ---------------------------------------------------------------------------
// connect / disconnect / reconnect
// ---------------------------------------------------------------------------

pub(super) async fn handle_connect(rest: &[&str], ctx: &CommandContext) -> Result<CommandResult> {
    match rest.first() {
        Some(name) => {
            let report =
                run_mcp_runtime_operation(&ctx.cwd, McpRuntimeOperation::Connect, name).await;
            Ok(CommandResult::Output(report.text))
        }
        None => Ok(CommandResult::Output(
            "Usage: /mcp connect <name>".to_string(),
        )),
    }
}

pub(super) async fn handle_disconnect(
    rest: &[&str],
    ctx: &CommandContext,
) -> Result<CommandResult> {
    match rest.first() {
        Some(name) => {
            let report =
                run_mcp_runtime_operation(&ctx.cwd, McpRuntimeOperation::Disconnect, name).await;
            Ok(CommandResult::Output(report.text))
        }
        None => Ok(CommandResult::Output(
            "Usage: /mcp disconnect <name>".to_string(),
        )),
    }
}

pub(super) async fn handle_reconnect(rest: &[&str], ctx: &CommandContext) -> Result<CommandResult> {
    match rest.first() {
        Some(name) => {
            let report =
                run_mcp_runtime_operation(&ctx.cwd, McpRuntimeOperation::Reconnect, name).await;
            Ok(CommandResult::Output(report.text))
        }
        None => Ok(CommandResult::Output(
            "Usage: /mcp reconnect <name>".to_string(),
        )),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum McpRuntimeOperation {
    Connect,
    Disconnect,
    Reconnect,
}

impl McpRuntimeOperation {
    fn verb(self) -> &'static str {
        match self {
            Self::Connect => "connect",
            Self::Disconnect => "disconnect",
            Self::Reconnect => "reconnect",
        }
    }

    fn past_tense(self) -> &'static str {
        match self {
            Self::Connect => "Connected",
            Self::Disconnect => "Disconnected",
            Self::Reconnect => "Reconnected",
        }
    }
}

struct McpRuntimeReport {
    text: String,
}

// MCP runtime operations mutate the shared manager and its live clients; keep
// the manager guard across the async operation to preserve existing sequencing.
#[allow(clippy::await_holding_invalid_type)]
async fn run_mcp_runtime_operation(
    cwd: &std::path::Path,
    operation: McpRuntimeOperation,
    server_name: &str,
) -> McpRuntimeReport {
    let Some(manager) = allthecodes_mcp::runtime::current_manager() else {
        return report(format!(
            "Cannot {} MCP server `{}` because the runtime manager is not available.",
            operation.verb(),
            server_name
        ));
    };

    if operation == McpRuntimeOperation::Disconnect {
        let had_client = {
            let mut manager = manager.lock().await;
            manager.disconnect_server(server_name).await
        };
        let text = if had_client {
            format!("Disconnected MCP server `{}`.", server_name)
        } else {
            format!(
                "MCP server `{}` had no active connection; marked disconnected.",
                server_name
            )
        };
        return report(text);
    }

    let config = match find_mcp_runtime_config(cwd, server_name) {
        Ok(config) => config,
        Err(message) => return report(message),
    };
    let disabled = config.disabled.unwrap_or(false);
    let result = {
        let mut manager = manager.lock().await;
        match operation {
            McpRuntimeOperation::Connect => manager.connect_server(config).await,
            McpRuntimeOperation::Reconnect => manager.reconnect_server(config).await,
            McpRuntimeOperation::Disconnect => unreachable!("disconnect handled above"),
        }
    };

    match result {
        Ok(()) if disabled => report(format!(
            "MCP server `{}` is disabled in settings; no live client was kept.",
            server_name
        )),
        Ok(()) => report(format!(
            "{} MCP server `{}`.",
            operation.past_tense(),
            server_name
        )),
        Err(err) => report(format!(
            "Failed to {} MCP server `{}`: {}",
            operation.verb(),
            server_name,
            err
        )),
    }
}

fn find_mcp_runtime_config(
    cwd: &std::path::Path,
    server_name: &str,
) -> Result<allthecodes_mcp::McpServerConfig, String> {
    allthecodes_mcp::discovery::discover_mcp_servers(cwd)
        .map_err(|err| format!("Failed to discover MCP servers: {err}"))?
        .into_iter()
        .find(|config| config.name == server_name)
        .ok_or_else(|| format!("No MCP server named `{}` found.", server_name))
}

fn report(text: String) -> McpRuntimeReport {
    McpRuntimeReport { text }
}
