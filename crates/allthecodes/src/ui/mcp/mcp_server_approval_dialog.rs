//! MCP `.mcp.json` server approval dialog rendering.

use crate::ui::better_view_panel::{plain_row, selected_row, BetterViewPanel};
use crate::ui::selection_surface::{SelectionItem, SelectionSurface};

use super::mcp_server_dialog_copy::mcp_server_safety_copy;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerApprovalChoice {
    pub id: String,
    pub label: String,
    pub description: String,
    pub command: String,
}

pub fn mcp_server_approval_choices(server_name: &str) -> Vec<McpServerApprovalChoice> {
    vec![
        McpServerApprovalChoice {
            id: "yes_all".to_string(),
            label: "Use this and all future MCP servers in this project".to_string(),
            description: "enable current server and trust future project MCP entries".to_string(),
            command: format!("/mcp approve {server_name} --all-project"),
        },
        McpServerApprovalChoice {
            id: "yes".to_string(),
            label: "Use this MCP server".to_string(),
            description: "enable only this project MCP server".to_string(),
            command: format!("/mcp approve {server_name}"),
        },
        McpServerApprovalChoice {
            id: "no".to_string(),
            label: "Continue without using this MCP server".to_string(),
            description: "record this project MCP server as disabled".to_string(),
            command: format!("/mcp reject {server_name}"),
        },
    ]
}
pub fn build_mcp_server_approval_surface(server_name: &str) -> SelectionSurface {
    SelectionSurface::new(
        format!("New MCP server found in .mcp.json: {server_name}"),
        mcp_server_approval_choices(server_name)
            .into_iter()
            .map(|choice| SelectionItem {
                id: choice.id,
                label: choice.label,
                description: format!("{} -> {}", choice.description, choice.command),
                enabled: true,
                disabled_reason: None,
                preview_lines: Vec::new(),
                actions: Vec::new(),
                search_terms: Vec::new(),
            })
            .collect(),
    )
}

pub fn render_mcp_server_approval_dialog(server_name: &str, selected_index: usize) -> String {
    let choices = mcp_server_approval_choices(server_name);
    let selected = selected_index.min(choices.len().saturating_sub(1));
    let mut lines = choices
        .iter()
        .enumerate()
        .map(|(idx, choice)| {
            selected_row(
                &choice.label,
                format!("{}  {}", choice.description, choice.command),
                idx == selected,
            )
        })
        .collect::<Vec<_>>();
    lines.push(String::new());
    lines.push("Commands".to_string());
    lines.push(plain_row(
        "approve all:",
        format!("/mcp approve {server_name} --all-project"),
    ));
    lines.push(plain_row(
        "approve one:",
        format!("/mcp approve {server_name}"),
    ));
    lines.push(plain_row("reject:", format!("/mcp reject {server_name}")));
    BetterViewPanel::new("New MCP server")
        .summary(format!("server={server_name} source=.mcp.json risk=code"))
        .sections_title("Decisions")
        .sections(
            vec![
                "Use future project servers".to_string(),
                "Use this server".to_string(),
                "Continue without server".to_string(),
            ],
            selected,
        )
        .detail_title(mcp_server_safety_copy())
        .detail_lines(lines)
        .footer("Up/Down decision | Enter submit | Esc reject")
        .render()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_surface_keeps_choice_commands_in_descriptions() {
        let surface = build_mcp_server_approval_surface("docs");

        assert_eq!(surface.title, "New MCP server found in .mcp.json: docs");
        assert_eq!(surface.items.len(), 3);
        assert_eq!(surface.items[0].id, "yes_all");
        assert!(surface.items[0]
            .description
            .contains("/mcp approve docs --all-project"));
        assert!(surface.items[2].description.contains("/mcp reject docs"));
    }
}
