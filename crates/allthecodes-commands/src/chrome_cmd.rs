//! `/chrome` command — first-party Chrome integration status + actions.
//!
//! Subcommands:
//! - `/chrome`             — show current status (enablement, extension, connection, errors)
//! - `/chrome reconnect`   — re-run detection + native-host install (stub in #4; real retry in #5)
//! - `/chrome help`        — usage + install instructions
//!
//! Non-interactive by design: prints state, returns links to the extension
//! install page and the permissions manager. The TUI-facing interactive menu
//! (the `Dialog` component from the bun version) can layer on top later
//! without changing this handler.

use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandHandler, CommandResult};
use allthecodes_browser::common::{
    supports_claude_in_chrome, CHROME_EXTENSION_URL, CHROME_PERMISSIONS_URL, CHROME_RECONNECT_URL,
};
use allthecodes_browser::session::{ChromeEnablement, ChromeSession};
use allthecodes_browser::state::{self, ChromeConnectionState};

/// Handler for the `/chrome` slash command.
pub struct ChromeHandler;

#[async_trait]
impl CommandHandler for ChromeHandler {
    async fn execute(&self, args: &str, _ctx: &mut CommandContext) -> Result<CommandResult> {
        let sub = args.split_whitespace().next().unwrap_or("");
        match sub {
            "" | "status" => Ok(CommandResult::Output(render_status())),
            "reconnect" => Ok(CommandResult::Output(handle_reconnect())),
            "help" | "?" => Ok(CommandResult::Output(render_help())),
            other => Ok(CommandResult::Output(format!(
                "Unknown /chrome subcommand: '{other}'\n\n{}",
                render_help()
            ))),
        }
    }
}

fn render_status() -> String {
    let mut lines = Vec::new();
    lines.push("Claude in Chrome (first-party Chrome integration)".to_string());
    lines.push(String::new());

    if !supports_claude_in_chrome() {
        lines.push("  Platform: UNSUPPORTED".into());
        lines.push("  Claude in Chrome is only available on macOS, Linux, and Windows.".into());
        return lines.join("\n");
    }

    let snap = state::snapshot();

    lines.push(format!("  Status: {}", snap.connection.label()));

    // Extension install
    match snap.extension_installed {
        Some(true) => {
            let browser = snap
                .detected_browser
                .map(|b| format!(" ({})", b.display_name()))
                .unwrap_or_default();
            lines.push(format!("  Extension: installed{}", browser));
        }
        Some(false) => {
            lines.push("  Extension: NOT detected".to_string());
            lines.push(format!("    Install: {}", CHROME_EXTENSION_URL));
        }
        None => {
            lines.push("  Extension: (not yet checked — enable with --chrome)".into());
        }
    }

    // Detailed per-state hints
    match &snap.connection {
        ChromeConnectionState::Disabled => {
            lines.push(String::new());
            lines.push("  Onboarding:".into());
            lines.push(
                "    1. Start allthecodes with `--chrome` or set `ALLTHECODES_ENABLE_CFC=1`."
                    .into(),
            );
            lines.push(format!(
                "    2. Install the Chrome extension: {}",
                CHROME_EXTENSION_URL
            ));
            lines.push("    3. Run `/chrome reconnect` after enabling the subsystem.".into());
        }
        ChromeConnectionState::Enabled => {
            lines.push(String::new());
            lines.push(
                "  Onboarding: subsystem is on; waiting for the Chrome extension to connect."
                    .into(),
            );
            lines.push(format!("    Extension: {}", CHROME_EXTENSION_URL));
            lines.push(format!("    Reconnect URL: {}", CHROME_RECONNECT_URL));
            lines.push("    Retry native-host setup: /chrome reconnect".into());
        }
        ChromeConnectionState::Connecting => {
            lines.push("  Handshake in progress — reconnect if this sticks...".into());
        }
        ChromeConnectionState::Connected { browser } => {
            lines.push(format!(
                "  Connected via {} — browser tools are live.",
                browser.display_name()
            ));
            lines.push(format!("  Permissions: {}", CHROME_PERMISSIONS_URL));
        }
        ChromeConnectionState::Error(msg) => {
            lines.push(format!("  Error: {}", msg));
            lines.push("  Try: /chrome reconnect".into());
        }
    }

    if let Some(err) = snap.last_error.as_deref() {
        if !matches!(snap.connection, ChromeConnectionState::Error(_)) {
            lines.push(format!("  Last error: {}", err));
        }
    }

    lines.push(String::new());
    lines.push("  Docs: docs/reference/browser-mcp-config.md".into());
    lines.push("  Usage: /chrome, /chrome reconnect, /chrome help".into());

    lines.join("\n")
}

fn render_help() -> String {
    format!(
        "Claude in Chrome (first-party Chrome integration)\n\n\
         Usage:\n  \
           /chrome              -- show status (extension + connection)\n  \
           /chrome reconnect    -- re-run setup + reconnect attempt\n  \
           /chrome help         -- this message\n\n\
         CLI flags:\n  \
           claude --chrome      -- enable Chrome subsystem\n  \
           claude --no-chrome   -- explicitly disable Chrome subsystem\n\n\
         Environment:\n  \
           ALLTHECODES_ENABLE_CFC=1   -- enable by default\n  \
           ALLTHECODES_ENABLE_CFC=0   -- disable by default\n\n\
         Install the Chrome extension:  {install}\n\
         Manage per-site permissions:   {perms}\n\
         Troubleshoot disconnects:      {reconnect}",
        install = CHROME_EXTENSION_URL,
        perms = CHROME_PERMISSIONS_URL,
        reconnect = CHROME_RECONNECT_URL,
    )
}

fn handle_reconnect() -> String {
    let snap = state::snapshot();
    if matches!(snap.connection, ChromeConnectionState::Disabled) {
        return "Chrome subsystem is disabled. Start allthecodes with --chrome first.".into();
    }

    // Re-run setup (detection + manifest install). #5 will additionally
    // attempt to re-open the socket.
    let session = ChromeSession::new(ChromeEnablement::Enabled);
    match session.reconnect() {
        Ok(()) => {
            // Re-read state in case reconnect updated it.
            let snap = state::snapshot();
            match &snap.connection {
                ChromeConnectionState::Error(msg) => {
                    format!(
                        "Reconnect ran but reported an error: {}\nCheck that Chrome is installed and the extension is present.\nReconnect URL: {}",
                        msg, CHROME_RECONNECT_URL
                    )
                }
                _ => format!(
                    "Re-ran detection + manifest install.\nNow visit {} in Chrome to reconnect the extension.\n\n{}",
                    CHROME_RECONNECT_URL,
                    render_status()
                ),
            }
        }
        Err(e) => format!("Reconnect failed: {}", e),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
    use allthecodes_browser::common::ChromiumBrowser;
    use allthecodes_browser::state::{self, ChromeConnectionState};
    use std::path::PathBuf;

    fn test_ctx() -> CommandContext {
        CommandContext {
            messages: vec![],
            cwd: PathBuf::from("/tmp"),
            app_state: Default::default(),
            session_id: SessionId::new(),
        }
    }

    fn test_lock() -> std::sync::MutexGuard<'static, ()> {
        use std::sync::{Mutex, OnceLock};
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn status_when_disabled_mentions_cli_flag() {
        let _guard = test_lock();
        state::reset_for_tests();

        let handler = ChromeHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(t) => {
                assert!(t.contains("disabled"));
                assert!(t.contains("--chrome"));
            }
            _ => panic!("expected Output"),
        }
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn status_when_connected_shows_browser() {
        let _guard = test_lock();
        state::reset_for_tests();
        state::set_extension_detection(true, Some(ChromiumBrowser::Chrome));
        state::set_connection(ChromeConnectionState::Connected {
            browser: ChromiumBrowser::Chrome,
        });

        let handler = ChromeHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(t) => {
                assert!(t.contains("Connected via Google Chrome"));
                assert!(t.contains("installed"));
            }
            _ => panic!("expected Output"),
        }

        state::reset_for_tests();
    }

    #[tokio::test]
    async fn help_subcommand_shows_usage() {
        let handler = ChromeHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("help", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(t) => {
                assert!(t.contains("--chrome"));
                assert!(t.contains("--no-chrome"));
                assert!(t.contains("ALLTHECODES_ENABLE_CFC"));
            }
            _ => panic!("expected Output"),
        }
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn reconnect_from_disabled_reports_disabled() {
        let _guard = test_lock();
        state::reset_for_tests();
        let handler = ChromeHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("reconnect", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(t) => {
                assert!(t.contains("disabled"));
            }
            _ => panic!("expected Output"),
        }
    }

    #[tokio::test]
    async fn unknown_subcommand_shows_help() {
        let handler = ChromeHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("nonsense", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(t) => {
                assert!(t.contains("Unknown"));
                assert!(t.contains("Usage"));
            }
            _ => panic!("expected Output"),
        }
    }
}
