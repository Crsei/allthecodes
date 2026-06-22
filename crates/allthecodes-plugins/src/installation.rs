//! Plugin installation, update, and uninstall orchestration.
//!
//! Provides the main entry points for plugin lifecycle operations:
//! - `install_plugin` — download, validate, extract, and register
//! - `uninstall_plugin_ext` — enhanced uninstall with scope awareness
//! - `update_plugin` — download new version, replace, preserve config
//! - `lookup_plugin_by_source` — resolve plugin from marketplace or source

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::{info, warn};

use crate::blocklist;
use crate::dependency_resolver::DependencyResolver;
use crate::flagging;
use crate::loader;
use crate::manifest::{
    load_manifest, validate_official_manifest, PluginManifest, SkillContribution,
};
use crate::marketplace::{self, validate_official_download_url, OFFICIAL_MARKETPLACE_SOURCE_NAME};
use crate::mcpb::{parse_mcpb_from_bytes, verify_mcpb_integrity};
use crate::policy::{PluginPolicyEnforcer, PolicyDecision};
use crate::sources::{resolve_source, ResolveSource};
use crate::versioning::check_engine_compatibility;
use crate::zip_cache::{extract_tgz_to, extract_zip_to, extract_zip_to_strict, StrictZipLimits};
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

/// Strict official marketplace install request.
#[derive(Debug, Clone)]
pub struct OfficialPluginInstallRequest {
    pub id: String,
    pub version: String,
    pub download_url: String,
    pub sha256: Option<String>,
    pub homepage: Option<String>,
}

const MAX_OFFICIAL_ZIP_BYTES: u64 = 128 * 1024 * 1024;

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
        installed_version: Some(manifest.version.clone()),
        official: false,
        download_url: None,
        homepage: None,
        sha256: None,
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

/// Install an official marketplace plugin from its signed marketplace fields.
pub async fn install_official_plugin(
    request: OfficialPluginInstallRequest,
    engine_version: Option<&str>,
    available_plugins: &HashMap<String, String>,
    all_manifests: &HashMap<String, PluginManifest>,
) -> std::result::Result<InstallResult, InstallError> {
    validate_official_download_url(&request.download_url)
        .map_err(|e| InstallError::ValidationFailed(e.to_string()))?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| InstallError::DownloadFailed(e.to_string()))?;
    let response = client
        .get(&request.download_url)
        .send()
        .await
        .map_err(|e| InstallError::DownloadFailed(e.to_string()))?;
    if !response.status().is_success() {
        return Err(InstallError::DownloadFailed(format!(
            "Official plugin download returned HTTP {}",
            response.status()
        )));
    }
    if let Some(length) = response.content_length() {
        if length > MAX_OFFICIAL_ZIP_BYTES {
            return Err(InstallError::DownloadFailed(format!(
                "Official plugin download is {} bytes, exceeding limit {}",
                length, MAX_OFFICIAL_ZIP_BYTES
            )));
        }
    }
    if let Some(content_type) = response.headers().get(reqwest::header::CONTENT_TYPE) {
        let content_type = content_type
            .to_str()
            .unwrap_or_default()
            .to_ascii_lowercase();
        if !content_type.contains("application/zip")
            && !content_type.contains("application/octet-stream")
            && !content_type.contains("application/x-zip-compressed")
        {
            return Err(InstallError::DownloadFailed(format!(
                "Official plugin download returned unsupported content type '{}'",
                content_type
            )));
        }
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| InstallError::DownloadFailed(e.to_string()))?;
    if bytes.len() as u64 > MAX_OFFICIAL_ZIP_BYTES {
        return Err(InstallError::DownloadFailed(format!(
            "Official plugin download is {} bytes, exceeding limit {}",
            bytes.len(),
            MAX_OFFICIAL_ZIP_BYTES
        )));
    }
    if let Some(expected) = request.sha256.as_deref() {
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let actual = hex::encode(hasher.finalize());
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(InstallError::ValidationFailed(format!(
                "Official plugin SHA-256 mismatch: expected {}, got {}",
                expected, actual
            )));
        }
    }

    install_official_plugin_from_zip(
        request,
        &bytes,
        engine_version,
        available_plugins,
        all_manifests,
    )
}

pub fn install_official_plugin_from_zip(
    request: OfficialPluginInstallRequest,
    zip_data: &[u8],
    engine_version: Option<&str>,
    available_plugins: &HashMap<String, String>,
    all_manifests: &HashMap<String, PluginManifest>,
) -> std::result::Result<InstallResult, InstallError> {
    validate_official_download_url(&request.download_url)
        .map_err(|e| InstallError::ValidationFailed(e.to_string()))?;
    validate_official_path_segment("plugin id", &request.id)?;
    validate_official_path_segment("plugin version", &request.version)?;

    let installed = loader::load_installed_plugins();
    if installed
        .iter()
        .any(|p| p.id == request.id && p.version == request.version)
    {
        return Err(InstallError::AlreadyInstalled(request.id));
    }

    let plugins_root = crate::plugins_dir();
    let target_root = plugins_root.join(&request.id).join(&request.version);
    if target_root.exists() {
        return Err(InstallError::AlreadyInstalled(format!(
            "{}@{}",
            request.id, request.version
        )));
    }

    let tmp_root = plugins_root.join(".tmp").join(format!(
        "{}-{}-{}",
        sanitize_id(&request.id),
        sanitize_id(&request.version),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    if let Some(parent) = tmp_root.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if tmp_root.exists() {
        std::fs::remove_dir_all(&tmp_root)?;
    }
    std::fs::create_dir_all(&tmp_root)?;

    let result = (|| -> std::result::Result<InstallResult, InstallError> {
        extract_zip_to_strict(zip_data, &tmp_root, StrictZipLimits::default())
            .map_err(|e| InstallError::ValidationFailed(e.to_string()))?;
        let plugin_root = normalize_official_plugin_root(&tmp_root)?;
        let manifest = load_manifest(&plugin_root)
            .map_err(|e| InstallError::ValidationFailed(format!("Invalid manifest: {}", e)))?;
        validate_official_manifest(&manifest, &plugin_root, &request.id, &request.version)
            .map_err(|e| InstallError::ValidationFailed(e.to_string()))?;

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

        let _deps = DependencyResolver::resolve_dependencies(&manifest, available_plugins)
            .map_err(|e| InstallError::MissingDependency(e.to_string()))?;
        if let Err(e) = DependencyResolver::check_dependency_cycle(&manifest, all_manifests) {
            warn!(error = %e, "Dependency cycle detected during official install");
        }

        if let Some(parent) = target_root.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::rename(&plugin_root, &target_root).with_context(|| {
            format!(
                "Failed to move official plugin from {} to {}",
                plugin_root.display(),
                target_root.display()
            )
        })?;

        let entry = PluginEntry {
            id: manifest.name.clone(),
            name: manifest
                .display_name
                .clone()
                .unwrap_or_else(|| manifest.name.clone()),
            version: manifest.version.clone(),
            description: manifest.description.clone(),
            source: PluginSource::Marketplace {
                id: manifest.name.clone(),
                source_name: OFFICIAL_MARKETPLACE_SOURCE_NAME.to_string(),
            },
            status: PluginStatus::Installed,
            marketplace: Some(OFFICIAL_MARKETPLACE_SOURCE_NAME.to_string()),
            cache_path: Some(target_root.clone()),
            installed_version: Some(manifest.version.clone()),
            official: true,
            download_url: Some(request.download_url.clone()),
            homepage: request.homepage.clone(),
            sha256: request.sha256.clone(),
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

        let mut installed = loader::load_installed_plugins();
        installed.retain(|p| p.id != entry.id);
        installed.push(entry.clone());
        loader::save_installed_plugins(&installed)?;
        crate::register_plugin(entry.clone());

        Ok(InstallResult {
            plugin: entry,
            install_path: target_root,
            fresh_install: true,
        })
    })();

    if tmp_root.exists() {
        let _ = std::fs::remove_dir_all(&tmp_root);
    }

    result
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

fn normalize_official_plugin_root(
    extract_dir: &Path,
) -> std::result::Result<PathBuf, InstallError> {
    if extract_dir.join("plugin.json").is_file() {
        return Ok(extract_dir.to_path_buf());
    }

    let Some(content_root) = single_top_level_dir(extract_dir)? else {
        return Err(InstallError::ValidationFailed(
            "Official plugin ZIP must contain plugin.json at root or under a single top-level directory"
                .to_string(),
        ));
    };
    if content_root.join("plugin.json").is_file() {
        return Ok(content_root);
    }

    Err(InstallError::ValidationFailed(
        "Official plugin ZIP does not contain plugin.json".to_string(),
    ))
}

fn validate_official_path_segment(
    label: &str,
    value: &str,
) -> std::result::Result<(), InstallError> {
    if value.is_empty() {
        return Err(InstallError::ValidationFailed(format!(
            "Official {} must not be empty",
            label
        )));
    }
    if value == "." || value == ".." {
        return Err(InstallError::ValidationFailed(format!(
            "Official {} must be a single path segment",
            label
        )));
    }
    if value.contains('/') || value.contains('\\') || value.contains(':') {
        return Err(InstallError::ValidationFailed(format!(
            "Official {} must be a single path segment",
            label
        )));
    }

    let mut components = Path::new(value).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(segment)), None) if segment == value => Ok(()),
        _ => Err(InstallError::ValidationFailed(format!(
            "Official {} must be a single path segment",
            label
        ))),
    }
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
    use std::io::Write;

    struct EnvGuard {
        old_home: Option<String>,
    }

    impl EnvGuard {
        fn set_home(path: &Path) -> Self {
            let old_home = std::env::var("ALLTHECODES_HOME").ok();
            std::env::set_var("ALLTHECODES_HOME", path);
            crate::clear_plugins();
            Self { old_home }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            crate::clear_plugins();
            match &self.old_home {
                Some(value) => std::env::set_var("ALLTHECODES_HOME", value),
                None => std::env::remove_var("ALLTHECODES_HOME"),
            }
        }
    }

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

    #[test]
    #[serial_test::serial]
    fn official_zip_installs_to_versioned_root_and_registry() {
        let home = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set_home(home.path());
        let mut zip_buf = Vec::new();
        {
            let mut zip_writer = zip::ZipWriter::new(std::io::Cursor::new(&mut zip_buf));
            let options = zip::write::FileOptions::<()>::default().unix_permissions(0o755);
            zip_writer
                .start_file("eco-boost/plugin.json", options)
                .unwrap();
            zip_writer
                .write_all(
                    br#"{
                        "name": "eco-boost",
                        "display_name": "Eco Boost",
                        "version": "0.1.0",
                        "description": "Eco helper",
                        "mcp_servers": [
                            {"name": "eco", "command": "bin/eco", "args": ["--stdio"]}
                        ],
                        "skills": [
                            {"name": "eco", "path": "skills/eco/SKILL.md"}
                        ]
                    }"#,
                )
                .unwrap();
            zip_writer.start_file("eco-boost/bin/eco", options).unwrap();
            zip_writer.write_all(b"#!/bin/sh\n").unwrap();
            zip_writer
                .start_file("eco-boost/skills/eco/SKILL.md", options)
                .unwrap();
            zip_writer.write_all(b"# Eco\n").unwrap();
            zip_writer.finish().unwrap();
        }

        let result = install_official_plugin_from_zip(
            OfficialPluginInstallRequest {
                id: "eco-boost".into(),
                version: "0.1.0".into(),
                download_url: "https://allthecodes.cc/api/downloads/plugins/eco-boost/v0.1.0/eco-boost-0.1.0.zip".into(),
                sha256: None,
                homepage: Some("https://allthecodes.cc/plugins/eco-boost".into()),
            },
            &zip_buf,
            Some("1.0.0"),
            &HashMap::new(),
            &HashMap::new(),
        )
        .unwrap();

        assert_eq!(result.plugin.id, "eco-boost");
        assert!(result.install_path.ends_with("plugins/eco-boost/0.1.0"));
        assert!(result.install_path.join("plugin.json").is_file());
        let installed = loader::load_installed_plugins();
        assert_eq!(installed.len(), 1);
        assert!(installed[0].official);
        assert_eq!(
            installed[0].download_url.as_deref(),
            Some(
                "https://allthecodes.cc/api/downloads/plugins/eco-boost/v0.1.0/eco-boost-0.1.0.zip"
            )
        );
    }

    #[test]
    #[serial_test::serial]
    fn official_zip_id_mismatch_preserves_registry() {
        let home = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set_home(home.path());
        let mut zip_buf = Vec::new();
        {
            let mut zip_writer = zip::ZipWriter::new(std::io::Cursor::new(&mut zip_buf));
            let options = zip::write::FileOptions::<()>::default();
            zip_writer.start_file("plugin.json", options).unwrap();
            zip_writer
                .write_all(br#"{"name":"wrong","version":"0.1.0","description":""}"#)
                .unwrap();
            zip_writer.finish().unwrap();
        }

        let result = install_official_plugin_from_zip(
            OfficialPluginInstallRequest {
                id: "eco-boost".into(),
                version: "0.1.0".into(),
                download_url: "https://allthecodes.cc/api/downloads/plugins/eco-boost/v0.1.0/eco-boost-0.1.0.zip".into(),
                sha256: None,
                homepage: None,
            },
            &zip_buf,
            Some("1.0.0"),
            &HashMap::new(),
            &HashMap::new(),
        );

        assert!(result.is_err());
        assert!(loader::load_installed_plugins().is_empty());
    }

    #[test]
    #[serial_test::serial]
    fn official_zip_rejects_path_like_request_segments_before_writing() {
        let home = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set_home(home.path());

        for (id, version) in [
            ("../eco-boost", "0.1.0"),
            ("eco/boost", "0.1.0"),
            ("eco\\boost", "0.1.0"),
            ("C:eco-boost", "0.1.0"),
            ("eco-boost", "../0.1.0"),
            ("eco-boost", "0/1/0"),
            ("eco-boost", "0\\1\\0"),
            ("eco-boost", "C:0.1.0"),
        ] {
            let result = install_official_plugin_from_zip(
                OfficialPluginInstallRequest {
                    id: id.into(),
                    version: version.into(),
                    download_url: "https://allthecodes.cc/api/downloads/plugins/eco-boost/v0.1.0/eco-boost-0.1.0.zip".into(),
                    sha256: None,
                    homepage: None,
                },
                &[],
                Some("1.0.0"),
                &HashMap::new(),
                &HashMap::new(),
            );

            assert!(matches!(result, Err(InstallError::ValidationFailed(_))));
            assert!(loader::load_installed_plugins().is_empty());
            assert!(!crate::plugins_dir().join("eco-boost").exists());
        }
    }
}
