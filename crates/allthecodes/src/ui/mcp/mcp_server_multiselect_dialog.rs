//! MCP `.mcp.json` multi-server approval dialog rendering.

use crate::ui::better_view_panel::{plain_row, selected_row, BetterViewPanel};

use super::mcp_server_dialog_copy::mcp_server_safety_copy;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerMultiSelectItem {
    pub name: String,
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerMultiselectState {
    pub servers: Vec<McpServerMultiSelectItem>,
    pub selected_index: usize,
}

impl McpServerMultiselectState {
    pub fn new(names: Vec<String>) -> Self {
        Self {
            servers: names
                .into_iter()
                .map(|name| McpServerMultiSelectItem {
                    name,
                    selected: true,
                })
                .collect(),
            selected_index: 0,
        }
    }

    pub fn selected_names(&self) -> Vec<String> {
        self.servers
            .iter()
            .filter(|server| server.selected)
            .map(|server| server.name.clone())
            .collect()
    }

    pub fn rejected_names(&self) -> Vec<String> {
        self.servers
            .iter()
            .filter(|server| !server.selected)
            .map(|server| server.name.clone())
            .collect()
    }

    pub fn approve_command(&self) -> Option<String> {
        command_for("approve", &self.selected_names())
    }

    pub fn reject_command(&self) -> Option<String> {
        command_for("reject", &self.rejected_names())
    }
}

pub fn render_mcp_server_multiselect_dialog(state: &McpServerMultiselectState) -> String {
    let mut lines = Vec::new();
    for (idx, server) in state.servers.iter().enumerate() {
        let checked = if server.selected { "x" } else { " " };
        let decision = if server.selected { "enable" } else { "disable" };
        lines.push(selected_row(
            format!("[{checked}] {}", server.name),
            decision,
            idx == state.selected_index,
        ));
    }

    lines.push(String::new());
    lines.push("Commands".to_string());
    if let Some(command) = state.approve_command() {
        lines.push(plain_row("enable:", command));
    }
    if let Some(command) = state.reject_command() {
        lines.push(plain_row("disable:", command));
    }
    BetterViewPanel::new("New MCP servers")
        .summary(format!(
            "count={} source=.mcp.json risk=code",
            state.servers.len()
        ))
        .sections_title("Servers")
        .sections(
            state
                .servers
                .iter()
                .map(|server| server.name.clone())
                .collect::<Vec<_>>(),
            state.selected_index,
        )
        .detail_title(mcp_server_safety_copy())
        .detail_lines(lines)
        .footer("Up/Down server | Space toggle | Enter apply | Esc reject all")
        .render()
}

fn command_for(verb: &str, names: &[String]) -> Option<String> {
    if names.is_empty() {
        return None;
    }
    Some(format!("/mcp {verb} {}", names.join(" ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_multiselect_state_selects_all_servers() {
        let state = McpServerMultiselectState::new(vec!["docs".into(), "shell".into()]);

        assert_eq!(state.selected_index, 0);
        assert_eq!(state.selected_names(), vec!["docs", "shell"]);
        assert_eq!(state.rejected_names(), Vec::<String>::new());
        assert_eq!(
            state.approve_command().as_deref(),
            Some("/mcp approve docs shell")
        );
        assert_eq!(state.reject_command(), None);
    }
}
