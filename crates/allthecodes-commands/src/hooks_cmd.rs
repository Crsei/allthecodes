//! `/hooks` slash command — read-only browser for the merged hook tree.
//!
//! Rust issue #40. The command aggregates hooks declared across every
//! settings layer (managed / user / project / local) plus the subagent
//! hooks that the engine fires internally, then renders the result as an
//! indented tree grouped by event → matcher → hook command.
//!
//! This is intentionally read-only: in-TUI editing is cheap for keybindings
//! (one flat file) but error-prone for hooks, where the same event key is
//! spread across multiple files. The browser instead surfaces every source
//! with a badge and offers an editor-jump subcommand that opens the
//! settings file that owns each layer.
//!
//! ## Subcommands
//!
//! | Command                         | Behavior                              |
//! |---------------------------------|---------------------------------------|
//! | `/hooks`                        | full merged tree (all layers)         |
//! | `/hooks list [event]`           | tree filtered to a single event       |
//! | `/hooks path <managed|user|project|local>` | print settings file path     |
//! | `/hooks open <managed|user|project|local>` | open the settings file in `$EDITOR` |

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

use crate::browser::{ensure_and_open, format_open_outcome, render_with_footer, TreeNode};
use crate::{CommandContext, CommandHandler, CommandResult};
use allthecodes_config::settings::{
    load_effective, local_settings_path, managed_settings_path, project_settings_path,
    user_settings_path, SettingsSource,
};

/// Recognised hook event names the tree groups on. Unknown events are still
/// rendered — this list only controls the display order and helps us render
/// empty sections when the user is scanning for "does event X fire at all?".
const KNOWN_EVENTS: &[&str] = &[
    "PreToolUse",
    "PostToolUse",
    "PostToolUseFailure",
    "Stop",
    "SubagentStart",
    "SubagentStop",
    "WorktreeCreate",
    "WorktreeRemove",
    "Notification",
];

pub struct HooksHandler;

#[async_trait]
impl CommandHandler for HooksHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let args = args.trim();
        let mut parts = args.split_whitespace();
        let sub = parts.next().unwrap_or("").to_ascii_lowercase();

        match sub.as_str() {
            "" | "list" | "show" => {
                let event_filter = parts.next();
                Ok(CommandResult::Output(render_tree(&ctx.cwd, event_filter)))
            }
            "path" => {
                let layer = parts.next().unwrap_or("");
                Ok(CommandResult::Output(render_path(&ctx.cwd, layer)))
            }
            "open" | "edit" => {
                let layer = parts.next().unwrap_or("");
                Ok(CommandResult::Output(open_layer(&ctx.cwd, layer)))
            }
            other => Ok(CommandResult::Output(format!(
                "Unknown /hooks subcommand '{}'.\n\n\
                 Usage:\n  \
                 /hooks                       — merged hook tree (event → matcher → hook)\n  \
                 /hooks list [event]          — filter to a single event (e.g. PreToolUse)\n  \
                 /hooks path <layer>          — print settings file path\n  \
                 /hooks open <layer>          — create (if missing) and open in $EDITOR\n\n\
                 Layers: managed, user, project, local",
                other
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// Aggregation
// ---------------------------------------------------------------------------

/// One hook entry flattened out of the settings tree, tagged with which
/// layer contributed it. The browser walks every layer and produces one of
/// these per hook entry — later grouped by event and matcher.
#[derive(Debug, Clone)]
struct FlatHook {
    event: String,
    matcher: String,
    hook_type: String,
    command: String,
    timeout: Option<u64>,
    source: SettingsSource,
}

fn collect_hooks(cwd: &Path) -> (Vec<FlatHook>, Vec<String>) {
    let mut out = Vec::new();
    let mut issues = Vec::new();
    let loaded = match load_effective(cwd) {
        Ok(l) => l,
        Err(e) => {
            issues.push(format!("failed to load settings: {}", e));
            return (out, issues);
        }
    };

    for (source, raw) in [
        (SettingsSource::Managed, loaded.managed.as_ref()),
        (SettingsSource::User, loaded.user.as_ref()),
        (SettingsSource::Project, loaded.project.as_ref()),
        (SettingsSource::Local, loaded.local.as_ref()),
    ] {
        let Some(raw) = raw else { continue };
        let Some(hooks) = raw.hooks.as_ref() else {
            continue;
        };
        for (event, value) in hooks {
            flatten_event(event, value, source, &mut out, &mut issues);
        }
    }

    (out, issues)
}

fn flatten_event(
    event: &str,
    value: &Value,
    source: SettingsSource,
    out: &mut Vec<FlatHook>,
    issues: &mut Vec<String>,
) {
    let Some(arr) = value.as_array() else {
        issues.push(format!(
            "{}: event '{}' is not an array",
            source.as_str(),
            event
        ));
        return;
    };

    for config in arr {
        let matcher = config
            .get("matcher")
            .and_then(|v| v.as_str())
            .unwrap_or("*")
            .to_string();
        let hooks = config.get("hooks").and_then(|v| v.as_array());
        let Some(hooks) = hooks else {
            issues.push(format!(
                "{}: '{}' config missing 'hooks' array",
                source.as_str(),
                event
            ));
            continue;
        };
        for entry in hooks {
            let hook_type = entry
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("command")
                .to_string();
            let command = entry
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("(missing)")
                .to_string();
            let timeout = entry.get("timeout").and_then(|v| v.as_u64());
            out.push(FlatHook {
                event: event.to_string(),
                matcher: matcher.clone(),
                hook_type,
                command,
                timeout,
                source,
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn render_tree(cwd: &Path, event_filter: Option<&str>) -> String {
    let (flat, issues) = collect_hooks(cwd);
    let filtered: Vec<&FlatHook> = match event_filter {
        Some(ev) => flat
            .iter()
            .filter(|h| h.event.eq_ignore_ascii_case(ev))
            .collect(),
        None => flat.iter().collect(),
    };

    // Group by event, then by matcher, preserving original source order.
    // BTreeMap ensures stable, deterministic rendering.
    let mut by_event: BTreeMap<String, BTreeMap<String, Vec<&FlatHook>>> = BTreeMap::new();
    for h in &filtered {
        by_event
            .entry(h.event.clone())
            .or_default()
            .entry(h.matcher.clone())
            .or_default()
            .push(h);
    }

    // Preserve "known event" ordering, then append unknown events.
    let mut event_order: Vec<String> = KNOWN_EVENTS
        .iter()
        .filter(|ev| by_event.contains_key(**ev))
        .map(|ev| ev.to_string())
        .collect();
    for ev in by_event.keys() {
        if !event_order.contains(ev) {
            event_order.push(ev.clone());
        }
    }

    let mut roots: Vec<TreeNode> = Vec::new();
    for event in &event_order {
        let Some(matchers) = by_event.get(event) else {
            continue;
        };
        let mut event_node = TreeNode::leaf(event.clone());
        for (matcher, entries) in matchers {
            let mut matcher_node = TreeNode::leaf(format!("matcher: {}", matcher));
            for entry in entries {
                let mut label = format!("{}: {}", entry.hook_type, short_command(&entry.command));
                if let Some(t) = entry.timeout {
                    label.push_str(&format!(" (timeout {}s)", t));
                }
                matcher_node.push_child(
                    TreeNode::leaf(label).with_badge(entry.source.as_str().to_string()),
                );
            }
            event_node.push_child(matcher_node);
        }
        roots.push(event_node);
    }

    let title = match event_filter {
        Some(ev) => format!("Hooks — {}", ev),
        None => "Hooks".to_string(),
    };
    let mut footer = String::new();
    if !issues.is_empty() {
        footer.push_str("Issues:\n");
        for issue in &issues {
            footer.push_str(&format!("  - {}\n", issue));
        }
        footer.push('\n');
    }
    footer.push_str(
        "Source precedence (low→high): managed → user → project → local.\n\
         Use `/hooks open <layer>` to edit a specific settings file.\n",
    );

    render_with_footer(&title, &roots, &footer)
}

fn short_command(cmd: &str) -> String {
    // Hook commands can be arbitrarily long shell pipelines; keep the tree
    // readable by truncating while leaving enough context to recognise.
    const MAX: usize = 80;
    let first_line = cmd.lines().next().unwrap_or(cmd);
    if first_line.chars().count() <= MAX {
        return first_line.to_string();
    }
    let truncated: String = first_line.chars().take(MAX).collect();
    format!("{}…", truncated)
}

fn render_path(cwd: &Path, layer: &str) -> String {
    match resolve_layer_path(cwd, layer) {
        Some(path) => path.display().to_string(),
        None => usage_layers(),
    }
}

fn open_layer(cwd: &Path, layer: &str) -> String {
    let Some(path) = resolve_layer_path(cwd, layer) else {
        return usage_layers();
    };
    // Minimal JSON skeleton — we only write if the file is absent and the
    // user asked to open it. Matches the settings schema so downstream
    // loaders do not trip on an empty file.
    let template = "{\n  \"hooks\": {}\n}\n";
    let outcome = ensure_and_open(&path, template);
    format_open_outcome(&outcome, &path)
}

fn resolve_layer_path(cwd: &Path, layer: &str) -> Option<PathBuf> {
    match layer.to_ascii_lowercase().as_str() {
        "managed" | "policy" => Some(managed_settings_path()),
        "user" | "global" => Some(user_settings_path()),
        "project" => Some(project_settings_path(cwd)),
        "local" | "override" => Some(local_settings_path(cwd)),
        _ => None,
    }
}

fn usage_layers() -> String {
    "Usage: /hooks path|open <layer> where layer is one of: managed, user, project, local."
        .to_string()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
    use allthecodes_engine::types::app_state::AppState;
    use serde_json::json;
    use std::path::PathBuf;

    fn make_ctx() -> CommandContext {
        CommandContext {
            messages: vec![],
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            app_state: AppState::default(),
            session_id: SessionId::new(),
        }
    }

    #[tokio::test]
    async fn unknown_subcommand_returns_usage() {
        let handler = HooksHandler;
        let mut ctx = make_ctx();
        let result = handler.execute("xyzzy", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(s) => {
                assert!(s.contains("Unknown /hooks"));
                assert!(s.contains("Layers"));
            }
            _ => panic!("expected Output"),
        }
    }

    #[tokio::test]
    async fn default_emits_hooks_title() {
        let handler = HooksHandler;
        let mut ctx = make_ctx();
        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(s) => assert!(s.contains("Hooks")),
            _ => panic!("expected Output"),
        }
    }

    #[tokio::test]
    async fn path_requires_known_layer() {
        let handler = HooksHandler;
        let mut ctx = make_ctx();
        let bogus = handler.execute("path banana", &mut ctx).await.unwrap();
        match bogus {
            CommandResult::Output(s) => assert!(s.contains("Usage")),
            _ => panic!("expected Output"),
        }
        let good = handler.execute("path user", &mut ctx).await.unwrap();
        match good {
            CommandResult::Output(s) => assert!(s.contains("settings.json")),
            _ => panic!("expected Output"),
        }
    }

    #[test]
    fn flatten_event_extracts_hooks() {
        let value = json!([
            {
                "matcher": "Bash",
                "hooks": [
                    { "type": "command", "command": "echo pre", "timeout": 15 },
                    { "type": "command", "command": "echo also-pre" }
                ]
            }
        ]);
        let mut flat = Vec::new();
        let mut issues = Vec::new();
        flatten_event(
            "PreToolUse",
            &value,
            SettingsSource::User,
            &mut flat,
            &mut issues,
        );
        assert_eq!(flat.len(), 2);
        assert_eq!(flat[0].event, "PreToolUse");
        assert_eq!(flat[0].matcher, "Bash");
        assert_eq!(flat[0].command, "echo pre");
        assert_eq!(flat[0].timeout, Some(15));
        assert_eq!(flat[1].timeout, None);
        assert!(issues.is_empty());
    }

    #[test]
    fn flatten_event_reports_bad_shape() {
        let value = json!("not-an-array");
        let mut flat = Vec::new();
        let mut issues = Vec::new();
        flatten_event(
            "PreToolUse",
            &value,
            SettingsSource::Managed,
            &mut flat,
            &mut issues,
        );
        assert!(flat.is_empty());
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("PreToolUse"));
    }

    #[test]
    fn short_command_truncates_long_values() {
        let long: String = "x".repeat(120);
        let short = short_command(&long);
        assert!(short.ends_with("…"));
        assert!(short.chars().count() < long.chars().count());
    }

    #[test]
    fn resolve_layer_path_maps_known_layers() {
        let cwd = PathBuf::from("/tmp/nowhere");
        assert!(resolve_layer_path(&cwd, "managed").is_some());
        assert!(resolve_layer_path(&cwd, "user").is_some());
        assert!(resolve_layer_path(&cwd, "project").is_some());
        assert!(resolve_layer_path(&cwd, "local").is_some());
        assert!(resolve_layer_path(&cwd, "banana").is_none());
    }
}
