//! /ide command — IDE detection + selection + MCP bridge (issue #41).
//!
//! Subcommands:
//! - `/ide`            — show help.
//! - `/ide detect`     — run detection; print one row per IDE.
//! - `/ide status`     — show detected IDEs + current selection.
//! - `/ide select <id>`-persist selection; triggers a bridge reconnect.
//! - `/ide clear`      — remove the persisted selection.
//! - `/ide reconnect`  — re-establish the MCP bridge for the selected IDE.

use anyhow::Result;
use async_trait::async_trait;
use std::sync::{OnceLock, RwLock};

use crate::{CommandContext, CommandHandler, CommandResult};
use allthecodes_ipc_protocol::subsystem_types::IdeInfo;

#[derive(Clone, Copy)]
pub struct IdeCommandRuntime {
    pub detect_ides: fn() -> Vec<IdeInfo>,
    pub selected_ide: fn() -> Option<String>,
    pub select_ide: fn(&str) -> Result<()>,
    pub clear_selection: fn() -> Result<()>,
    pub reconnect_selected: fn() -> Result<()>,
}

static IDE_RUNTIME: OnceLock<RwLock<Option<IdeCommandRuntime>>> = OnceLock::new();

pub fn set_ide_command_runtime(runtime: IdeCommandRuntime) {
    let slot = IDE_RUNTIME.get_or_init(|| RwLock::new(None));
    if let Ok(mut guard) = slot.write() {
        *guard = Some(runtime);
    }
}

fn ide_runtime() -> Result<IdeCommandRuntime> {
    let Some(slot) = IDE_RUNTIME.get() else {
        anyhow::bail!("IDE command runtime is unavailable; install IdeCommandRuntime adapter");
    };
    let Ok(guard) = slot.read() else {
        anyhow::bail!("IDE command runtime lock is poisoned");
    };
    guard.as_ref().copied().ok_or_else(|| {
        anyhow::anyhow!("IDE command runtime is unavailable; install IdeCommandRuntime adapter")
    })
}

/// Handler for the `/ide` slash command.
pub struct IdeHandler;

#[async_trait]
impl CommandHandler for IdeHandler {
    async fn execute(&self, args: &str, _ctx: &mut CommandContext) -> Result<CommandResult> {
        let runtime = ide_runtime()?;
        let mut parts = args.split_whitespace();
        let sub = parts.next();
        let rest: Vec<&str> = parts.collect();

        match sub {
            None => Ok(CommandResult::Output(render_picker(runtime))),
            Some("detect") => Ok(CommandResult::Output(render_list(
                &(runtime.detect_ides)(),
                "IDE detection results",
            ))),
            Some("status") => Ok(CommandResult::Output(render_status(runtime))),
            Some("select") => handle_select(runtime, &rest),
            Some("clear") => handle_clear(runtime),
            Some("reconnect") => handle_reconnect(runtime),
            Some("help" | "?") => Ok(CommandResult::Output(render_help())),
            Some(other) => Ok(CommandResult::Output(format!(
                "Unknown ide subcommand: '{}'\n\n{}",
                other,
                render_help()
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// Subcommand handlers
// ---------------------------------------------------------------------------

fn handle_select(runtime: IdeCommandRuntime, rest: &[&str]) -> Result<CommandResult> {
    let Some(id) = rest.first().copied() else {
        return Ok(CommandResult::Output(
            "Usage: /ide select <id>\n\nRun `/ide detect` to see the available IDE ids."
                .to_string(),
        ));
    };
    match (runtime.select_ide)(id) {
        Ok(()) => {
            let ides = (runtime.detect_ides)();
            Ok(CommandResult::Output(render_list(
                &ides,
                &format!("Selected IDE: {}", id),
            )))
        }
        Err(e) => Ok(CommandResult::Output(format!(
            "Failed to select IDE: {}",
            e
        ))),
    }
}

fn handle_clear(runtime: IdeCommandRuntime) -> Result<CommandResult> {
    match (runtime.clear_selection)() {
        Ok(()) => Ok(CommandResult::Output("Cleared IDE selection.".to_string())),
        Err(e) => Ok(CommandResult::Output(format!(
            "Failed to clear IDE selection: {}",
            e
        ))),
    }
}

fn handle_reconnect(runtime: IdeCommandRuntime) -> Result<CommandResult> {
    match (runtime.reconnect_selected)() {
        Ok(()) => Ok(CommandResult::Output(
            "Scheduled an IDE MCP bridge reconnect.".to_string(),
        )),
        Err(e) => Ok(CommandResult::Output(format!("Reconnect failed: {}", e))),
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn render_help() -> String {
    "IDE integration (MCP bridge) management.\n\n\
     Usage:\n  \
       /ide                   -- show this help\n  \
       /ide detect            -- run OS-level detection, print results\n  \
       /ide status            -- show detected IDEs + current selection\n  \
       /ide select <id>       -- persist selection and trigger bridge reconnect\n  \
       /ide clear             -- remove the persisted selection\n  \
       /ide reconnect         -- re-establish MCP bridge for the selected IDE\n\n\
     Supported IDEs: vscode, cursor, intellij, goland, pycharm, rubymine, webstorm.\n\n\
     Detection uses PATH lookups (e.g. `code`, `cursor`) and terminal env\n\
     vars (TERM_PROGRAM, VSCODE_PID, IDEA_INITIAL_DIRECTORY) to decide if\n\
     each IDE is installed and/or currently running.\n\n\
     Selection is persisted under `selectedIde` in\n\
     `{data_root}/settings.json` (usually `~/.allthecodes/settings.json`).\n"
        .to_string()
}

fn render_picker(runtime: IdeCommandRuntime) -> String {
    let mut text = render_status(runtime);
    text.push_str("\n\nUse `/ide select <id>` to pick an IDE, `/ide clear` to reset, or `/ide help` for details.");
    text
}

fn render_status(runtime: IdeCommandRuntime) -> String {
    let ides = (runtime.detect_ides)();
    let selected = (runtime.selected_ide)();
    let heading = match &selected {
        Some(id) => format!("IDE status (selected: {}):", id),
        None => "IDE status (no selection):".to_string(),
    };
    render_list(&ides, &heading)
}

fn render_list(ides: &[IdeInfo], heading: &str) -> String {
    let mut lines = Vec::new();
    lines.push(heading.to_string());
    lines.push(String::new());

    if ides.is_empty() {
        lines.push("  (no IDEs known)".to_string());
        return lines.join("\n");
    }

    for info in ides {
        let installed = if info.installed { "yes" } else { "no " };
        let running = if info.running { "yes" } else { "no " };
        let marker = if info.selected { "*" } else { " " };
        let connection = info.connection_state.as_deref().unwrap_or("disconnected");
        lines.push(format!(
            "  {} {:<9} {:<24} installed={} running={} connection={}",
            marker, info.id, info.name, installed, running, connection
        ));
        if let Some(error) = &info.error {
            lines.push(format!("      error={error}"));
        }
    }
    lines.push(String::new());
    lines.push("  * = currently selected".to_string());
    lines.push("  Select with: /ide select <id>".to_string());
    lines.join("\n")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, OnceLock};
    use tempfile::TempDir;

    struct HomeGuard {
        previous: Option<String>,
    }

    impl HomeGuard {
        fn set(path: &Path) -> Self {
            let previous = std::env::var("ALLTHECODES_HOME").ok();
            std::env::set_var("ALLTHECODES_HOME", path);
            Self { previous }
        }
    }

    impl Drop for HomeGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(v) => std::env::set_var("ALLTHECODES_HOME", v),
                None => std::env::remove_var("ALLTHECODES_HOME"),
            }
        }
    }

    fn selected_slot() -> &'static Mutex<Option<String>> {
        static SELECTED: OnceLock<Mutex<Option<String>>> = OnceLock::new();
        SELECTED.get_or_init(|| Mutex::new(None))
    }

    fn selected() -> Option<String> {
        selected_slot()
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }

    fn detect() -> Vec<IdeInfo> {
        let selected = selected();
        ["vscode", "cursor", "intellij"]
            .into_iter()
            .map(|id| IdeInfo {
                id: id.to_string(),
                name: match id {
                    "vscode" => "Visual Studio Code",
                    "cursor" => "Cursor",
                    _ => "IntelliJ IDEA",
                }
                .to_string(),
                installed: false,
                running: false,
                selected: selected.as_deref() == Some(id),
                connection_state: None,
                error: None,
            })
            .collect()
    }

    fn select(id: &str) -> Result<()> {
        if !matches!(id, "vscode" | "cursor" | "intellij") {
            anyhow::bail!("unknown IDE id '{}'", id);
        }
        *selected_slot()
            .lock()
            .unwrap_or_else(|err| err.into_inner()) = Some(id.to_string());
        Ok(())
    }

    fn clear() -> Result<()> {
        *selected_slot()
            .lock()
            .unwrap_or_else(|err| err.into_inner()) = None;
        Ok(())
    }

    fn reconnect() -> Result<()> {
        if selected().is_none() {
            anyhow::bail!("no IDE is currently selected; run `/ide select <id>` first");
        }
        Ok(())
    }

    fn install_test_runtime() {
        let _ = clear();
        set_ide_command_runtime(IdeCommandRuntime {
            detect_ides: detect,
            selected_ide: selected,
            select_ide: select,
            clear_selection: clear,
            reconnect_selected: reconnect,
        });
    }

    fn test_ctx() -> CommandContext {
        CommandContext {
            messages: Vec::new(),
            cwd: PathBuf::from("/test/project"),
            app_state: Default::default(),
            session_id: SessionId::new(),
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn no_args_shows_help() {
        let tmp = TempDir::new().unwrap();
        let _guard = HomeGuard::set(tmp.path());
        install_test_runtime();

        let handler = IdeHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("IDE status"));
                assert!(text.contains("/ide select"));
                assert!(text.contains("/ide select"));
            }
            _ => panic!("expected Output"),
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn detect_lists_supported_ides() {
        let tmp = TempDir::new().unwrap();
        let _guard = HomeGuard::set(tmp.path());
        install_test_runtime();

        let handler = IdeHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("detect", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("IDE detection results"));
                assert!(text.contains("vscode"));
                assert!(text.contains("cursor"));
                assert!(text.contains("intellij"));
            }
            _ => panic!("expected Output"),
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn status_reports_no_selection_by_default() {
        let tmp = TempDir::new().unwrap();
        let _guard = HomeGuard::set(tmp.path());
        install_test_runtime();

        let handler = IdeHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("status", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("no selection"));
            }
            _ => panic!("expected Output"),
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn unknown_subcommand_returns_help_hint() {
        let tmp = TempDir::new().unwrap();
        let _guard = HomeGuard::set(tmp.path());
        install_test_runtime();

        let handler = IdeHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("wat", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Unknown ide subcommand"));
                assert!(text.contains("/ide detect"));
            }
            _ => panic!("expected Output"),
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn select_without_id_shows_usage() {
        let tmp = TempDir::new().unwrap();
        let _guard = HomeGuard::set(tmp.path());
        install_test_runtime();

        let handler = IdeHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("select", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Usage: /ide select <id>"));
            }
            _ => panic!("expected Output"),
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn select_then_clear_round_trip() {
        let tmp = TempDir::new().unwrap();
        let _guard = HomeGuard::set(tmp.path());
        install_test_runtime();

        let handler = IdeHandler;
        let mut ctx = test_ctx();

        // Select vscode.
        let result = handler.execute("select vscode", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Selected IDE: vscode"));
            }
            _ => panic!("expected Output"),
        }
        assert_eq!(selected().as_deref(), Some("vscode"));

        // Clear.
        let result = handler.execute("clear", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Cleared IDE selection"));
            }
            _ => panic!("expected Output"),
        }
        assert!(selected().is_none());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn select_unknown_id_reports_error() {
        let tmp = TempDir::new().unwrap();
        let _guard = HomeGuard::set(tmp.path());
        install_test_runtime();

        let handler = IdeHandler;
        let mut ctx = test_ctx();
        let result = handler
            .execute("select nonexistent", &mut ctx)
            .await
            .unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(
                    text.contains("Failed to select IDE"),
                    "unexpected output: {}",
                    text
                );
            }
            _ => panic!("expected Output"),
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn reconnect_without_selection_reports_error() {
        let tmp = TempDir::new().unwrap();
        let _guard = HomeGuard::set(tmp.path());
        install_test_runtime();

        let handler = IdeHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("reconnect", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Reconnect failed"));
            }
            _ => panic!("expected Output"),
        }
    }
}
