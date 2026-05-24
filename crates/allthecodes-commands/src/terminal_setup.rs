//! `/terminal-setup` slash command: diagnose the current terminal
//! environment and print configuration tips (issue #12).
//!
//! Output covers:
//! - detected shell / terminal program / multiplexer
//! - status of the `ALLTHECODES_*` env toggles
//! - manual setup tips for Shift+Enter support across common terminals
//! - tmux passthrough advice when `$TMUX` is set
//! - transcript-export expectations for `$VISUAL` / `$EDITOR`
//! - notification / bell guidance
//!
//! The command is read-only. It never mutates the user's config.
//!
//! Usage:
//!
//! ```text
//!   /terminal-setup         # print diagnostics + tips
//!   /terminal-setup env     # just the env-var table
//!   /terminal-setup tips    # just the per-terminal tips
//! ```

use anyhow::Result;
use async_trait::async_trait;

use crate::terminal_env::{parse_editor_command, TerminalEnvConfig};
use crate::{CommandContext, CommandHandler, CommandResult};

pub struct TerminalSetupHandler;

#[async_trait]
impl CommandHandler for TerminalSetupHandler {
    async fn execute(&self, args: &str, _ctx: &mut CommandContext) -> Result<CommandResult> {
        let args = args.trim().to_ascii_lowercase();
        let probe = EnvProbe::from_env();
        let out = match args.as_str() {
            "" | "all" | "full" => render_all(&probe),
            "env" | "config" => render_env(&probe),
            "tips" | "help" => render_tips(&probe),
            other => format!(
                "Unknown /terminal-setup subcommand '{}'.\n\nUsage:\n  \
                 /terminal-setup         - environment diagnostics + tips\n  \
                 /terminal-setup env     - just the env-var table\n  \
                 /terminal-setup tips    - just the per-terminal tips",
                other
            ),
        };
        Ok(CommandResult::Output(out))
    }
}

/// Snapshot of the subset of env vars we care about.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnvProbe {
    pub term: Option<String>,
    pub term_program: Option<String>,
    pub colorterm: Option<String>,
    pub shell: Option<String>,
    pub tmux: Option<String>,
    pub zellij: Option<String>,
    pub screen_socket: Option<String>,
    pub ssh_tty: Option<String>,
    pub visual: Option<String>,
    pub editor: Option<String>,
    pub vte_version: Option<String>,
    pub wt_session: Option<String>,
    pub term_program_version: Option<String>,
    pub claude_code_no_flicker: Option<String>,
    pub claude_code_enable_mouse_capture: Option<String>,
    pub claude_code_disable_mouse: Option<String>,
    pub claude_code_scroll_speed: Option<String>,
}

impl EnvProbe {
    pub fn from_env() -> Self {
        Self::from_pairs(std::env::vars())
    }

    pub fn from_pairs<I, K, V>(iter: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<str>,
        V: AsRef<str>,
    {
        let mut p = Self::default();
        for (k, v) in iter {
            let value = normalize_env_value(v.as_ref());
            match k.as_ref() {
                "TERM" => p.term = value,
                "TERM_PROGRAM" => p.term_program = value,
                "TERM_PROGRAM_VERSION" => p.term_program_version = value,
                "COLORTERM" => p.colorterm = value,
                "SHELL" => p.shell = value,
                "TMUX" => p.tmux = value,
                "ZELLIJ" => p.zellij = value,
                "STY" => p.screen_socket = value,
                "SSH_TTY" => p.ssh_tty = value,
                "VISUAL" => p.visual = value,
                "EDITOR" => p.editor = value,
                "VTE_VERSION" => p.vte_version = value,
                "WT_SESSION" => p.wt_session = value,
                "ALLTHECODES_NO_FLICKER" => p.claude_code_no_flicker = value,
                "ALLTHECODES_ENABLE_MOUSE_CAPTURE" => p.claude_code_enable_mouse_capture = value,
                "ALLTHECODES_DISABLE_MOUSE" => p.claude_code_disable_mouse = value,
                "ALLTHECODES_SCROLL_SPEED" => p.claude_code_scroll_speed = value,
                _ => {}
            }
        }
        p
    }

    /// Normalize `$TERM_PROGRAM` into a coarse terminal label so we can
    /// pick the right tip bullet below.
    pub fn terminal_label(&self) -> TerminalLabel {
        if self.wt_session.is_some() {
            return TerminalLabel::WindowsTerminal;
        }
        match self.term_program.as_deref().map(str::to_ascii_lowercase) {
            Some(ref s) if s.contains("iterm") => TerminalLabel::ITerm2,
            Some(ref s) if s.contains("apple_terminal") => TerminalLabel::AppleTerminal,
            Some(ref s) if s.contains("vscode") => TerminalLabel::VsCode,
            Some(ref s) if s.contains("wezterm") => TerminalLabel::WezTerm,
            Some(ref s) if s.contains("alacritty") => TerminalLabel::Alacritty,
            Some(ref s) if s.contains("kitty") => TerminalLabel::Kitty,
            Some(ref s) if s.contains("ghostty") => TerminalLabel::Ghostty,
            Some(ref s) if s.contains("hyper") => TerminalLabel::Hyper,
            Some(ref s) if s.contains("tabby") => TerminalLabel::Tabby,
            _ => {
                if self.vte_version.is_some() {
                    TerminalLabel::GnomeLikeVte
                } else {
                    TerminalLabel::Unknown
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalLabel {
    ITerm2,
    AppleTerminal,
    VsCode,
    WezTerm,
    Alacritty,
    Kitty,
    Ghostty,
    Hyper,
    Tabby,
    WindowsTerminal,
    GnomeLikeVte,
    Unknown,
}

impl TerminalLabel {
    pub fn as_str(self) -> &'static str {
        match self {
            TerminalLabel::ITerm2 => "iTerm2",
            TerminalLabel::AppleTerminal => "Terminal.app",
            TerminalLabel::VsCode => "VS Code",
            TerminalLabel::WezTerm => "WezTerm",
            TerminalLabel::Alacritty => "Alacritty",
            TerminalLabel::Kitty => "Kitty",
            TerminalLabel::Ghostty => "Ghostty",
            TerminalLabel::Hyper => "Hyper",
            TerminalLabel::Tabby => "Tabby",
            TerminalLabel::WindowsTerminal => "Windows Terminal",
            TerminalLabel::GnomeLikeVte => "GNOME / VTE-based",
            TerminalLabel::Unknown => "unknown",
        }
    }

    /// Short Shift+Enter / multi-line tip for this terminal.
    pub fn shift_enter_tip(self) -> &'static str {
        match self {
            TerminalLabel::ITerm2 => {
                "iTerm2 -> Settings -> Keys -> Key Bindings -> map Shift+Return to 'Send Escape Sequence' with value `[27;2;13~`."
            }
            TerminalLabel::AppleTerminal => {
                "Terminal.app -> Settings -> Profiles -> Keyboard -> add Shift+Return sending `\\033[27;2;13~`."
            }
            TerminalLabel::VsCode => {
                "Add to keybindings.json: `{\"key\":\"shift+enter\",\"command\":\"workbench.action.terminal.sendSequence\",\"args\":{\"text\":\"\\u001b[27;2;13~\"}}`."
            }
            TerminalLabel::WezTerm => {
                "wezterm.lua: `keys = { { key='Enter', mods='SHIFT', action = wezterm.action.SendString('\\x1b[27;2;13~') } }`."
            }
            TerminalLabel::Alacritty => {
                "alacritty.toml: `[[keyboard.bindings]] key=\"Return\" mods=\"Shift\" chars=\"\\x1b[27;2;13~\"`."
            }
            TerminalLabel::Kitty => "kitty.conf: `map shift+enter send_text all \\x1b[27;2;13~`.",
            TerminalLabel::Ghostty => {
                "Ghostty honors `keybind = shift+enter=text:\\x1b[27;2;13~` in config.toml."
            }
            TerminalLabel::Hyper => {
                "Hyper: enable `hyper-csi-u` or `hyperterm-shift-enter` plugins for Shift+Enter support."
            }
            TerminalLabel::Tabby => {
                "Tabby -> Settings -> Hotkeys -> add Shift+Enter sending `\\x1b[27;2;13~`."
            }
            TerminalLabel::WindowsTerminal => {
                "Windows Terminal settings.json: add `{ \"command\": { \"action\": \"sendInput\", \"input\": \"\\u001b[27;2;13~\" }, \"keys\": \"shift+enter\" }`."
            }
            TerminalLabel::GnomeLikeVte => {
                "gnome-terminal / tilix: no stable Shift+Enter binding. Try tmux passthrough, transcript mode search, or transcript export (`e`) for long edits."
            }
            TerminalLabel::Unknown => {
                "If Shift+Enter is not working, try Alt+Enter, Ctrl+J, or map Shift+Return to emit `\\x1b[27;2;13~`."
            }
        }
    }
}

fn render_all(p: &EnvProbe) -> String {
    let mut out = render_env(p);
    out.push('\n');
    out.push_str(&render_tips(p));
    out
}

fn render_env(p: &EnvProbe) -> String {
    let effective = TerminalEnvConfig::from_pairs([
        (
            "ALLTHECODES_NO_FLICKER",
            p.claude_code_no_flicker.clone().unwrap_or_default(),
        ),
        (
            "ALLTHECODES_ENABLE_MOUSE_CAPTURE",
            p.claude_code_enable_mouse_capture
                .clone()
                .unwrap_or_default(),
        ),
        (
            "ALLTHECODES_DISABLE_MOUSE",
            p.claude_code_disable_mouse.clone().unwrap_or_default(),
        ),
        (
            "ALLTHECODES_SCROLL_SPEED",
            p.claude_code_scroll_speed.clone().unwrap_or_default(),
        ),
    ]);

    let mut out = String::new();
    out.push_str("Terminal setup\n");
    out.push_str("--------------\n");
    out.push_str(&format!("  Detected:   {}\n", p.terminal_label().as_str()));
    out.push_str(&row("TERM", &p.term));
    out.push_str(&row("TERM_PROGRAM", &p.term_program));
    out.push_str(&row("TERM_PROGRAM_VERSION", &p.term_program_version));
    out.push_str(&row("COLORTERM", &p.colorterm));
    out.push_str(&row("SHELL", &p.shell));
    out.push_str(&row("TMUX", &p.tmux));
    out.push_str(&row("ZELLIJ", &p.zellij));
    out.push_str(&row("STY (screen)", &p.screen_socket));
    out.push_str(&row("SSH_TTY", &p.ssh_tty));
    out.push_str(&row("VISUAL", &p.visual));
    out.push_str(&row("EDITOR", &p.editor));
    if let Some(note) = editor_launch_note(p) {
        out.push_str(&format!("    -> editor export:        {}\n", note));
    }

    out.push_str("\nALLTHECODES_* toggles (issue #12)\n");
    out.push_str(&row("ALLTHECODES_NO_FLICKER", &p.claude_code_no_flicker));
    out.push_str(&format!(
        "    -> synchronized updates: {}\n",
        if effective.sync_updates { "on" } else { "off" }
    ));
    out.push_str(&row(
        "ALLTHECODES_ENABLE_MOUSE_CAPTURE",
        &p.claude_code_enable_mouse_capture,
    ));
    out.push_str(&row(
        "ALLTHECODES_DISABLE_MOUSE",
        &p.claude_code_disable_mouse,
    ));
    out.push_str(&format!(
        "    -> mouse capture:        {}\n",
        if TerminalEnvConfig::DISABLE_MOUSE_RUNTIME_SUPPORTED {
            if effective.disable_mouse {
                "disabled (native selection/copy)"
            } else {
                "enabled for wheel events; terminal drag selection is captured"
            }
        } else {
            "not implemented in current runtime (env is diagnostic-only)"
        }
    ));
    out.push_str(&row(
        "ALLTHECODES_SCROLL_SPEED",
        &p.claude_code_scroll_speed,
    ));
    out.push_str(&format!(
        "    -> scroll speed:         {} lines / step\n",
        effective.scroll_speed
    ));

    out
}

fn render_tips(p: &EnvProbe) -> String {
    let label = p.terminal_label();
    let mut out = String::new();
    out.push_str("Tips\n");
    out.push_str("----\n");
    out.push_str(
        "Manual setup only: /terminal-setup is read-only and cannot edit terminal, tmux, shell, or editor configuration files.\n\n",
    );

    out.push_str(&format!(
        "Manual Shift+Enter setup ({}):\n  - {}\n\n",
        label.as_str(),
        label.shift_enter_tip()
    ));

    if p.tmux.is_some() {
        out.push_str("tmux detected:\n");
        out.push_str(
            "  - Manual setup: add `set -g extended-keys on` + `set -as terminal-features ',xterm*:extkeys'` for Shift+Enter passthrough.\n",
        );
        out.push_str(
            "  - Manual setup: use `set -g allow-passthrough on` (tmux >= 3.3) so allthecodes can emit notifications / OSC sequences.\n\n",
        );
    }

    out.push_str("Notifications:\n");
    out.push_str(
        "  - Terminal bell / OSC 9 notifications are emitted when the agent finishes a turn. If your terminal suppresses them, check its bell settings.\n",
    );
    out.push_str(
        "  - On Linux desktops, `notify-send` hooks can be wired through `statusLine.command`.\n\n",
    );

    out.push_str("Editor export:\n");
    out.push_str(
        "  - Transcript export currently launches `$VISUAL` / `$EDITOR` as a single executable path.\n",
    );
    out.push_str(
        "  - Values with arguments such as `code --wait` or `nvim -f` are not supported yet; use a wrapper script or set the env var to the executable only.\n\n",
    );

    if !TerminalEnvConfig::DISABLE_MOUSE_RUNTIME_SUPPORTED {
        out.push_str("Mouse flag status:\n");
        out.push_str(
            "  - Mouse capture env flags are parsed and shown here, but the current Rust TUI does not change mouse capture at runtime yet.\n\n",
        );
    }

    out.push_str("Transcript / focus view (issue #12):\n");
    out.push_str("  - Ctrl+O cycles Prompt -> Transcript -> Focus.\n");
    out.push_str(
        "  - In transcript mode: `/` search, `n`/`N` next/prev, `e` export to the configured editor, `q`/Esc exit.\n",
    );

    out
}

fn row(label: &str, value: &Option<String>) -> String {
    let shown = value
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or("(unset)");
    format!("  {:<25} {}\n", format!("{}:", label), shown)
}

fn editor_launch_note(p: &EnvProbe) -> Option<String> {
    for (name, raw) in [
        ("VISUAL", p.visual.as_deref()),
        ("EDITOR", p.editor.as_deref()),
    ] {
        let Some(raw) = raw else {
            continue;
        };
        let Some(parsed) = parse_editor_command(raw) else {
            return Some(format!("{name} is set but could not be parsed"));
        };
        return Some(if parsed.transcript_export_supported() {
            format!("{name} -> {}", parsed.program)
        } else {
            format!(
                "{name} includes arguments; transcript export currently needs a bare executable path"
            )
        });
    }
    None
}

fn normalize_env_value(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
    fn make_ctx() -> CommandContext {
        CommandContext {
            messages: vec![],
            cwd: std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
            app_state: Default::default(),
            session_id: SessionId::new(),
        }
    }

    #[tokio::test]
    async fn default_invocation_prints_diagnostic_table() {
        let handler = TerminalSetupHandler;
        let mut ctx = make_ctx();
        let r = handler.execute("", &mut ctx).await.unwrap();
        match r {
            CommandResult::Output(s) => {
                assert!(s.contains("Terminal setup"));
                assert!(s.contains("ALLTHECODES_NO_FLICKER"));
                assert!(s.contains("Tips"));
            }
            _ => panic!("expected Output"),
        }
    }

    #[tokio::test]
    async fn env_subcommand_omits_tips() {
        let handler = TerminalSetupHandler;
        let mut ctx = make_ctx();
        let r = handler.execute("env", &mut ctx).await.unwrap();
        match r {
            CommandResult::Output(s) => {
                assert!(s.contains("ALLTHECODES_NO_FLICKER"));
                assert!(!s.contains("Tips\n----\n"));
            }
            _ => panic!("expected Output"),
        }
    }

    #[tokio::test]
    async fn tips_subcommand_omits_env_table() {
        let handler = TerminalSetupHandler;
        let mut ctx = make_ctx();
        let r = handler.execute("tips", &mut ctx).await.unwrap();
        match r {
            CommandResult::Output(s) => {
                assert!(s.contains("Tips"));
                assert!(!s.contains("Terminal setup\n--------------"));
            }
            _ => panic!("expected Output"),
        }
    }

    #[tokio::test]
    async fn unknown_subcommand_returns_usage() {
        let handler = TerminalSetupHandler;
        let mut ctx = make_ctx();
        let r = handler.execute("bogus", &mut ctx).await.unwrap();
        match r {
            CommandResult::Output(s) => {
                assert!(s.contains("Unknown /terminal-setup"));
                assert!(s.contains("Usage:"));
            }
            _ => panic!("expected Output"),
        }
    }

    #[test]
    fn terminal_label_detects_iterm2_even_lowercase() {
        let p = EnvProbe::from_pairs(vec![("TERM_PROGRAM", "iTerm.app")]);
        assert_eq!(p.terminal_label(), TerminalLabel::ITerm2);
    }

    #[test]
    fn terminal_label_prefers_windows_terminal_when_wt_session_set() {
        let p = EnvProbe::from_pairs(vec![("WT_SESSION", "abc"), ("TERM_PROGRAM", "unknown")]);
        assert_eq!(p.terminal_label(), TerminalLabel::WindowsTerminal);
    }

    #[test]
    fn terminal_label_falls_back_to_vte() {
        let p = EnvProbe::from_pairs(vec![("VTE_VERSION", "7206")]);
        assert_eq!(p.terminal_label(), TerminalLabel::GnomeLikeVte);
    }

    #[test]
    fn env_table_shows_effective_scroll_speed() {
        let p = EnvProbe::from_pairs(vec![("ALLTHECODES_SCROLL_SPEED", "7")]);
        let out = render_env(&p);
        assert!(out.contains("7 lines / step"), "{}", out);
    }

    #[test]
    fn gnome_tip_no_longer_mentions_export_transcript_command() {
        let p = EnvProbe::from_pairs(vec![("VTE_VERSION", "7206")]);
        let out = render_tips(&p);
        assert!(!out.contains("/export-transcript"));
        assert!(out.contains("transcript export (`e`)"));
    }

    #[test]
    fn env_table_reports_editor_argument_limitations() {
        let p = EnvProbe::from_pairs(vec![("VISUAL", "code --wait")]);
        let out = render_env(&p);
        assert!(out.contains("VISUAL includes arguments"));
        assert!(out.contains("bare executable path"));
    }

    #[test]
    fn env_table_reports_default_mouse_capture() {
        let p = EnvProbe::from_pairs(Vec::<(&str, &str)>::new());
        let out = render_env(&p);
        assert!(out.contains("enabled for wheel events"));
    }

    #[test]
    fn env_table_reports_enable_mouse_capture_runtime_effect() {
        let p = EnvProbe::from_pairs(vec![("ALLTHECODES_ENABLE_MOUSE_CAPTURE", "1")]);
        let out = render_env(&p);
        assert!(out.contains("enabled for wheel events"));
    }

    #[test]
    fn env_table_reports_disable_mouse_runtime_effect() {
        let p = EnvProbe::from_pairs(vec![("ALLTHECODES_DISABLE_MOUSE", "1")]);
        let out = render_env(&p);
        assert!(out.contains("disabled (native selection/copy)"));
    }
}
