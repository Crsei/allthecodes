use std::collections::BTreeMap;

use allthecodes_types::kairos::KairosValueSource;
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
    /// Named profile from user-level `configProfiles`.
    #[serde(rename = "user_profile")]
    UserProfile,
    /// Project-level settings (`.allthecodes/settings.json`).
    Project,
    /// Project-local overrides (`.allthecodes/settings.local.json`).
    Local,
    /// Environment variable override.
    Env,
    /// CLI flag override (set by `main.rs` after loading).
    Cli,
    /// Runtime override applied after CLI flags.
    Runtime,
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
            SettingsSource::UserProfile => 3,
            SettingsSource::Project => 4,
            SettingsSource::Local => 5,
            SettingsSource::Env => 6,
            SettingsSource::Cli => 7,
            SettingsSource::Runtime => 8,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            SettingsSource::Default => "default",
            SettingsSource::Managed => "managed",
            SettingsSource::User => "user",
            SettingsSource::UserProfile => "user_profile",
            SettingsSource::Project => "project",
            SettingsSource::Local => "local",
            SettingsSource::Env => "env",
            SettingsSource::Cli => "cli",
            SettingsSource::Runtime => "runtime",
        }
    }
}

impl From<SettingsSource> for KairosValueSource {
    fn from(source: SettingsSource) -> Self {
        match source {
            SettingsSource::Default => Self::Default,
            SettingsSource::Managed => Self::Managed,
            SettingsSource::User => Self::User,
            SettingsSource::UserProfile => Self::UserProfile,
            SettingsSource::Project => Self::Project,
            SettingsSource::Local => Self::Local,
            SettingsSource::Env => Self::Environment,
            SettingsSource::Cli => Self::Cli,
            SettingsSource::Runtime => Self::Runtime,
        }
    }
}

/// Per-key provenance for merged settings. Uses a `BTreeMap` so the output
/// of `/config show` is deterministic.
pub type SourceMap = BTreeMap<String, SettingsSource>;
