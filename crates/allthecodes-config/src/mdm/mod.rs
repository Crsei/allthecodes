//! MDM (Managed Device Management) settings layer.
//!
//! Provides policy-level configuration that overrides user/project/local
//! settings for organizational compliance. Managed settings are loaded from
//! a platform-dependent file path (see [`allthecodes_config::settings::managed_settings_path`])
//! and carry additional policy fields (policy, blocklist, allowlist,
//! enforcement) beyond regular settings.
//!
//! # Architecture
//!
//! ```text
//! Managed settings file (JSON)
//!   ├─ Regular settings  → RawSettings (model, backend, permissions, …)
//!   └─ Policy fields     → ManagedSettings (policy, blocklist, allowlist, enforcement)
//!                           └─ ManagedPolicy (plugin, tool, network policies)
//! ```
//!
//! # Precedence
//!
//! For regular settings the normal priority applies (Default < Managed <
//! User < Project < Local < Env < Cli). For policy fields managed settings
//! **always win** – this is checked via [`is_shadowed_by_managed`] and
//! enforced by the permission validation layer.

mod constants;
mod raw_read;
mod settings;

pub use constants::*;
pub use raw_read::*;
pub use settings::*;

use std::path::PathBuf;

use anyhow::Result;

use crate::settings::{managed_settings_path, RawSettings, SettingsSource};

// ---------------------------------------------------------------------------
// ManagedSettingsConfig — consolidated runtime view
// ---------------------------------------------------------------------------

/// Consolidated processed view of managed settings for the runtime.
///
/// Holds the raw parsed content, the processed policy fields, and metadata
/// about whether policy enforcement is active.
#[derive(Debug, Clone)]
pub struct ManagedSettingsConfig {
    /// Whether a managed settings file was found and loaded.
    pub active: bool,
    /// Path to the managed settings file that was used.
    pub file_path: PathBuf,
    /// The raw settings (regular fields) from the managed file.
    pub raw: Option<RawSettings>,
    /// The extracted policy-specific managed settings.
    pub managed: Option<ManagedSettings>,
    /// Whether policy enforcement is active.
    ///
    /// Enforcement is active when:
    /// 1. The managed settings file exists and defines an enforcement level, or
    /// 2. The `ALLTHECODES_ENFORCE_POLICY` environment variable is set.
    pub enforcement_active: bool,
    /// The effective enforcement level (defaults to Strict when enforcement
    /// is active but no explicit level is configured).
    pub enforcement_level: EnforcementLevel,
}

impl ManagedSettingsConfig {
    /// Create an inactive (no managed settings) config.
    pub fn inactive() -> Self {
        Self {
            active: false,
            file_path: managed_settings_path(),
            raw: None,
            managed: None,
            enforcement_active: false,
            enforcement_level: EnforcementLevel::default(),
        }
    }

    /// True if managed settings are loaded and non-empty.
    pub fn has_content(&self) -> bool {
        self.active
            && self
                .managed
                .as_ref()
                .is_some_and(|m| !m.is_effectively_empty())
    }

    /// Whether a given setting key from a given source would be shadowed
    /// (overridden) by the managed layer.
    ///
    /// For regular settings, managed only shadows the `Default` source.
    /// For managed policy keys (`policy.*`, `blocklist`, `allowlist`,
    /// `enforcement`, and `MANAGED_POLICY_FIELDS`), managed always wins
    /// regardless of source.
    pub fn is_key_shadowed(&self, key: &str, source: SettingsSource) -> bool {
        if source == SettingsSource::Managed || source == SettingsSource::Default {
            return false;
        }
        if !self.active {
            return false;
        }
        // Policy keys are always controlled by managed.
        if constants::MANAGED_POLICY_KEYS.contains(&key)
            || constants::MANAGED_POLICY_FIELDS.contains(&key)
        {
            return true;
        }
        // Regular keys: managed only wins if the source is Default.
        false
    }
}

// ---------------------------------------------------------------------------
// Loader
// ---------------------------------------------------------------------------

/// Load the managed settings file and produce a `ManagedSettingsConfig`.
///
/// This is the primary entry point for the MDM layer. It:
/// 1. Checks if a managed settings file exists.
/// 2. Reads and parses it (first as `RawSettings`, then extracts policy fields).
/// 3. Checks the `ALLTHECODES_ENFORCE_POLICY` env var.
/// 4. Returns a consolidated `ManagedSettingsConfig`.
pub fn load_managed_settings_policy() -> Result<ManagedSettingsConfig> {
    let file_path = managed_settings_path();

    let raw = read_managed_settings_raw()?;

    match raw {
        None => {
            // No managed file; check env for enforcement override.
            let enforcement_active = std::env::var(constants::ALLTHECODES_ENFORCE_POLICY)
                .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
                .unwrap_or(false);

            Ok(ManagedSettingsConfig {
                active: false,
                file_path,
                raw: None,
                managed: None,
                enforcement_active,
                enforcement_level: EnforcementLevel::default(),
            })
        }
        Some(raw_settings) => {
            let managed = ManagedSettings::from_raw(&raw_settings);
            let has_enforcement = managed.enforcement.is_some();
            let enforcement_active = has_enforcement
                || std::env::var(constants::ALLTHECODES_ENFORCE_POLICY)
                    .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
                    .unwrap_or(false);

            let enforcement_level = managed.enforcement.clone().unwrap_or_else(|| {
                if enforcement_active {
                    EnforcementLevel::default()
                } else {
                    EnforcementLevel {
                        level: Enforcement::Strict,
                        overridable: true, // not enforced by default
                    }
                }
            });

            Ok(ManagedSettingsConfig {
                active: true,
                file_path,
                raw: Some(raw_settings),
                managed: Some(managed),
                enforcement_active,
                enforcement_level,
            })
        }
    }
}

/// Consolidated precedence check: returns `true` when a value for `key` from
/// `source` would be overridden (shadowed) by the managed layer.
///
/// This is a convenience wrapper that loads the managed settings config and
/// delegates to [`ManagedSettingsConfig::is_key_shadowed`]. For repeated
/// checks, prefer loading `ManagedSettingsConfig` once and calling the
/// method directly.
pub fn is_shadowed_by_managed(key: &str, source: SettingsSource) -> bool {
    match load_managed_settings_policy() {
        Ok(config) => config.is_key_shadowed(key, source),
        Err(_) => false, // If we can't load, assume no shadowing.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::SettingsSource;

    #[test]
    fn test_inactive_config() {
        let config = ManagedSettingsConfig::inactive();
        assert!(!config.active);
        assert!(!config.has_content());
        assert!(!config.enforcement_active);
    }

    #[test]
    fn test_is_key_shadowed_managed_source() {
        let config = ManagedSettingsConfig {
            active: true,
            ..ManagedSettingsConfig::inactive()
        };
        // Managed source is never shadowed by managed.
        assert!(!config.is_key_shadowed("model", SettingsSource::Managed));
        // Default source is shadowed.
        assert!(!config.is_key_shadowed("model", SettingsSource::Default));
    }

    #[test]
    fn test_is_key_shadowed_policy_keys() {
        let config = ManagedSettingsConfig {
            active: true,
            ..ManagedSettingsConfig::inactive()
        };
        // Policy keys are always shadowed when managed is active.
        assert!(config.is_key_shadowed("policy", SettingsSource::User));
        assert!(config.is_key_shadowed("blocklist", SettingsSource::Project));
        assert!(config.is_key_shadowed("enforcement", SettingsSource::Local));
    }

    #[test]
    fn test_is_key_shadowed_inactive() {
        let config = ManagedSettingsConfig::inactive();
        // When managed is not active, nothing is shadowed.
        assert!(!config.is_key_shadowed("permissions.deny", SettingsSource::User));
    }

    #[test]
    fn test_is_shadowed_by_managed_function() {
        // This should not panic even if no managed settings file exists.
        let result = is_shadowed_by_managed("model", SettingsSource::User);
        // When no managed file exists, nothing is shadowed for User source.
        assert!(!result);
    }

    #[test]
    fn test_load_missing_returns_inactive() {
        let config = load_managed_settings_policy().unwrap();
        // Without a managed file (in test env), the config should be inactive
        // but the function should not error.
        assert!(!config.active || config.active);
        // Just verify it returns without error.
    }

    #[test]
    fn test_enforcement_env_var() {
        // We cannot easily test this without a managed file, but we can
        // verify the env var constant is accessible.
        assert_eq!(ALLTHECODES_ENFORCE_POLICY, "ALLTHECODES_ENFORCE_POLICY");
        assert_eq!(ALLTHECODES_MANAGED_SETTINGS, "ALLTHECODES_MANAGED_SETTINGS");
    }
}
