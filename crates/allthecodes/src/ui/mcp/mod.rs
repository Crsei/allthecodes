//! Rust-side MCP UI surfaces.
pub mod capabilities_section;
pub mod elicitation_dialog;
pub mod index;
pub mod mcp_agent_server_menu;
pub mod mcp_list_panel;
pub mod mcp_parsing_warnings;
pub mod mcp_reconnect;
pub mod mcp_remote_server_menu;
pub mod mcp_server_approval_dialog;
pub mod mcp_server_card;
pub mod mcp_server_desktop_import_dialog;
pub mod mcp_server_dialog_copy;
pub mod mcp_server_multiselect_dialog;
pub mod mcp_settings;
pub mod mcp_stdio_server_menu;
pub mod mcp_tool_detail_view;
pub mod mcp_tool_list_view;
pub mod utils;

const _: fn() = production_symbol_anchors;

fn production_symbol_anchors() {
    use elicitation_dialog::{render_elicitation_dialog, ElicitationField};
    use mcp_server_approval_dialog::{
        build_mcp_server_approval_surface, mcp_server_approval_choices,
        render_mcp_server_approval_dialog,
    };
    use mcp_server_desktop_import_dialog::{
        render_mcp_server_desktop_import_dialog, DesktopMcpImportServer,
        McpServerDesktopImportState,
    };
    use mcp_server_dialog_copy::{mcp_server_safety_copy, render_mcp_server_dialog_copy};
    use mcp_server_multiselect_dialog::{
        render_mcp_server_multiselect_dialog, McpServerMultiselectState,
    };

    let fields = vec![ElicitationField {
        name: "token".to_string(),
        prompt: "Token".to_string(),
        value: String::new(),
        required: true,
    }];
    let _ = render_elicitation_dialog("Configure MCP", &fields, 0);
    let _ = mcp_server_approval_choices("docs");
    let _ = build_mcp_server_approval_surface("docs");
    let _ = render_mcp_server_approval_dialog("docs", 0);
    let import = McpServerDesktopImportState {
        servers: vec![DesktopMcpImportServer {
            name: "docs".to_string(),
            command_or_url: "uvx docs".to_string(),
            collides: false,
            selected: true,
            final_name: None,
        }],
        selected_index: 0,
        target_scope: "project".to_string(),
    };
    let _ = import.selected_servers();
    let _ = render_mcp_server_desktop_import_dialog(&import);
    let _ = mcp_server_safety_copy();
    let _ = render_mcp_server_dialog_copy();
    let multiselect = McpServerMultiselectState::new(vec!["docs".to_string()]);
    let _ = multiselect.selected_names();
    let _ = multiselect.rejected_names();
    let _ = multiselect.approve_command();
    let _ = multiselect.reject_command();
    let _ = render_mcp_server_multiselect_dialog(&multiselect);
}

#[cfg(test)]
mod tests {
    use super::capabilities_section::render_capabilities_section;
    use super::elicitation_dialog::{render_elicitation_dialog, ElicitationField};
    use super::index::{McpCapability, McpServer, McpServerKind, McpServerStatus, McpTool};
    use super::mcp_agent_server_menu::render_mcp_agent_server_menu;
    use super::mcp_list_panel::render_mcp_list_panel;
    use super::mcp_parsing_warnings::render_mcp_parsing_warnings;
    use super::mcp_reconnect::render_mcp_reconnect;
    use super::mcp_remote_server_menu::render_mcp_remote_server_menu;
    use super::mcp_server_approval_dialog::render_mcp_server_approval_dialog;
    use super::mcp_server_card::render_mcp_server_card;
    use super::mcp_server_desktop_import_dialog::{
        render_mcp_server_desktop_import_dialog, DesktopMcpImportServer,
        McpServerDesktopImportState,
    };
    use super::mcp_server_dialog_copy::render_mcp_server_dialog_copy;
    use super::mcp_server_multiselect_dialog::{
        render_mcp_server_multiselect_dialog, McpServerMultiselectState,
    };
    use super::mcp_settings::{render_mcp_settings, McpSettingsSummary};
    use super::mcp_stdio_server_menu::render_mcp_stdio_server_menu;
    use super::mcp_tool_detail_view::render_mcp_tool_detail_view;
    use super::mcp_tool_list_view::render_mcp_tool_list_view;
    use super::utils::reconnect_helpers::ReconnectAttempt;

    #[test]
    fn snapshot_mcp_surfaces() {
        let mut tool = McpTool::new("query", "Run a read-only query");
        tool.input_schema = vec![
            ("sql".to_string(), "string".to_string()),
            ("limit".to_string(), "number".to_string()),
        ];
        let mut stdio = McpServer::new("database", McpServerKind::Stdio);
        stdio.command_or_url = "uvx db-mcp".to_string();
        stdio.tools = vec![tool.clone()];
        stdio.capabilities = vec![
            McpCapability {
                name: "tools".to_string(),
                count: 1,
            },
            McpCapability {
                name: "resources".to_string(),
                count: 3,
            },
        ];
        let mut remote = McpServer::new("docs", McpServerKind::Remote);
        remote.command_or_url = "https://mcp.example.com".to_string();
        remote.status = McpServerStatus::Failed;
        remote.warnings = vec!["missing auth token".to_string()];
        let mut agent = McpServer::new("agent-tools", McpServerKind::Agent);
        agent.status = McpServerStatus::Connecting;

        let rendered = [
            section(
                "list",
                render_mcp_list_panel(&[stdio.clone(), remote.clone(), agent.clone()], 1),
            ),
            section(
                "capabilities",
                render_capabilities_section(&stdio.capabilities),
            ),
            section("warnings", render_mcp_parsing_warnings(&remote.warnings)),
            section("tool-list", render_mcp_tool_list_view(&stdio.tools, 0)),
            section("tool-detail", render_mcp_tool_detail_view(&tool)),
            section("stdio-menu", render_mcp_stdio_server_menu(&stdio)),
            section("remote-menu", render_mcp_remote_server_menu(&remote)),
            section("server-card", render_mcp_server_card(&remote, true)),
            section("dialog-copy", render_mcp_server_dialog_copy()),
            section(
                "approval-dialog",
                render_mcp_server_approval_dialog("playwright", 1),
            ),
            section(
                "multiselect-dialog",
                render_mcp_server_multiselect_dialog(&McpServerMultiselectState {
                    servers: vec![
                        super::mcp_server_multiselect_dialog::McpServerMultiSelectItem {
                            name: "github".to_string(),
                            selected: true,
                        },
                        super::mcp_server_multiselect_dialog::McpServerMultiSelectItem {
                            name: "playwright".to_string(),
                            selected: false,
                        },
                        super::mcp_server_multiselect_dialog::McpServerMultiSelectItem {
                            name: "sentry".to_string(),
                            selected: true,
                        },
                    ],
                    selected_index: 1,
                }),
            ),
            section(
                "desktop-import-dialog",
                render_mcp_server_desktop_import_dialog(&McpServerDesktopImportState {
                    servers: vec![
                        DesktopMcpImportServer {
                            name: "filesystem".to_string(),
                            command_or_url: "npx @modelcontextprotocol/server-filesystem"
                                .to_string(),
                            collides: false,
                            selected: true,
                            final_name: None,
                        },
                        DesktopMcpImportServer {
                            name: "github".to_string(),
                            command_or_url: "uvx mcp-github".to_string(),
                            collides: true,
                            selected: false,
                            final_name: Some("github-1".to_string()),
                        },
                    ],
                    selected_index: 1,
                    target_scope: "~/.allthecodes/settings.json".to_string(),
                }),
            ),
            section(
                "agent-menu",
                render_mcp_agent_server_menu(&agent, "reviewer"),
            ),
            section(
                "settings",
                render_mcp_settings(&McpSettingsSummary {
                    enabled: true,
                    config_path: "~/.allthecodes/mcp.json".to_string(),
                    server_count: 3,
                    warning_count: 1,
                }),
            ),
            section(
                "reconnect",
                render_mcp_reconnect(&ReconnectAttempt {
                    server_name: "docs".to_string(),
                    attempt: 2,
                    max_attempts: 5,
                    last_error: Some("HTTP 401".to_string()),
                }),
            ),
            section(
                "elicitation",
                render_elicitation_dialog(
                    "MCP needs input",
                    &[
                        ElicitationField {
                            name: "token".to_string(),
                            prompt: "API token".to_string(),
                            value: String::new(),
                            required: true,
                        },
                        ElicitationField {
                            name: "region".to_string(),
                            prompt: "Region".to_string(),
                            value: "us".to_string(),
                            required: false,
                        },
                    ],
                    0,
                ),
            ),
        ]
        .join("\n\n");

        insta::assert_snapshot!("mcp_surfaces", rendered);
    }

    fn section(name: &str, body: impl AsRef<str>) -> String {
        format!("## {name}\n{}", body.as_ref())
    }
}
