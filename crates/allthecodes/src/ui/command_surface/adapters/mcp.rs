use std::path::Path;

use crate::ui::command_surface::CommandSurfaceOutcome;
use crate::ui::mcp::index::{McpServer, McpServerKind, McpServerStatus, McpTool};
use crate::ui::mcp::mcp_list_panel::McpListPanelState;
use allthecodes_ipc_protocol::subsystem_types::McpServerStatusInfo;
use allthecodes_types::mcp::McpBinding;

pub(crate) fn build_mcp_servers(cwd: &Path) -> Vec<McpServer> {
    crate::app_runtime_adapters::ensure_installed();
    let entries = allthecodes_ipc::subsystem_handlers::build_mcp_server_config_entries(cwd);
    let status = allthecodes_ipc::subsystem_handlers::build_mcp_server_info_list();
    let discovered = allthecodes_mcp::discovery::discover_bound_mcp_servers(cwd, None).ok();
    let bindings = runtime_bindings().unwrap_or_else(|| {
        discovered
            .as_ref()
            .map(|found| found.bindings.clone())
            .unwrap_or_default()
    });
    entries
        .into_iter()
        .map(|entry| {
            let live = status.iter().find(|item| item.name == entry.name);
            let source_label = entry.scope.label();
            let server_id = discovered
                .as_ref()
                .and_then(|found| {
                    found
                        .servers
                        .iter()
                        .find(|server| {
                            server.display_name == entry.name && server.source_scope == source_label
                        })
                        .map(|server| server.server_id.clone())
                })
                .unwrap_or_else(|| entry.name.clone());
            let kind = match entry.transport.as_str() {
                "sse" | "streamable-http" => McpServerKind::Remote,
                _ => McpServerKind::Stdio,
            };
            let mut server = McpServer::new(entry.name.clone(), kind);
            server.config_source = Some(source_label.clone());
            if let Some(binding) = binding_for_server(&bindings, &server_id, &source_label) {
                server.binding_scope = Some(binding.scope.as_str().to_string());
                server.binding_permissions = binding
                    .effective_permissions()
                    .into_iter()
                    .map(|permission| permission.as_str().to_string())
                    .collect();
            }
            server.status = if entry.disabled.unwrap_or(false) {
                McpServerStatus::Disabled
            } else {
                live.map(|item| status_from_label(&item.state))
                    .unwrap_or(McpServerStatus::Connecting)
            };
            server.command_or_url = entry
                .url
                .or(entry.command)
                .unwrap_or_else(|| entry.scope.label());
            let tools_count = live.map(|item| item.tools_count).unwrap_or(0);
            server.tools = (0..tools_count)
                .map(|idx| McpTool::new(format!("tool-{}", idx + 1), "registered MCP tool"))
                .collect();
            if let Some(error) = live.and_then(|item| item.error.clone()) {
                server.warnings.push(error);
            }
            if let Some(live) = live {
                apply_health_details(&mut server, live);
            }
            server
        })
        .collect()
}

fn apply_health_details(server: &mut McpServer, live: &McpServerStatusInfo) {
    server.health.last_error = live.error.clone();
    server.health.last_error_kind = live.last_error_kind.clone();
    server.health.failure_count = live.failure_count;
    server.health.connect_attempt_count = live.connect_attempt_count;
    server.health.retry_scheduled_count = live.retry_scheduled_count;
    server.health.retry_exhausted_count = live.retry_exhausted_count;
    server.health.recovered_count = live.recovered_count;
    server.health.next_retry_at = live.next_retry_at;
    server.health.last_attempt_at = live.last_attempt_at;
    server.health.last_success_at = live.last_success_at;
    server.health.stderr_tail = live.stderr_tail.clone();
    server.health.stderr_tail_dropped_line_count = live.stderr_tail_dropped_line_count;

    let summary = server.health.summary_fields();
    if !summary.is_empty() {
        server
            .warnings
            .push(format!("health: {}", summary.join(", ")));
    }
}

fn runtime_bindings() -> Option<Vec<McpBinding>> {
    let manager = allthecodes_mcp::runtime::current_manager()?;
    let manager = manager.try_lock().ok()?;
    Some(manager.bindings())
}

fn binding_for_server<'a>(
    bindings: &'a [McpBinding],
    server_id: &str,
    source_label: &str,
) -> Option<&'a McpBinding> {
    bindings
        .iter()
        .find(|binding| binding.server_id == server_id)
        .or_else(|| {
            bindings.iter().find(|binding| {
                binding.source_scope.as_deref() == Some(source_label)
                    && binding.server_id == server_id
            })
        })
}

pub(crate) fn status_from_label(value: &str) -> McpServerStatus {
    match value {
        "connected" | "running" => McpServerStatus::Connected,
        "connecting" | "starting" => McpServerStatus::Connecting,
        "disabled" | "disconnected" | "stopped" => McpServerStatus::Disabled,
        _ => McpServerStatus::Failed,
    }
}

pub(crate) fn selected_server_command(
    state: &McpListPanelState,
    prefix: &str,
    suffix: &str,
) -> CommandSurfaceOutcome {
    state
        .selected_server()
        .map(|server| {
            if prefix.contains(" edit ") {
                CommandSurfaceOutcome::FillPrompt(format!("{prefix}{}{suffix}", server.name))
            } else {
                CommandSurfaceOutcome::Submit(format!("{prefix}{}{suffix}", server.name))
            }
        })
        .unwrap_or(CommandSurfaceOutcome::None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_details_are_copied_to_ui_server() {
        let mut server = McpServer::new("bridge", McpServerKind::Stdio);
        let live = McpServerStatusInfo {
            name: "bridge".to_string(),
            state: "error".to_string(),
            transport: "stdio".to_string(),
            tools_count: 0,
            resources_count: 0,
            error: Some("failed to spawn MCP server".to_string()),
            last_error_kind: Some("spawn_failed".to_string()),
            failure_count: Some(3),
            connect_attempt_count: Some(3),
            retry_scheduled_count: Some(2),
            retry_exhausted_count: Some(1),
            recovered_count: Some(0),
            next_retry_at: Some(12345),
            stderr_tail: vec!["startup failed".to_string()],
            stderr_tail_dropped_line_count: Some(4),
            ..Default::default()
        };

        apply_health_details(&mut server, &live);

        assert_eq!(
            server.health.last_error_kind.as_deref(),
            Some("spawn_failed")
        );
        assert_eq!(server.health.failure_count, Some(3));
        assert_eq!(server.health.connect_attempt_count, Some(3));
        assert_eq!(server.health.retry_scheduled_count, Some(2));
        assert_eq!(server.health.retry_exhausted_count, Some(1));
        assert_eq!(server.health.recovered_count, Some(0));
        assert_eq!(server.health.stderr_tail, vec!["startup failed"]);
        assert_eq!(server.health.stderr_tail_dropped_line_count, Some(4));
        assert!(server
            .warnings
            .iter()
            .any(|warning| warning.contains("error_kind=spawn_failed")));
        assert!(server
            .warnings
            .iter()
            .any(|warning| warning.contains("attempts=3")));
    }
}
