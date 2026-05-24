//! IDE detection, selection persistence, and dynamic MCP bridge config.
//!
//! This is the crate-owned version of the root `ide` module. Hosts can wire
//! these APIs directly into `/ide`, IPC subsystem handlers, and MCP discovery.

use std::path::PathBuf;
use std::sync::LazyLock;

use allthecodes_ipc_protocol::subsystem_events::{IdeEvent, SubsystemEvent};
use allthecodes_ipc_protocol::subsystem_types::IdeInfo;
use allthecodes_mcp::McpServerConfig;
use anyhow::{Context, Result};
use parking_lot::Mutex;
use serde_json::{json, Value};
use tokio::sync::broadcast;
use tracing::warn;

static EVENT_TX: LazyLock<Mutex<Option<broadcast::Sender<SubsystemEvent>>>> =
    LazyLock::new(|| Mutex::new(None));

/// Inject the event sender used by IDE selection and reconnect events.
pub fn set_event_sender(tx: broadcast::Sender<SubsystemEvent>) {
    *EVENT_TX.lock() = Some(tx);
}

fn emit_event(event: SubsystemEvent) {
    if let Some(tx) = EVENT_TX.lock().as_ref() {
        let _ = tx.send(event);
    }
}

struct IdeSpec {
    id: &'static str,
    name: &'static str,
    binaries: &'static [&'static str],
    term_programs: &'static [&'static str],
    env_markers: &'static [&'static str],
}

const IDE_SPECS: &[IdeSpec] = &[
    IdeSpec {
        id: "vscode",
        name: "Visual Studio Code",
        binaries: &["code", "code-insiders"],
        term_programs: &["vscode"],
        env_markers: &["VSCODE_PID", "VSCODE_IPC_HOOK"],
    },
    IdeSpec {
        id: "cursor",
        name: "Cursor",
        binaries: &["cursor"],
        term_programs: &["cursor"],
        env_markers: &["CURSOR_PID"],
    },
    IdeSpec {
        id: "intellij",
        name: "IntelliJ IDEA",
        binaries: &["idea"],
        term_programs: &["JetBrains.IntelliJIdea"],
        env_markers: &["IDEA_INITIAL_DIRECTORY", "JEDITERM_SOURCE"],
    },
    IdeSpec {
        id: "goland",
        name: "GoLand",
        binaries: &["goland"],
        term_programs: &["JetBrains.GoLand"],
        env_markers: &[],
    },
    IdeSpec {
        id: "pycharm",
        name: "PyCharm",
        binaries: &["pycharm"],
        term_programs: &["JetBrains.PyCharm"],
        env_markers: &[],
    },
    IdeSpec {
        id: "rubymine",
        name: "RubyMine",
        binaries: &["rubymine"],
        term_programs: &["JetBrains.RubyMine"],
        env_markers: &[],
    },
    IdeSpec {
        id: "webstorm",
        name: "WebStorm",
        binaries: &["webstorm"],
        term_programs: &["JetBrains.WebStorm"],
        env_markers: &[],
    },
];

/// Run OS-level detection for every supported IDE.
pub fn detect_ides() -> Vec<IdeInfo> {
    let selected = selected_ide();
    IDE_SPECS
        .iter()
        .map(|spec| IdeInfo {
            id: spec.id.to_string(),
            name: spec.name.to_string(),
            installed: is_installed(spec),
            running: is_running(spec),
            selected: selected.as_deref() == Some(spec.id),
            connection_state: None,
            error: None,
        })
        .collect()
}

fn is_installed(spec: &IdeSpec) -> bool {
    for bin in spec.binaries {
        if which::which(bin).is_ok() {
            return true;
        }
    }

    #[cfg(windows)]
    {
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
            let base = PathBuf::from(local_app_data).join("Programs");
            let candidates: &[&str] = match spec.id {
                "vscode" => &[
                    "Microsoft VS Code/Code.exe",
                    "Microsoft VS Code Insiders/Code - Insiders.exe",
                ],
                "cursor" => &["cursor/Cursor.exe"],
                _ => &[],
            };
            for rel in candidates {
                if base.join(rel).exists() {
                    return true;
                }
            }
        }
    }

    false
}

fn is_running(spec: &IdeSpec) -> bool {
    if let Ok(term_program) = std::env::var("TERM_PROGRAM") {
        for tp in spec.term_programs {
            if term_program.eq_ignore_ascii_case(tp) {
                return true;
            }
        }
    }
    for marker in spec.env_markers {
        if std::env::var_os(marker).is_some() {
            return true;
        }
    }
    false
}

fn settings_path() -> PathBuf {
    allthecodes_config::paths::data_root().join("settings.json")
}

/// Read the currently selected IDE id, if any.
pub fn selected_ide() -> Option<String> {
    let path = settings_path();
    let text = std::fs::read_to_string(&path).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    value
        .get("selectedIde")
        .and_then(Value::as_str)
        .map(|s| s.to_string())
}

/// Persist `id` as the selected IDE and emit selection/reconnect events.
pub fn select_ide(id: &str) -> Result<()> {
    validate_ide_id(id)?;
    write_selection(Some(id))?;
    emit_event(SubsystemEvent::Ide(IdeEvent::SelectionChanged {
        ide_id: Some(id.to_string()),
    }));
    emit_event(SubsystemEvent::Ide(IdeEvent::ConnectionStateChanged {
        ide_id: id.to_string(),
        state: "connecting".to_string(),
        error: None,
    }));
    Ok(())
}

/// Remove the persisted IDE selection.
pub fn clear_selection() -> Result<()> {
    write_selection(None)?;
    emit_event(SubsystemEvent::Ide(IdeEvent::SelectionChanged {
        ide_id: None,
    }));
    Ok(())
}

/// Re-publish a connection event for the selected IDE.
pub fn reconnect_selected() -> Result<()> {
    let Some(id) = selected_ide() else {
        anyhow::bail!("no IDE is currently selected; run `/ide select <id>` first");
    };
    emit_event(SubsystemEvent::Ide(IdeEvent::ConnectionStateChanged {
        ide_id: id,
        state: "connecting".to_string(),
        error: None,
    }));
    Ok(())
}

fn validate_ide_id(id: &str) -> Result<()> {
    if IDE_SPECS.iter().any(|s| s.id == id) {
        Ok(())
    } else {
        let known = IDE_SPECS
            .iter()
            .map(|s| s.id)
            .collect::<Vec<_>>()
            .join(", ");
        anyhow::bail!("unknown IDE id '{}' (known: {})", id, known)
    }
}

fn write_selection(id: Option<&str>) -> Result<()> {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create settings dir {}", parent.display()))?;
    }

    let mut value: Value = match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
            warn!(
                path = %path.display(),
                error = %e,
                "ide: existing settings.json is not valid JSON; overwriting only selectedIde"
            );
            json!({})
        }),
        Err(_) => json!({}),
    };

    let obj = value
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("settings.json root is not a JSON object"))?;

    match id {
        Some(id) => {
            obj.insert("selectedIde".to_string(), Value::String(id.to_string()));
        }
        None => {
            obj.remove("selectedIde");
        }
    }

    let pretty = serde_json::to_string_pretty(&value)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, pretty).with_context(|| format!("write {}", tmp.display()))?;
    std::fs::rename(&tmp, &path)
        .with_context(|| format!("rename {} -> {}", tmp.display(), path.display()))?;
    Ok(())
}

/// Build a dynamic MCP config for the selected IDE bridge.
pub fn ide_mcp_config(id: &str) -> Option<McpServerConfig> {
    let spec = IDE_SPECS.iter().find(|s| s.id == id)?;
    let binary = spec
        .binaries
        .iter()
        .find(|b| which::which(b).is_ok())
        .copied()
        .or_else(|| spec.binaries.first().copied())?;

    let args = match id {
        "vscode" | "cursor" => vec!["--mcp-server".to_string()],
        _ => vec!["mcp".to_string()],
    };

    Some(McpServerConfig {
        name: format!("ide-{}", id),
        transport: "stdio".to_string(),
        command: Some(binary.to_string()),
        args: Some(args),
        url: None,
        headers: None,
        oauth: None,
        env: None,
        browser_mcp: None,
        disabled: None,
    })
}

/// Return the IDE MCP config to inject into MCP discovery.
pub fn selected_ide_mcp_config() -> Vec<McpServerConfig> {
    match selected_ide() {
        Some(id) => ide_mcp_config(&id).into_iter().collect(),
        None => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::TempDir;

    struct HomeGuard {
        previous: Option<String>,
    }

    impl HomeGuard {
        fn set(path: &Path) -> Self {
            let previous = std::env::var("CC_RUST_HOME").ok();
            std::env::set_var("CC_RUST_HOME", path);
            Self { previous }
        }
    }

    impl Drop for HomeGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(v) => std::env::set_var("CC_RUST_HOME", v),
                None => std::env::remove_var("CC_RUST_HOME"),
            }
        }
    }

    #[test]
    #[serial_test::serial]
    fn detect_ides_returns_full_registry_without_panicking() {
        let ides = detect_ides();
        let ids: Vec<_> = ides.iter().map(|i| i.id.as_str()).collect();
        assert!(ids.contains(&"vscode"));
        assert!(ids.contains(&"cursor"));
        assert!(ids.contains(&"intellij"));
    }

    #[test]
    #[serial_test::serial]
    fn selected_ide_round_trips_through_settings() {
        let tmp = TempDir::new().expect("tmp");
        let _guard = HomeGuard::set(tmp.path());

        assert!(selected_ide().is_none());
        select_ide("vscode").expect("select_ide");
        assert_eq!(selected_ide().as_deref(), Some("vscode"));

        let settings_file = tmp.path().join("settings.json");
        let text = std::fs::read_to_string(&settings_file).expect("settings.json");
        let value: Value = serde_json::from_str(&text).expect("parse");
        assert_eq!(value["selectedIde"], "vscode");

        clear_selection().expect("clear_selection");
        assert!(selected_ide().is_none());
    }

    #[test]
    #[serial_test::serial]
    fn selected_ide_preserves_other_settings_keys() {
        let tmp = TempDir::new().expect("tmp");
        let _guard = HomeGuard::set(tmp.path());

        let settings_file = tmp.path().join("settings.json");
        std::fs::write(
            &settings_file,
            serde_json::to_string_pretty(&json!({
                "theme": "dark",
                "model": "claude-opus-4-7"
            }))
            .unwrap(),
        )
        .unwrap();

        select_ide("cursor").expect("select_ide");

        let text = std::fs::read_to_string(&settings_file).expect("settings.json");
        let value: Value = serde_json::from_str(&text).expect("parse");
        assert_eq!(value["theme"], "dark");
        assert_eq!(value["model"], "claude-opus-4-7");
        assert_eq!(value["selectedIde"], "cursor");
    }

    #[test]
    #[serial_test::serial]
    fn select_ide_rejects_unknown_id() {
        let tmp = TempDir::new().expect("tmp");
        let _guard = HomeGuard::set(tmp.path());

        let err = select_ide("nonexistent-ide").expect_err("should reject unknown id");
        assert!(format!("{}", err).contains("unknown IDE id"));
    }

    #[test]
    #[serial_test::serial]
    fn reconnect_selected_errors_when_nothing_selected() {
        let tmp = TempDir::new().expect("tmp");
        let _guard = HomeGuard::set(tmp.path());

        let err = reconnect_selected().expect_err("should error");
        assert!(format!("{}", err).contains("no IDE is currently selected"));
    }

    #[test]
    fn ide_mcp_config_returns_stdio_spawn_for_vscode_and_cursor() {
        if let Some(cfg) = ide_mcp_config("vscode") {
            assert_eq!(cfg.name, "ide-vscode");
            assert_eq!(cfg.transport, "stdio");
            assert!(cfg.command.is_some());
            assert!(cfg
                .args
                .clone()
                .unwrap_or_default()
                .iter()
                .any(|a| a == "--mcp-server"));
        }
        if let Some(cfg) = ide_mcp_config("cursor") {
            assert_eq!(cfg.name, "ide-cursor");
            assert_eq!(cfg.transport, "stdio");
        }
    }
}
