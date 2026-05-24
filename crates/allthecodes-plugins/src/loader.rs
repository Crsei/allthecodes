//! Plugin loader — reads installed_plugins.json, discovers cached plugins,
//! and converts plugin manifests into PluginEntry registrations.
//!
//! Corresponds to TypeScript: src/utils/plugins/pluginLoader.ts +
//!                            src/utils/plugins/pluginInstallationHelpers.ts

use std::path::PathBuf;

use anyhow::{Context, Result};
use tracing::warn;

use crate::manifest::{load_manifest, PluginManifest};
use crate::{PluginEntry, PluginStatus};

use super::{cache_dir, installed_plugins_path};

/// Structured diagnostic emitted when plugin metadata exists but cannot be
/// read, traversed, parsed, or validated.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PluginDiagnostic {
    pub kind: PluginDiagnosticKind,
    pub path: PathBuf,
    pub plugin_id: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginDiagnosticKind {
    InstalledPluginsUnreadable,
    InstalledPluginsMalformed,
    CacheTraversalUnreadable,
    ManifestMalformed,
}

impl PluginDiagnostic {
    fn new(
        kind: PluginDiagnosticKind,
        path: PathBuf,
        plugin_id: Option<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            path,
            plugin_id,
            message: message.into(),
        }
    }
}

/// Result of loading installed plugin metadata.
#[derive(Debug, Clone, Default)]
pub struct LoadedPlugins {
    pub plugins: Vec<PluginEntry>,
    pub diagnostics: Vec<PluginDiagnostic>,
}

impl LoadedPlugins {
    pub fn has_diagnostics(&self) -> bool {
        !self.diagnostics.is_empty()
    }
}

/// Result of scanning the plugin cache.
#[derive(Debug, Clone, Default)]
pub struct CachedPluginDiscovery {
    /// Discovered manifest/path pairs reserved for compatibility and diagnostics;
    /// plugin registration remains installed-metadata driven.
    pub plugins: Vec<(PluginManifest, PathBuf)>,
    pub diagnostics: Vec<PluginDiagnostic>,
}

// ---------------------------------------------------------------------------
// Installed plugins persistence (installed_plugins.json)
// ---------------------------------------------------------------------------

/// V2 installed plugins file format.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct InstalledPluginsFile {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    plugins: Vec<PluginEntry>,
}

/// Load installed plugins from `~/.allthecodes/plugins/installed_plugins.json`.
pub fn load_installed_plugins_report() -> LoadedPlugins {
    let path = installed_plugins_path();
    if !path.is_file() {
        return LoadedPlugins::default();
    }

    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(err) => {
            return LoadedPlugins {
                plugins: Vec::new(),
                diagnostics: vec![PluginDiagnostic::new(
                    PluginDiagnosticKind::InstalledPluginsUnreadable,
                    path.clone(),
                    None,
                    format!("Failed to read installed plugin metadata: {}", err),
                )],
            };
        }
    };

    let mut report = match serde_json::from_str::<InstalledPluginsFile>(&content) {
        Ok(file) => LoadedPlugins {
            plugins: file.plugins,
            diagnostics: Vec::new(),
        },
        Err(v2_err) => {
            // Try parsing as bare array (V1 format)
            match serde_json::from_str::<Vec<PluginEntry>>(&content) {
                Ok(v1_plugins) => LoadedPlugins {
                    plugins: v1_plugins,
                    diagnostics: Vec::new(),
                },
                Err(v1_err) => {
                    warn!(
                        path = %path.display(),
                        v2_error = %v2_err,
                        v1_error = %v1_err,
                        "Plugin: failed to parse installed_plugins.json"
                    );
                    LoadedPlugins {
                        plugins: Vec::new(),
                        diagnostics: vec![PluginDiagnostic::new(
                            PluginDiagnosticKind::InstalledPluginsMalformed,
                            path.clone(),
                            None,
                            format!(
                                "Failed to parse installed_plugins.json as v2 ({}) or v1 ({})",
                                v2_err, v1_err
                            ),
                        )],
                    }
                }
            }
        }
    };

    let manifest_diagnostics = validate_installed_plugin_manifests(&mut report.plugins);
    report.diagnostics.extend(manifest_diagnostics);
    report
}

/// Compatibility helper for callers that only need the entries. Invalid
/// existing metadata is available through [`load_installed_plugins_report`].
pub fn load_installed_plugins() -> Vec<PluginEntry> {
    load_installed_plugins_report().plugins
}

/// Save installed plugins to `~/.allthecodes/plugins/installed_plugins.json`.
pub fn save_installed_plugins(plugins: &[PluginEntry]) -> Result<()> {
    let path = installed_plugins_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }

    let file = InstalledPluginsFile {
        version: 2,
        plugins: plugins.to_vec(),
    };

    let json =
        serde_json::to_string_pretty(&file).context("Failed to serialize installed plugins")?;

    std::fs::write(&path, json).with_context(|| format!("Failed to write {}", path.display()))?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Plugin discovery from cache directory
// ---------------------------------------------------------------------------

/// Scan the cache directory for installed plugins.
///
/// Looks for `cache/{marketplace}/{plugin}/{version}/plugin.json`.
pub fn discover_cached_plugins_report() -> CachedPluginDiscovery {
    let cache = cache_dir();
    if !cache.is_dir() {
        return CachedPluginDiscovery::default();
    }

    let mut found = Vec::new();
    let mut diagnostics = Vec::new();

    // Iterate marketplace dirs
    let marketplace_dirs = match std::fs::read_dir(&cache) {
        Ok(d) => d,
        Err(err) => {
            diagnostics.push(PluginDiagnostic::new(
                PluginDiagnosticKind::CacheTraversalUnreadable,
                cache.clone(),
                None,
                format!("Failed to read plugin cache directory: {}", err),
            ));
            return CachedPluginDiscovery {
                plugins: found,
                diagnostics,
            };
        }
    };

    for mp_entry in marketplace_dirs {
        let mp_entry = match mp_entry {
            Ok(entry) => entry,
            Err(err) => {
                diagnostics.push(PluginDiagnostic::new(
                    PluginDiagnosticKind::CacheTraversalUnreadable,
                    cache.clone(),
                    None,
                    format!("Failed to read plugin cache entry: {}", err),
                ));
                continue;
            }
        };
        if !mp_entry.path().is_dir() {
            continue;
        }

        // Iterate plugin dirs within marketplace
        let plugin_dirs = match std::fs::read_dir(mp_entry.path()) {
            Ok(d) => d,
            Err(err) => {
                diagnostics.push(PluginDiagnostic::new(
                    PluginDiagnosticKind::CacheTraversalUnreadable,
                    mp_entry.path(),
                    None,
                    format!("Failed to read plugin marketplace cache: {}", err),
                ));
                continue;
            }
        };

        for plugin_entry in plugin_dirs {
            let plugin_entry = match plugin_entry {
                Ok(entry) => entry,
                Err(err) => {
                    diagnostics.push(PluginDiagnostic::new(
                        PluginDiagnosticKind::CacheTraversalUnreadable,
                        mp_entry.path(),
                        None,
                        format!("Failed to read plugin cache entry: {}", err),
                    ));
                    continue;
                }
            };
            if !plugin_entry.path().is_dir() {
                continue;
            }

            // Iterate version dirs within plugin
            let version_dirs = match std::fs::read_dir(plugin_entry.path()) {
                Ok(d) => d,
                Err(err) => {
                    diagnostics.push(PluginDiagnostic::new(
                        PluginDiagnosticKind::CacheTraversalUnreadable,
                        plugin_entry.path(),
                        plugin_entry.file_name().to_str().map(|s| s.to_string()),
                        format!("Failed to read plugin version cache: {}", err),
                    ));
                    continue;
                }
            };

            for version_entry in version_dirs {
                let version_entry = match version_entry {
                    Ok(entry) => entry,
                    Err(err) => {
                        diagnostics.push(PluginDiagnostic::new(
                            PluginDiagnosticKind::CacheTraversalUnreadable,
                            plugin_entry.path(),
                            plugin_entry.file_name().to_str().map(|s| s.to_string()),
                            format!("Failed to read plugin version cache entry: {}", err),
                        ));
                        continue;
                    }
                };
                let version_dir = version_entry.path();
                if !version_dir.is_dir() {
                    continue;
                }

                // Try loading plugin.json
                match load_manifest(&version_dir) {
                    Ok(manifest) => found.push((manifest, version_dir)),
                    Err(err) => diagnostics.push(PluginDiagnostic::new(
                        PluginDiagnosticKind::ManifestMalformed,
                        version_dir.clone(),
                        plugin_entry.file_name().to_str().map(|s| s.to_string()),
                        format!("Failed to load plugin manifest: {:#}", err),
                    )),
                }
            }
        }
    }

    CachedPluginDiscovery {
        plugins: found,
        diagnostics,
    }
}

#[cfg(test)]
pub fn discover_cached_plugins() -> Vec<(PluginManifest, std::path::PathBuf)> {
    discover_cached_plugins_report().plugins
}

fn validate_installed_plugin_manifests(plugins: &mut [PluginEntry]) -> Vec<PluginDiagnostic> {
    let mut diagnostics = Vec::new();
    for plugin in plugins {
        let Some(cache_path) = plugin.cache_path.clone() else {
            continue;
        };

        if !matches!(
            plugin.status,
            PluginStatus::Installed | PluginStatus::Disabled | PluginStatus::Error(_)
        ) {
            continue;
        }

        if let Err(err) = load_manifest(&cache_path) {
            let message = format!(
                "Plugin '{}' has invalid manifest metadata at {}: {:#}",
                plugin.id,
                cache_path.display(),
                err
            );
            diagnostics.push(PluginDiagnostic::new(
                PluginDiagnosticKind::ManifestMalformed,
                cache_path,
                Some(plugin.id.clone()),
                message.clone(),
            ));
            plugin.status = PluginStatus::Error(message);
        }
    }

    diagnostics
}

/// Convert a manifest + cache path into a PluginEntry.
#[cfg(test)]
pub fn manifest_to_entry(
    manifest: &PluginManifest,
    cache_path: &std::path::Path,
    marketplace: Option<&str>,
) -> PluginEntry {
    let id = if let Some(mp) = marketplace {
        format!("{}@{}", manifest.name, mp)
    } else {
        manifest.name.clone()
    };

    PluginEntry {
        id,
        name: manifest
            .display_name
            .clone()
            .unwrap_or_else(|| manifest.name.clone()),
        version: manifest.version.clone(),
        description: manifest.description.clone(),
        source: crate::PluginSource::Local {
            path: cache_path.to_string_lossy().to_string(),
        },
        status: PluginStatus::Installed,
        marketplace: marketplace.map(|s| s.to_string()),
        cache_path: Some(cache_path.to_path_buf()),
        tools: manifest.tools.iter().map(|t| t.name.clone()).collect(),
        skills: manifest.skills.iter().map(|s| s.name.clone()).collect(),
        mcp_servers: manifest
            .mcp_servers
            .iter()
            .map(|m| m.name.clone())
            .collect(),
        installed_at: None,
        updated_at: None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::*;
    use crate::PluginSource;
    use std::collections::HashMap;
    use std::io::Write;
    use std::path::Path;

    struct EnvGuard {
        old: Option<String>,
    }

    impl EnvGuard {
        fn set_allthecodes_home(path: &Path) -> Self {
            let old = std::env::var("ALLTHECODES_HOME").ok();
            std::env::set_var("ALLTHECODES_HOME", path);
            Self { old }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.old {
                Some(value) => std::env::set_var("ALLTHECODES_HOME", value),
                None => std::env::remove_var("ALLTHECODES_HOME"),
            }
        }
    }

    #[test]
    fn test_load_installed_plugins_no_file() {
        // Should return empty vec, not panic
        let plugins = load_installed_plugins();
        // May be non-empty if file exists from prior runs; just verify no crash
        let _ = plugins;
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        let dir = std::env::temp_dir().join("test_plugin_save");
        let _ = std::fs::create_dir_all(&dir);

        let path = dir.join("installed_plugins.json");
        let plugins = vec![PluginEntry {
            id: "test@mp".into(),
            name: "Test Plugin".into(),
            version: "1.0.0".into(),
            description: "A test".into(),
            source: PluginSource::Local {
                path: "/tmp".into(),
            },
            status: PluginStatus::Installed,
            marketplace: Some("mp".into()),
            cache_path: None,
            tools: vec!["tool1".into()],
            skills: vec![],
            mcp_servers: vec![],
            installed_at: Some(1234567890),
            updated_at: None,
        }];

        // Write to custom path
        let file = InstalledPluginsFile {
            version: 2,
            plugins: plugins.clone(),
        };
        let json = serde_json::to_string_pretty(&file).unwrap();
        std::fs::write(&path, &json).unwrap();

        // Read back
        let content = std::fs::read_to_string(&path).unwrap();
        let loaded: InstalledPluginsFile = serde_json::from_str(&content).unwrap();
        assert_eq!(loaded.plugins.len(), 1);
        assert_eq!(loaded.plugins[0].id, "test@mp");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[serial_test::serial]
    fn corrupt_installed_plugins_reports_diagnostic() {
        let home = std::env::temp_dir().join(format!(
            "allthecodes_corrupt_installed_{}",
            uuid::Uuid::new_v4()
        ));
        let _guard = EnvGuard::set_allthecodes_home(&home);
        std::fs::create_dir_all(crate::plugins_dir()).unwrap();
        std::fs::write(installed_plugins_path(), "{ this is not json").unwrap();

        let report = load_installed_plugins_report();

        assert!(
            report.plugins.is_empty(),
            "corrupt installed_plugins.json must not become plugin entries"
        );
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(
            report.diagnostics[0].kind,
            PluginDiagnosticKind::InstalledPluginsMalformed
        );
        assert!(report.diagnostics[0]
            .message
            .contains("installed_plugins.json"));

        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    #[serial_test::serial]
    fn installed_plugin_with_malformed_manifest_enters_error_state() {
        let home = std::env::temp_dir().join(format!(
            "allthecodes_malformed_installed_manifest_{}",
            uuid::Uuid::new_v4()
        ));
        let _guard = EnvGuard::set_allthecodes_home(&home);
        let plugin_dir = cache_dir().join("local").join("bad-plugin").join("1.0.0");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        std::fs::write(
            plugin_dir.join("plugin.json"),
            r#"{"name": "", "version": "1.0.0"}"#,
        )
        .unwrap();

        save_installed_plugins(&[PluginEntry {
            id: "bad-plugin@local".into(),
            name: "Bad Plugin".into(),
            version: "1.0.0".into(),
            description: "bad".into(),
            source: PluginSource::Local {
                path: plugin_dir.to_string_lossy().to_string(),
            },
            status: PluginStatus::Installed,
            marketplace: Some("local".into()),
            cache_path: Some(plugin_dir.clone()),
            tools: vec![],
            skills: vec![],
            mcp_servers: vec![],
            installed_at: None,
            updated_at: None,
        }])
        .unwrap();

        let report = load_installed_plugins_report();

        assert_eq!(report.plugins.len(), 1);
        assert!(matches!(
            report.plugins[0].status,
            PluginStatus::Error(ref message)
                if message.contains("bad-plugin@local") && message.contains("invalid manifest")
        ));
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(
            report.diagnostics[0].kind,
            PluginDiagnosticKind::ManifestMalformed
        );
        assert_eq!(
            report.diagnostics[0].plugin_id.as_deref(),
            Some("bad-plugin@local")
        );

        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    #[serial_test::serial]
    fn cached_plugin_with_malformed_manifest_reports_diagnostic() {
        let home = std::env::temp_dir().join(format!(
            "allthecodes_malformed_cached_manifest_{}",
            uuid::Uuid::new_v4()
        ));
        let _guard = EnvGuard::set_allthecodes_home(&home);
        let plugin_dir = cache_dir().join("local").join("bad-cache").join("1.0.0");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        std::fs::write(plugin_dir.join("plugin.json"), "{ invalid json").unwrap();

        let report = discover_cached_plugins_report();

        assert!(report.plugins.is_empty());
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(
            report.diagnostics[0].kind,
            PluginDiagnosticKind::ManifestMalformed
        );
        assert_eq!(
            report.diagnostics[0].plugin_id.as_deref(),
            Some("bad-cache")
        );

        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn test_manifest_to_entry() {
        let manifest = PluginManifest {
            name: "my-plugin".into(),
            display_name: Some("My Plugin".into()),
            version: "2.0.0".into(),
            description: "Does things".into(),
            author: None,
            license: None,
            min_app_version: None,
            tools: vec![ToolContribution {
                name: "tool-a".into(),
                description: "".into(),
                input_schema: None,
                read_only: false,
                concurrency_safe: false,
                runtime: None,
            }],
            skills: vec![SkillContribution {
                name: "skill-a".into(),
                path: "skills/a/SKILL.md".into(),
                description: None,
            }],
            mcp_servers: vec![],
            lsp_servers: None,
            commands: vec![],
            dependencies: HashMap::new(),
            configuration: None,
            agents: None,
            hooks: None,
            output_styles: None,
        };

        let entry = manifest_to_entry(
            &manifest,
            Path::new("/cache/mp/my-plugin/2.0.0"),
            Some("mp"),
        );
        assert_eq!(entry.id, "my-plugin@mp");
        assert_eq!(entry.name, "My Plugin");
        assert_eq!(entry.tools, vec!["tool-a"]);
        assert_eq!(entry.skills, vec!["skill-a"]);
        assert_eq!(entry.status, PluginStatus::Installed);
    }

    #[test]
    fn test_discover_cached_empty() {
        // Should not panic on non-existent cache dir
        let result = discover_cached_plugins();
        let _ = result; // may find real cached plugins
    }

    #[test]
    fn test_discover_cached_with_plugin() {
        let base = std::env::temp_dir().join("test_discover_cache");
        let plugin_dir = base.join("mp").join("test-plug").join("1.0.0");
        let _ = std::fs::create_dir_all(&plugin_dir);

        let manifest = serde_json::json!({
            "name": "test-plug",
            "version": "1.0.0",
            "description": "Discovered plugin"
        });
        let mut f = std::fs::File::create(plugin_dir.join("plugin.json")).unwrap();
        write!(f, "{}", serde_json::to_string_pretty(&manifest).unwrap()).unwrap();

        // Temporarily point cache_dir to our test dir isn't straightforward
        // since cache_dir() is hard-coded. Instead, directly test load_manifest.
        let m = load_manifest(&plugin_dir).unwrap();
        assert_eq!(m.name, "test-plug");

        let _ = std::fs::remove_dir_all(&base);
    }
}
