use std::collections::HashMap;

use anyhow::Result;

use super::settings::{describe_entry, discover_config_entries};
use super::{CommandContext, CommandResult};
use allthecodes_ipc_protocol::subsystem_types::{
    McpServerConfigEntry, McpServerInfoBrief, McpServerStatusInfo,
};

// ---------------------------------------------------------------------------
// list / status
// ---------------------------------------------------------------------------

pub(super) async fn handle_list(ctx: &CommandContext) -> Result<CommandResult> {
    let entries = discover_config_entries(&ctx.cwd);
    let status = build_status_from_discovery(&ctx.cwd).await;

    if entries.is_empty() {
        return Ok(CommandResult::Output(
            "No MCP servers discovered.\n\n\
             Add servers to ~/.allthecodes/settings.json or .allthecodes/settings.json, or run:\n  \
               /mcp add <name> --command=<cmd> [--arg=<arg> …]"
                .to_string(),
        ));
    }

    let browser_count = entries
        .iter()
        .filter(|e| {
            e.browser_mcp.unwrap_or(false)
                || allthecodes_browser::detection::is_browser_server(&e.name)
        })
        .count();

    let mut lines = Vec::new();
    if browser_count > 0 {
        lines.push(format!(
            "Discovered MCP servers ({}; {} browser):",
            entries.len(),
            browser_count
        ));
    } else {
        lines.push(format!("Discovered MCP servers ({}):", entries.len()));
    }
    lines.push(String::new());

    // Group by scope label for readability.
    let mut by_scope: Vec<(String, Vec<&McpServerConfigEntry>)> = Vec::new();
    for entry in &entries {
        let label = entry.scope.label();
        if let Some(bucket) = by_scope.iter_mut().find(|(l, _)| *l == label) {
            bucket.1.push(entry);
        } else {
            by_scope.push((label, vec![entry]));
        }
    }

    for (label, bucket) in &by_scope {
        lines.push(format!("[{}]", label));
        for entry in bucket {
            let state = status
                .iter()
                .find(|s| s.name == entry.name)
                .map(|s| s.state.clone())
                .unwrap_or_else(|| "unknown".to_string());
            let desc = describe_entry(entry);
            let tag = if entry.browser_mcp.unwrap_or(false)
                || allthecodes_browser::detection::is_browser_server(&entry.name)
            {
                " [browser]"
            } else {
                ""
            };
            lines.push(format!("  {}{} -- {} -- {}", entry.name, tag, state, desc));
        }
        lines.push(String::new());
    }

    if browser_count > 0 {
        lines.push(
            "Browser-tagged servers expose browser-automation tools (navigate, \
             read_page, click, -. See development/reference/browser-mcp-config.md."
                .to_string(),
        );
    }

    Ok(CommandResult::Output(
        lines.join("\n").trim_end().to_string(),
    ))
}

pub(super) async fn handle_status(ctx: &CommandContext) -> Result<CommandResult> {
    let status = build_status_from_discovery(&ctx.cwd).await;
    if status.is_empty() {
        return Ok(CommandResult::Output(
            "No MCP servers discovered.".to_string(),
        ));
    }

    let mut lines = Vec::new();
    lines.push(format!("MCP server status ({}):", status.len()));
    lines.push(String::new());
    for info in &status {
        let err = info
            .error
            .as_ref()
            .map(|e| format!(" -- {}", e))
            .unwrap_or_default();
        let detail = format_status_detail(info);
        lines.push(format!(
            "  {} -- {} ({} tools, {} resources){}{}",
            info.name, info.state, info.tools_count, info.resources_count, detail, err
        ));
        if !info.stderr_tail.is_empty() {
            lines.push(format!(
                "    stderr: {}",
                info.stderr_tail.last().cloned().unwrap_or_default()
            ));
        }
    }
    Ok(CommandResult::Output(lines.join("\n")))
}

fn format_status_detail(info: &McpServerStatusInfo) -> String {
    let mut fields = Vec::new();
    if let Some(kind) = info.last_error_kind.as_deref() {
        fields.push(format!("error_kind={kind}"));
    }
    if let Some(count) = info.failure_count {
        fields.push(format!("failures={count}"));
    }
    if let Some(count) = info.connect_attempt_count {
        fields.push(format!("attempts={count}"));
    }
    if let Some(count) = info.retry_scheduled_count {
        fields.push(format!("retries_scheduled={count}"));
    }
    if let Some(count) = info.retry_exhausted_count {
        fields.push(format!("retries_exhausted={count}"));
    }
    if let Some(count) = info.recovered_count {
        fields.push(format!("recovered={count}"));
    }
    if let Some(next_retry_at) = info.next_retry_at {
        fields.push(format!("next_retry_at={next_retry_at}"));
    }
    if let Some(count) = info.stderr_tail_dropped_line_count {
        fields.push(format!("stderr_dropped={count}"));
    }
    if fields.is_empty() {
        String::new()
    } else {
        format!(" [{}]", fields.join(", "))
    }
}

async fn build_status_from_discovery(cwd: &std::path::Path) -> Vec<McpServerStatusInfo> {
    let configs = match allthecodes_mcp::discovery::discover_mcp_servers(cwd) {
        Ok(configs) => configs,
        Err(err) => {
            return vec![McpServerStatusInfo {
                name: "discovery".to_string(),
                state: "error".to_string(),
                transport: "settings".to_string(),
                tools_count: 0,
                resources_count: 0,
                server_info: None,
                instructions: None,
                error: Some(format!("Failed to discover MCP servers: {err:#}")),
                ..Default::default()
            }];
        }
    };

    let manager_handle = allthecodes_mcp::runtime::current_manager();
    let manager = match manager_handle.as_ref() {
        Some(manager) => Some(manager.lock().await),
        None => None,
    };
    let health_by_name = manager.as_ref().map(|manager| {
        manager
            .health_snapshots()
            .into_iter()
            .map(|snapshot| (snapshot.server_name.clone(), snapshot))
            .collect::<HashMap<_, _>>()
    });

    configs
        .into_iter()
        .map(|cfg| {
            let health = health_by_name
                .as_ref()
                .and_then(|health| health.get(&cfg.name));
            if cfg.disabled.unwrap_or(false) {
                return McpServerStatusInfo {
                    name: cfg.name,
                    state: "disabled".to_string(),
                    transport: cfg.transport,
                    tools_count: 0,
                    resources_count: 0,
                    server_info: None,
                    instructions: None,
                    error: None,
                    ..mcp_health_defaults(health)
                };
            }

            if let Some(client) = manager.as_ref().and_then(|m| m.clients.get(&cfg.name)) {
                let (state, error) = match &client.state {
                    allthecodes_mcp::McpConnectionState::Pending => ("pending".to_string(), None),
                    allthecodes_mcp::McpConnectionState::Connected => {
                        ("connected".to_string(), None)
                    }
                    allthecodes_mcp::McpConnectionState::Disconnected => {
                        ("disconnected".to_string(), None)
                    }
                    allthecodes_mcp::McpConnectionState::Error(error) => {
                        ("error".to_string(), Some(error.clone()))
                    }
                };
                let server_info =
                    (!client.server_info.name.is_empty()).then(|| McpServerInfoBrief {
                        name: client.server_info.name.clone(),
                        version: client.server_info.version.clone(),
                    });
                return McpServerStatusInfo {
                    name: cfg.name,
                    state,
                    transport: cfg.transport,
                    tools_count: client.tools.len(),
                    resources_count: client.resources.len(),
                    server_info,
                    instructions: client.instructions.clone(),
                    error: error.or_else(|| health.and_then(|health| health.last_error.clone())),
                    ..mcp_health_defaults(health)
                };
            }

            let remembered = allthecodes_mcp::runtime::server_state(&cfg.name);
            let remembered_state = remembered.as_ref().map(|state| state.state.clone());
            let remembered_error = remembered.and_then(|state| state.error);
            McpServerStatusInfo {
                name: cfg.name,
                state: health
                    .map(|health| health.state.clone())
                    .or(remembered_state)
                    .unwrap_or_else(|| "pending".to_string()),
                transport: cfg.transport,
                tools_count: health.and_then(|health| health.tools_count).unwrap_or(0),
                resources_count: health
                    .and_then(|health| health.resources_count)
                    .unwrap_or(0),
                server_info: None,
                instructions: None,
                error: health
                    .and_then(|health| health.last_error.clone())
                    .or(remembered_error),
                ..mcp_health_defaults(health)
            }
        })
        .collect()
}

fn mcp_health_defaults(
    health: Option<&allthecodes_mcp::McpServerHealthSnapshot>,
) -> McpServerStatusInfo {
    let Some(health) = health else {
        return McpServerStatusInfo::default();
    };

    McpServerStatusInfo {
        last_success_at: health.last_success_at,
        last_attempt_at: health.last_attempt_at,
        last_error_kind: health.last_error_kind.clone(),
        failure_count: (health.failure_count > 0).then_some(health.failure_count),
        connect_attempt_count: Some(health.connect_attempt_count),
        retry_scheduled_count: Some(health.retry_scheduled_count),
        retry_exhausted_count: Some(health.retry_exhausted_count),
        recovered_count: Some(health.recovered_count),
        next_retry_at: health.next_retry_at,
        stderr_tail: health.stderr_tail.clone(),
        stderr_tail_dropped_line_count: Some(health.stderr_tail_dropped_line_count),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_detail_includes_health_fields() {
        let detail = format_status_detail(&McpServerStatusInfo {
            last_error_kind: Some("spawn_failed".to_string()),
            failure_count: Some(3),
            connect_attempt_count: Some(3),
            retry_scheduled_count: Some(2),
            retry_exhausted_count: Some(1),
            recovered_count: Some(0),
            next_retry_at: Some(12345),
            stderr_tail_dropped_line_count: Some(4),
            ..Default::default()
        });

        assert!(detail.contains("error_kind=spawn_failed"));
        assert!(detail.contains("failures=3"));
        assert!(detail.contains("attempts=3"));
        assert!(detail.contains("retries_scheduled=2"));
        assert!(detail.contains("retries_exhausted=1"));
        assert!(detail.contains("recovered=0"));
        assert!(detail.contains("next_retry_at=12345"));
        assert!(detail.contains("stderr_dropped=4"));
    }
}
