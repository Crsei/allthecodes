//! Plugin marketplace index.
//!
//! Manages multiple marketplace sources (URL, GitHub, npm, file) and provides
//! browsing, searching, and caching of marketplace plugin listings.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;
use std::time::Duration;

use crate::PluginSource;

pub const DEFAULT_MARKETPLACE_SOURCE_NAME: &str = "default-marketplace-source";
pub const OFFICIAL_MARKETPLACE_SOURCE_NAME: &str = "official-allthecodes";
pub const OFFICIAL_MARKETPLACE_URL: &str =
    "https://download.allthecodes.cc/plugins/marketplace.json";
pub const OFFICIAL_MARKETPLACE_REFRESH_TTL_SECONDS: i64 = 300;
const OFFICIAL_DOWNLOAD_HOSTS: &[&str] = &["allthecodes.cc", "download.allthecodes.cc"];
pub const SUPERPOWERS_PLUGIN_ID: &str = "superpowers";
const SUPERPOWERS_VERSION: &str = "5.1.0";
const SUPERPOWERS_REPO: &str = "obra/superpowers";
const SUPERPOWERS_HOMEPAGE: &str = "https://github.com/obra/superpowers";
const SUPERPOWERS_DOWNLOAD_URL: &str =
    "https://github.com/obra/superpowers/archive/refs/tags/v5.1.0.zip";

/// A marketplace source configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplaceSource {
    /// Unique name for this marketplace.
    pub name: String,
    /// Source type and location.
    pub source: PluginSource,
    /// Human-readable description.
    #[serde(default)]
    pub description: String,
    /// Whether auto-refresh is enabled.
    #[serde(default = "default_true")]
    pub auto_update: bool,
    /// Priority (lower = higher priority for conflict resolution).
    #[serde(default)]
    pub priority: u32,
}

fn default_true() -> bool {
    true
}

/// A plugin listing from a marketplace index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplacePluginEntry {
    /// Unique plugin ID within the marketplace.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Short description.
    pub description: String,
    /// Latest version.
    pub version: String,
    /// Author name or organization.
    #[serde(default)]
    pub author: Option<String>,
    /// Marketplace source name.
    #[serde(default)]
    pub source_name: String,
    /// Download URL for the plugin package.
    #[serde(default)]
    pub download_url: Option<String>,
    /// Expected checksum (SHA-256 hex).
    #[serde(default)]
    pub checksum: Option<String>,
    /// Expected SHA-256 hex digest, preserved for frontend compatibility.
    #[serde(default)]
    pub sha256: Option<String>,
    /// Plugin tags/categories.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Homepage URL.
    #[serde(default)]
    pub homepage: Option<String>,
    /// License identifier.
    #[serde(default)]
    pub license: Option<String>,
}

/// The marketplace index, managing multiple sources.
pub struct MarketplaceIndex {
    /// Registered marketplace sources.
    sources: Mutex<HashMap<String, MarketplaceSource>>,
    /// Cached entries from all marketplaces.
    cache: Mutex<HashMap<String, Vec<MarketplacePluginEntry>>>,
    /// Last refresh timestamp per source (Unix seconds).
    refresh_times: Mutex<HashMap<String, i64>>,
}

impl MarketplaceIndex {
    /// Create a new empty marketplace index.
    pub fn new() -> Self {
        Self {
            sources: Mutex::new(HashMap::new()),
            cache: Mutex::new(HashMap::new()),
            refresh_times: Mutex::new(HashMap::new()),
        }
    }

    /// Create a marketplace index seeded with built-in entries.
    pub fn with_builtin_defaults() -> Self {
        let index = Self::new();
        index.register_source(default_marketplace_source());
        index.register_source(official_marketplace_source());
        index.set_marketplace_entries(
            DEFAULT_MARKETPLACE_SOURCE_NAME,
            default_marketplace_entries(),
        );
        index
    }

    /// Register a marketplace source.
    pub fn register_source(&self, source: MarketplaceSource) {
        self.sources.lock().insert(source.name.clone(), source);
    }

    /// Remove a marketplace source by name.
    pub fn unregister_source(&self, name: &str) {
        self.sources.lock().remove(name);
        self.cache.lock().remove(name);
        self.refresh_times.lock().remove(name);
    }

    /// Get all registered sources.
    pub fn list_sources(&self) -> Vec<MarketplaceSource> {
        self.sources.lock().values().cloned().collect()
    }

    /// List all cached marketplace entries across all sources.
    pub fn list_all_entries(&self) -> Vec<MarketplacePluginEntry> {
        let cache = self.cache.lock();
        let mut all: Vec<MarketplacePluginEntry> = cache.values().flat_map(|v| v.clone()).collect();
        all.sort_by(|a, b| a.name.cmp(&b.name));
        all
    }

    /// Search for plugins by name or description across all marketplaces.
    pub fn search(&self, query: &str) -> Vec<MarketplacePluginEntry> {
        let query_lower = query.to_lowercase();
        let cache = self.cache.lock();
        let mut results: Vec<MarketplacePluginEntry> = cache
            .values()
            .flat_map(|v| v.iter())
            .filter(|entry| {
                entry.name.to_lowercase().contains(&query_lower)
                    || entry.description.to_lowercase().contains(&query_lower)
                    || entry
                        .tags
                        .iter()
                        .any(|t| t.to_lowercase().contains(&query_lower))
                    || entry.id.to_lowercase().contains(&query_lower)
            })
            .cloned()
            .collect();
        results.sort_by(|a, b| a.name.cmp(&b.name));
        results
    }

    /// Get cached entries from a specific marketplace.
    pub fn get_marketplace_entries(&self, name: &str) -> Vec<MarketplacePluginEntry> {
        self.cache.lock().get(name).cloned().unwrap_or_default()
    }

    /// Update the cached entries for a marketplace.
    pub fn set_marketplace_entries(&self, name: &str, entries: Vec<MarketplacePluginEntry>) {
        use std::time::{SystemTime, UNIX_EPOCH};
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        self.cache.lock().insert(name.to_string(), entries);
        self.refresh_times.lock().insert(name.to_string(), now);
    }

    /// Check if a marketplace needs refresh (cache older than TTL).
    pub fn needs_refresh(&self, name: &str, ttl_seconds: i64) -> bool {
        use std::time::{SystemTime, UNIX_EPOCH};
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let last = self.refresh_times.lock().get(name).copied().unwrap_or(0);
        now - last > ttl_seconds
    }

    /// Refresh the official marketplace only when its cache is empty or stale.
    ///
    /// This is best-effort for UI listing: failures keep the previous cache and
    /// do not advance the refresh timestamp.
    pub async fn refresh_official_if_stale(&self, ttl_seconds: i64) -> Result<usize> {
        let current_entries = self.get_marketplace_entries(OFFICIAL_MARKETPLACE_SOURCE_NAME);
        if !current_entries.is_empty()
            && !self.needs_refresh(OFFICIAL_MARKETPLACE_SOURCE_NAME, ttl_seconds)
        {
            return Ok(current_entries.len());
        }

        let Some(source) = self
            .sources
            .lock()
            .get(OFFICIAL_MARKETPLACE_SOURCE_NAME)
            .cloned()
        else {
            return Ok(0);
        };

        match load_entries_from_source(&source).await {
            Ok(entries) => {
                let count = entries.len();
                self.set_marketplace_entries(OFFICIAL_MARKETPLACE_SOURCE_NAME, entries);
                Ok(count)
            }
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    "Official plugin marketplace refresh failed; keeping cached entries"
                );
                Ok(current_entries.len())
            }
        }
    }

    /// Find a plugin entry by ID across all marketplaces.
    pub fn find_plugin(&self, id: &str) -> Option<MarketplacePluginEntry> {
        let cache = self.cache.lock();
        for entries in cache.values() {
            if let Some(entry) = entries.iter().find(|e| e.id == id) {
                return Some(entry.clone());
            }
        }
        None
    }

    /// Clear all cached data.
    pub fn clear_cache(&self) {
        self.cache.lock().clear();
        self.refresh_times.lock().clear();
    }

    /// Load known marketplaces from a JSON file.
    pub fn load_from_file(&self, path: &Path) -> Result<()> {
        if !path.exists() {
            return Ok(());
        }
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read marketplaces file: {}", path.display()))?;
        let sources: Vec<MarketplaceSource> = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse marketplaces file: {}", path.display()))?;
        for source in sources {
            self.register_source(source);
        }
        Ok(())
    }

    /// Refresh all registered marketplace sources and populate cached entries.
    pub async fn refresh_all(&self) -> Result<usize> {
        let sources = self.list_sources();
        let mut total = 0usize;
        for source in sources {
            let entries = if source.name == DEFAULT_MARKETPLACE_SOURCE_NAME {
                default_marketplace_entries()
            } else {
                match load_entries_from_source(&source).await {
                    Ok(entries) => entries,
                    Err(error) if source.name == OFFICIAL_MARKETPLACE_SOURCE_NAME => {
                        tracing::warn!(
                            error = %error,
                            "Official plugin marketplace refresh failed; keeping cached entries"
                        );
                        total += self.get_marketplace_entries(&source.name).len();
                        continue;
                    }
                    Err(error) => {
                        return Err(error).with_context(|| {
                            format!("Failed to refresh marketplace '{}'", source.name)
                        });
                    }
                }
            };
            total += entries.len();
            self.set_marketplace_entries(&source.name, entries);
        }
        Ok(total)
    }

    /// Save known marketplaces to a JSON file.
    pub fn save_to_file(&self, path: &Path) -> Result<()> {
        let sources = self.list_sources();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
        }
        let content = serde_json::to_string_pretty(&sources)
            .context("Failed to serialize marketplace sources")?;
        std::fs::write(path, &content)
            .with_context(|| format!("Failed to write marketplaces file: {}", path.display()))?;
        Ok(())
    }
}

impl Default for MarketplaceIndex {
    fn default() -> Self {
        Self::new()
    }
}

/// Global marketplace index instance.
pub static GLOBAL_MARKETPLACE_INDEX: LazyLock<MarketplaceIndex> =
    LazyLock::new(MarketplaceIndex::with_builtin_defaults);

/// List marketplace entries for a given source name.
///
/// Returns cached entries if available, or an empty vec if the marketplace
/// hasn't been refreshed yet.
pub fn list_marketplace(name: &str) -> Vec<MarketplacePluginEntry> {
    GLOBAL_MARKETPLACE_INDEX.get_marketplace_entries(name)
}

/// List all plugins from all marketplaces.
pub fn list_all_marketplaces() -> Vec<MarketplacePluginEntry> {
    GLOBAL_MARKETPLACE_INDEX.list_all_entries()
}

/// Search across all marketplaces.
pub fn search_marketplace(query: &str) -> Vec<MarketplacePluginEntry> {
    GLOBAL_MARKETPLACE_INDEX.search(query)
}

/// Refresh all configured marketplace sources in the global index.
pub async fn refresh_all_marketplaces() -> Result<usize> {
    GLOBAL_MARKETPLACE_INDEX.refresh_all().await
}

/// Best-effort refresh for the official marketplace used by UI listing.
pub async fn refresh_official_marketplace_if_stale() -> Result<usize> {
    GLOBAL_MARKETPLACE_INDEX
        .refresh_official_if_stale(OFFICIAL_MARKETPLACE_REFRESH_TTL_SECONDS)
        .await
}

/// Get the marketplace directory path.
pub fn marketplaces_cache_dir() -> PathBuf {
    crate::marketplaces_dir()
}

pub fn default_marketplace_source() -> MarketplaceSource {
    MarketplaceSource {
        name: DEFAULT_MARKETPLACE_SOURCE_NAME.to_string(),
        source: PluginSource::GitHub {
            repo: SUPERPOWERS_REPO.to_string(),
            ref_spec: Some(format!("v{SUPERPOWERS_VERSION}")),
        },
        description: "Built-in allthecodes marketplace entries".to_string(),
        auto_update: false,
        priority: 0,
    }
}

pub fn default_marketplace_entries() -> Vec<MarketplacePluginEntry> {
    vec![MarketplacePluginEntry {
        id: SUPERPOWERS_PLUGIN_ID.to_string(),
        name: "Superpowers".to_string(),
        description: "Composable skills and workflow methodology for coding agents.".to_string(),
        version: SUPERPOWERS_VERSION.to_string(),
        author: Some("Jesse Vincent".to_string()),
        source_name: DEFAULT_MARKETPLACE_SOURCE_NAME.to_string(),
        download_url: Some(SUPERPOWERS_DOWNLOAD_URL.to_string()),
        checksum: None,
        sha256: None,
        tags: vec![
            "tool".to_string(),
            "skills".to_string(),
            "workflow".to_string(),
            "featured".to_string(),
        ],
        homepage: Some(SUPERPOWERS_HOMEPAGE.to_string()),
        license: Some("MIT".to_string()),
    }]
}

pub fn official_marketplace_source() -> MarketplaceSource {
    MarketplaceSource {
        name: OFFICIAL_MARKETPLACE_SOURCE_NAME.to_string(),
        source: PluginSource::Url {
            url: OFFICIAL_MARKETPLACE_URL.to_string(),
        },
        description: "Official allthecodes plugin marketplace".to_string(),
        auto_update: true,
        priority: 0,
    }
}

async fn load_entries_from_source(
    source: &MarketplaceSource,
) -> Result<Vec<MarketplacePluginEntry>> {
    let content = match &source.source {
        PluginSource::Local { path } => {
            let path = PathBuf::from(path);
            let index_path = if path.is_dir() {
                let marketplace = path.join("marketplace.json");
                if marketplace.exists() {
                    marketplace
                } else {
                    path.join("index.json")
                }
            } else {
                path
            };
            std::fs::read_to_string(&index_path).with_context(|| {
                format!("Failed to read marketplace index: {}", index_path.display())
            })?
        }
        PluginSource::Git { url, .. } | PluginSource::Url { url } => {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(20))
                .build()
                .context("Failed to build marketplace HTTP client")?;
            let response = client
                .get(url)
                .send()
                .await
                .with_context(|| format!("Failed to fetch marketplace index: {url}"))?;
            if !response.status().is_success() {
                anyhow::bail!(
                    "Marketplace index {} returned HTTP {}",
                    url,
                    response.status()
                );
            }
            response
                .text()
                .await
                .context("Failed to read marketplace index response body")?
        }
        PluginSource::GitHub { repo, ref_spec } => {
            let branch = ref_spec.as_deref().unwrap_or("main");
            let url = format!("https://raw.githubusercontent.com/{repo}/{branch}/marketplace.json");
            let response = reqwest::get(&url)
                .await
                .with_context(|| format!("Failed to fetch GitHub marketplace index: {url}"))?;
            if !response.status().is_success() {
                anyhow::bail!(
                    "GitHub marketplace index {} returned HTTP {}",
                    url,
                    response.status()
                );
            }
            response
                .text()
                .await
                .context("Failed to read GitHub marketplace index response body")?
        }
        PluginSource::Npm { .. } | PluginSource::Marketplace { .. } => {
            anyhow::bail!("Unsupported marketplace source kind: {:?}", source.source);
        }
    };

    let mut entries = parse_marketplace_entries(&content)?;
    for entry in &mut entries {
        if entry.source_name.trim().is_empty() {
            entry.source_name = source.name.clone();
        }
        normalize_marketplace_entry(entry);
        if source.name == OFFICIAL_MARKETPLACE_SOURCE_NAME {
            validate_official_marketplace_entry(entry)?;
        }
    }
    Ok(entries)
}

fn normalize_marketplace_entry(entry: &mut MarketplacePluginEntry) {
    match (&entry.checksum, &entry.sha256) {
        (Some(checksum), None) => entry.sha256 = Some(checksum.clone()),
        (None, Some(sha256)) => entry.checksum = Some(sha256.clone()),
        _ => {}
    }
}

pub fn validate_official_marketplace_entry(entry: &MarketplacePluginEntry) -> Result<()> {
    if entry.id.trim().is_empty() {
        anyhow::bail!("Official marketplace entry has empty id");
    }
    if entry.version.trim().is_empty() {
        anyhow::bail!(
            "Official marketplace entry '{}' has empty version",
            entry.id
        );
    }
    let Some(download_url) = entry.download_url.as_deref() else {
        anyhow::bail!(
            "Official marketplace entry '{}' is missing download_url",
            entry.id
        );
    };
    validate_official_download_url(download_url)
        .with_context(|| format!("Invalid official download_url for '{}'", entry.id))
}

pub fn validate_official_download_url(download_url: &str) -> Result<()> {
    let parsed = url::Url::parse(download_url).context("download_url must be a valid URL")?;
    if parsed.scheme() != "https" {
        anyhow::bail!("download_url must use https");
    }
    if !parsed
        .host_str()
        .is_some_and(|host| OFFICIAL_DOWNLOAD_HOSTS.contains(&host))
    {
        anyhow::bail!("download_url host must be allthecodes.cc or download.allthecodes.cc");
    }
    Ok(())
}

fn parse_marketplace_entries(content: &str) -> Result<Vec<MarketplacePluginEntry>> {
    let value: serde_json::Value =
        serde_json::from_str(content).context("Failed to parse marketplace index JSON")?;
    let mut entries = if value.is_array() {
        serde_json::from_value(value).context("Failed to parse marketplace entries array")?
    } else {
        let mut parsed = None;
        for key in ["plugins", "entries"] {
            if let Some(entries) = value.get(key) {
                parsed = Some(
                    serde_json::from_value(entries.clone())
                        .with_context(|| format!("Failed to parse marketplace '{key}' array"))?,
                );
                break;
            }
        }
        parsed.ok_or_else(|| {
            anyhow::anyhow!("Marketplace index must be an array or contain a plugins/entries array")
        })?
    };
    for entry in &mut entries {
        normalize_marketplace_entry(entry);
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_index() {
        let index = MarketplaceIndex::new();
        assert!(index.list_sources().is_empty());
        assert!(index.list_all_entries().is_empty());
    }

    #[test]
    fn builtin_index_lists_superpowers_by_default() {
        let index = MarketplaceIndex::with_builtin_defaults();
        let entries = index.list_all_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, SUPERPOWERS_PLUGIN_ID);
        assert_eq!(entries[0].source_name, DEFAULT_MARKETPLACE_SOURCE_NAME);
        assert_eq!(
            entries[0].download_url.as_deref(),
            Some(SUPERPOWERS_DOWNLOAD_URL)
        );
        assert_eq!(
            index.list_sources()[0].name,
            DEFAULT_MARKETPLACE_SOURCE_NAME
        );
        assert!(index.find_plugin(SUPERPOWERS_PLUGIN_ID).is_some());
    }

    #[tokio::test]
    async fn builtin_marketplace_refresh_stays_local() {
        let index = MarketplaceIndex::with_builtin_defaults();
        let count = index.refresh_all().await.unwrap();
        assert_eq!(count, 1);
        assert_eq!(
            index
                .find_plugin(SUPERPOWERS_PLUGIN_ID)
                .unwrap()
                .source_name,
            DEFAULT_MARKETPLACE_SOURCE_NAME
        );
    }

    #[test]
    fn test_register_and_unregister_source() {
        let index = MarketplaceIndex::new();
        let source = MarketplaceSource {
            name: "test-mp".into(),
            source: PluginSource::Local {
                path: "/tmp".into(),
            },
            description: "Test marketplace".into(),
            auto_update: true,
            priority: 0,
        };

        index.register_source(source);
        assert_eq!(index.list_sources().len(), 1);

        index.unregister_source("test-mp");
        assert!(index.list_sources().is_empty());
    }

    #[test]
    fn test_set_and_get_entries() {
        let index = MarketplaceIndex::new();
        let entries = vec![
            MarketplacePluginEntry {
                id: "plugin-a".into(),
                name: "Plugin A".into(),
                description: "First plugin".into(),
                version: "1.0.0".into(),
                author: Some("Author".into()),
                source_name: "test-mp".into(),
                download_url: None,
                checksum: None,
                sha256: None,
                tags: vec!["utility".into()],
                homepage: None,
                license: None,
            },
            MarketplacePluginEntry {
                id: "plugin-b".into(),
                name: "Plugin B".into(),
                description: "Second plugin".into(),
                version: "2.0.0".into(),
                author: None,
                source_name: "test-mp".into(),
                download_url: None,
                checksum: None,
                sha256: None,
                tags: vec![],
                homepage: None,
                license: None,
            },
        ];

        index.set_marketplace_entries("test-mp", entries.clone());
        assert_eq!(index.get_marketplace_entries("test-mp").len(), 2);
        assert_eq!(index.list_all_entries().len(), 2);
    }

    #[test]
    fn test_search() {
        let index = MarketplaceIndex::new();
        let entries = vec![
            MarketplacePluginEntry {
                id: "fmt".into(),
                name: "Formatter".into(),
                description: "Code formatting tool".into(),
                version: "1.0.0".into(),
                author: None,
                source_name: "mp".into(),
                download_url: None,
                checksum: None,
                sha256: None,
                tags: vec!["lint".into(), "style".into()],
                homepage: None,
                license: None,
            },
            MarketplacePluginEntry {
                id: "lint".into(),
                name: "Linter".into(),
                description: "Static analysis".into(),
                version: "1.0.0".into(),
                author: None,
                source_name: "mp".into(),
                download_url: None,
                checksum: None,
                sha256: None,
                tags: vec!["lint".into()],
                homepage: None,
                license: None,
            },
        ];

        index.set_marketplace_entries("mp", entries);

        let results = index.search("format");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "fmt");

        let results = index.search("lint");
        assert_eq!(results.len(), 2);

        let results = index.search("nonexistent");
        assert!(results.is_empty());
    }

    #[test]
    fn test_find_plugin() {
        let index = MarketplaceIndex::new();
        let entries = vec![MarketplacePluginEntry {
            id: "my-plugin".into(),
            name: "My Plugin".into(),
            description: "".into(),
            version: "1.0.0".into(),
            author: None,
            source_name: "mp".into(),
            download_url: None,
            checksum: None,
            sha256: None,
            tags: vec![],
            homepage: None,
            license: None,
        }];
        index.set_marketplace_entries("mp", entries);

        assert!(index.find_plugin("my-plugin").is_some());
        assert!(index.find_plugin("missing").is_none());
    }

    #[test]
    fn test_needs_refresh() {
        let index = MarketplaceIndex::new();
        assert!(index.needs_refresh("test", 3600));

        index.set_marketplace_entries("test", vec![]);
        assert!(!index.needs_refresh("test", 3600));
    }

    #[test]
    fn test_save_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("marketplaces.json");

        let index = MarketplaceIndex::new();
        index.register_source(MarketplaceSource {
            name: "official".into(),
            source: PluginSource::Local {
                path: "/tmp".into(),
            },
            description: "Official".into(),
            auto_update: true,
            priority: 0,
        });
        index.save_to_file(&path).unwrap();

        let loaded = MarketplaceIndex::new();
        loaded.load_from_file(&path).unwrap();
        assert_eq!(loaded.list_sources().len(), 1);
        assert_eq!(loaded.list_sources()[0].name, "official");
    }

    #[tokio::test]
    async fn refresh_local_marketplace_populates_entries() {
        let dir = tempfile::tempdir().unwrap();
        let index_path = dir.path().join("marketplace.json");
        std::fs::write(
            &index_path,
            r#"{
                "plugins": [
                    {
                        "id": "rust-tools",
                        "name": "Rust Tools",
                        "description": "Rust helpers",
                        "version": "1.2.3",
                        "download_url": "file:///tmp/rust-tools.zip",
                        "tags": ["rust"]
                    },
                    {
                        "id": "python-tools",
                        "name": "Python Tools",
                        "description": "Python helpers",
                        "version": "2.0.0"
                    }
                ]
            }"#,
        )
        .unwrap();

        let index = MarketplaceIndex::new();
        index.register_source(MarketplaceSource {
            name: "local-fixture".into(),
            source: PluginSource::Local {
                path: index_path.to_string_lossy().to_string(),
            },
            description: "Local fixture".into(),
            auto_update: true,
            priority: 0,
        });

        let count = index.refresh_all().await.unwrap();
        assert_eq!(count, 2);
        assert_eq!(index.search("rust").len(), 1);
        assert_eq!(
            index.find_plugin("rust-tools").unwrap().source_name,
            "local-fixture"
        );
    }

    #[tokio::test]
    async fn official_refresh_if_stale_populates_empty_cache() {
        let dir = tempfile::tempdir().unwrap();
        let index_path = dir.path().join("marketplace.json");
        std::fs::write(
            &index_path,
            r#"{
                "plugins": [
                    {
                        "id": "eco-boost",
                        "name": "Eco Boost",
                        "description": "Eco helper",
                        "version": "0.1.0",
                        "download_url": "https://allthecodes.cc/api/downloads/plugins/eco-boost/v0.1.0/eco-boost-0.1.0.zip"
                    }
                ]
            }"#,
        )
        .unwrap();

        let index = MarketplaceIndex::new();
        index.register_source(MarketplaceSource {
            name: OFFICIAL_MARKETPLACE_SOURCE_NAME.into(),
            source: PluginSource::Local {
                path: index_path.to_string_lossy().to_string(),
            },
            description: "Official fixture".into(),
            auto_update: true,
            priority: 0,
        });

        let count = index.refresh_official_if_stale(300).await.unwrap();

        assert_eq!(count, 1);
        assert_eq!(
            index.get_marketplace_entries(OFFICIAL_MARKETPLACE_SOURCE_NAME)[0].id,
            "eco-boost"
        );
    }

    #[tokio::test]
    async fn official_refresh_failure_preserves_cache_and_timestamp() {
        let index = MarketplaceIndex::new();
        index.register_source(MarketplaceSource {
            name: OFFICIAL_MARKETPLACE_SOURCE_NAME.into(),
            source: PluginSource::Local {
                path: "/path/that/does/not/exist/marketplace.json".into(),
            },
            description: "Official fixture".into(),
            auto_update: true,
            priority: 0,
        });
        index.set_marketplace_entries(
            OFFICIAL_MARKETPLACE_SOURCE_NAME,
            vec![MarketplacePluginEntry {
                id: "cached-plugin".into(),
                name: "Cached Plugin".into(),
                description: "".into(),
                version: "1.0.0".into(),
                author: None,
                source_name: OFFICIAL_MARKETPLACE_SOURCE_NAME.into(),
                download_url: Some(
                    "https://allthecodes.cc/api/downloads/plugins/cached-plugin/v1.0.0/cached-plugin-1.0.0.zip"
                        .into(),
                ),
                checksum: None,
                sha256: None,
                tags: vec![],
                homepage: None,
                license: None,
            }],
        );
        index
            .refresh_times
            .lock()
            .insert(OFFICIAL_MARKETPLACE_SOURCE_NAME.into(), 0);

        let count = index.refresh_official_if_stale(300).await.unwrap();

        assert_eq!(count, 1);
        assert_eq!(
            index.get_marketplace_entries(OFFICIAL_MARKETPLACE_SOURCE_NAME)[0].id,
            "cached-plugin"
        );
        assert_eq!(
            index
                .refresh_times
                .lock()
                .get(OFFICIAL_MARKETPLACE_SOURCE_NAME)
                .copied(),
            Some(0)
        );

        let count = index.refresh_all().await.unwrap();
        assert_eq!(count, 1);
        assert_eq!(
            index
                .refresh_times
                .lock()
                .get(OFFICIAL_MARKETPLACE_SOURCE_NAME)
                .copied(),
            Some(0)
        );
    }

    #[test]
    fn parses_sha256_alias_into_checksum() {
        let entries = parse_marketplace_entries(
            r#"{"plugins":[{"id":"eco-boost","name":"Eco Boost","description":"","version":"0.1.0","download_url":"https://allthecodes.cc/api/downloads/plugins/eco-boost/v0.1.0/eco-boost-0.1.0.zip","sha256":"abc"}]}"#,
        )
        .unwrap();
        assert_eq!(entries[0].checksum.as_deref(), Some("abc"));
    }

    #[test]
    fn official_entry_accepts_cdn_download_host() {
        let entry = MarketplacePluginEntry {
            id: "eco-boost".into(),
            name: "Eco Boost".into(),
            description: "".into(),
            version: "0.1.0".into(),
            author: None,
            source_name: OFFICIAL_MARKETPLACE_SOURCE_NAME.into(),
            download_url: Some(
                "https://download.allthecodes.cc/allthecodesapp/plugins/eco-boost/v0.1.0/eco-boost-0.1.0.zip"
                    .into(),
            ),
            checksum: None,
            sha256: None,
            tags: vec![],
            homepage: None,
            license: None,
        };

        validate_official_marketplace_entry(&entry).unwrap();
    }

    #[test]
    fn official_entry_rejects_non_official_download_host() {
        let entry = MarketplacePluginEntry {
            id: "eco-boost".into(),
            name: "Eco Boost".into(),
            description: "".into(),
            version: "0.1.0".into(),
            author: None,
            source_name: OFFICIAL_MARKETPLACE_SOURCE_NAME.into(),
            download_url: Some("https://example.com/plugin.zip".into()),
            checksum: None,
            sha256: None,
            tags: vec![],
            homepage: None,
            license: None,
        };

        assert!(validate_official_marketplace_entry(&entry).is_err());
    }
}
