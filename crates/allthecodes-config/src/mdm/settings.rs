//! Managed-specific DTOs extending `RawSettings` with policy fields.
//!
//! The managed settings file can carry both regular settings (model, backend,
//! theme, etc.) and managed-specific policy fields (policy, blocklist,
//! allowlist, enforcement). This module defines the typed policy DTOs and
//! the conversion from `RawSettings` into `ManagedSettings`.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::settings::{RawSettings, SettingsSource};

// ---------------------------------------------------------------------------
// Enforcement
// ---------------------------------------------------------------------------

/// How strictly enforcement is applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Enforcement {
    /// Block non-compliant operations entirely.
    Strict,
    /// Log a warning but allow the operation.
    WarningOnly,
    /// Log the event for audit but take no action.
    AuditOnly,
}

/// Enforcement level with an overridable flag.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct EnforcementLevel {
    /// How strictly enforcement is applied.
    pub level: Enforcement,
    /// Whether the enforcement can be overridden by user/project settings.
    /// When `false`, managed policy is mandatory.
    pub overridable: bool,
}

impl Default for EnforcementLevel {
    fn default() -> Self {
        Self {
            level: Enforcement::Strict,
            overridable: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Plugin policy
// ---------------------------------------------------------------------------

/// Source from which a plugin may be installed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PluginSource {
    /// Official marketplace / registry.
    Marketplace,
    /// Local file path or network share.
    Local,
    /// Git repository URL.
    Git,
    /// Arbitrary URL.
    Url,
    /// Custom / unknown source (carries a string identifier).
    Custom(String),
}

/// Policy that controls plugin installation behaviour.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct PluginInstallPolicy {
    /// Sources from which plugins may be installed.
    pub allowed_sources: Vec<PluginSource>,
    /// Whether plugin signatures are required before installation.
    pub require_signature: Option<bool>,
    /// Maximum number of plugins that may be installed simultaneously.
    pub max_plugins: Option<u32>,
}

// ---------------------------------------------------------------------------
// Tool execution policy
// ---------------------------------------------------------------------------

/// Policy that controls which tools may be executed and under what conditions.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct ToolExecutionPolicy {
    /// Tool names or patterns that are always denied execution.
    pub deny_list: Vec<String>,
    /// Tool names or patterns that are always allowed execution.
    pub allow_list: Vec<String>,
    /// If set, overrides the default permission mode (e.g. "ask", "auto").
    pub require_permission_mode: Option<String>,
}

// ---------------------------------------------------------------------------
// Network access policy
// ---------------------------------------------------------------------------

/// Policy that controls network access from subprocesses and WebFetch.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct NetworkAccessPolicy {
    /// Domains that are allowed for network access. Empty = no restriction.
    pub allowed_domains: Vec<String>,
    /// Whether file downloads are blocked entirely.
    pub block_downloads: Option<bool>,
    /// Whether a proxy is required for all outbound connections.
    pub proxy_required: Option<bool>,
}

// ---------------------------------------------------------------------------
// Managed policy
// ---------------------------------------------------------------------------

/// Container for all managed policy fields.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct ManagedPolicy {
    pub plugin_install_policy: Option<PluginInstallPolicy>,
    pub tool_execution_policy: Option<ToolExecutionPolicy>,
    pub network_access_policy: Option<NetworkAccessPolicy>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

// ---------------------------------------------------------------------------
// Blocklist / Allowlist entries
// ---------------------------------------------------------------------------

/// An entry in the managed blocklist.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct BlocklistEntry {
    /// Pattern to match (tool name, URL pattern, file glob, etc.).
    pub pattern: String,
    /// Optional human-readable reason for the block.
    pub reason: Option<String>,
    /// Source that provided this entry.
    #[serde(skip)]
    pub source: SettingsSource,
}

impl BlocklistEntry {
    /// Create a new blocklist entry with the managed source.
    pub fn new(pattern: impl Into<String>) -> Self {
        Self {
            pattern: pattern.into(),
            reason: None,
            source: SettingsSource::Managed,
        }
    }

    /// Attach a reason to this entry.
    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }
}

impl Default for BlocklistEntry {
    fn default() -> Self {
        Self {
            pattern: String::new(),
            reason: None,
            source: SettingsSource::Managed,
        }
    }
}

/// An entry in the managed allowlist.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct AllowlistEntry {
    /// Pattern to match.
    pub pattern: String,
    /// Optional human-readable reason for the allowance.
    pub reason: Option<String>,
    /// Source that provided this entry.
    #[serde(skip)]
    pub source: SettingsSource,
}

impl AllowlistEntry {
    /// Create a new allowlist entry with the managed source.
    pub fn new(pattern: impl Into<String>) -> Self {
        Self {
            pattern: pattern.into(),
            reason: None,
            source: SettingsSource::Managed,
        }
    }

    /// Attach a reason to this entry.
    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }
}

impl Default for AllowlistEntry {
    fn default() -> Self {
        Self {
            pattern: String::new(),
            reason: None,
            source: SettingsSource::Managed,
        }
    }
}

// ---------------------------------------------------------------------------
// ManagedSettings — the full managed layer view
// ---------------------------------------------------------------------------

/// The full view of a managed settings file's policy-specific content.
///
/// Constructed from a [`RawSettings`] by extracting managed-specific fields
/// from its `extra` map. The regular settings fields (model, backend, etc.)
/// remain accessible through the original [`RawSettings`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ManagedSettings {
    /// Managed policy container.
    pub policy: Option<ManagedPolicy>,
    /// Blocklist entries.
    pub blocklist: Option<Vec<BlocklistEntry>>,
    /// Allowlist entries.
    pub allowlist: Option<Vec<AllowlistEntry>>,
    /// Enforcement level configuration.
    pub enforcement: Option<EnforcementLevel>,
    /// Forward-compat passthrough for unknown managed-specific fields.
    pub extra: HashMap<String, Value>,
}

impl ManagedSettings {
    /// Extract managed-specific fields from a [`RawSettings`].
    ///
    /// The managed settings file may contain top-level keys like `policy`,
    /// `blocklist`, `allowlist`, and `enforcement` alongside regular settings.
    /// This method deserialises those keys from the raw extra map into typed
    /// DTOs.
    pub fn from_raw(raw: &RawSettings) -> Self {
        let mut extra = raw.extra.clone();

        let policy = extra
            .remove("policy")
            .and_then(|v| serde_json::from_value::<ManagedPolicy>(v).ok());

        let blocklist = extra
            .remove("blocklist")
            .and_then(|v| serde_json::from_value::<Vec<BlocklistEntry>>(v).ok())
            .map(|entries| {
                entries
                    .into_iter()
                    .map(|e| BlocklistEntry {
                        source: SettingsSource::Managed,
                        ..e
                    })
                    .collect()
            });

        let allowlist = extra
            .remove("allowlist")
            .and_then(|v| serde_json::from_value::<Vec<AllowlistEntry>>(v).ok())
            .map(|entries| {
                entries
                    .into_iter()
                    .map(|e| AllowlistEntry {
                        source: SettingsSource::Managed,
                        ..e
                    })
                    .collect()
            });

        let enforcement = extra
            .remove("enforcement")
            .and_then(|v| serde_json::from_value::<EnforcementLevel>(v).ok());

        Self {
            policy,
            blocklist,
            allowlist,
            enforcement,
            extra, // whatever remains
        }
    }

    /// Whether there is any managed policy content.
    pub fn is_effectively_empty(&self) -> bool {
        self.policy.is_none()
            && self.blocklist.is_none()
            && self.allowlist.is_none()
            && self.enforcement.is_none()
            && self.extra.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Source-merge logic for managed settings
// ---------------------------------------------------------------------------

/// Merge managed policy fields from multiple sources.
///
/// When multiple sources define policy fields (e.g. multiple managed files or
/// cascading policy), this merge ensures managed settings always win for
/// policy fields. Regular settings follow the normal priority.
pub fn merge_managed_settings(base: &mut ManagedSettings, over: &ManagedSettings) {
    if let Some(ref policy) = over.policy {
        base.policy = Some(merge_managed_policy(base.policy.take(), policy));
    }
    if let Some(ref blocklist) = over.blocklist {
        base.blocklist = Some(blocklist.clone());
    }
    if let Some(ref allowlist) = over.allowlist {
        base.allowlist = Some(allowlist.clone());
    }
    if let Some(ref enforcement) = over.enforcement {
        base.enforcement = Some(enforcement.clone());
    }
    for (k, v) in &over.extra {
        base.extra.insert(k.clone(), v.clone());
    }
}

fn merge_managed_policy(base: Option<ManagedPolicy>, over: &ManagedPolicy) -> ManagedPolicy {
    let mut out = base.unwrap_or_default();
    if let Some(ref pp) = over.plugin_install_policy {
        out.plugin_install_policy = Some(pp.clone());
    }
    if let Some(ref tep) = over.tool_execution_policy {
        out.tool_execution_policy = Some(tep.clone());
    }
    if let Some(ref nap) = over.network_access_policy {
        out.network_access_policy = Some(nap.clone());
    }
    for (k, v) in &over.extra {
        out.extra.insert(k.clone(), v.clone());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_enforcement_default() {
        let e = EnforcementLevel::default();
        assert_eq!(e.level, Enforcement::Strict);
        assert!(!e.overridable);
    }

    #[test]
    fn test_enforcement_deserialize() {
        let json = r#"{"level": "warningOnly", "overridable": true}"#;
        let e: EnforcementLevel = serde_json::from_str(json).unwrap();
        assert_eq!(e.level, Enforcement::WarningOnly);
        assert!(e.overridable);
    }

    #[test]
    fn test_managed_settings_from_raw_with_policy() {
        let raw_json = serde_json::json!({
            "model": "claude-opus-4-6",
            "policy": {
                "pluginInstallPolicy": {
                    "allowedSources": ["marketplace"],
                    "requireSignature": true,
                    "maxPlugins": 10
                },
                "toolExecutionPolicy": {
                    "denyList": ["Bash(*rm*)"],
                    "allowList": ["Read", "Write"],
                    "requirePermissionMode": "ask"
                },
                "networkAccessPolicy": {
                    "allowedDomains": ["github.com"],
                    "blockDownloads": true,
                    "proxyRequired": false
                }
            },
            "enforcement": {
                "level": "strict",
                "overridable": false
            }
        });

        let raw: RawSettings = serde_json::from_value(raw_json).unwrap();
        let managed = ManagedSettings::from_raw(&raw);

        let policy = managed.policy.expect("policy should be present");
        let plugin = policy.plugin_install_policy.expect("plugin install policy");
        assert_eq!(plugin.allowed_sources.len(), 1);
        assert_eq!(plugin.require_signature, Some(true));
        assert_eq!(plugin.max_plugins, Some(10));

        let tool = policy.tool_execution_policy.expect("tool policy");
        assert!(tool.deny_list.contains(&"Bash(*rm*)".to_string()));

        let network = policy.network_access_policy.expect("network policy");
        assert!(network.block_downloads == Some(true));

        let enforcement = managed.enforcement.expect("enforcement");
        assert_eq!(enforcement.level, Enforcement::Strict);
        assert!(!enforcement.overridable);
    }

    #[test]
    fn test_managed_settings_from_raw_with_blocklist() {
        let raw_json = serde_json::json!({
            "blocklist": [
                {"pattern": "Bash(*rm -rf*)", "reason": "Dangerous recursive delete"},
                {"pattern": "Bash(*dd*)", "reason": "Dangerous disk write"}
            ]
        });

        let raw: RawSettings = serde_json::from_value(raw_json).unwrap();
        let managed = ManagedSettings::from_raw(&raw);

        let blocklist = managed.blocklist.expect("blocklist");
        assert_eq!(blocklist.len(), 2);
        assert_eq!(blocklist[0].pattern, "Bash(*rm -rf*)");
        assert_eq!(
            blocklist[0].reason.as_deref(),
            Some("Dangerous recursive delete")
        );
        assert_eq!(blocklist[0].source, SettingsSource::Managed);
    }

    #[test]
    fn test_managed_settings_from_raw_empty() {
        let raw = RawSettings::default();
        let managed = ManagedSettings::from_raw(&raw);
        assert!(managed.is_effectively_empty());
    }

    #[test]
    fn test_managed_settings_preserves_unknown_extra() {
        let raw_json = serde_json::json!({
            "policy": {"toolExecutionPolicy": {"denyList": ["Bash"]}},
            "customManagedField": {"owner": "it-admin"}
        });

        let raw: RawSettings = serde_json::from_value(raw_json).unwrap();
        let managed = ManagedSettings::from_raw(&raw);

        assert!(managed.policy.is_some());
        // "policy" was extracted and removed from extra
        assert!(!managed.extra.contains_key("policy"));
        // "customManagedField" remains in extra
        assert!(managed.extra.contains_key("customManagedField"));
    }

    #[test]
    fn test_merge_managed_settings_overrides() {
        let mut base = ManagedSettings::default();
        let over = ManagedSettings {
            enforcement: Some(EnforcementLevel {
                level: Enforcement::Strict,
                overridable: false,
            }),
            ..Default::default()
        };

        merge_managed_settings(&mut base, &over);
        assert_eq!(
            base.enforcement.as_ref().unwrap().level,
            Enforcement::Strict
        );

        let over2 = ManagedSettings {
            enforcement: Some(EnforcementLevel {
                level: Enforcement::WarningOnly,
                overridable: true,
            }),
            ..Default::default()
        };
        merge_managed_settings(&mut base, &over2);
        // managed always wins for policy fields
        assert_eq!(
            base.enforcement.as_ref().unwrap().level,
            Enforcement::WarningOnly
        );
    }

    #[test]
    fn test_blocklist_entry_new() {
        let entry = BlocklistEntry::new("Bash(*rm*)").with_reason("No recursive deletes");
        assert_eq!(entry.pattern, "Bash(*rm*)");
        assert_eq!(entry.reason.as_deref(), Some("No recursive deletes"));
        assert_eq!(entry.source, SettingsSource::Managed);
    }

    #[test]
    fn test_allowlist_entry_new() {
        let entry = AllowlistEntry::new("Read").with_reason("Safe read operation");
        assert_eq!(entry.pattern, "Read");
        assert_eq!(entry.source, SettingsSource::Managed);
    }
}
