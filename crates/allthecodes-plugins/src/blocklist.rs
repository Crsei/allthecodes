//! Plugin blocklist/allowlist management.
//!
//! Manages lists of plugin IDs that are blocked or allowed via managed policy,
//! user settings, or other sources.

use parking_lot::RwLock;
use std::sync::LazyLock;

/// A single blocklist or allowlist entry.
#[derive(Debug, Clone)]
pub struct BlocklistEntry {
    /// Pattern to match against plugin IDs.
    pub pattern: String,
    /// Optional reason.
    pub reason: Option<String>,
}

impl BlocklistEntry {
    pub fn new(pattern: impl Into<String>) -> Self {
        Self {
            pattern: pattern.into(),
            reason: None,
        }
    }

    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }
}

/// Thread-safe plugin blocklist manager.
pub struct PluginBlocklist {
    /// Blocklist entries - plugin IDs matching any entry are blocked.
    blocklist: RwLock<Vec<BlocklistEntry>>,
    /// Allowlist entries - overrides blocklist for matching plugin IDs.
    allowlist: RwLock<Vec<BlocklistEntry>>,
}

impl PluginBlocklist {
    /// Create a new empty blocklist.
    pub fn new() -> Self {
        Self {
            blocklist: RwLock::new(Vec::new()),
            allowlist: RwLock::new(Vec::new()),
        }
    }

    /// Apply blocklist entries, replacing the current blocklist.
    pub fn apply_blocklist(&self, entries: Vec<BlocklistEntry>) {
        *self.blocklist.write() = entries;
    }

    /// Merge additional blocklist entries (append, don't replace).
    pub fn merge_blocklist(&self, entries: Vec<BlocklistEntry>) {
        self.blocklist.write().extend(entries);
    }

    /// Apply allowlist entries, replacing the current allowlist.
    pub fn apply_allowlist(&self, entries: Vec<BlocklistEntry>) {
        *self.allowlist.write() = entries;
    }

    /// Check whether a plugin ID is blocklisted.
    ///
    /// Allowlist takes precedence: if a plugin matches an allowlist entry,
    /// it is NOT considered blocklisted even if it also matches a blocklist
    /// entry.
    pub fn is_blocklisted(&self, plugin_id: &str) -> bool {
        let allowlist = self.allowlist.read();
        // If allowlisted, never blocklisted
        if allowlist
            .iter()
            .any(|e| pattern_matches(&e.pattern, plugin_id))
        {
            return false;
        }
        drop(allowlist);

        let blocklist = self.blocklist.read();
        blocklist
            .iter()
            .any(|e| pattern_matches(&e.pattern, plugin_id))
    }

    /// Check whether a plugin ID is allowlisted.
    pub fn is_allowlisted(&self, plugin_id: &str) -> bool {
        self.allowlist
            .read()
            .iter()
            .any(|e| pattern_matches(&e.pattern, plugin_id))
    }

    /// Get current blocklist entries.
    pub fn get_blocklist(&self) -> Vec<BlocklistEntry> {
        self.blocklist.read().clone()
    }

    /// Get current allowlist entries.
    pub fn get_allowlist(&self) -> Vec<BlocklistEntry> {
        self.allowlist.read().clone()
    }

    /// Clear all blocklist and allowlist entries.
    pub fn clear(&self) {
        self.blocklist.write().clear();
        self.allowlist.write().clear();
    }
}

impl Default for PluginBlocklist {
    fn default() -> Self {
        Self::new()
    }
}

/// Simple pattern matching for blocklist/allowlist entries.
///
/// Supported patterns:
/// - Exact match: `"my-plugin"` matches exactly `"my-plugin"`
/// - Prefix wildcard: `"danger-*"` matches `"danger-tool"` or `"danger-anything"`
/// - Suffix wildcard: `"*@untrusted"` matches `"my-plugin@untrusted"`
/// - Contains wildcard: `"*suspicious*"` matches anything containing "suspicious"
fn pattern_matches(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }

    if pattern.starts_with('*') && pattern.ends_with('*') {
        // Contains matching: *substring*
        let inner = &pattern[1..pattern.len() - 1];
        return value.contains(inner);
    }

    if pattern.ends_with('*') {
        // Prefix matching: prefix*
        let prefix = &pattern[..pattern.len() - 1];
        return value.starts_with(prefix);
    }

    if pattern.starts_with('*') {
        // Suffix matching: *suffix
        let suffix = &pattern[1..];
        return value.ends_with(suffix);
    }

    // Exact match
    pattern == value
}

/// Global blocklist/allowlist state.
pub static GLOBAL_BLOCKLIST: LazyLock<PluginBlocklist> = LazyLock::new(PluginBlocklist::new);

/// Convert `allthecodes_config::mdm::settings::BlocklistEntry` to our local type.
pub fn from_config_blocklist(
    entries: &[allthecodes_config::mdm::BlocklistEntry],
) -> Vec<BlocklistEntry> {
    entries
        .iter()
        .map(|e| BlocklistEntry {
            pattern: e.pattern.clone(),
            reason: e.reason.clone(),
        })
        .collect()
}

/// Convert `allthecodes_config::mdm::settings::AllowlistEntry` to our local type.
pub fn from_config_allowlist(
    entries: &[allthecodes_config::mdm::AllowlistEntry],
) -> Vec<BlocklistEntry> {
    entries
        .iter()
        .map(|e| BlocklistEntry {
            pattern: e.pattern.clone(),
            reason: e.reason.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blocklist_exact_match() {
        let bl = PluginBlocklist::new();
        bl.apply_blocklist(vec![BlocklistEntry::new("bad-plugin")]);

        assert!(bl.is_blocklisted("bad-plugin"));
        assert!(!bl.is_blocklisted("good-plugin"));
        assert!(!bl.is_blocklisted("bad-plugin-extra"));
    }

    #[test]
    fn test_blocklist_wildcard_match() {
        let bl = PluginBlocklist::new();
        bl.apply_blocklist(vec![
            BlocklistEntry::new("danger-*"),
            BlocklistEntry::new("*@untrusted"),
            BlocklistEntry::new("*malware*"),
        ]);

        assert!(bl.is_blocklisted("danger-tool"));
        assert!(bl.is_blocklisted("danger-anything"));
        assert!(bl.is_blocklisted("my-plugin@untrusted"));
        assert!(bl.is_blocklisted("a-malware-plugin"));
        assert!(!bl.is_blocklisted("safe-plugin"));
    }

    #[test]
    fn test_allowlist_overrides_blocklist() {
        let bl = PluginBlocklist::new();
        bl.apply_blocklist(vec![BlocklistEntry::new("*@untrusted")]);
        bl.apply_allowlist(vec![BlocklistEntry::new("safe@untrusted")]);

        assert!(bl.is_blocklisted("bad@untrusted"));
        assert!(!bl.is_blocklisted("safe@untrusted"));
    }

    #[test]
    fn test_star_matches_all() {
        let bl = PluginBlocklist::new();
        bl.apply_blocklist(vec![BlocklistEntry::new("*")]);

        assert!(bl.is_blocklisted("anything"));
        assert!(bl.is_blocklisted(""));
    }

    #[test]
    fn test_clear_resets() {
        let bl = PluginBlocklist::new();
        bl.apply_blocklist(vec![BlocklistEntry::new("bad")]);
        assert!(bl.is_blocklisted("bad"));
        bl.clear();
        assert!(!bl.is_blocklisted("bad"));
    }
}
