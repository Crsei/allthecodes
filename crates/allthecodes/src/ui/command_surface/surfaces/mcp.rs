use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent};

use crate::ui::better_view_panel::{plain_row, BetterViewPanel};
use crate::ui::command_surface::adapters::mcp::{build_mcp_servers, selected_server_command};
use crate::ui::command_surface::{cycle_index, CommandSurfaceOutcome};
use crate::ui::mcp::capabilities_section::render_capabilities_section;
use crate::ui::mcp::index::{McpServer, McpServerKind, McpServerStatus};
use crate::ui::mcp::mcp_agent_server_menu::render_mcp_agent_server_menu;
use crate::ui::mcp::mcp_list_panel::McpListPanelState;
use crate::ui::mcp::mcp_parsing_warnings::render_mcp_parsing_warnings;
use crate::ui::mcp::mcp_reconnect::render_mcp_reconnect;
use crate::ui::mcp::mcp_remote_server_menu::render_mcp_remote_server_menu;
use crate::ui::mcp::mcp_server_card::render_mcp_server_card;
use crate::ui::mcp::mcp_settings::{render_mcp_settings, McpSettingsSummary};
use crate::ui::mcp::mcp_stdio_server_menu::render_mcp_stdio_server_menu;
use crate::ui::mcp::mcp_tool_detail_view::render_mcp_tool_detail_view;
use crate::ui::mcp::mcp_tool_list_view::render_mcp_tool_list_view;
use crate::ui::mcp::utils::reconnect_helpers::ReconnectAttempt;

const LIST_ACTION_COUNT: usize = 4;
const VIEW_SERVER_DETAIL: usize = 100;
const VIEW_SETTINGS: usize = 101;
const VIEW_TOOL_LIST_BASE: usize = 1_000;
const VIEW_TOOL_DETAIL_BASE: usize = 2_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpSurface {
    pub(crate) state: McpListPanelState,
    pub(crate) action_index: usize,
}

impl McpSurface {
    pub(crate) fn new(cwd: &Path) -> Self {
        Self {
            state: McpListPanelState::new(build_mcp_servers(cwd)),
            action_index: 0,
        }
    }

    pub(crate) fn render(&self) -> String {
        if self.action_index == VIEW_SERVER_DETAIL {
            return self.render_server_detail();
        }
        if self.action_index == VIEW_SETTINGS {
            return self.render_settings();
        }
        if let Some(tool_index) = self.tool_list_index() {
            return self.render_tool_list(tool_index);
        }
        if let Some(tool_index) = self.tool_detail_index() {
            return self.render_tool_detail(tool_index);
        }
        self.render_server_list()
    }

    fn render_server_list(&self) -> String {
        let sections = vec![
            "Status".to_string(),
            "Edit".to_string(),
            "Reconnect".to_string(),
            "Remove".to_string(),
        ];
        let mut detail_lines = self
            .state
            .render()
            .lines()
            .map(str::to_string)
            .collect::<Vec<_>>();
        detail_lines.push(String::new());
        detail_lines.push("Commands".to_string());
        detail_lines.push(match self.action_index {
            0 => plain_row("Enter:", "/mcp status"),
            1 => plain_row("Enter:", "/mcp edit <server>"),
            2 => plain_row("Enter:", "/mcp reconnect <server>"),
            3 => plain_row("Enter:", "/mcp remove <server> direct-execute"),
            _ => plain_row("Enter:", "select"),
        });
        detail_lines.push(plain_row("v:", "server details"));
        detail_lines.push(plain_row("a:", "/mcp add "));
        detail_lines.push(plain_row("t:", "tools for selected server"));
        detail_lines.push(plain_row("s:", "MCP settings"));
        BetterViewPanel::new("MCP")
            .summary(format!(
                "servers={} action={}",
                self.state.servers.len(),
                sections.get(self.action_index).cloned().unwrap_or_default()
            ))
            .sections_title("Actions")
            .sections(sections, self.action_index)
            .detail_title("Servers")
            .detail_lines(detail_lines)
            .footer("Left/Right action | Up/Down server | Enter select | a add | Esc close")
            .render()
    }

    fn render_server_detail(&self) -> String {
        let Some(server) = self.state.selected_server() else {
            return self.render_empty("Server details");
        };

        let mut detail_lines = split_lines(render_mcp_server_card(server, true));
        detail_lines.push(String::new());
        detail_lines.extend(split_lines(self.render_server_kind_menu(server)));
        detail_lines.push(String::new());
        detail_lines.extend(split_lines(render_capabilities_section(
            &server.capabilities,
        )));
        if !server.warnings.is_empty() {
            detail_lines.push(String::new());
            detail_lines.extend(split_lines(render_mcp_parsing_warnings(&server.warnings)));
        }
        if matches!(
            server.status,
            McpServerStatus::Connecting | McpServerStatus::Failed
        ) {
            detail_lines.push(String::new());
            detail_lines.extend(split_lines(render_mcp_reconnect(&ReconnectAttempt {
                server_name: server.name.clone(),
                attempt: 1,
                max_attempts: 3,
                last_error: server.warnings.first().cloned(),
            })));
        }
        detail_lines.push(String::new());
        detail_lines.push("Commands".to_string());
        detail_lines.push(plain_row("Enter/t:", "tools"));
        detail_lines.push(plain_row("s:", "settings"));
        detail_lines.push(plain_row("e:", format!("/mcp edit {} ", server.name)));
        detail_lines.push(plain_row("r:", format!("/mcp reconnect {}", server.name)));
        detail_lines.push(plain_row("d:", format!("/mcp remove {}", server.name)));

        BetterViewPanel::new("MCP")
            .summary(format!(
                "server={} kind={} status={}",
                server.name,
                server.kind.label(),
                server.status.label()
            ))
            .sections_title("View")
            .sections(
                vec![
                    "Server".to_string(),
                    "Tools".to_string(),
                    "Settings".to_string(),
                ],
                0,
            )
            .detail_title("Server details")
            .detail_lines(detail_lines)
            .footer("Up/Down server | Enter tools | s settings | Backspace list | Esc close")
            .render()
    }

    fn render_tool_list(&self, selected_index: usize) -> String {
        let Some(server) = self.state.selected_server() else {
            return self.render_empty("Tools");
        };
        let tool_index = selected_index.min(server.tools.len().saturating_sub(1));
        let mut detail_lines = split_lines(render_mcp_tool_list_view(&server.tools, tool_index));
        detail_lines.push(String::new());
        detail_lines.push("Commands".to_string());
        detail_lines.push(plain_row("Enter:", "tool detail"));
        detail_lines.push(plain_row("Backspace:", "server details"));
        detail_lines.push(plain_row("s:", "settings"));

        BetterViewPanel::new("MCP")
            .summary(format!(
                "server={} tools={}",
                server.name,
                server.tools.len()
            ))
            .sections_title("View")
            .sections(
                vec![
                    "Server".to_string(),
                    "Tools".to_string(),
                    "Settings".to_string(),
                ],
                1,
            )
            .detail_title("Tools")
            .detail_lines(detail_lines)
            .footer("Up/Down tool | Enter detail | Backspace server | Esc close")
            .render()
    }

    fn render_tool_detail(&self, selected_index: usize) -> String {
        let Some(server) = self.state.selected_server() else {
            return self.render_empty("Tool detail");
        };
        let Some(tool) = server.tools.get(selected_index) else {
            return self.render_tool_list(0);
        };
        let mut detail_lines = split_lines(render_mcp_tool_detail_view(tool));
        detail_lines.push(String::new());
        detail_lines.push("Commands".to_string());
        detail_lines.push(plain_row("Backspace/t:", "tools"));
        detail_lines.push(plain_row("s:", "settings"));

        BetterViewPanel::new("MCP")
            .summary(format!("server={} tool={}", server.name, tool.name))
            .sections_title("View")
            .sections(
                vec![
                    "Server".to_string(),
                    "Tools".to_string(),
                    "Settings".to_string(),
                ],
                1,
            )
            .detail_title("Tool detail")
            .detail_lines(detail_lines)
            .footer("Up/Down tool | Backspace tools | s settings | Esc close")
            .render()
    }

    fn render_settings(&self) -> String {
        let warning_count = self
            .state
            .servers
            .iter()
            .map(|server| server.warnings.len())
            .sum();
        let enabled = self
            .state
            .servers
            .iter()
            .any(|server| server.status != McpServerStatus::Disabled);
        let mut detail_lines = split_lines(render_mcp_settings(&McpSettingsSummary {
            enabled,
            config_path: "~/.allthecodes/settings.json".to_string(),
            server_count: self.state.servers.len(),
            warning_count,
        }));
        detail_lines.push(String::new());
        detail_lines.push("Commands".to_string());
        detail_lines.push(plain_row("Enter:", "/mcp status"));
        detail_lines.push(plain_row("Backspace:", "server list"));
        detail_lines.push(plain_row("a:", "/mcp add "));

        BetterViewPanel::new("MCP")
            .summary(format!(
                "servers={} warnings={warning_count}",
                self.state.servers.len()
            ))
            .sections_title("View")
            .sections(
                vec![
                    "Server".to_string(),
                    "Tools".to_string(),
                    "Settings".to_string(),
                ],
                2,
            )
            .detail_title("Settings")
            .detail_lines(detail_lines)
            .footer("Enter status | Backspace list | a add | Esc close")
            .render()
    }

    fn render_empty(&self, title: &str) -> String {
        BetterViewPanel::new("MCP")
            .summary("servers=0")
            .detail_title(title)
            .detail_lines(vec!["No servers configured".to_string()])
            .footer("a add | s settings | Esc close")
            .render()
    }

    fn render_server_kind_menu(&self, server: &McpServer) -> String {
        match server.kind {
            McpServerKind::Stdio => render_mcp_stdio_server_menu(server),
            McpServerKind::Remote => render_mcp_remote_server_menu(server),
            McpServerKind::Agent => {
                let owner_agent = if server.command_or_url.is_empty() {
                    "agent"
                } else {
                    &server.command_or_url
                };
                render_mcp_agent_server_menu(server, owner_agent)
            }
        }
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> CommandSurfaceOutcome {
        if self.action_index == VIEW_SERVER_DETAIL {
            return self.handle_server_detail_key(key);
        }
        if self.action_index == VIEW_SETTINGS {
            return self.handle_settings_key(key);
        }
        if self.tool_list_index().is_some() {
            return self.handle_tool_list_key(key);
        }
        if self.tool_detail_index().is_some() {
            return self.handle_tool_detail_key(key);
        }

        match key.code {
            KeyCode::Left | KeyCode::Char('[') => {
                self.action_index = cycle_index(self.action_index, LIST_ACTION_COUNT, -1);
                CommandSurfaceOutcome::None
            }
            KeyCode::Right | KeyCode::Char(']') => {
                self.action_index = cycle_index(self.action_index, LIST_ACTION_COUNT, 1);
                CommandSurfaceOutcome::None
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.state.move_prev();
                CommandSurfaceOutcome::None
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                self.state.move_next();
                CommandSurfaceOutcome::None
            }
            KeyCode::Enter => self.selected_mcp_action(),
            KeyCode::Char('a') => CommandSurfaceOutcome::FillPrompt("/mcp add ".to_string()),
            KeyCode::Char('e') => selected_server_command(&self.state, "/mcp edit ", " "),
            KeyCode::Char('r') => selected_server_command(&self.state, "/mcp reconnect ", ""),
            KeyCode::Char('d') => selected_server_command(&self.state, "/mcp remove ", ""),
            KeyCode::Char('s') => {
                self.action_index = VIEW_SETTINGS;
                CommandSurfaceOutcome::None
            }
            KeyCode::Char('t') => self.open_tool_list(),
            KeyCode::Char('v') => {
                self.action_index = VIEW_SERVER_DETAIL;
                CommandSurfaceOutcome::None
            }
            _ => CommandSurfaceOutcome::None,
        }
    }

    fn handle_server_detail_key(&mut self, key: KeyEvent) -> CommandSurfaceOutcome {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.state.move_prev();
                CommandSurfaceOutcome::None
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                self.state.move_next();
                CommandSurfaceOutcome::None
            }
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('t') => self.open_tool_list(),
            KeyCode::Left | KeyCode::Backspace | KeyCode::Char('b') => {
                self.action_index = 0;
                CommandSurfaceOutcome::None
            }
            KeyCode::Char('s') => {
                self.action_index = VIEW_SETTINGS;
                CommandSurfaceOutcome::None
            }
            KeyCode::Char('a') => CommandSurfaceOutcome::FillPrompt("/mcp add ".to_string()),
            KeyCode::Char('e') => selected_server_command(&self.state, "/mcp edit ", " "),
            KeyCode::Char('r') => selected_server_command(&self.state, "/mcp reconnect ", ""),
            KeyCode::Char('d') => selected_server_command(&self.state, "/mcp remove ", ""),
            _ => CommandSurfaceOutcome::None,
        }
    }

    fn handle_tool_list_key(&mut self, key: KeyEvent) -> CommandSurfaceOutcome {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_tool_selection(-1, true);
                CommandSurfaceOutcome::None
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                self.move_tool_selection(1, true);
                CommandSurfaceOutcome::None
            }
            KeyCode::Enter => {
                if let Some(index) = self.tool_list_index() {
                    if self.selected_server_has_tool(index) {
                        self.action_index = VIEW_TOOL_DETAIL_BASE + index;
                    }
                }
                CommandSurfaceOutcome::None
            }
            KeyCode::Left | KeyCode::Backspace | KeyCode::Char('b') => {
                self.action_index = VIEW_SERVER_DETAIL;
                CommandSurfaceOutcome::None
            }
            KeyCode::Char('s') => {
                self.action_index = VIEW_SETTINGS;
                CommandSurfaceOutcome::None
            }
            KeyCode::Char('a') => CommandSurfaceOutcome::FillPrompt("/mcp add ".to_string()),
            _ => CommandSurfaceOutcome::None,
        }
    }

    fn handle_tool_detail_key(&mut self, key: KeyEvent) -> CommandSurfaceOutcome {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_tool_selection(-1, false);
                CommandSurfaceOutcome::None
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                self.move_tool_selection(1, false);
                CommandSurfaceOutcome::None
            }
            KeyCode::Left | KeyCode::Backspace | KeyCode::Char('b') | KeyCode::Char('t') => {
                let index = self.tool_detail_index().unwrap_or_default();
                self.action_index = VIEW_TOOL_LIST_BASE + index;
                CommandSurfaceOutcome::None
            }
            KeyCode::Char('s') => {
                self.action_index = VIEW_SETTINGS;
                CommandSurfaceOutcome::None
            }
            _ => CommandSurfaceOutcome::None,
        }
    }

    fn handle_settings_key(&mut self, key: KeyEvent) -> CommandSurfaceOutcome {
        match key.code {
            KeyCode::Enter => CommandSurfaceOutcome::Submit("/mcp status".to_string()),
            KeyCode::Left | KeyCode::Backspace | KeyCode::Char('b') => {
                self.action_index = 0;
                CommandSurfaceOutcome::None
            }
            KeyCode::Right | KeyCode::Char('v') => {
                self.action_index = VIEW_SERVER_DETAIL;
                CommandSurfaceOutcome::None
            }
            KeyCode::Char('a') => CommandSurfaceOutcome::FillPrompt("/mcp add ".to_string()),
            _ => CommandSurfaceOutcome::None,
        }
    }

    pub(crate) fn selected_mcp_action(&self) -> CommandSurfaceOutcome {
        match self.action_index {
            0 => CommandSurfaceOutcome::Submit("/mcp status".to_string()),
            1 => selected_server_command(&self.state, "/mcp edit ", " "),
            2 => selected_server_command(&self.state, "/mcp reconnect ", ""),
            3 => selected_server_command(&self.state, "/mcp remove ", ""),
            _ => CommandSurfaceOutcome::None,
        }
    }

    fn open_tool_list(&mut self) -> CommandSurfaceOutcome {
        self.action_index = VIEW_TOOL_LIST_BASE;
        CommandSurfaceOutcome::None
    }

    fn move_tool_selection(&mut self, direction: isize, list_view: bool) {
        let Some(server) = self.state.selected_server() else {
            return;
        };
        if server.tools.is_empty() {
            self.action_index = if list_view {
                VIEW_TOOL_LIST_BASE
            } else {
                VIEW_TOOL_DETAIL_BASE
            };
            return;
        }
        let current = if list_view {
            self.tool_list_index().unwrap_or_default()
        } else {
            self.tool_detail_index().unwrap_or_default()
        };
        let next = cycle_index(
            current.min(server.tools.len() - 1),
            server.tools.len(),
            direction,
        );
        self.action_index = if list_view {
            VIEW_TOOL_LIST_BASE + next
        } else {
            VIEW_TOOL_DETAIL_BASE + next
        };
    }

    fn selected_server_has_tool(&self, index: usize) -> bool {
        self.state
            .selected_server()
            .map(|server| index < server.tools.len())
            .unwrap_or(false)
    }

    fn tool_list_index(&self) -> Option<usize> {
        self.action_index
            .checked_sub(VIEW_TOOL_LIST_BASE)
            .filter(|index| *index < VIEW_TOOL_LIST_BASE)
    }

    fn tool_detail_index(&self) -> Option<usize> {
        self.action_index.checked_sub(VIEW_TOOL_DETAIL_BASE)
    }
}

fn split_lines(value: String) -> Vec<String> {
    value.lines().map(str::to_string).collect()
}
