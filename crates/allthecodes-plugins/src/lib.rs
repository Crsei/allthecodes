//! cc-plugins — plugin registry, manifest loading, marketplace, installation,
//! and contribution discovery.

pub mod agents;
pub mod autoupdate;
pub mod blocklist;
pub mod commands;
pub mod configuration;
pub mod dependency_resolver;
pub mod flagging;
pub mod hooks;
pub mod installation;
pub mod lsp;
pub mod manifest;
pub mod marketplace;
pub mod mcpb;
pub mod output_styles;
pub mod policy;
pub mod reconciler;
pub mod sources;
pub mod validation;
pub mod versioning;
pub mod zip_cache;

#[path = "mod.rs"]
mod runtime;

pub use runtime::*;

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Source from which a plugin can be installed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source")]
pub enum PluginSource {
    /// NPM package.
    #[serde(rename = "npm")]
    Npm {
        package: String,
        version: Option<String>,
    },
    /// GitHub repository.
    #[serde(rename = "github")]
    GitHub {
        repo: String,
        ref_spec: Option<String>,
    },
    /// Generic git URL.
    #[serde(rename = "git")]
    Git {
        url: String,
        ref_spec: Option<String>,
    },
    /// Direct HTTP(S) download URL.
    #[serde(rename = "url")]
    Url { url: String },
    /// Marketplace entry resolved from a configured marketplace index.
    #[serde(rename = "marketplace")]
    Marketplace { id: String, source_name: String },
    /// Local filesystem path.
    #[serde(rename = "local")]
    Local { path: String },
}

/// Plugin installation status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PluginStatus {
    /// Not yet installed.
    NotInstalled,
    /// Installed and available.
    Installed,
    /// Installed but disabled by user.
    Disabled,
    /// Installation or load error.
    Error(String),
}

/// A registered plugin in the system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginEntry {
    /// Plugin identifier (e.g. "my-plugin@official-marketplace").
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Version string.
    pub version: String,
    /// Plugin description.
    pub description: String,
    /// Installation source.
    pub source: PluginSource,
    /// Current status.
    pub status: PluginStatus,
    /// Marketplace that provides this plugin (if any).
    pub marketplace: Option<String>,
    /// Local cache path where the plugin is materialized.
    pub cache_path: Option<PathBuf>,
    /// Tools contributed by this plugin.
    pub tools: Vec<String>,
    /// Skills contributed by this plugin.
    pub skills: Vec<String>,
    /// MCP servers contributed by this plugin.
    pub mcp_servers: Vec<String>,
    /// Installation timestamp (Unix seconds).
    pub installed_at: Option<i64>,
    /// Last update timestamp.
    pub updated_at: Option<i64>,
}

/// A marketplace that hosts plugins.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplaceEntry {
    /// Marketplace name (e.g. "official-marketplace").
    pub name: String,
    /// Source for fetching the marketplace.
    pub source: PluginSource,
    /// Local path where marketplace is cached.
    pub install_location: Option<PathBuf>,
    /// Last refresh timestamp.
    pub last_updated: Option<String>,
    /// Whether to auto-update on startup.
    pub auto_update: bool,
}
