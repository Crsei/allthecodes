//! Plugin state reconciler.
//!
//! Synchronises the in-memory plugin registry with on-disk state,
//! detecting orphaned, missing, or drifted plugins.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tracing::info;

use super::{get_all_plugins, register_plugin, unregister_plugin};
use crate::loader::load_installed_plugins;
use crate::{PluginEntry, PluginStatus};

/// Report of reconciliation findings.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReconciliationReport {
    /// Plugins found on disk but not in the registry.
    pub added: Vec<PluginEntry>,
    /// Plugins in the registry but missing from disk.
    pub removed: Vec<String>,
    /// Plugins whose metadata has changed.
    pub changed: Vec<PluginChange>,
    /// Orphaned cache directories with no matching plugin metadata.
    pub orphaned: Vec<String>,
    /// Total plugins after reconciliation.
    pub total_count: usize,
}

/// A change detected during reconciliation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginChange {
    pub plugin_id: String,
    pub field: String,
    pub old_value: String,
    pub new_value: String,
}

/// Plugin state reconciler.
pub struct Reconciler;

impl Reconciler {
    /// Reconcile the plugin state: compare registry, disk persistence,
    /// and cache directories.
    ///
    /// Returns a report of what was found and what actions can be taken.
    pub fn reconcile_plugin_state() -> ReconciliationReport {
        let mut report = ReconciliationReport::default();

        // 1. Load disk state
        let disk_plugins = load_installed_plugins();
        let disk_by_id: HashMap<&str, &PluginEntry> =
            disk_plugins.iter().map(|p| (p.id.as_str(), p)).collect();

        // 2. Load in-memory state
        let mem_plugins = get_all_plugins();
        let mem_by_id: HashMap<&str, &PluginEntry> =
            mem_plugins.iter().map(|p| (p.id.as_str(), p)).collect();

        // 3. Detect added (on disk but not in memory)
        for (id, disk_entry) in &disk_by_id {
            if !mem_by_id.contains_key(id) {
                report.added.push((*disk_entry).clone());
            } else {
                // Check for changes
                let mem_entry = mem_by_id[id];
                let changes = detect_changes(mem_entry, disk_entry);
                report.changed.extend(changes);
            }
        }

        // 4. Detect removed (in memory but not on disk)
        for id in mem_by_id.keys() {
            if !disk_by_id.contains_key(id) {
                report.removed.push((*id).to_string());
            }
        }

        // 5. Detect orphaned cache directories
        if let Ok(orphaned) = detect_orphaned_cache_dirs(&disk_by_id) {
            report.orphaned = orphaned;
        }

        report.total_count = disk_plugins.len();
        report
    }

    /// Apply automatic fixes based on the reconciliation report.
    ///
    /// Returns the number of fixes applied.
    pub fn fix_reconciliation(report: &ReconciliationReport) -> usize {
        let mut fixes = 0usize;

        // Register plugins that exist on disk but not in memory
        for plugin in &report.added {
            register_plugin(plugin.clone());
            fixes += 1;
            info!(plugin = %plugin.id, "Reconciliation: registered missing plugin");
        }

        // Unregister plugins that are in memory but missing from disk
        for plugin_id in &report.removed {
            unregister_plugin(plugin_id);
            fixes += 1;
            info!(%plugin_id, "Reconciliation: unregistered orphaned plugin");
        }

        // Update changed plugins in the registry
        for change in &report.changed {
            if let Some(disk_entry) = load_installed_plugins()
                .into_iter()
                .find(|p| p.id == change.plugin_id)
            {
                register_plugin(disk_entry);
                fixes += 1;
            }
        }

        // Clean orphaned cache dirs (log only, no automatic removal)
        if !report.orphaned.is_empty() {
            info!(
                count = report.orphaned.len(),
                "Reconciliation: orphaned cache directories detected (manual cleanup recommended)"
            );
        }

        fixes
    }
}

/// Detect changes between memory and disk plugin entries.
fn detect_changes(mem: &PluginEntry, disk: &PluginEntry) -> Vec<PluginChange> {
    let mut changes = Vec::new();

    if mem.version != disk.version {
        changes.push(PluginChange {
            plugin_id: mem.id.clone(),
            field: "version".to_string(),
            old_value: mem.version.clone(),
            new_value: disk.version.clone(),
        });
    }

    if mem.status != disk.status {
        changes.push(PluginChange {
            plugin_id: mem.id.clone(),
            field: "status".to_string(),
            old_value: status_to_string(&mem.status),
            new_value: status_to_string(&disk.status),
        });
    }

    if mem.description != disk.description {
        changes.push(PluginChange {
            plugin_id: mem.id.clone(),
            field: "description".to_string(),
            old_value: mem.description.clone(),
            new_value: disk.description.clone(),
        });
    }

    changes
}

fn status_to_string(status: &PluginStatus) -> String {
    match status {
        PluginStatus::Installed => "installed".to_string(),
        PluginStatus::Disabled => "disabled".to_string(),
        PluginStatus::Error(e) => format!("error: {}", e),
        PluginStatus::NotInstalled => "not_installed".to_string(),
    }
}

/// Detect orphaned cache directories that have no matching plugin in
/// the installed plugins database.
fn detect_orphaned_cache_dirs(
    disk_by_id: &HashMap<&str, &PluginEntry>,
) -> Result<Vec<String>, std::io::Error> {
    let cache_base = crate::cache_dir();
    if !cache_base.exists() {
        return Ok(Vec::new());
    }

    let mut orphaned = Vec::new();

    // Walk cache/marketplace/plugin directories
    if let Ok(entries) = std::fs::read_dir(&cache_base) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            // Check if any installed plugin references this cache entry
            let path_str = path.to_string_lossy().to_string();
            let referenced = disk_by_id.values().any(|p| {
                p.cache_path
                    .as_ref()
                    .map(|cp| {
                        cp.starts_with(&path)
                            || path_str.contains(&cp.to_string_lossy().to_string())
                    })
                    .unwrap_or(false)
            });

            if !referenced {
                orphaned.push(path_str);
            }
        }
    }

    orphaned.sort();
    Ok(orphaned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reconcile_empty_state() {
        let report = Reconciler::reconcile_plugin_state();
        // Should not panic with empty state
        let _ = report;
    }

    #[test]
    fn test_detect_changes_version() {
        let mut mem = make_plugin_entry("test", "1.0.0");
        let mut disk = make_plugin_entry("test", "2.0.0");
        mem.status = PluginStatus::Installed;
        disk.status = PluginStatus::Installed;

        let changes = detect_changes(&mem, &disk);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].field, "version");
    }

    #[test]
    fn test_detect_changes_status() {
        let mem = PluginEntry {
            status: PluginStatus::Installed,
            ..make_plugin_entry("test", "1.0.0")
        };
        let disk = PluginEntry {
            status: PluginStatus::Disabled,
            ..make_plugin_entry("test", "1.0.0")
        };

        let changes = detect_changes(&mem, &disk);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].field, "status");
    }

    #[test]
    fn test_no_changes_when_identical() {
        let entry = make_plugin_entry("test", "1.0.0");
        let changes = detect_changes(&entry, &entry);
        assert!(changes.is_empty());
    }

    fn make_plugin_entry(id: &str, version: &str) -> PluginEntry {
        PluginEntry {
            id: id.to_string(),
            name: id.to_string(),
            version: version.to_string(),
            description: "".to_string(),
            source: crate::PluginSource::Local {
                path: "/tmp".into(),
            },
            status: PluginStatus::Installed,
            marketplace: None,
            cache_path: None,
            tools: vec![],
            skills: vec![],
            mcp_servers: vec![],
            installed_at: None,
            updated_at: None,
        }
    }
}
