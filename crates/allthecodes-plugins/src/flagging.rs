//! Plugin flagging system.
//!
//! Tracks plugins that have been flagged for review due to policy violations,
//! security concerns, or other issues.

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

/// Reason a plugin was flagged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FlagReason {
    /// Plugin comes from an unknown or untrusted source.
    UnknownSource,
    /// Publisher is not trusted or verified.
    UntrustedPublisher,
    /// Known security vulnerability.
    Vulnerability(String),
    /// Policy violation (carries a detail string).
    PolicyViolation(String),
    /// Corrupted or invalid plugin content.
    Corrupted,
    /// Other reason.
    Other(String),
}

/// A flagged plugin record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlaggedPlugin {
    /// Plugin ID.
    pub plugin_id: String,
    /// Reason for flagging.
    pub reason: FlagReason,
    /// Human-readable detail.
    pub detail: String,
    /// Timestamp (Unix seconds) when the flag was raised.
    pub flagged_at: i64,
}

/// Thread-safe plugin flagging state.
pub struct PluginFlagging {
    flagged: Mutex<Vec<FlaggedPlugin>>,
}

impl PluginFlagging {
    /// Create a new empty flagging state.
    pub fn new() -> Self {
        Self {
            flagged: Mutex::new(Vec::new()),
        }
    }

    /// Flag a plugin with the given reason.
    pub fn flag_plugin(
        &self,
        plugin_id: impl Into<String>,
        reason: FlagReason,
        detail: impl Into<String>,
    ) {
        use std::time::{SystemTime, UNIX_EPOCH};
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let plugin_id: String = plugin_id.into();
        let detail: String = detail.into();

        let mut flagged = self.flagged.lock();
        // Remove any existing flag for this plugin
        flagged.retain(|f| f.plugin_id != plugin_id);
        flagged.push(FlaggedPlugin {
            plugin_id,
            reason,
            detail,
            flagged_at: now,
        });
    }

    /// Clear all flags for a plugin.
    pub fn clear_flag(&self, plugin_id: &str) {
        self.flagged.lock().retain(|f| f.plugin_id != plugin_id);
    }

    /// Get all currently flagged plugins.
    pub fn get_flagged_plugins(&self) -> Vec<FlaggedPlugin> {
        self.flagged.lock().clone()
    }

    /// Check whether a specific plugin is flagged.
    pub fn is_flagged(&self, plugin_id: &str) -> bool {
        self.flagged.lock().iter().any(|f| f.plugin_id == plugin_id)
    }

    /// Get the flag reason for a specific plugin, if any.
    pub fn flag_reason(&self, plugin_id: &str) -> Option<FlagReason> {
        self.flagged
            .lock()
            .iter()
            .find(|f| f.plugin_id == plugin_id)
            .map(|f| f.reason.clone())
    }
}

impl Default for PluginFlagging {
    fn default() -> Self {
        Self::new()
    }
}

/// Global plugin flagging state.
pub static GLOBAL_FLAGGING: LazyLock<PluginFlagging> = LazyLock::new(PluginFlagging::new);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flag_and_clear() {
        let state = PluginFlagging::new();
        assert!(!state.is_flagged("test-plugin"));

        state.flag_plugin("test-plugin", FlagReason::UnknownSource, "Untrusted source");
        assert!(state.is_flagged("test-plugin"));

        let flags = state.get_flagged_plugins();
        assert_eq!(flags.len(), 1);
        assert_eq!(flags[0].plugin_id, "test-plugin");
        assert_eq!(flags[0].reason, FlagReason::UnknownSource);

        state.clear_flag("test-plugin");
        assert!(!state.is_flagged("test-plugin"));
        assert!(state.get_flagged_plugins().is_empty());
    }

    #[test]
    fn test_flag_updates_reason() {
        let state = PluginFlagging::new();
        state.flag_plugin("p", FlagReason::UnknownSource, "first");
        state.flag_plugin("p", FlagReason::Vulnerability("CVE-2024".into()), "second");

        let flags = state.get_flagged_plugins();
        assert_eq!(flags.len(), 1);
        assert_eq!(
            flags[0].reason,
            FlagReason::Vulnerability("CVE-2024".into())
        );
    }

    #[test]
    fn test_flag_reason_returns_correct() {
        let state = PluginFlagging::new();
        state.flag_plugin("p", FlagReason::Corrupted, "bad hash");
        assert_eq!(state.flag_reason("p"), Some(FlagReason::Corrupted));
        assert_eq!(state.flag_reason("missing"), None);
    }

    #[test]
    fn test_global_flagged_accessible() {
        // Just verify the global is accessible and doesn't panic
        let _ = GLOBAL_FLAGGING.get_flagged_plugins();
    }

    #[test]
    fn test_multiple_plugins_independent() {
        let state = PluginFlagging::new();
        state.flag_plugin("a", FlagReason::UnknownSource, "src");
        state.flag_plugin("b", FlagReason::PolicyViolation("policy".into()), "pol");

        assert!(state.is_flagged("a"));
        assert!(state.is_flagged("b"));

        state.clear_flag("a");
        assert!(!state.is_flagged("a"));
        assert!(state.is_flagged("b"));
    }
}
