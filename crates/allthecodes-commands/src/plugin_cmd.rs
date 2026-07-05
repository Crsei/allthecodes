//! /plugin command — layered plugin-state management (issue #47).
//!
//! The `/plugin` UI exposes three distinct layers:
//!
//! - **Install** — present in `installed_plugins.json` on disk.
//! - **Enablement** — `PluginStatus::Installed` (enabled) vs `Disabled`.
//! - **Active** — loaded into the current session's in-memory registry.
//!
//! A plugin can be installed-but-disabled (listed on disk, Enable=false), or
//! installed-and-enabled but not active (persisted changes not yet reloaded
//! into the running session).
//!
//! Subcommands:
//! - `/plugin` or `/plugin list`          — all plugins with all three columns
//! - `/plugin installed`                  — only enabled/installed plugins
//! - `/plugin disabled`                   — only disabled plugins
//! - `/plugin errors`                     — only plugins with an error status
//! - `/plugin status`                     — summary + drift diagnostics
//! - `/plugin enable <plugin-id>`         — flip status to Installed
//! - `/plugin disable <plugin-id>`        — flip status to Disabled
//! - `/plugin uninstall <plugin-id>`      — drop from installed_plugins.json
//! - `/plugin uninstall <id> --purge`     — also delete the cache dir
//! - `/plugin install <source>`           — install plugin from marketplace/source
//! - `/plugin search <query>`             — search installed/active/cache plugins
//! - `/plugin marketplace [list|refresh|search <q>]` — browse/refresh marketplace
//! - `/plugin update [id]`                — update plugin(s)
//! - `/plugin validate [id]`              — validate installed plugin(s)
//! - `/plugin info <id>`                  — detailed plugin info

use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use std::sync::{OnceLock, RwLock};

use crate::{
    search_format::format_discovery_search_results, CommandContext, CommandHandler, CommandResult,
};
use allthecodes_plugins::{PluginEntry, PluginStatus};

#[derive(Clone, Copy)]
pub struct PluginCommandRuntime {
    pub load_installed_plugins: fn() -> Vec<PluginEntry>,
    pub save_installed_plugins: fn(&[PluginEntry]) -> Result<()>,
    pub get_all_plugins: fn() -> Vec<PluginEntry>,
    pub needs_refresh: fn() -> Option<String>,
    pub find_plugin: fn(&str) -> Option<PluginEntry>,
    pub set_plugin_status: fn(&str, PluginStatus) -> Option<PluginEntry>,
    pub register_plugin: fn(PluginEntry),
    pub emit_event_external: fn(allthecodes_ipc_protocol::subsystem_events::SubsystemEvent),
    pub uninstall_plugin: fn(&str, bool) -> Result<Option<PluginEntry>>,
    // Marketplace / installation / validation extensions
    pub install_plugin: fn(&str, Option<&str>) -> Result<String>,
    pub list_marketplace: fn(&str) -> Result<Vec<String>>,
    pub refresh_marketplace_cache: fn() -> Result<String>,
    pub update_plugin: fn(&str) -> Result<String>,
    pub validate_plugin: fn(&str) -> Result<Vec<String>>,
    pub get_plugin_info: fn(&str) -> Result<String>,
}

static PLUGIN_RUNTIME: OnceLock<RwLock<Option<PluginCommandRuntime>>> = OnceLock::new();

pub fn set_plugin_command_runtime(runtime: PluginCommandRuntime) {
    let slot = PLUGIN_RUNTIME.get_or_init(|| RwLock::new(None));
    if let Ok(mut guard) = slot.write() {
        *guard = Some(runtime);
    }
}

fn plugin_runtime() -> Result<PluginCommandRuntime> {
    let Some(slot) = PLUGIN_RUNTIME.get() else {
        bail!("Plugin command runtime is unavailable; install PluginCommandRuntime adapter");
    };
    let Ok(guard) = slot.read() else {
        bail!("Plugin command runtime lock is poisoned");
    };
    guard.as_ref().copied().ok_or_else(|| {
        anyhow!("Plugin command runtime is unavailable; install PluginCommandRuntime adapter")
    })
}

/// Handler for `/plugin`.
pub struct PluginHandler;

#[async_trait]
impl CommandHandler for PluginHandler {
    async fn execute(&self, args: &str, _ctx: &mut CommandContext) -> Result<CommandResult> {
        let parts: Vec<&str> = args.split_whitespace().collect();

        match parts.first().copied() {
            None | Some("list") | Some("ls") => handle_list(Filter::All),
            Some("installed") => handle_list(Filter::Installed),
            Some("disabled") => handle_list(Filter::Disabled),
            Some("errors") | Some("error") => handle_list(Filter::Errored),
            Some("status") => handle_status(),
            Some("enable") => {
                let id = parts.get(1).copied().unwrap_or("");
                handle_set_enabled(id, true)
            }
            Some("disable") => {
                let id = parts.get(1).copied().unwrap_or("");
                handle_set_enabled(id, false)
            }
            Some("uninstall") | Some("remove") | Some("rm") => {
                let id = parts.get(1).copied().unwrap_or("");
                // Check for --purge anywhere in the remaining tokens.
                let purge = parts.iter().skip(2).any(|p| *p == "--purge");
                handle_uninstall(id, purge)
            }
            Some("install") => {
                let source = parts.get(1).copied().unwrap_or("");
                let scope = parts.get(2).copied();
                handle_install(source, scope)
            }
            Some("search") => {
                let query = args
                    .trim()
                    .strip_prefix("search")
                    .map(str::trim)
                    .unwrap_or_default();
                handle_search(query)
            }
            Some("marketplace") | Some("mp") => {
                let sub = parts.get(1).copied().unwrap_or("list");
                match sub {
                    "refresh" | "reload" | "sync" => handle_marketplace_refresh(),
                    "search" => {
                        let query = parts.get(2).copied().unwrap_or("");
                        handle_marketplace_search(query)
                    }
                    _ => handle_marketplace_list(),
                }
            }
            Some("update") | Some("upgrade") => {
                let id = parts.get(1).copied().unwrap_or("");
                handle_update(id)
            }
            Some("validate") => {
                let id = parts.get(1).copied().unwrap_or("");
                handle_validate(id)
            }
            Some("info") | Some("inspect") => {
                let id = parts.get(1).copied().unwrap_or("");
                handle_info(id)
            }
            Some("help") => Ok(handle_help()),
            Some(sub) => Ok(CommandResult::Output(format!(
                "Unknown plugin subcommand: '{}'\n{}",
                sub,
                usage_block()
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// Help / usage
// ---------------------------------------------------------------------------

fn usage_block() -> &'static str {
    "Usage:\n  \
       /plugin                          -- list all plugins (layered view)\n  \
       /plugin installed                -- only installed & enabled\n  \
       /plugin disabled                 -- only disabled\n  \
       /plugin errors                   -- only plugins with an error status\n  \
       /plugin status                   -- summary + drift diagnostics\n  \
       /plugin enable <plugin-id>       -- enable plugin\n  \
       /plugin disable <plugin-id>      -- disable plugin\n  \
       /plugin uninstall <plugin-id>    -- remove from installed_plugins.json\n  \
       /plugin uninstall <id> --purge   -- also delete the cache directory\n  \
       /plugin install <source>         -- install plugin from source/marketplace\n  \
       /plugin search <query>           -- search installed/active/marketplace-cache plugins\n  \
       /plugin marketplace              -- list marketplace sources\n  \
       /plugin marketplace refresh      -- refresh marketplace cache\n  \
       /plugin marketplace search <q>   -- search marketplace\n  \
       /plugin update [id]              -- update plugin(s)\n  \
       /plugin validate [id]            -- validate installed plugin(s)\n  \
       /plugin info <id>                -- detailed plugin information"
}

fn handle_help() -> CommandResult {
    CommandResult::Output(format!(
        "Plugin management.\n\n\
         {}\n\n\
         Plugin metadata is persisted at ~/.allthecodes/plugins/installed_plugins.json.\n\
         Cache directories live at   ~/.allthecodes/plugins/cache/{{marketplace}}/{{id}}/.",
        usage_block()
    ))
}

// ---------------------------------------------------------------------------
// List (layered view)
// ---------------------------------------------------------------------------

#[derive(Copy, Clone, PartialEq, Eq)]
enum Filter {
    All,
    Installed,
    Disabled,
    Errored,
}

impl Filter {
    fn label(&self) -> &'static str {
        match self {
            Filter::All => "Plugins",
            Filter::Installed => "Installed plugins",
            Filter::Disabled => "Disabled plugins",
            Filter::Errored => "Plugins with errors",
        }
    }

    fn matches(&self, entry: &PluginEntry) -> bool {
        match self {
            Filter::All => true,
            Filter::Installed => matches!(entry.status, PluginStatus::Installed),
            Filter::Disabled => matches!(entry.status, PluginStatus::Disabled),
            Filter::Errored => matches!(entry.status, PluginStatus::Error(_)),
        }
    }
}

/// Row of the layered-state table for a single plugin.
struct Row {
    id: String,
    version: String,
    install_col: &'static str, // "yes" / "no"
    enabled_col: &'static str, // "yes" / "no" / "error"
    active_col: &'static str,  // "yes" / "no"
    error_detail: Option<String>,
    skills: Vec<String>,
    tools: Vec<String>,
    mcp: Vec<String>,
}

fn handle_list(filter: Filter) -> Result<CommandResult> {
    let runtime = plugin_runtime()?;
    let rows = build_rows(runtime, filter);

    if rows.is_empty() {
        let empty_msg = match filter {
            Filter::All => "No plugins registered.".to_string(),
            other => format!(
                "No plugins match filter '{}'.",
                other.label().to_lowercase()
            ),
        };
        return Ok(CommandResult::Output(empty_msg));
    }

    let mut lines = Vec::new();
    lines.push(format!("{} ({}):", filter.label(), rows.len()));
    lines.push(String::new());
    lines.push(format!(
        "  {:<36} {:<10} {:<9} {:<8} {:<7}",
        "plugin", "version", "installed", "enabled", "active"
    ));
    lines.push(format!(
        "  {:-<36} {:-<10} {:-<9} {:-<8} {:-<7}",
        "", "", "", "", ""
    ));

    for row in &rows {
        lines.push(format!(
            "  {:<36} {:<10} {:<9} {:<8} {:<7}",
            truncate(&row.id, 36),
            truncate(&row.version, 10),
            row.install_col,
            row.enabled_col,
            row.active_col
        ));
        if let Some(ref err) = row.error_detail {
            lines.push(format!("      error: {}", err));
        }
        if !row.skills.is_empty() {
            lines.push(format!("      skills: {}", row.skills.join(", ")));
        }
        if !row.tools.is_empty() {
            lines.push(format!("      tools:  {}", row.tools.join(", ")));
        }
        if !row.mcp.is_empty() {
            lines.push(format!("      mcp:    {}", row.mcp.join(", ")));
        }
    }

    if let Some(reason) = (runtime.needs_refresh)() {
        lines.push(String::new());
        lines.push(format!(
            "Session drift: {} — run /reload-plugins to sync this session.",
            reason
        ));
    }

    Ok(CommandResult::Output(lines.join("\n")))
}

/// Build the layered rows for the view. Combines:
///   * `installed_plugins.json` on disk  — "installed" column
///   * Status within disk entries        — "enabled" column
///   * In-memory registry presence       — "active" column
fn build_rows(runtime: PluginCommandRuntime, filter: Filter) -> Vec<Row> {
    let disk_plugins = (runtime.load_installed_plugins)();
    let in_memory = (runtime.get_all_plugins)();

    use std::collections::HashMap;
    let mut by_id: HashMap<String, (Option<PluginEntry>, Option<PluginEntry>)> = HashMap::new();
    for p in &disk_plugins {
        by_id.entry(p.id.clone()).or_insert((None, None)).0 = Some(p.clone());
    }
    for p in &in_memory {
        by_id.entry(p.id.clone()).or_insert((None, None)).1 = Some(p.clone());
    }

    let mut rows: Vec<Row> = by_id
        .into_iter()
        .filter_map(|(id, (disk, mem))| {
            // Prefer the richest source for display metadata.
            let display_entry = disk.as_ref().or(mem.as_ref())?;
            if !filter.matches(display_entry) {
                return None;
            }

            let install_col = if disk.is_some() { "yes" } else { "no" };
            let enabled_col = match &display_entry.status {
                PluginStatus::Installed => "yes",
                PluginStatus::Disabled => "no",
                PluginStatus::Error(_) => "error",
                PluginStatus::NotInstalled => "no",
            };
            let active_col = if mem.is_some() { "yes" } else { "no" };

            let error_detail = if let PluginStatus::Error(e) = &display_entry.status {
                Some(e.clone())
            } else {
                None
            };

            Some(Row {
                id,
                version: display_entry.version.clone(),
                install_col,
                enabled_col,
                active_col,
                error_detail,
                skills: display_entry.skills.clone(),
                tools: display_entry.tools.clone(),
                mcp: display_entry.mcp_servers.clone(),
            })
        })
        .collect();

    rows.sort_by(|a, b| a.id.cmp(&b.id));
    rows
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max.saturating_sub(3)])
    }
}

// ---------------------------------------------------------------------------
// Status summary
// ---------------------------------------------------------------------------

fn handle_status() -> Result<CommandResult> {
    let runtime = plugin_runtime()?;
    let disk = (runtime.load_installed_plugins)();
    let memory = (runtime.get_all_plugins)();

    if disk.is_empty() && memory.is_empty() {
        return Ok(CommandResult::Output("No plugins registered.".to_string()));
    }

    let mut installed = 0usize;
    let mut disabled = 0usize;
    let mut errored = 0usize;
    let mut not_installed = 0usize;

    for p in &disk {
        match p.status {
            PluginStatus::Installed => installed += 1,
            PluginStatus::Disabled => disabled += 1,
            PluginStatus::Error(_) => errored += 1,
            PluginStatus::NotInstalled => not_installed += 1,
        }
    }

    let mut lines = Vec::new();
    lines.push("Plugin status summary:".to_string());
    lines.push(format!("  - total on disk: {}", disk.len()));
    lines.push(format!("  - active in session: {}", memory.len()));
    lines.push(format!("  - installed (enabled): {}", installed));
    lines.push(format!("  - disabled: {}", disabled));
    lines.push(format!("  - error: {}", errored));
    if not_installed > 0 {
        lines.push(format!("  - not_installed: {}", not_installed));
    }

    if let Some(reason) = (runtime.needs_refresh)() {
        lines.push(String::new());
        lines.push(format!(
            "Session drift: {} — run /reload-plugins to bring this session back in sync.",
            reason
        ));
    } else {
        lines.push(String::new());
        lines.push("Session is in sync with disk.".to_string());
    }

    Ok(CommandResult::Output(lines.join("\n")))
}

// ---------------------------------------------------------------------------
// Enable / Disable (with drift-aware reload hint)
// ---------------------------------------------------------------------------

fn handle_set_enabled(plugin_id: &str, enable: bool) -> Result<CommandResult> {
    if plugin_id.trim().is_empty() {
        let action = if enable { "enable" } else { "disable" };
        bail!("Usage: /plugin {} <plugin-id>", action);
    }

    let runtime = plugin_runtime()?;
    let mut installed_plugins = (runtime.load_installed_plugins)();
    let Some(persisted) = installed_plugins.iter_mut().find(|p| p.id == plugin_id) else {
        return Ok(CommandResult::Output(format!(
            "Plugin '{}' not found in installed plugins.",
            plugin_id
        )));
    };

    let new_status = if enable {
        PluginStatus::Installed
    } else {
        PluginStatus::Disabled
    };

    persisted.status = new_status.clone();
    (runtime.save_installed_plugins)(&installed_plugins)?;

    // Keep in-memory state in sync for current session when possible.
    let before = (runtime.find_plugin)(plugin_id);
    if (runtime.set_plugin_status)(plugin_id, new_status.clone()).is_none() {
        // If not present in memory yet, try an in-place refresh of just this id
        // without nuking the whole registry.
        if let Some(disk_entry) = installed_plugins
            .iter()
            .find(|p| p.id == plugin_id)
            .cloned()
        {
            (runtime.register_plugin)(disk_entry);
        }
    }

    let action_done = if enable { "enabled" } else { "disabled" };
    let mut msg = format!("Plugin '{}' {}.", plugin_id, action_done);

    // After the change, check whether the session still matches disk. If the
    // active plugin list now differs (e.g. enable flipped an entry that isn't
    // yet reflected in discovered tools/skills), emit a RefreshNeeded event.
    if let Some(reason) = (runtime.needs_refresh)() {
        msg.push_str(&format!(
            "\nSession drift: {} — run /reload-plugins to apply.",
            reason
        ));
        emit_refresh_needed(runtime, reason);
    } else if before.is_none() && (runtime.find_plugin)(plugin_id).is_some() {
        // Newly-registered plugin: active tool/skill/mcp sets won't reflect
        // contributions until the session reloads. Signal softly.
        let reason = format!("'{}' added to session", plugin_id);
        msg.push_str("\nNote: run /reload-plugins to refresh contributed tools/skills/mcp.");
        emit_refresh_needed(runtime, reason);
    }

    Ok(CommandResult::Output(msg))
}

fn emit_refresh_needed(runtime: PluginCommandRuntime, reason: String) {
    let event = allthecodes_ipc_protocol::subsystem_events::SubsystemEvent::Plugin(
        allthecodes_ipc_protocol::subsystem_events::PluginEvent::RefreshNeeded { reason },
    );
    (runtime.emit_event_external)(event);
}

// ---------------------------------------------------------------------------
// Uninstall
// ---------------------------------------------------------------------------

fn handle_uninstall(plugin_id: &str, purge: bool) -> Result<CommandResult> {
    if plugin_id.trim().is_empty() {
        bail!("Usage: /plugin uninstall <plugin-id> [--purge]");
    }

    let runtime = plugin_runtime()?;
    let removed = (runtime.uninstall_plugin)(plugin_id, purge)?;

    match removed {
        Some(entry) => {
            let mut msg = format!("Plugin '{}' uninstalled.", entry.id);
            if purge {
                msg.push_str(" Cache directory purged.");
            } else {
                msg.push_str(" (Cache directory kept; re-run with --purge to delete it.)");
            }
            Ok(CommandResult::Output(msg))
        }
        None => Ok(CommandResult::Output(format!(
            "Plugin '{}' is not installed.",
            plugin_id
        ))),
    }
}

// ---------------------------------------------------------------------------
// Install
// ---------------------------------------------------------------------------

fn handle_install(source: &str, scope: Option<&str>) -> Result<CommandResult> {
    if source.trim().is_empty() {
        bail!("Usage: /plugin install <source> [scope]");
    }

    let runtime = plugin_runtime()?;
    match (runtime.install_plugin)(source, scope) {
        Ok(msg) => Ok(CommandResult::Output(msg)),
        Err(e) => Ok(CommandResult::Output(format!("Install failed: {}", e))),
    }
}

fn handle_search(query: &str) -> Result<CommandResult> {
    if query.trim().is_empty() {
        return Ok(CommandResult::Output(
            "Usage: /plugin search <query>".to_string(),
        ));
    }

    match allthecodes_tools::discovery_search::run_plugin_search(
        allthecodes_tools::discovery_search::DiscoverySearchInput {
            query: query.to_string(),
            source_filter: None,
            max_results: 10,
            include_summaries: true,
        },
    ) {
        Ok(output) => {
            let mut notes = vec![
                "Use /plugin status for installed/enabled/active state.",
                "Use ToolSearch source=plugin for exact callable tool schemas.",
            ];
            if let Some(preview) = output.display_preview.as_deref() {
                notes.push(preview);
            }
            let results = serde_json::from_value::<Vec<
                allthecodes_tools::discovery_search::DiscoverySearchResult,
            >>(output.data["results"].clone())
            .unwrap_or_default();
            Ok(CommandResult::Output(format_discovery_search_results(
                "Plugin search results",
                query,
                &results,
                &notes,
            )))
        }
        Err(error) => Ok(CommandResult::Output(format!(
            "Plugin search failed: {error}"
        ))),
    }
}

// ---------------------------------------------------------------------------
// Marketplace
// ---------------------------------------------------------------------------

fn handle_marketplace_list() -> Result<CommandResult> {
    let runtime = plugin_runtime()?;
    match (runtime.list_marketplace)("") {
        Ok(entries) => {
            if entries.is_empty() {
                Ok(CommandResult::Output(
                    "No marketplace sources configured.".to_string(),
                ))
            } else {
                let mut lines = vec!["Marketplace sources:".to_string()];
                for entry in entries {
                    lines.push(format!("  - {}", entry));
                }
                Ok(CommandResult::Output(lines.join("\n")))
            }
        }
        Err(e) => Ok(CommandResult::Output(format!(
            "Failed to list marketplaces: {}",
            e
        ))),
    }
}

fn handle_marketplace_refresh() -> Result<CommandResult> {
    let runtime = plugin_runtime()?;
    match (runtime.refresh_marketplace_cache)() {
        Ok(msg) => Ok(CommandResult::Output(msg)),
        Err(e) => Ok(CommandResult::Output(format!(
            "Marketplace refresh failed: {}",
            e
        ))),
    }
}

fn handle_marketplace_search(query: &str) -> Result<CommandResult> {
    if query.trim().is_empty() {
        bail!("Usage: /plugin marketplace search <query>");
    }
    let runtime = plugin_runtime()?;
    match (runtime.list_marketplace)(query) {
        Ok(results) => {
            if results.is_empty() {
                Ok(CommandResult::Output(format!(
                    "No marketplace results for '{}'.",
                    query
                )))
            } else {
                let mut lines = vec![format!("Marketplace results for '{}':", query)];
                for entry in results {
                    lines.push(format!("  - {}", entry));
                }
                Ok(CommandResult::Output(lines.join("\n")))
            }
        }
        Err(e) => Ok(CommandResult::Output(format!("Search failed: {}", e))),
    }
}

// ---------------------------------------------------------------------------
// Update
// ---------------------------------------------------------------------------

fn handle_update(plugin_id: &str) -> Result<CommandResult> {
    let runtime = plugin_runtime()?;
    match (runtime.update_plugin)(plugin_id) {
        Ok(msg) => Ok(CommandResult::Output(msg)),
        Err(e) => Ok(CommandResult::Output(format!("Update failed: {}", e))),
    }
}

// ---------------------------------------------------------------------------
// Validate
// ---------------------------------------------------------------------------

fn handle_validate(plugin_id: &str) -> Result<CommandResult> {
    let runtime = plugin_runtime()?;
    match (runtime.validate_plugin)(plugin_id) {
        Ok(errors) => {
            if errors.is_empty() {
                let id = if plugin_id.trim().is_empty() {
                    "all plugins"
                } else {
                    plugin_id
                };
                Ok(CommandResult::Output(format!(
                    "{}: no validation issues found.",
                    id
                )))
            } else {
                let mut lines = vec![format!("Validation issues:")];
                for err in &errors {
                    lines.push(format!("  - {}", err));
                }
                Ok(CommandResult::Output(lines.join("\n")))
            }
        }
        Err(e) => Ok(CommandResult::Output(format!("Validation failed: {}", e))),
    }
}

// ---------------------------------------------------------------------------
// Info
// ---------------------------------------------------------------------------

fn handle_info(plugin_id: &str) -> Result<CommandResult> {
    if plugin_id.trim().is_empty() {
        bail!("Usage: /plugin info <plugin-id>");
    }

    let runtime = plugin_runtime()?;
    match (runtime.get_plugin_info)(plugin_id) {
        Ok(info) => Ok(CommandResult::Output(info)),
        Err(e) => Ok(CommandResult::Output(format!(
            "Failed to get plugin info: {}",
            e
        ))),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
    use allthecodes_plugins::{PluginEntry, PluginSource, PluginStatus};
    use allthecodes_tools::discovery_search::{
        install_discovery_search_runtime, DiscoveryNextAction, DiscoveryResultKind,
        DiscoverySearchResult, DiscoverySearchRuntime, DiscoveryStatusSummary,
    };
    use std::path::PathBuf;
    use std::sync::{Mutex, OnceLock};

    fn test_ctx() -> CommandContext {
        CommandContext {
            messages: Vec::new(),
            cwd: PathBuf::from("/test/project"),
            app_state: Default::default(),
            session_id: SessionId::new(),
        }
    }

    fn make_plugin(id: &str, status: PluginStatus) -> PluginEntry {
        PluginEntry {
            id: id.to_string(),
            name: id.to_string(),
            version: "1.0.0".to_string(),
            description: String::new(),
            source: PluginSource::Local {
                path: "/tmp".to_string(),
            },
            status,
            marketplace: None,
            cache_path: None,
            installed_version: Some("1.0.0".to_string()),
            official: false,
            download_url: None,
            homepage: None,
            sha256: None,
            tools: vec![],
            skills: vec![],
            mcp_servers: vec![],
            installed_at: None,
            updated_at: None,
        }
    }

    struct DiscoveryRuntimeGuard;

    impl DiscoveryRuntimeGuard {
        fn install(runtime: DiscoverySearchRuntime) -> Self {
            install_discovery_search_runtime(runtime);
            Self
        }
    }

    impl Drop for DiscoveryRuntimeGuard {
        fn drop(&mut self) {
            install_discovery_search_runtime(DiscoverySearchRuntime::new());
        }
    }

    mod test_plugins {
        use super::*;

        fn disk() -> &'static Mutex<Vec<PluginEntry>> {
            static DISK: OnceLock<Mutex<Vec<PluginEntry>>> = OnceLock::new();
            DISK.get_or_init(|| Mutex::new(Vec::new()))
        }

        fn memory() -> &'static Mutex<Vec<PluginEntry>> {
            static MEMORY: OnceLock<Mutex<Vec<PluginEntry>>> = OnceLock::new();
            MEMORY.get_or_init(|| Mutex::new(Vec::new()))
        }

        fn lock_vec(
            slot: &'static Mutex<Vec<PluginEntry>>,
        ) -> std::sync::MutexGuard<'static, Vec<PluginEntry>> {
            slot.lock().unwrap_or_else(|err| err.into_inner())
        }

        pub fn load_installed_plugins() -> Vec<PluginEntry> {
            lock_vec(disk()).clone()
        }

        pub fn save_installed_plugins(plugins: &[PluginEntry]) -> Result<()> {
            *lock_vec(disk()) = plugins.to_vec();
            Ok(())
        }

        pub fn get_all_plugins() -> Vec<PluginEntry> {
            lock_vec(memory()).clone()
        }

        pub fn find_plugin(id: &str) -> Option<PluginEntry> {
            lock_vec(memory())
                .iter()
                .find(|plugin| plugin.id == id)
                .cloned()
        }

        pub fn set_plugin_status(id: &str, status: PluginStatus) -> Option<PluginEntry> {
            let mut plugins = lock_vec(memory());
            let plugin = plugins.iter_mut().find(|plugin| plugin.id == id)?;
            plugin.status = status;
            Some(plugin.clone())
        }

        pub fn register_plugin(plugin: PluginEntry) {
            let mut plugins = lock_vec(memory());
            if let Some(existing) = plugins.iter_mut().find(|item| item.id == plugin.id) {
                *existing = plugin;
            } else {
                plugins.push(plugin);
            }
        }

        pub fn clear_plugins() {
            lock_vec(disk()).clear();
            lock_vec(memory()).clear();
        }

        pub fn needs_refresh() -> Option<String> {
            let disk = load_installed_plugins();
            let memory = get_all_plugins();
            let mut drift = Vec::new();
            for plugin in &disk {
                match memory.iter().find(|entry| entry.id == plugin.id) {
                    Some(mem)
                        if std::mem::discriminant(&mem.status)
                            == std::mem::discriminant(&plugin.status) => {}
                    Some(_) => drift.push(format!("status changed ({})", plugin.id)),
                    None => drift.push(format!("added on disk ({})", plugin.id)),
                }
            }
            for plugin in &memory {
                if !disk.iter().any(|entry| entry.id == plugin.id) {
                    drift.push(format!("removed from disk ({})", plugin.id));
                }
            }
            if drift.is_empty() {
                None
            } else {
                Some(drift.join("; "))
            }
        }

        pub fn emit_event_external(_: allthecodes_ipc_protocol::subsystem_events::SubsystemEvent) {}

        pub fn uninstall_plugin(plugin_id: &str, _purge: bool) -> Result<Option<PluginEntry>> {
            let mut disk = lock_vec(disk());
            let Some(pos) = disk.iter().position(|plugin| plugin.id == plugin_id) else {
                return Ok(None);
            };
            let removed = disk.remove(pos);
            lock_vec(memory()).retain(|plugin| plugin.id != plugin_id);
            Ok(Some(removed))
        }

        pub fn install_plugin_stub(_source: &str, _scope: Option<&str>) -> Result<String> {
            Ok("Plugin installed (stub).".to_string())
        }

        pub fn list_marketplace_stub(_query: &str) -> Result<Vec<String>> {
            Ok(vec!["official-marketplace".to_string()])
        }

        pub fn refresh_marketplace_cache_stub() -> Result<String> {
            Ok("Marketplace cache refreshed (stub).".to_string())
        }

        pub fn update_plugin_stub(_id: &str) -> Result<String> {
            Ok("Plugin updated (stub).".to_string())
        }

        pub fn validate_plugin_stub(_id: &str) -> Result<Vec<String>> {
            Ok(Vec::new())
        }

        pub fn get_plugin_info_stub(id: &str) -> Result<String> {
            let plugin =
                find_plugin(id).or_else(|| lock_vec(disk()).iter().find(|p| p.id == id).cloned());
            match plugin {
                Some(p) => {
                    Ok(format!(
                    "ID: {}\nName: {}\nVersion: {}\nStatus: {:?}\nTools: {}\nSkills: {}\nMCP: {}",
                    p.id, p.name, p.version, p.status,
                    p.tools.join(", "),
                    p.skills.join(", "),
                    p.mcp_servers.join(", "),
                ))
                }
                None => Ok(format!("Plugin '{}' not found.", id)),
            }
        }

        pub fn install_runtime() {
            clear_plugins();
            set_plugin_command_runtime(PluginCommandRuntime {
                load_installed_plugins,
                save_installed_plugins,
                get_all_plugins,
                needs_refresh,
                find_plugin,
                set_plugin_status,
                register_plugin,
                emit_event_external,
                uninstall_plugin,
                install_plugin: install_plugin_stub,
                list_marketplace: list_marketplace_stub,
                refresh_marketplace_cache: refresh_marketplace_cache_stub,
                update_plugin: update_plugin_stub,
                validate_plugin: validate_plugin_stub,
                get_plugin_info: get_plugin_info_stub,
            });
        }
    }

    /// Isolate ALLTHECODES_HOME + clear registry around a closure. Tests that touch
    /// installed_plugins.json must run serially.
    fn with_clean_state<T>(f: impl FnOnce() -> T) -> T {
        let tmp = tempfile::tempdir().expect("tempdir");
        let old = std::env::var("ALLTHECODES_HOME").ok();
        std::env::set_var("ALLTHECODES_HOME", tmp.path());
        test_plugins::install_runtime();
        let result = f();
        test_plugins::clear_plugins();
        match old {
            Some(v) => std::env::set_var("ALLTHECODES_HOME", v),
            None => std::env::remove_var("ALLTHECODES_HOME"),
        }
        result
    }

    #[tokio::test]
    async fn plugin_help_works() {
        let handler = PluginHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("help", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => assert!(text.contains("Plugin management")),
            _ => panic!("Expected Output result"),
        }
    }

    #[tokio::test]
    async fn plugin_unknown_subcommand() {
        let handler = PluginHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("wat", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => assert!(text.contains("Unknown plugin subcommand")),
            _ => panic!("Expected Output result"),
        }
    }

    #[tokio::test]
    async fn plugin_enable_missing_id_errors() {
        let handler = PluginHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("enable", &mut ctx).await;
        assert!(result.is_err());
        assert!(result
            .err()
            .unwrap()
            .to_string()
            .contains("Usage: /plugin enable"));
    }

    #[tokio::test]
    async fn plugin_uninstall_missing_id_errors() {
        let handler = PluginHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("uninstall", &mut ctx).await;
        assert!(result.is_err());
        assert!(result
            .err()
            .unwrap()
            .to_string()
            .contains("Usage: /plugin uninstall"));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn plugin_search_outputs_reasons_actions_and_schema_boundary() {
        let _runtime = DiscoveryRuntimeGuard::install(
            DiscoverySearchRuntime::new().with_plugin_items_provider(|| {
                vec![
                    DiscoverySearchResult::new(DiscoveryResultKind::Plugin, "rust-tools")
                        .with_id("rust-tools")
                        .with_source("active")
                        .with_description("Rust formatting and cargo diagnostics")
                        .with_status(DiscoveryStatusSummary::new("installed"))
                        .with_tools(["plugin__rust_tools__fmt"])
                        .with_skills(["rust-review"])
                        .with_next_action(DiscoveryNextAction::new(
                            "Inspect plugin",
                            "/plugin info rust-tools",
                        )),
                ]
            }),
        );
        let handler = PluginHandler;
        let mut ctx = test_ctx();

        let result = handler.execute("search rust", &mut ctx).await.unwrap();

        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Plugin search results for 'rust'"), "{text}");
                assert!(text.contains("rust-tools"), "{text}");
                assert!(text.contains("matches:"), "{text}");
                assert!(text.contains("Next:"), "{text}");
                assert!(text.contains("/plugin info rust-tools"), "{text}");
                assert!(text.contains("ToolSearch"), "{text}");
            }
            _ => panic!("Expected Output result"),
        }
    }

    #[tokio::test]
    async fn plugin_search_empty_query_shows_usage() {
        let handler = PluginHandler;
        let mut ctx = test_ctx();

        let result = handler.execute("search", &mut ctx).await.unwrap();

        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Usage: /plugin search <query>"), "{text}");
            }
            _ => panic!("Expected Output result"),
        }
    }

    /// Synchronously run a handler's async execute — used inside a blocking
    /// test so we can mix async dispatch with env-var setup.
    fn run(handler: &PluginHandler, args: &str) -> CommandResult {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let mut ctx = test_ctx();
        rt.block_on(handler.execute(args, &mut ctx)).unwrap()
    }

    #[test]
    #[serial_test::serial]
    fn plugin_list_default_shows_all_layers() {
        let handler = PluginHandler;
        let output = with_clean_state(|| {
            test_plugins::save_installed_plugins(&[
                make_plugin("alpha", PluginStatus::Installed),
                make_plugin("beta", PluginStatus::Disabled),
            ])
            .unwrap();
            test_plugins::register_plugin(make_plugin("alpha", PluginStatus::Installed));
            // `beta` disabled on disk but NOT active (realistic scenario after disable).
            run(&handler, "list")
        });
        match output {
            CommandResult::Output(text) => {
                assert!(text.contains("alpha"), "missing alpha row: {}", text);
                assert!(text.contains("beta"), "missing beta row: {}", text);
                // Columns present.
                assert!(text.contains("installed"));
                assert!(text.contains("enabled"));
                assert!(text.contains("active"));
            }
            _ => panic!("expected Output"),
        }
    }

    #[test]
    #[serial_test::serial]
    fn plugin_installed_filter_excludes_disabled() {
        let handler = PluginHandler;
        let output = with_clean_state(|| {
            test_plugins::save_installed_plugins(&[
                make_plugin("alpha", PluginStatus::Installed),
                make_plugin("beta", PluginStatus::Disabled),
            ])
            .unwrap();
            test_plugins::register_plugin(make_plugin("alpha", PluginStatus::Installed));
            test_plugins::register_plugin(make_plugin("beta", PluginStatus::Disabled));
            run(&handler, "installed")
        });
        match output {
            CommandResult::Output(text) => {
                assert!(text.contains("alpha"), "text: {}", text);
                assert!(
                    !text.contains("beta"),
                    "beta should not be listed: {}",
                    text
                );
            }
            _ => panic!("expected Output"),
        }
    }

    #[test]
    #[serial_test::serial]
    fn plugin_disabled_filter_shows_only_disabled() {
        let handler = PluginHandler;
        let output = with_clean_state(|| {
            test_plugins::save_installed_plugins(&[
                make_plugin("alpha", PluginStatus::Installed),
                make_plugin("beta", PluginStatus::Disabled),
            ])
            .unwrap();
            test_plugins::register_plugin(make_plugin("alpha", PluginStatus::Installed));
            test_plugins::register_plugin(make_plugin("beta", PluginStatus::Disabled));
            run(&handler, "disabled")
        });
        match output {
            CommandResult::Output(text) => {
                assert!(text.contains("beta"), "text: {}", text);
                assert!(!text.contains("alpha"), "alpha should not be listed");
            }
            _ => panic!("expected Output"),
        }
    }

    #[test]
    #[serial_test::serial]
    fn plugin_errors_filter_shows_error_only() {
        let handler = PluginHandler;
        let output = with_clean_state(|| {
            test_plugins::save_installed_plugins(&[
                make_plugin("working", PluginStatus::Installed),
                make_plugin("broken", PluginStatus::Error("boom".to_string())),
            ])
            .unwrap();
            test_plugins::register_plugin(make_plugin("working", PluginStatus::Installed));
            test_plugins::register_plugin(make_plugin(
                "broken",
                PluginStatus::Error("boom".to_string()),
            ));
            run(&handler, "errors")
        });
        match output {
            CommandResult::Output(text) => {
                assert!(text.contains("broken"), "text: {}", text);
                assert!(text.contains("boom"), "error detail missing: {}", text);
                assert!(
                    !text.contains("working"),
                    "healthy plugin should be filtered: {}",
                    text
                );
            }
            _ => panic!("expected Output"),
        }
    }

    #[test]
    #[serial_test::serial]
    fn plugin_status_includes_drift_when_diverged() {
        let handler = PluginHandler;
        let output = with_clean_state(|| {
            // Disk has one plugin, memory has none -> drift.
            test_plugins::save_installed_plugins(&[make_plugin(
                "disk-only",
                PluginStatus::Installed,
            )])
            .unwrap();
            run(&handler, "status")
        });
        match output {
            CommandResult::Output(text) => {
                assert!(
                    text.contains("Session drift"),
                    "drift line missing: {}",
                    text
                );
                assert!(
                    text.contains("disk-only"),
                    "drift plugin not named: {}",
                    text
                );
            }
            _ => panic!("expected Output"),
        }
    }

    #[test]
    #[serial_test::serial]
    fn plugin_status_reports_in_sync() {
        let handler = PluginHandler;
        let output = with_clean_state(|| {
            test_plugins::save_installed_plugins(&[make_plugin("p", PluginStatus::Installed)])
                .unwrap();
            test_plugins::register_plugin(make_plugin("p", PluginStatus::Installed));
            run(&handler, "status")
        });
        match output {
            CommandResult::Output(text) => {
                assert!(
                    text.contains("in sync"),
                    "expected in-sync line, got: {}",
                    text
                );
            }
            _ => panic!("expected Output"),
        }
    }

    #[test]
    #[serial_test::serial]
    fn plugin_uninstall_removes_entry() {
        let handler = PluginHandler;
        let (output, still_on_disk) = with_clean_state(|| {
            test_plugins::save_installed_plugins(&[make_plugin("doomed", PluginStatus::Installed)])
                .unwrap();
            test_plugins::register_plugin(make_plugin("doomed", PluginStatus::Installed));
            let out = run(&handler, "uninstall doomed");
            let remaining = test_plugins::load_installed_plugins();
            (out, remaining.iter().any(|p| p.id == "doomed"))
        });
        assert!(!still_on_disk, "doomed should have been removed");
        match output {
            CommandResult::Output(text) => {
                assert!(text.contains("doomed"), "got: {}", text);
                assert!(text.contains("uninstalled"), "got: {}", text);
            }
            _ => panic!("expected Output"),
        }
    }

    #[test]
    #[serial_test::serial]
    fn plugin_uninstall_absent_reports_not_installed() {
        let handler = PluginHandler;
        let output = with_clean_state(|| run(&handler, "uninstall ghost"));
        match output {
            CommandResult::Output(text) => {
                assert!(
                    text.contains("not installed") || text.contains("is not installed"),
                    "got: {}",
                    text
                );
            }
            _ => panic!("expected Output"),
        }
    }

    #[test]
    #[serial_test::serial]
    fn plugin_disable_flips_disk_status() {
        let handler = PluginHandler;
        let persisted = with_clean_state(|| {
            test_plugins::save_installed_plugins(&[make_plugin(
                "togglable",
                PluginStatus::Installed,
            )])
            .unwrap();
            test_plugins::register_plugin(make_plugin("togglable", PluginStatus::Installed));
            run(&handler, "disable togglable");
            let disk = test_plugins::load_installed_plugins();
            disk.iter().find(|p| p.id == "togglable").cloned()
        });
        let p = persisted.expect("togglable should still be on disk");
        assert_eq!(p.status, PluginStatus::Disabled);
    }
}
