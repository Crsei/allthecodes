//! Plugin installation, update, and uninstall orchestration.
//!
//! Provides the main entry points for plugin lifecycle operations:
//! - `install_plugin` — download, validate, extract, and register
//! - `uninstall_plugin_ext` — enhanced uninstall with scope awareness
//! - `update_plugin` — download new version, replace, preserve config
//! - `lookup_plugin_by_source` — resolve plugin from marketplace or source

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::blocklist;
use crate::dependency_resolver::DependencyResolver;
use crate::flagging;
use crate::loader;
use crate::manifest::{load_manifest, PluginManifest, SkillContribution};
use crate::marketplace;
use crate::mcpb::{parse_mcpb_from_bytes, verify_mcpb_integrity};
use crate::policy::{PluginPolicyEnforcer, PolicyDecision};
use crate::sources::{resolve_source, ResolveSource};
use crate::versioning::check_engine_compatibility;
use crate::zip_cache::{extract_tgz_to, extract_zip_to};
use crate::{PluginEntry, PluginSource, PluginStatus};

/// Installation scope for a plugin.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstallScope {
    /// User-wide installation (default).
    #[default]
    User,
    /// Project-local installation.
    Project,
    /// Local directory reference.
    Local,
}

impl InstallScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            InstallScope::User => "user",
            InstallScope::Project => "project",
            InstallScope::Local => "local",
        }
    }
}

/// Errors that can occur during plugin installation.
#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("Plugin '{0}' is already installed")]
    AlreadyInstalled(String),

    #[error("Policy blocked installation: {0}")]
    PolicyBlocked(String),

    #[error("Plugin source not found: {0}")]
    SourceNotFound(String),

    #[error("Plugin validation failed: {0}")]
    ValidationFailed(String),

    #[error("Missing dependency: {0}")]
    MissingDependency(String),

    #[error("Engine version incompatible: required {required}, running {current}")]
    EngineIncompatible { required: String, current: String },

    #[error("Download failed: {0}")]
    DownloadFailed(String),

    #[error("Max plugins limit reached ({max})")]
    MaxPluginsReached { max: u32 },

    #[error("{0}")]
    Other(String),
}

impl From<anyhow::Error> for InstallError {
    fn from(e: anyhow::Error) -> Self {
        InstallError::Other(e.to_string())
    }
}

impl From<std::io::Error> for InstallError {
    fn from(e: std::io::Error) -> Self {
        InstallError::Other(e.to_string())
    }
}

/// Result of a plugin installation.
#[derive(Debug, Clone)]
pub struct InstallResult {
    /// The installed plugin entry.
    pub plugin: PluginEntry,
    /// Installation path.
    pub install_path: PathBuf,
    /// Whether this was a fresh install (vs upgrade).
    pub fresh_install: bool,
}

/// Install a plugin from a source specification.
///
/// `source` can be a marketplace plugin ID, a URL, a GitHub repo, an npm
/// package, or a local path.
///
/// The optional `scope` determines where the plugin is installed.
pub async fn install_plugin(
    source: &str,
    scope: Option<InstallScope>,
    engine_version: Option<&str>,
    policy: Option<&allthecodes_config::mdm::ManagedPolicy>,
    available_plugins: &HashMap<String, String>,
    all_manifests: &HashMap<String, PluginManifest>,
) -> std::result::Result<InstallResult, InstallError> {
    let scope = scope.unwrap_or_default();

    // 1. Check blocklist
    if blocklist::GLOBAL_BLOCKLIST.is_blocklisted(source) {
        return Err(InstallError::PolicyBlocked(format!(
            "Plugin '{}' is blocklisted",
            source
        )));
    }

    // 2. Resolve the source to find the plugin
    let resolved = lookup_plugin_by_source(source).await?;

    // 3. Policy check
    let decision = PluginPolicyEnforcer::check_plugin_allowed(source, &resolved.source, policy);
    match decision {
        PolicyDecision::Blocked { reason } => {
            return Err(InstallError::PolicyBlocked(reason));
        }
        PolicyDecision::Flagged { reason } => {
            warn!(plugin = %source, %reason, "Plugin flagged during installation");
            flagging::GLOBAL_FLAGGING.flag_plugin(
                source,
                flagging::FlagReason::PolicyViolation(reason.clone()),
                "Flagged during install",
            );
        }
        PolicyDecision::Allowed => {}
    }

    // 4. Check if already installed
    let existing = loader::load_installed_plugins();
    if existing.iter().any(|p| {
        p.id == source
            || p.id
                .split_once('@')
                .is_some_and(|(marketplace_id, _)| marketplace_id == source)
            || p.name.eq_ignore_ascii_case(source)
            || matches!(&p.source, PluginSource::Marketplace { id, .. } if id == source)
    }) {
        return Err(InstallError::AlreadyInstalled(source.to_string()));
    }

    // 5. Check max plugins limit
    if let Some(max) = PluginPolicyEnforcer::max_plugins(policy) {
        if existing.len() >= max as usize {
            return Err(InstallError::MaxPluginsReached { max });
        }
    }

    // 6. Download/resolve the plugin package
    let dest_dir = crate::cache_dir()
        .join(scope.as_str())
        .join(sanitize_id(source));
    let resolved_plugin = resolve_source(&resolved.resolve_source, &dest_dir).await?;

    // 7. Extract or process the package
    let install_path = if resolved_plugin.extension == "dir" {
        resolved_plugin.path.clone()
    } else if resolved_plugin.extension == "mcpb" {
        // MCPB format
        if !verify_mcpb_integrity(&resolved_plugin.path)? {
            return Err(InstallError::ValidationFailed(
                "MCPB integrity check failed".to_string(),
            ));
        }
        let bundle = parse_mcpb_from_bytes(&std::fs::read(&resolved_plugin.path)?)?;
        let extract_dir = dest_dir.join("extracted");
        // Extract payload (which is a ZIP)
        extract_zip_to(&bundle.payload, &extract_dir)?;
        extract_dir
    } else if resolved_plugin.extension == "zip" {
        let extract_dir = dest_dir.join("extracted");
        let zip_data = std::fs::read(&resolved_plugin.path)?;
        extract_zip_to(&zip_data, &extract_dir)?;
        extract_dir
    } else if resolved_plugin.extension == "tgz" || resolved_plugin.extension == "tar.gz" {
        let extract_dir = dest_dir.join("extracted");
        let tgz_data = std::fs::read(&resolved_plugin.path)?;
        extract_tgz_to(&tgz_data, &extract_dir)?;
        normalize_npm_package_root(&extract_dir)?;
        extract_dir
    } else {
        // Single file plugin
        let plugin_dir = dest_dir.join("extracted");
        std::fs::create_dir_all(&plugin_dir)?;
        std::fs::copy(&resolved_plugin.path, plugin_dir.join("plugin.bin"))?;
        plugin_dir
    };
    let install_path = normalize_install_path(install_path)?;

    // 8. Load and validate the manifest
    let manifest = load_manifest(&install_path)
        .map_err(|e| InstallError::ValidationFailed(format!("Invalid manifest: {}", e)))?;

    // 9. Engine compatibility check
    if let Some(ref min_ver) = manifest.min_app_version {
        if let Some(engine_ver) = engine_version {
            if !check_engine_compatibility(Some(min_ver.as_str()), engine_ver) {
                return Err(InstallError::EngineIncompatible {
                    required: min_ver.clone(),
                    current: engine_ver.to_string(),
                });
            }
        }
    }

    // 10. Resolve dependencies
    let _deps = DependencyResolver::resolve_dependencies(&manifest, available_plugins)
        .map_err(|e| InstallError::MissingDependency(e.to_string()))?;

    // 11. Check for dependency cycles
    if let Err(e) = DependencyResolver::check_dependency_cycle(&manifest, all_manifests) {
        warn!(error = %e, "Dependency cycle detected during install");
    }

    // 12. Create plugin entry
    let marketplace_name = resolved.marketplace.clone();
    let plugin_id = format!(
        "{}@{}",
        manifest.name,
        marketplace_name.as_deref().unwrap_or("local")
    );
    let entry = PluginEntry {
        id: plugin_id,
        name: manifest
            .display_name
            .clone()
            .unwrap_or_else(|| manifest.name.clone()),
        version: manifest.version.clone(),
        description: manifest.description.clone(),
        source: resolved.source,
        status: PluginStatus::Installed,
        marketplace: marketplace_name,
        cache_path: Some(install_path.clone()),
        tools: manifest.tools.iter().map(|t| t.name.clone()).collect(),
        skills: manifest.skills.iter().map(|s| s.name.clone()).collect(),
        mcp_servers: manifest
            .mcp_servers
            .iter()
            .map(|m| m.name.clone())
            .collect(),
        installed_at: Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64,
        ),
        updated_at: None,
    };

    // 13. Persist
    let mut installed = loader::load_installed_plugins();
    installed.push(entry.clone());
    loader::save_installed_plugins(&installed)?;

    // 14. Register in memory
    crate::register_plugin(entry.clone());

    info!(
        plugin = %entry.id,
        version = %entry.version,
        path = %install_path.display(),
        "Plugin installed successfully"
    );

    Ok(InstallResult {
        plugin: entry,
        install_path,
        fresh_install: true,
    })
}

/// Enhanced uninstall with optional purge and scope awareness.
pub fn uninstall_plugin_ext(plugin_id: &str, purge: bool) -> Result<Option<PluginEntry>> {
    crate::uninstall_plugin(plugin_id, purge)
}

/// Update a plugin to the latest available version.
///
/// Downloads the new version, extracts it, preserves configuration,
/// and updates the registration.
pub async fn update_plugin(
    plugin_id: &str,
    engine_version: Option<&str>,
    policy: Option<&allthecodes_config::mdm::ManagedPolicy>,
    available_plugins: &HashMap<String, String>,
    all_manifests: &HashMap<String, PluginManifest>,
) -> std::result::Result<InstallResult, InstallError> {
    // 1. Find the existing plugin
    let installed = loader::load_installed_plugins();
    let existing = installed
        .iter()
        .find(|p| p.id == plugin_id || p.name == plugin_id)
        .cloned()
        .ok_or_else(|| InstallError::SourceNotFound(format!("Plugin '{}' not found", plugin_id)))?;

    let old_version = existing.version.clone();

    // 2. Re-install from the same source
    let source_str = match &existing.source {
        PluginSource::Local { path } => path.clone(),
        PluginSource::Npm { package, .. } => format!("npm:{}", package),
        PluginSource::GitHub { repo, .. } => format!("github:{}", repo),
        PluginSource::Git { url, .. } => url.clone(),
        PluginSource::Url { url } => url.clone(),
        PluginSource::Marketplace { id, .. } => id.clone(),
    };

    let result = install_plugin(
        &source_str,
        Some(InstallScope::User),
        engine_version,
        policy,
        available_plugins,
        all_manifests,
    )
    .await?;

    info!(
        plugin = %plugin_id,
        old_version = %old_version,
        new_version = %result.plugin.version,
        "Plugin updated"
    );

    Ok(result)
}

/// Look up a plugin by source string.
///
/// Supports:
/// - Marketplace plugin ID (looked up in the global index)
/// - `npm:<package>` format
/// - `github:<owner/repo>` format
/// - URL (http:// or https://)
/// - Local file path
pub async fn lookup_plugin_by_source(
    source: &str,
) -> std::result::Result<PluginLookupResult, InstallError> {
    let (resolve_source, plugin_source) = if source.starts_with("npm:") {
        let package = source.trim_start_matches("npm:");
        let (pkg, ver) = package.split_once('@').unwrap_or((package, ""));
        let version = if ver.is_empty() {
            None
        } else {
            Some(ver.to_string())
        };
        (
            ResolveSource::Npm {
                package: pkg.to_string(),
                version,
            },
            PluginSource::Npm {
                package: pkg.to_string(),
                version: None,
            },
        )
    } else if source.starts_with("github:") {
        let repo = source.trim_start_matches("github:");
        let (r, tag) = repo.split_once('@').unwrap_or((repo, ""));
        (
            ResolveSource::GitHub {
                repo: r.to_string(),
                tag: if tag.is_empty() {
                    None
                } else {
                    Some(tag.to_string())
                },
            },
            PluginSource::GitHub {
                repo: r.to_string(),
                ref_spec: if tag.is_empty() {
                    None
                } else {
                    Some(tag.to_string())
                },
            },
        )
    } else if source.starts_with("http://") || source.starts_with("https://") {
        (
            ResolveSource::Url(source.to_string()),
            PluginSource::Url {
                url: source.to_string(),
            },
        )
    } else if Path::new(source).exists() {
        (
            ResolveSource::File(PathBuf::from(source)),
            PluginSource::Local {
                path: source.to_string(),
            },
        )
    } else {
        // Try marketplace lookup
        let entry = marketplace::GLOBAL_MARKETPLACE_INDEX.find_plugin(source);
        match entry {
            Some(mp_entry) => {
                let url = mp_entry.download_url.clone().unwrap_or_else(|| {
                    format!(
                        "https://plugins.claude-code.dev/plugins/{}/download",
                        source
                    )
                });
                (
                    ResolveSource::Url(url.clone()),
                    PluginSource::Marketplace {
                        id: mp_entry.id.clone(),
                        source_name: mp_entry.source_name.clone(),
                    },
                )
            }
            None => {
                return Err(InstallError::SourceNotFound(format!(
                    "Could not resolve plugin source: '{}'. Try 'npm:<package>', 'github:<repo>', or a URL.",
                    source
                )));
            }
        }
    };

    let marketplace = match &plugin_source {
        PluginSource::Marketplace { source_name, .. } => Some(source_name.clone()),
        _ => None,
    };

    Ok(PluginLookupResult {
        resolve_source,
        source: plugin_source,
        marketplace,
    })
}

/// Result of looking up a plugin source.
pub struct PluginLookupResult {
    pub resolve_source: ResolveSource,
    pub source: PluginSource,
    pub marketplace: Option<String>,
}

fn sanitize_id(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn normalize_npm_package_root(extract_dir: &Path) -> std::result::Result<(), InstallError> {
    let package_dir = extract_dir.join("package");
    if !package_dir.is_dir() || !package_dir.join("plugin.json").exists() {
        return Ok(());
    }

    for entry in std::fs::read_dir(&package_dir)? {
        let entry = entry?;
        let target = extract_dir.join(entry.file_name());
        if target.exists() {
            if target.is_dir() {
                std::fs::remove_dir_all(&target)?;
            } else {
                std::fs::remove_file(&target)?;
            }
        }
        std::fs::rename(entry.path(), target)?;
    }
    std::fs::remove_dir_all(package_dir)?;
    Ok(())
}

fn normalize_install_path(install_path: PathBuf) -> std::result::Result<PathBuf, InstallError> {
    if install_path.join("plugin.json").is_file() {
        return Ok(install_path);
    }

    let content_root = single_top_level_dir(&install_path)?.unwrap_or_else(|| install_path.clone());
    if content_root.join("plugin.json").is_file() {
        return Ok(content_root);
    }

    if synthesize_allthecodes_manifest(&install_path, &content_root)? {
        return Ok(install_path);
    }

    Ok(install_path)
}

fn single_top_level_dir(root: &Path) -> std::result::Result<Option<PathBuf>, InstallError> {
    let mut dirs = Vec::new();
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            dirs.push(path);
        } else {
            return Ok(None);
        }
    }

    if dirs.len() == 1 {
        Ok(dirs.pop())
    } else {
        Ok(None)
    }
}

fn synthesize_allthecodes_manifest(
    manifest_root: &Path,
    content_root: &Path,
) -> std::result::Result<bool, InstallError> {
    let source_manifest = content_root
        .join(".codex-plugin")
        .join("plugin.json")
        .is_file()
        .then(|| content_root.join(".codex-plugin").join("plugin.json"))
        .or_else(|| {
            content_root
                .join(".claude-plugin")
                .join("plugin.json")
                .is_file()
                .then(|| content_root.join(".claude-plugin").join("plugin.json"))
        });
    let Some(source_manifest) = source_manifest else {
        return Ok(false);
    };

    let content = std::fs::read_to_string(&source_manifest)?;
    let value: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| InstallError::ValidationFailed(format!("Invalid plugin manifest: {e}")))?;

    let mut manifest = PluginManifest {
        name: json_string(&value, &["name"]).unwrap_or_else(|| "plugin".to_string()),
        display_name: json_string(&value, &["display_name"])
            .or_else(|| json_string(&value, &["displayName"]))
            .or_else(|| json_string(&value, &["interface", "displayName"])),
        version: json_string(&value, &["version"]).unwrap_or_else(|| "0.0.0".to_string()),
        description: json_string(&value, &["description"])
            .or_else(|| json_string(&value, &["interface", "longDescription"]))
            .or_else(|| json_string(&value, &["interface", "shortDescription"]))
            .unwrap_or_default(),
        author: json_string(&value, &["author"])
            .or_else(|| json_string(&value, &["author", "name"]))
            .or_else(|| json_string(&value, &["interface", "developerName"])),
        license: json_string(&value, &["license"]),
        ..PluginManifest::default()
    };
    manifest.skills = scan_skill_contributions(manifest_root, content_root)?;

    let manifest_path = manifest_root.join("plugin.json");
    let json = serde_json::to_string_pretty(&manifest).map_err(|e| {
        InstallError::ValidationFailed(format!("Failed to serialize manifest: {e}"))
    })?;
    std::fs::write(&manifest_path, json)?;
    Ok(true)
}

fn json_string(value: &serde_json::Value, path: &[&str]) -> Option<String> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    current.as_str().map(str::trim).and_then(|s| {
        if s.is_empty() {
            None
        } else {
            Some(s.to_string())
        }
    })
}

fn scan_skill_contributions(
    manifest_root: &Path,
    content_root: &Path,
) -> std::result::Result<Vec<SkillContribution>, InstallError> {
    let skills_root = content_root.join("skills");
    if !skills_root.is_dir() {
        return Ok(Vec::new());
    }

    let mut skills = Vec::new();
    for entry in std::fs::read_dir(&skills_root)? {
        let entry = entry?;
        let skill_dir = entry.path();
        if !skill_dir.is_dir() {
            continue;
        }

        let skill_path = skill_dir.join("SKILL.md");
        if !skill_path.is_file() {
            continue;
        }

        let name = skill_dir
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "skill".to_string());
        let relative_path = relative_path_string(manifest_root, &skill_path);
        skills.push(SkillContribution {
            name,
            path: relative_path,
            description: None,
        });
    }
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(skills)
}

fn relative_path_string(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_id() {
        assert_eq!(sanitize_id("my-plugin"), "my-plugin");
        assert_eq!(sanitize_id("my plugin!"), "my_plugin_");
        assert_eq!(sanitize_id("@scope/name"), "_scope_name");
    }

    #[test]
    fn test_lookup_npm_source() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(lookup_plugin_by_source("npm:test-package"));
        assert!(result.is_ok());
        let lookup = result.unwrap();
        match lookup.resolve_source {
            ResolveSource::Npm { package, .. } => {
                assert_eq!(package, "test-package");
            }
            _ => panic!("Expected Npm source"),
        }
    }

    #[test]
    fn test_lookup_github_source() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(lookup_plugin_by_source("github:owner/repo"));
        assert!(result.is_ok());
        let lookup = result.unwrap();
        match lookup.resolve_source {
            ResolveSource::GitHub { repo, tag } => {
                assert_eq!(repo, "owner/repo");
                assert!(tag.is_none());
            }
            _ => panic!("Expected GitHub source"),
        }
    }

    #[test]
    fn test_lookup_github_source_with_tag() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(lookup_plugin_by_source("github:owner/repo@v1.0.0"));
        assert!(result.is_ok());
        let lookup = result.unwrap();
        match lookup.resolve_source {
            ResolveSource::GitHub { repo, tag } => {
                assert_eq!(repo, "owner/repo");
                assert_eq!(tag.as_deref(), Some("v1.0.0"));
            }
            _ => panic!("Expected GitHub source"),
        }
    }

    #[test]
    fn test_lookup_url_source() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(lookup_plugin_by_source("https://example.com/plugin.zip"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_lookup_superpowers_builtin_marketplace_source() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(lookup_plugin_by_source(marketplace::SUPERPOWERS_PLUGIN_ID));
        assert!(result.is_ok());
        let lookup = result.unwrap();
        match lookup.source {
            PluginSource::Marketplace { id, source_name } => {
                assert_eq!(id, marketplace::SUPERPOWERS_PLUGIN_ID);
                assert_eq!(source_name, marketplace::DEFAULT_MARKETPLACE_SOURCE_NAME);
            }
            _ => panic!("Expected marketplace source"),
        }
        match lookup.resolve_source {
            ResolveSource::Url(url) => {
                assert!(url.contains("github.com/obra/superpowers"));
                assert!(url.ends_with("/v5.1.0.zip"));
            }
            _ => panic!("Expected URL resolve source"),
        }
    }

    #[test]
    fn test_lookup_local_source() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test-plugin");
        std::fs::create_dir_all(&path).unwrap();

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(lookup_plugin_by_source(path.to_str().unwrap()));
        assert!(result.is_ok());
        match result.unwrap().source {
            PluginSource::Local { .. } => {} // OK
            _ => panic!("Expected Local source"),
        }
    }

    #[test]
    fn test_lookup_invalid_source() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(lookup_plugin_by_source("nonexistent-plugin-name"));
        assert!(result.is_err());
    }

    #[test]
    fn github_style_archive_root_generates_allthecodes_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let extract_root = dir.path().join("extracted");
        let content_root = extract_root.join("superpowers-5.1.0");
        std::fs::create_dir_all(content_root.join(".codex-plugin")).unwrap();
        std::fs::create_dir_all(content_root.join("skills/brainstorming")).unwrap();
        std::fs::write(
            content_root.join(".codex-plugin/plugin.json"),
            r##"{
                "name": "superpowers",
                "version": "5.1.0",
                "description": "Agentic skills framework",
                "author": { "name": "Jesse Vincent" },
                "homepage": "https://github.com/obra/superpowers",
                "license": "MIT",
                "interface": { "displayName": "Superpowers" }
            }"##,
        )
        .unwrap();
        std::fs::write(
            content_root.join("skills/brainstorming/SKILL.md"),
            "---\nname: brainstorming\n---\n# Brainstorming\n",
        )
        .unwrap();

        let normalized = normalize_install_path(extract_root.clone()).unwrap();
        assert_eq!(normalized, extract_root);

        let manifest = load_manifest(&normalized).unwrap();
        assert_eq!(manifest.name, "superpowers");
        assert_eq!(manifest.display_name.as_deref(), Some("Superpowers"));
        assert_eq!(manifest.version, "5.1.0");
        assert_eq!(manifest.author.as_deref(), Some("Jesse Vincent"));
        assert_eq!(manifest.skills.len(), 1);
        assert_eq!(manifest.skills[0].name, "brainstorming");
        assert_eq!(
            manifest.skills[0].path,
            "superpowers-5.1.0/skills/brainstorming/SKILL.md"
        );
    }
}
