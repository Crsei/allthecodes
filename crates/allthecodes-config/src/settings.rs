//! Settings management (layered, source-aware).
//!
//! # Layered configuration
//!
//! Configuration is merged from the following sources, in **ascending**
//! priority (higher overrides lower):
//!
//! 1. `Managed` — policy-level settings. Windows prefers
//!    `%ProgramData%\allthecodes\settings.json`; other platforms use
//!    `/etc/allthecodes/managed-settings.json`. Overridable with
//!    `ALLTHECODES_MANAGED_SETTINGS`.
//! 2. `User` — `{data_root}/settings.json` (i.e. `~/.allthecodes/settings.json`
//!    or `$ALLTHECODES_HOME/settings.json`).
//! 3. `Project` — `.allthecodes/settings.json` in CWD or any ancestor directory.
//! 4. `Local` — `.allthecodes/settings.local.json` next to the project settings
//!    (intended for gitignored per-machine overrides).
//! 5. `Env` — a handful of CLAUDE_* / CC_* environment variables.
//! 6. `Cli` — command-line flags (applied by the caller, not this module).
//!
//! The merge produces an [`EffectiveSettings`] together with a
//! [`SourceMap`] that records which layer won for each key.
//!
//! # Backward compatibility
//!
//! The legacy names [`GlobalConfig`], [`ProjectConfig`], [`MergedConfig`],
//! [`load_global_config`], [`load_project_config`], [`merge_configs`], and
//! [`load_and_merge`] are preserved so existing callers keep compiling.
//! New callers should prefer [`load_effective`], [`RawSettings`], and
//! [`EffectiveSettings`].

mod effective;
mod first_run;
mod layers;
mod load;
mod paths;
mod providers;
mod raw;
mod requirements;
mod schema;
mod source;
#[cfg(test)]
mod tests;
mod types;
mod write;

pub use effective::*;
pub use first_run::*;
pub use layers::*;
pub use load::*;
pub use paths::*;
pub use providers::*;
pub use raw::*;
pub use requirements::*;
pub use schema::*;
pub use source::*;
pub use types::*;
pub use write::*;
