//! MCP server import dialog for Claude Desktop data.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopMcpImportServer {
    pub name: String,
    pub command_or_url: String,
    pub collides: bool,
    pub selected: bool,
    pub final_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerDesktopImportState {
    pub servers: Vec<DesktopMcpImportServer>,
    pub selected_index: usize,
    pub target_scope: String,
}

impl McpServerDesktopImportState {
    pub fn selected_servers(&self) -> Vec<&DesktopMcpImportServer> {
        self.servers
            .iter()
            .filter(|server| server.selected)
            .collect()
    }
}

pub fn render_mcp_server_desktop_import_dialog(state: &McpServerDesktopImportState) -> String {
    let mut lines = vec![
        "Import MCP Servers from Claude Desktop".to_string(),
        format!(
            "Found {} MCP server(s) in Claude Desktop.",
            state.servers.len()
        ),
        format!("Target: {}", state.target_scope),
    ];

    if state.servers.iter().any(|server| server.collides) {
        lines.push("Name collisions import with a numbered suffix when selected.".to_string());
    }

    for (idx, server) in state.servers.iter().enumerate() {
        let marker = if idx == state.selected_index {
            ">"
        } else {
            " "
        };
        let checked = if server.selected { "x" } else { " " };
        let final_name = server.final_name.as_ref().unwrap_or(&server.name);
        let collision = if server.collides {
            format!(" -> {final_name}")
        } else {
            String::new()
        };
        lines.push(format!(
            "{marker} [{checked}] {}{} - {}",
            server.name, collision, server.command_or_url
        ));
    }

    let selected = state.selected_servers();
    if selected.is_empty() {
        lines.push("No servers selected for import.".to_string());
    } else {
        lines.push(format!("{} server(s) selected for import.", selected.len()));
    }
    lines.push("Enter import | Space toggle | Esc skip import".to_string());
    lines.join("\n")
}
