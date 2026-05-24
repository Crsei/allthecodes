//! Runtime projection of effective settings.
//!
//! This type used to live in `types::app_state` in the root crate. It was
//! moved here in Phase 3 (issue #72) because:
//!
//! 1. Its fields already reference concrete types from [`crate::settings`]
//!    (`PermissionsSettings`, `SandboxSettings`, `StatusLineSettings`,
//!    `SpinnerTipsSettings`, `SourceMap`), so cc-config is the natural
//!    home.
//! 2. `cc-config::validation` reads `SettingsJson` directly; keeping
//!    `SettingsJson` in the root crate would force a reverse dep
//!    cc-config → allthecodes.
//!
//! The root crate keeps `types::app_state::SettingsJson` as a re-export of
//! this type so existing call sites compile unchanged.

use std::collections::HashMap;

use serde_json::Value;

use crate::settings::{
    ModelCapabilitySettings, PermissionsSettings, ProviderProfileSettings, SandboxSettings,
    SourceMap, SpinnerTipsSettings, StatusLineSettings,
};

/// Runtime projection of [`crate::settings::EffectiveSettings`] —
/// start-up merges raw settings into this, `/config set` writes back here,
/// and serialization converts it to [`crate::settings::RawSettings`].
#[derive(Debug, Clone, Default)]
pub struct SettingsJson {
    // -- Core identity --------------------------------------------------
    pub model: Option<String>,
    pub backend: Option<String>,
    pub api_provider: Option<String>,
    pub active_auth_profile: Option<String>,
    pub auth_profiles: HashMap<String, ProviderProfileSettings>,
    pub theme: Option<String>,
    pub verbose: Option<bool>,

    // -- Permissions / sandbox -----------------------------------------
    pub permission_mode: Option<String>,
    pub permissions: PermissionsSettings,
    pub sandbox: SandboxSettings,

    // -- UI / UX --------------------------------------------------------
    pub status_line: StatusLineSettings,
    pub spinner_tips: SpinnerTipsSettings,
    pub output_style: Option<String>,
    pub language: Option<String>,
    pub voice_enabled: Option<bool>,
    pub editor_mode: Option<String>,
    pub view_mode: Option<String>,
    pub terminal_progress_bar_enabled: Option<bool>,

    // -- Models / effort -----------------------------------------------
    pub thinking: Option<Value>,
    pub output_config: Option<Value>,
    pub default_model: Option<String>,
    pub fallback_model: Option<String>,
    pub fast_model: Option<String>,
    pub sota_model: Option<String>,
    pub mota_model: Option<String>,
    pub fota_model: Option<String>,
    pub available_models: Vec<String>,
    pub model_capabilities: HashMap<String, ModelCapabilitySettings>,
    pub effort_level: Option<String>,
    pub model_reasoning_effort: Option<String>,
    pub fast_mode: Option<bool>,
    pub fast_mode_per_session_opt_in: Option<bool>,
    /// Optional advisor model id (issue #33). Persisted under
    /// `settings.json::advisorModel`. When set and the active provider
    /// supports advisors, this model is attached to the Messages request
    /// via `MessagesRequest::advisor_model`.
    pub advisor_model: Option<String>,

    // -- Modes / integrations ------------------------------------------
    pub teammate_mode: Option<bool>,
    pub claude_in_chrome_default_enabled: Option<bool>,

    // -- Memory (issue #45) --------------------------------------------
    /// Whether auto-memory capture + injection is enabled for this session.
    /// Persisted via `settings.json::autoMemoryEnabled`; toggled by
    /// `/memory auto on|off`. Default is `None` (off).
    pub auto_memory_enabled: Option<bool>,

    // -- Per-key source (provenance) -----------------------------------
    /// 来源映射: key -> 哪个 layer 提供了该值。由启动路径 + `/config set`
    /// 在写入对应键时一并更新。`/config show` 读取此 map 显示来源信息。
    pub sources: SourceMap,
}
