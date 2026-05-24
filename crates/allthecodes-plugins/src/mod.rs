use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock};
use tracing::warn;

pub mod loader;
pub mod refresh;
pub mod tools;

pub use loader::PluginDiagnostic;
pub use refresh::{reload_plugins, ReloadReport};

use super::{PluginEntry, PluginStatus};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginSubsystemEvent {
    Reloaded {
        count: usize,
        had_error: bool,
    },
    RefreshNeeded {
        reason: String,
    },
    StatusChanged {
        plugin_id: String,
        name: String,
        status: String,
        error: Option<String>,
    },
    /// A plugin was installed.
    Installed {
        plugin_id: String,
        name: String,
        version: String,
    },
    /// A plugin was updated to a new version.
    Updated {
        plugin_id: String,
        name: String,
        old_version: String,
        new_version: String,
    },
    /// A plugin was uninstalled.
    Uninstalled {
        plugin_id: String,
        name: String,
    },
    /// Plugin validation failed.
    ValidationFailed {
        plugin_id: String,
        errors: Vec<String>,
    },
    /// Plugin configuration changed.
    ConfigChanged {
        plugin_id: String,
    },
}

pub trait PluginEventSink: Send + Sync {
    fn emit(&self, event: PluginSubsystemEvent);
}

impl<F> PluginEventSink for F
where
    F: Fn(PluginSubsystemEvent) + Send + Sync,
{
    fn emit(&self, event: PluginSubsystemEvent) {
        self(event);
    }
}

pub type SharedPluginEventSink = Arc<dyn PluginEventSink>;

static EVENT_SINK: LazyLock<Mutex<Option<SharedPluginEventSink>>> =
    LazyLock::new(|| Mutex::new(None));

pub fn set_event_sink(sink: Option<SharedPluginEventSink>) {
    *EVENT_SINK.lock() = sink;
}

fn emit_event(event: PluginSubsystemEvent) {
    if let Some(sink) = EVENT_SINK.lock().as_ref() {
        sink.emit(event);
    }
}

pub fn emit_event_external(event: PluginSubsystemEvent) {
    emit_event(event);
}

pub fn plugins_dir() -> PathBuf {
    allthecodes_config::paths::plugins_dir()
}

pub fn cache_dir() -> PathBuf {
    plugins_dir().join("cache")
}

pub fn marketplaces_dir() -> PathBuf {
    plugins_dir().join("marketplaces")
}

pub fn installed_plugins_path() -> PathBuf {
    plugins_dir().join("installed_plugins.json")
}

pub fn known_marketplaces_path() -> PathBuf {
    plugins_dir().join("known_marketplaces.json")
}

static REGISTRY: LazyLock<Mutex<HashMap<String, PluginEntry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

static DIAGNOSTICS: LazyLock<Mutex<Vec<PluginDiagnostic>>> =
    LazyLock::new(|| Mutex::new(Vec::new()));

pub fn register_plugin(plugin: PluginEntry) {
    let event = PluginSubsystemEvent::StatusChanged {
        plugin_id: plugin.id.clone(),
        name: plugin.name.clone(),
        status: status_as_str(&plugin.status).to_string(),
        error: match &plugin.status {
            PluginStatus::Error(e) => Some(e.clone()),
            _ => None,
        },
    };
    REGISTRY.lock().insert(plugin.id.clone(), plugin);
    emit_event(event);
}

pub fn get_all_plugins() -> Vec<PluginEntry> {
    REGISTRY.lock().values().cloned().collect()
}

pub fn get_plugin_diagnostics() -> Vec<PluginDiagnostic> {
    DIAGNOSTICS.lock().clone()
}

pub fn find_plugin(id: &str) -> Option<PluginEntry> {
    REGISTRY.lock().get(id).cloned()
}

pub fn get_enabled_plugins() -> Vec<PluginEntry> {
    get_all_plugins()
        .into_iter()
        .filter(|p| p.status == PluginStatus::Installed)
        .collect()
}

pub fn set_plugin_status(id: &str, status: PluginStatus) -> Option<PluginEntry> {
    let mut reg = REGISTRY.lock();
    let plugin = reg.get_mut(id)?;
    plugin.status = status.clone();

    emit_event(PluginSubsystemEvent::StatusChanged {
        plugin_id: plugin.id.clone(),
        name: plugin.name.clone(),
        status: status_as_str(&status).to_string(),
        error: match status {
            PluginStatus::Error(e) => Some(e),
            _ => None,
        },
    });

    Some(plugin.clone())
}

pub fn unregister_plugin(id: &str) -> Option<PluginEntry> {
    let result = REGISTRY.lock().remove(id);
    if let Some(ref removed) = result {
        emit_event(PluginSubsystemEvent::StatusChanged {
            plugin_id: removed.id.clone(),
            name: removed.name.clone(),
            status: "not_installed".to_string(),
            error: None,
        });
    }
    result
}

pub fn clear_plugins() {
    REGISTRY.lock().clear();
    DIAGNOSTICS.lock().clear();
}

#[derive(Debug, Clone, Default)]
pub(crate) struct DriftReport {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub updated: Vec<String>,
}

impl DriftReport {
    fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.updated.is_empty()
    }
}

pub(crate) fn compute_drift() -> DriftReport {
    use std::collections::{HashMap, HashSet};

    let disk_report = loader::load_installed_plugins_report();
    let disk_plugins = disk_report.plugins;
    let in_memory = get_all_plugins();

    let disk_by_id: HashMap<String, &PluginEntry> =
        disk_plugins.iter().map(|p| (p.id.clone(), p)).collect();
    let mem_by_id: HashMap<String, &PluginEntry> =
        in_memory.iter().map(|p| (p.id.clone(), p)).collect();

    let disk_ids: HashSet<&String> = disk_by_id.keys().collect();
    let mem_ids: HashSet<&String> = mem_by_id.keys().collect();

    let mut added: Vec<String> = disk_ids
        .difference(&mem_ids)
        .map(|s| (*s).clone())
        .collect();
    let mut removed: Vec<String> = mem_ids
        .difference(&disk_ids)
        .map(|s| (*s).clone())
        .collect();
    let mut updated = Vec::new();

    for id in disk_ids.intersection(&mem_ids) {
        let disk = disk_by_id.get(*id).expect("id in disk");
        let mem = mem_by_id.get(*id).expect("id in mem");
        if status_variant_differs(&disk.status, &mem.status) {
            updated.push((*id).clone());
        }
    }

    added.sort();
    removed.sort();
    updated.sort();

    DriftReport {
        added,
        removed,
        updated,
    }
}

fn status_variant_differs(a: &PluginStatus, b: &PluginStatus) -> bool {
    !matches!(
        (a, b),
        (PluginStatus::NotInstalled, PluginStatus::NotInstalled)
            | (PluginStatus::Installed, PluginStatus::Installed)
            | (PluginStatus::Disabled, PluginStatus::Disabled)
            | (PluginStatus::Error(_), PluginStatus::Error(_)),
    )
}

fn status_as_str(status: &PluginStatus) -> &'static str {
    match status {
        PluginStatus::NotInstalled => "not_installed",
        PluginStatus::Installed => "installed",
        PluginStatus::Disabled => "disabled",
        PluginStatus::Error(_) => "error",
    }
}

pub fn needs_refresh() -> Option<String> {
    let load_report = loader::load_installed_plugins_report();
    if load_report.has_diagnostics() {
        let details = load_report
            .diagnostics
            .iter()
            .map(|diag| diag.message.clone())
            .collect::<Vec<_>>()
            .join("; ");
        return Some(format!("plugin metadata invalid ({})", details));
    }

    let drift = compute_drift();
    if drift.is_empty() {
        return None;
    }

    let mut parts = Vec::new();
    if !drift.added.is_empty() {
        parts.push(format!(
            "{} added on disk ({})",
            drift.added.len(),
            drift.added.join(", ")
        ));
    }
    if !drift.removed.is_empty() {
        parts.push(format!(
            "{} removed from disk ({})",
            drift.removed.len(),
            drift.removed.join(", ")
        ));
    }
    if !drift.updated.is_empty() {
        parts.push(format!(
            "{} status changed ({})",
            drift.updated.len(),
            drift.updated.join(", ")
        ));
    }
    Some(parts.join("; "))
}

pub fn uninstall_plugin(plugin_id: &str, purge_cache: bool) -> anyhow::Result<Option<PluginEntry>> {
    let mut installed = loader::load_installed_plugins();
    let Some(pos) = installed.iter().position(|p| p.id == plugin_id) else {
        return Ok(None);
    };
    let removed = installed.remove(pos);
    loader::save_installed_plugins(&installed)?;

    unregister_plugin(plugin_id);

    if purge_cache {
        if let Some(path) = cache_path_for(&removed) {
            if path.exists() {
                if let Err(e) = std::fs::remove_dir_all(&path) {
                    warn!(
                        plugin_id,
                        path = %path.display(),
                        error = %e,
                        "uninstall: failed to purge cache directory"
                    );
                }
            }
        }
    }

    Ok(Some(removed))
}

fn cache_path_for(entry: &PluginEntry) -> Option<PathBuf> {
    if let Some(ref cached) = entry.cache_path {
        let mut p = cached.clone();
        if p.parent().is_some() && p.file_name().is_some() {
            if let Some(parent) = p.parent() {
                let parent = parent.to_path_buf();
                if parent
                    .components()
                    .any(|c| c.as_os_str() == std::ffi::OsStr::new("cache"))
                {
                    p = parent;
                }
            }
        }
        return Some(p);
    }

    let marketplace = entry.marketplace.as_deref()?;
    let name = entry
        .id
        .split_once('@')
        .map(|(n, _)| n)
        .unwrap_or(entry.id.as_str());
    Some(cache_dir().join(marketplace).join(name))
}

pub fn init_plugins() {
    let installed = loader::load_installed_plugins_report();
    for diagnostic in &installed.diagnostics {
        warn!(
            path = %diagnostic.path.display(),
            plugin_id = diagnostic.plugin_id.as_deref().unwrap_or("<metadata>"),
            error = %diagnostic.message,
            "Plugin: metadata diagnostic during initialization"
        );
    }

    for plugin in installed.plugins {
        register_plugin(plugin);
    }

    let mut diagnostics = installed.diagnostics.clone();

    let cached = loader::discover_cached_plugins_report();
    for diagnostic in &cached.diagnostics {
        warn!(
            path = %diagnostic.path.display(),
            plugin_id = diagnostic.plugin_id.as_deref().unwrap_or("<cache>"),
            error = %diagnostic.message,
            "Plugin: cache diagnostic during initialization"
        );
    }
    diagnostics.extend(cached.diagnostics);
    *DIAGNOSTICS.lock() = diagnostics;
}

pub fn discover_plugin_tools() -> allthecodes_tools::tool::Tools {
    let mut out = Vec::new();

    for plugin in get_enabled_plugins() {
        let Some(cache_path) = plugin.cache_path.clone() else {
            continue;
        };

        let manifest = match super::manifest::load_manifest(&cache_path) {
            Ok(m) => m,
            Err(e) => {
                warn!(
                    plugin = %plugin.id,
                    path = %cache_path.display(),
                    error = %e,
                    "Plugin: failed to load manifest for tool contribution"
                );
                continue;
            }
        };

        for contributed in manifest.tools {
            if contributed.runtime.is_none() {
                continue;
            }

            out.push(Arc::new(tools::PluginToolWrapper::new(
                plugin.id.clone(),
                cache_path.clone(),
                contributed,
            )) as Arc<dyn allthecodes_tools::tool::Tool>);
        }
    }

    out
}

pub fn discover_plugin_mcp_servers() -> Vec<allthecodes_mcp::McpServerConfig> {
    discover_plugin_mcp_servers_scoped()
        .into_iter()
        .map(|(_id, cfg)| cfg)
        .collect()
}

pub fn discover_plugin_mcp_servers_scoped() -> Vec<(String, allthecodes_mcp::McpServerConfig)> {
    let mut out = Vec::new();

    for plugin in get_enabled_plugins() {
        let Some(cache_path) = plugin.cache_path.clone() else {
            continue;
        };

        let manifest = match super::manifest::load_manifest(&cache_path) {
            Ok(m) => m,
            Err(e) => {
                warn!(
                    plugin = %plugin.id,
                    path = %cache_path.display(),
                    error = %e,
                    "Plugin: failed to load manifest for MCP contribution"
                );
                continue;
            }
        };

        for mcp in manifest.mcp_servers {
            let env = if mcp.env.is_empty() {
                None
            } else {
                Some(mcp.env.clone())
            };
            out.push((
                plugin.id.clone(),
                allthecodes_mcp::McpServerConfig {
                    name: mcp.name,
                    transport: "stdio".to_string(),
                    command: Some(mcp.command),
                    args: Some(mcp.args),
                    url: None,
                    headers: None,
                    oauth: None,
                    env,
                    browser_mcp: None,
                    disabled: None,
                },
            ));
        }
    }

    out
}

#[derive(Debug, Clone)]
pub struct PluginSkillDefinition {
    pub plugin_id: String,
    pub name: String,
    pub path: PathBuf,
    pub description: Option<String>,
}

pub fn discover_plugin_skill_definitions() -> Vec<PluginSkillDefinition> {
    let mut out = Vec::new();

    for plugin in get_enabled_plugins() {
        let Some(cache_path) = plugin.cache_path.clone() else {
            continue;
        };

        let manifest = match super::manifest::load_manifest(&cache_path) {
            Ok(m) => m,
            Err(e) => {
                warn!(
                    plugin = %plugin.id,
                    path = %cache_path.display(),
                    error = %e,
                    "Plugin: failed to load manifest for skill contribution"
                );
                continue;
            }
        };

        for contributed in manifest.skills {
            out.push(PluginSkillDefinition {
                plugin_id: plugin.id.clone(),
                name: contributed.name,
                path: cache_path.join(&contributed.path),
                description: contributed.description,
            });
        }
    }

    out
}
