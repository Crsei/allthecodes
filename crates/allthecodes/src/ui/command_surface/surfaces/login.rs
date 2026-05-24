use crossterm::event::{KeyCode, KeyEvent};

use crate::ui::better_view_panel::{plain_row, selected_row, BetterViewPanel};
use crate::ui::command_surface::{cycle_index, CommandSurfaceOutcome};

const LOGIN_ACTIONS: &[LoginAction] = &[
    LoginAction {
        shortcut: 's',
        label: "Status",
        description: "show current authentication source",
        command: LoginCommand::Submit("/login status"),
    },
    LoginAction {
        shortcut: 'c',
        label: "Claude Code",
        description: "select the Claude Code / Anthropic-compatible profile",
        command: LoginCommand::Submit("/login claude-code"),
    },
    LoginAction {
        shortcut: 'k',
        label: "Claude Code key",
        description: "paste a Claude Code / Anthropic-compatible API key",
        command: LoginCommand::FillPrompt("/login sk-ant-api03-"),
    },
    LoginAction {
        shortcut: 'a',
        label: "Claude.ai",
        description: "start Claude.ai OAuth for Pro/Max accounts",
        command: LoginCommand::Submit("/login claude-ai"),
    },
    LoginAction {
        shortcut: 'n',
        label: "Console",
        description: "start Console OAuth for API billing",
        command: LoginCommand::Submit("/login console"),
    },
    LoginAction {
        shortcut: 'x',
        label: "Codex",
        description: "select the OpenAI Codex profile",
        command: LoginCommand::Submit("/login codex"),
    },
    LoginAction {
        shortcut: 'o',
        label: "Codex OAuth",
        description: "start OpenAI Codex OAuth for ChatGPT accounts",
        command: LoginCommand::Submit("/login codex-oauth"),
    },
    LoginAction {
        shortcut: 'i',
        label: "Codex CLI",
        description: "check or import ~/.codex/auth.json",
        command: LoginCommand::Submit("/login codex-cli"),
    },
    LoginAction {
        shortcut: 'u',
        label: "Custom",
        description: "select an existing custom auth profile",
        command: LoginCommand::Submit("/login custom"),
    },
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginSurface {
    pub(crate) action_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LoginAction {
    shortcut: char,
    label: &'static str,
    description: &'static str,
    command: LoginCommand,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoginCommand {
    FillPrompt(&'static str),
    Submit(&'static str),
}

impl LoginSurface {
    pub(crate) fn new() -> Self {
        Self { action_index: 0 }
    }

    pub(crate) fn render(&self) -> String {
        let sections = LOGIN_ACTIONS
            .iter()
            .map(|action| action.label.to_string())
            .collect::<Vec<_>>();
        let mut detail_lines = LOGIN_ACTIONS
            .iter()
            .enumerate()
            .map(|(idx, action)| {
                selected_row(
                    format!("{}. {}", action.shortcut, action.label),
                    action.description,
                    idx == self.action_index,
                )
            })
            .collect::<Vec<_>>();
        if let Some(action) = LOGIN_ACTIONS.get(self.action_index) {
            detail_lines.push(String::new());
            detail_lines.push("Next action".to_string());
            detail_lines.push(plain_row("command:", action.command.preview()));
            if matches!(action.shortcut, 'a' | 'n' | 'o') {
                detail_lines.push(plain_row(
                    "external:",
                    "OAuth URL is printed after the command starts",
                ));
                detail_lines.push(plain_row("completion:", "/login-code <code>"));
            }
        }
        BetterViewPanel::new("Login / allthecodes")
            .summary("profiles=claude-code,codex,custom status=ready")
            .sections_title("Steps")
            .sections(sections, self.action_index)
            .detail_title("OAuth details")
            .detail_lines(detail_lines)
            .footer("Enter start | Up/Down method | shortcut letter select | Esc close")
            .render()
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> CommandSurfaceOutcome {
        match key.code {
            KeyCode::Left | KeyCode::Char('[') | KeyCode::Up | KeyCode::Char('k') => {
                self.action_index = cycle_index(self.action_index, LOGIN_ACTIONS.len(), -1);
                CommandSurfaceOutcome::None
            }
            KeyCode::Right
            | KeyCode::Char(']')
            | KeyCode::Down
            | KeyCode::Char('j')
            | KeyCode::Tab => {
                self.action_index = cycle_index(self.action_index, LOGIN_ACTIONS.len(), 1);
                CommandSurfaceOutcome::None
            }
            KeyCode::Enter => self.selected_login_action(),
            KeyCode::Char(ch) => self.shortcut_action(ch),
            _ => CommandSurfaceOutcome::None,
        }
    }

    pub(crate) fn selected_login_action(&self) -> CommandSurfaceOutcome {
        let Some(action) = LOGIN_ACTIONS.get(self.action_index) else {
            return CommandSurfaceOutcome::None;
        };

        match action.command {
            LoginCommand::FillPrompt(command) => {
                CommandSurfaceOutcome::FillPrompt(command.to_string())
            }
            LoginCommand::Submit(command) => CommandSurfaceOutcome::Submit(command.to_string()),
        }
    }

    fn shortcut_action(&mut self, shortcut: char) -> CommandSurfaceOutcome {
        let Some(index) = LOGIN_ACTIONS
            .iter()
            .position(|action| action.shortcut.eq_ignore_ascii_case(&shortcut))
        else {
            return CommandSurfaceOutcome::None;
        };

        self.action_index = index;
        self.selected_login_action()
    }
}

impl LoginCommand {
    fn preview(self) -> &'static str {
        match self {
            LoginCommand::FillPrompt(command) | LoginCommand::Submit(command) => command,
        }
    }
}
