//! Terminal integration policy.

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TerminalEnvironment {
    pub term: String,
    pub tmux: bool,
    pub zellij: bool,
    pub ssh: bool,
    pub windows_terminal: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalPolicy {
    pub title_updates: bool,
    pub osc52_clipboard: bool,
    pub focus_events: bool,
    pub alternate_screen: bool,
    pub notification_backend: &'static str,
}

pub fn policy_for(env: &TerminalEnvironment) -> TerminalPolicy {
    TerminalPolicy {
        title_updates: !env.term.is_empty() && !env.term.contains("dumb"),
        osc52_clipboard: env.ssh || env.tmux,
        focus_events: !env.zellij && !env.term.contains("dumb"),
        alternate_screen: !env.ssh,
        notification_backend: if env.windows_terminal {
            "windows-toast"
        } else if env.ssh {
            "terminal-bell"
        } else {
            "desktop"
        },
    }
}

pub fn render_policy(env: &TerminalEnvironment) -> String {
    let policy = policy_for(env);
    [
        format!("term: {}", env.term),
        format!("title: {}", policy.title_updates),
        format!("osc52: {}", policy.osc52_clipboard),
        format!("focus: {}", policy.focus_events),
        format!("alt-screen: {}", policy.alternate_screen),
        format!("notifications: {}", policy.notification_backend),
    ]
    .join("\n")
}
