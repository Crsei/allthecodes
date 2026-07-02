//! MCP server list panel rendering.

use super::index::McpServer;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpListPanelState {
    pub servers: Vec<McpServer>,
    pub selected_index: usize,
}

impl McpListPanelState {
    pub fn new(servers: Vec<McpServer>) -> Self {
        Self {
            servers,
            selected_index: 0,
        }
    }

    pub fn move_next(&mut self) {
        if self.servers.is_empty() {
            return;
        }
        self.selected_index = (self.selected_index + 1) % self.servers.len();
    }

    pub fn move_prev(&mut self) {
        if self.servers.is_empty() {
            return;
        }
        self.selected_index = if self.selected_index == 0 {
            self.servers.len() - 1
        } else {
            self.selected_index - 1
        };
    }

    pub fn selected_server(&self) -> Option<&McpServer> {
        self.servers.get(self.selected_index)
    }

    pub fn render(&self) -> String {
        render_mcp_list_panel(&self.servers, self.selected_index)
    }
}

pub fn render_mcp_list_panel(servers: &[McpServer], selected_index: usize) -> String {
    if servers.is_empty() {
        return "MCP servers\nNo servers configured".to_string();
    }
    let mut lines = vec![format!("MCP servers ({})", servers.len())];
    for (idx, server) in servers.iter().enumerate() {
        let marker = if idx == selected_index { ">" } else { " " };
        lines.push(format!(
            "{marker} {:<14} {:<7} {:<10} tools={}",
            server.name,
            server.kind.label(),
            server.status.label(),
            server.tools.len()
        ));
        if !server.command_or_url.is_empty() {
            lines.push(format!("  {}", server.command_or_url));
        }
        let mut metadata = Vec::new();
        if let Some(source) = &server.config_source {
            metadata.push(format!("source={source}"));
        }
        if let Some(scope) = &server.binding_scope {
            metadata.push(format!("binding={scope}"));
        }
        if !server.binding_permissions.is_empty() {
            metadata.push(format!(
                "permissions={}",
                server.binding_permissions.join(",")
            ));
        }
        if !metadata.is_empty() {
            lines.push(format!("  {}", metadata.join("  ")));
        }
    }
    lines.join("\n")
}
