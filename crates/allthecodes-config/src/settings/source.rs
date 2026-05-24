use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Source tracking
// ---------------------------------------------------------------------------

/// Origin of a single configuration value.
///
/// Used by [`SourceMap`] so the user can introspect where each effective
/// value came from (`/config show --effective`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SettingsSource {
    /// Compiled-in default (no file / env provided a value).
    Default,
    /// Managed / policy-level settings.
    Managed,
    /// User-level settings (`~/.allthecodes/settings.json`).
    User,
    /// Project-level settings (`.allthecodes/settings.json`).
    Project,
    /// Project-local overrides (`.allthecodes/settings.local.json`).
    Local,
    /// Environment variable override.
    Env,
    /// CLI flag override (set by `main.rs` after loading).
    Cli,
}

impl SettingsSource {
    /// Priority ranking — higher wins in a merge.
    ///
    /// Exposed so callers (e.g. `/config sources`) can break ties or sort
    /// by priority order without re-implementing the table.
    pub fn rank(self) -> u8 {
        match self {
            SettingsSource::Default => 0,
            SettingsSource::Managed => 1,
            SettingsSource::User => 2,
            SettingsSource::Project => 3,
            SettingsSource::Local => 4,
            SettingsSource::Env => 5,
            SettingsSource::Cli => 6,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            SettingsSource::Default => "default",
            SettingsSource::Managed => "managed",
            SettingsSource::User => "user",
            SettingsSource::Project => "project",
            SettingsSource::Local => "local",
            SettingsSource::Env => "env",
            SettingsSource::Cli => "cli",
        }
    }
}

/// Per-key provenance for merged settings. Uses a `BTreeMap` so the output
/// of `/config show` is deterministic.
pub type SourceMap = BTreeMap<String, SettingsSource>;
