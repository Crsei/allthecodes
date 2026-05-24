//! Automatic plugin update management.
//!
//! Schedules and checks for plugin updates based on configurable intervals.
//! For offline testing, update checking uses local source comparisons.

use serde::{Deserialize, Serialize};

use crate::versioning::VersionRequirement;
use crate::PluginEntry;

/// Information about an available update.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateInfo {
    /// Plugin ID.
    pub plugin_id: String,
    /// Currently installed version.
    pub current_version: String,
    /// Available version from the source.
    pub available_version: String,
    /// Optional changelog or release notes URL.
    #[serde(default)]
    pub changelog: Option<String>,
    /// Severity of the update (e.g., "patch", "minor", "major").
    #[serde(default)]
    pub severity: String,
}

/// Auto-update manager for plugins.
pub struct AutoUpdateManager;

impl AutoUpdateManager {
    /// Check for available updates for installed plugins.
    ///
    /// This is a passive check — no downloads are performed.
    /// Returns a list of `UpdateInfo` for plugins with newer versions available.
    ///
    /// In production, this would query the marketplace or source registry.
    /// For offline testing, it can work with local version data.
    pub fn check_for_updates(
        installed_plugins: &[PluginEntry],
        available_versions: &[PluginVersionInfo],
    ) -> Vec<UpdateInfo> {
        let mut updates = Vec::new();

        for plugin in installed_plugins {
            let current = &plugin.version;

            // Find the latest available version for this plugin
            let available = available_versions
                .iter()
                .filter(|av| av.plugin_id == plugin.id || av.name == plugin.name)
                .max_by(|a, b| compare_versions(&a.version, &b.version));

            let Some(avail) = available else {
                continue;
            };

            if avail.version != *current {
                let severity = determine_severity(current, &avail.version);
                updates.push(UpdateInfo {
                    plugin_id: plugin.id.clone(),
                    current_version: current.clone(),
                    available_version: avail.version.clone(),
                    changelog: avail.changelog.clone(),
                    severity,
                });
            }
        }

        updates.sort_by(|a, b| {
            let sev_a = severity_rank(&a.severity);
            let sev_b = severity_rank(&b.severity);
            sev_b
                .cmp(&sev_a)
                .then_with(|| a.plugin_id.cmp(&b.plugin_id))
        });

        updates
    }

    /// Determine whether a plugin should be auto-updated based on its settings
    /// and the elapsed time since last check.
    pub fn should_auto_update(
        _plugin: &PluginEntry,
        update_interval_hours: u64,
        hours_since_last_check: u64,
    ) -> bool {
        if update_interval_hours == 0 {
            return false; // Auto-update disabled globally
        }
        hours_since_last_check >= update_interval_hours
    }

    /// Filter updates to only those matching the configured auto-update criteria.
    pub fn filter_auto_updates(updates: Vec<UpdateInfo>, max_severity: &str) -> Vec<UpdateInfo> {
        let max_rank = severity_rank(max_severity);
        updates
            .into_iter()
            .filter(|u| severity_rank(&u.severity) <= max_rank)
            .collect()
    }
}

/// A known version of a plugin (for version comparison).
#[derive(Debug, Clone)]
pub struct PluginVersionInfo {
    /// Plugin ID (for exact matching).
    pub plugin_id: String,
    /// Plugin name (for fuzzy matching).
    pub name: String,
    /// Available version string.
    pub version: String,
    /// Optional changelog.
    pub changelog: Option<String>,
}

/// Compare two semver-like version strings.
/// Returns `Ordering::Greater` if v1 > v2, etc.
fn compare_versions(v1: &str, v2: &str) -> std::cmp::Ordering {
    match (semver::Version::parse(v1), semver::Version::parse(v2)) {
        (Ok(a), Ok(b)) => a.cmp(&b),
        _ => v1.cmp(v2),
    }
}

/// Determine update severity based on version difference.
fn determine_severity(current: &str, available: &str) -> String {
    match (
        semver::Version::parse(current),
        semver::Version::parse(available),
    ) {
        (Ok(cur), Ok(avail)) => {
            if avail.major > cur.major {
                "major".to_string()
            } else if avail.minor > cur.minor {
                "minor".to_string()
            } else if avail.patch > cur.patch {
                "patch".to_string()
            } else {
                "unknown".to_string()
            }
        }
        _ => "unknown".to_string(),
    }
}

fn severity_rank(severity: &str) -> u8 {
    match severity {
        "unknown" => 0,
        "patch" => 1,
        "minor" => 2,
        "major" => 3,
        "security" => 4,
        _ => 0,
    }
}

/// Compatibility check: does the required version match the installed version?
pub fn check_plugin_version_compatibility(required: &str, installed: &str) -> bool {
    let req = VersionRequirement::new(required);
    req.satisfied_by(installed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PluginSource;

    fn make_plugin(id: &str, version: &str) -> PluginEntry {
        PluginEntry {
            id: id.to_string(),
            name: id.to_string(),
            version: version.to_string(),
            description: "".to_string(),
            source: PluginSource::Local {
                path: "/tmp".into(),
            },
            status: crate::PluginStatus::Installed,
            marketplace: None,
            cache_path: None,
            tools: vec![],
            skills: vec![],
            mcp_servers: vec![],
            installed_at: None,
            updated_at: None,
        }
    }

    #[test]
    fn test_no_updates_when_no_available() {
        let plugins = vec![make_plugin("test", "1.0.0")];
        let updates = AutoUpdateManager::check_for_updates(&plugins, &[]);
        assert!(updates.is_empty());
    }

    #[test]
    fn test_update_available() {
        let plugins = vec![make_plugin("test", "1.0.0")];
        let available = vec![PluginVersionInfo {
            plugin_id: "test".into(),
            name: "test".into(),
            version: "1.1.0".into(),
            changelog: Some("https://example.com/changelog".into()),
        }];

        let updates = AutoUpdateManager::check_for_updates(&plugins, &available);
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].plugin_id, "test");
        assert_eq!(updates[0].current_version, "1.0.0");
        assert_eq!(updates[0].available_version, "1.1.0");
        assert_eq!(updates[0].severity, "minor");
    }

    #[test]
    fn test_no_update_when_same_version() {
        let plugins = vec![make_plugin("test", "1.0.0")];
        let available = vec![PluginVersionInfo {
            plugin_id: "test".into(),
            name: "test".into(),
            version: "1.0.0".into(),
            changelog: None,
        }];

        let updates = AutoUpdateManager::check_for_updates(&plugins, &available);
        assert!(updates.is_empty());
    }

    #[test]
    fn test_update_severity_detection() {
        assert_eq!(determine_severity("1.0.0", "2.0.0"), "major");
        assert_eq!(determine_severity("1.0.0", "1.1.0"), "minor");
        assert_eq!(determine_severity("1.0.0", "1.0.1"), "patch");
        assert_eq!(determine_severity("1.0.0", "1.0.0"), "unknown");
    }

    #[test]
    fn test_should_auto_update() {
        let plugin = make_plugin("test", "1.0.0");

        assert!(AutoUpdateManager::should_auto_update(&plugin, 24, 48));
        assert!(!AutoUpdateManager::should_auto_update(&plugin, 24, 12));
        assert!(!AutoUpdateManager::should_auto_update(&plugin, 0, 100));
    }

    #[test]
    fn test_filter_auto_updates() {
        let updates = vec![
            UpdateInfo {
                plugin_id: "a".into(),
                current_version: "1.0.0".into(),
                available_version: "2.0.0".into(),
                changelog: None,
                severity: "major".into(),
            },
            UpdateInfo {
                plugin_id: "b".into(),
                current_version: "1.0.0".into(),
                available_version: "1.1.0".into(),
                changelog: None,
                severity: "minor".into(),
            },
        ];

        let filtered = AutoUpdateManager::filter_auto_updates(updates, "minor");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].plugin_id, "b");
    }

    #[test]
    fn test_version_comparison() {
        assert_eq!(compare_versions("1.0.0", "2.0.0"), std::cmp::Ordering::Less);
        assert_eq!(
            compare_versions("2.0.0", "1.0.0"),
            std::cmp::Ordering::Greater
        );
        assert_eq!(
            compare_versions("1.0.0", "1.0.0"),
            std::cmp::Ordering::Equal
        );
    }
}
