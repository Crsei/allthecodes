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

use serde_json::{json, Value};

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
    pub extra: HashMap<String, Value>,

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
    pub app_icon: Option<String>,
    pub auto_start: Option<bool>,
    pub start_minimized: Option<bool>,
    pub minimize_to_tray: Option<bool>,
    pub close_to_tray: Option<bool>,
    pub quick_chat_hide_on_blur: Option<bool>,
    pub quick_chat_inject_screen: Option<bool>,
    pub quick_chat_ambient: Option<bool>,
    pub auto_approve_tools: Option<bool>,
    pub analytics_enabled: Option<bool>,

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
    pub context_window: Option<u64>,
    pub max_messages: Option<u64>,
    pub auto_title: Option<bool>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u64>,
    pub streaming: Option<bool>,
    pub show_token_usage: Option<bool>,
    pub show_reasoning_details: Option<bool>,
    pub markdown_rendering: Option<bool>,
    pub single_dollar_math: Option<bool>,
    pub infographic: Option<bool>,
    pub auto_collapse_reasoning: Option<bool>,
    pub quick_reply_suggestions: Option<bool>,
    pub default_tool_selection: Option<String>,
    pub default_skill_selection: Option<String>,
    pub sound_effects: Option<bool>,
    pub auto_compact: Option<bool>,
    pub compact_threshold: Option<u8>,
    pub keep_recent_messages: Option<u8>,
    pub hashline_mode: Option<bool>,
    /// Optional advisor model id (issue #33). Persisted under
    /// `settings.json::advisorModel`. When set and the active provider
    /// supports advisors, this model is attached to the Messages request
    /// via `MessagesRequest::advisor_model`.
    pub advisor_model: Option<String>,

    // -- Prompts --------------------------------------------------------
    pub system_prompt: Option<String>,

    // -- Modes / integrations ------------------------------------------
    pub teammate_mode: Option<bool>,
    pub claude_in_chrome_default_enabled: Option<bool>,

    // -- Memory (issue #45) --------------------------------------------
    /// Whether auto-memory capture + injection is enabled for this session.
    /// Persisted via `settings.json::autoMemoryEnabled`; toggled by
    /// `/memory auto on|off`. Default is `None` (off).
    pub auto_memory_enabled: Option<bool>,
    pub memory_auto_retrieve: Option<bool>,
    pub memory_query_rewriting: Option<bool>,
    pub memory_max_retrieved: Option<u8>,
    pub memory_similarity_threshold: Option<u8>,
    pub memory_auto_summarize: Option<bool>,
    pub memory_nightly: Option<bool>,
    pub memory_sleep_time: Option<String>,
    pub memory_temp_ttl: Option<u32>,
    pub memory_archive_retention: Option<u32>,
    pub memory_tool_model: Option<String>,
    pub memory_embedding_model: Option<String>,

    // -- Network / search / speech -------------------------------------
    pub proxy_enabled: Option<bool>,
    pub proxy_url: Option<String>,
    pub prefer_ipv4: Option<bool>,
    pub request_timeout: Option<u64>,
    pub retry_attempts: Option<u8>,
    pub custom_user_agent: Option<String>,
    pub speech_enabled: Option<bool>,
    pub speech_active_model: Option<String>,
    pub speech_language: Option<String>,
    pub tts_provider: Option<String>,
    pub tts_api_key: Option<String>,
    pub tts_voice: Option<String>,
    pub tts_voice_custom_id: Option<String>,
    pub tts_model: Option<String>,
    pub search_engine: Option<String>,
    pub web_search_provider: Option<String>,
    pub web_search_tavily_api_key: Option<String>,
    pub web_search_brave_api_key: Option<String>,

    // -- Data / savings -------------------------------------------------
    pub cloud_sync_enabled: Option<bool>,
    pub cloud_sync_path: Option<String>,
    pub token_savings_tracking: Option<bool>,

    // -- Per-key source (provenance) -----------------------------------
    /// 来源映射: key -> 哪个 layer 提供了该值。由启动路径 + `/config set`
    /// 在写入对应键时一并更新。`/config show` 读取此 map 显示来源信息。
    pub sources: SourceMap,
}

impl SettingsJson {
    /// Flatten user-facing settings into the map consumed by the web UI.
    /// Sensitive values are intentionally omitted.
    pub fn settings_map(&self) -> HashMap<String, Value> {
        let mut out = sanitized_extra_map(&self.extra);

        macro_rules! insert_opt {
            ($key:expr, $field:expr) => {
                if let Some(value) = $field {
                    out.insert($key.to_string(), json!(value));
                }
            };
        }
        macro_rules! insert_opt_ref {
            ($key:expr, $field:expr) => {
                if let Some(value) = $field.as_ref() {
                    out.insert($key.to_string(), json!(value));
                }
            };
        }

        insert_opt_ref!("model", self.model);
        insert_opt_ref!("backend", self.backend);
        insert_opt_ref!("api_provider", self.api_provider);
        insert_opt_ref!("active_auth_profile", self.active_auth_profile);
        insert_opt_ref!("theme", self.theme);
        insert_opt!("verbose", self.verbose);
        insert_opt_ref!("permission_mode", self.permission_mode);
        insert_opt_ref!("output_style", self.output_style);
        insert_opt_ref!("language", self.language);
        insert_opt!("voice_enabled", self.voice_enabled);
        insert_opt_ref!("editor_mode", self.editor_mode);
        insert_opt_ref!("view_mode", self.view_mode);
        insert_opt!(
            "terminal_progress_bar_enabled",
            self.terminal_progress_bar_enabled
        );
        insert_opt_ref!("app_icon", self.app_icon);
        insert_opt!("auto_start", self.auto_start);
        insert_opt!("start_minimized", self.start_minimized);
        insert_opt!("minimize_to_tray", self.minimize_to_tray);
        insert_opt!("close_to_tray", self.close_to_tray);
        insert_opt!("quick_chat_hide_on_blur", self.quick_chat_hide_on_blur);
        insert_opt!("quick_chat_inject_screen", self.quick_chat_inject_screen);
        insert_opt!("quick_chat_ambient", self.quick_chat_ambient);
        insert_opt!("auto_approve_tools", self.auto_approve_tools);
        insert_opt!("analytics_enabled", self.analytics_enabled);
        insert_opt_ref!("default_model", self.default_model);
        insert_opt_ref!("fallback_model", self.fallback_model);
        insert_opt_ref!("fast_model", self.fast_model);
        insert_opt_ref!("sota_model", self.sota_model);
        insert_opt_ref!("mota_model", self.mota_model);
        insert_opt_ref!("fota_model", self.fota_model);
        insert_opt_ref!("effort_level", self.effort_level);
        insert_opt_ref!("model_reasoning_effort", self.model_reasoning_effort);
        insert_opt!("fast_mode", self.fast_mode);
        insert_opt!(
            "fast_mode_per_session_opt_in",
            self.fast_mode_per_session_opt_in
        );
        insert_opt!("context_window", self.context_window);
        insert_opt!("max_messages", self.max_messages);
        insert_opt!("auto_title", self.auto_title);
        insert_opt!("temperature", self.temperature);
        insert_opt!("max_tokens", self.max_tokens);
        insert_opt!("streaming", self.streaming);
        insert_opt!("show_token_usage", self.show_token_usage);
        insert_opt!("show_reasoning_details", self.show_reasoning_details);
        insert_opt!("markdown_rendering", self.markdown_rendering);
        insert_opt!("single_dollar_math", self.single_dollar_math);
        insert_opt!("infographic", self.infographic);
        insert_opt!("auto_collapse_reasoning", self.auto_collapse_reasoning);
        insert_opt!("quick_reply_suggestions", self.quick_reply_suggestions);
        insert_opt_ref!("default_tool_selection", self.default_tool_selection);
        insert_opt_ref!("default_skill_selection", self.default_skill_selection);
        insert_opt!("sound_effects", self.sound_effects);
        insert_opt!("auto_compact", self.auto_compact);
        insert_opt!("compact_threshold", self.compact_threshold);
        insert_opt!("keep_recent_messages", self.keep_recent_messages);
        insert_opt!("hashline_mode", self.hashline_mode);
        insert_opt!("teammate_mode", self.teammate_mode);
        insert_opt!(
            "claude_in_chrome_default_enabled",
            self.claude_in_chrome_default_enabled
        );
        insert_opt!("auto_memory_enabled", self.auto_memory_enabled);
        insert_opt!("memory_auto_retrieve", self.memory_auto_retrieve);
        insert_opt!("memory_query_rewriting", self.memory_query_rewriting);
        insert_opt!("memory_max_retrieved", self.memory_max_retrieved);
        insert_opt!(
            "memory_similarity_threshold",
            self.memory_similarity_threshold
        );
        insert_opt!("memory_auto_summarize", self.memory_auto_summarize);
        insert_opt!("memory_nightly", self.memory_nightly);
        insert_opt_ref!("memory_sleep_time", self.memory_sleep_time);
        insert_opt!("memory_temp_ttl", self.memory_temp_ttl);
        insert_opt!("memory_archive_retention", self.memory_archive_retention);
        insert_opt_ref!("memory_tool_model", self.memory_tool_model);
        insert_opt_ref!("memory_embedding_model", self.memory_embedding_model);
        insert_opt!("proxy_enabled", self.proxy_enabled);
        insert_opt_ref!("proxy_url", self.proxy_url);
        insert_opt!("prefer_ipv4", self.prefer_ipv4);
        insert_opt!("request_timeout", self.request_timeout);
        insert_opt!("retry_attempts", self.retry_attempts);
        insert_opt_ref!("custom_user_agent", self.custom_user_agent);
        insert_opt!("speech_enabled", self.speech_enabled);
        insert_opt_ref!("speech_active_model", self.speech_active_model);
        insert_opt_ref!("speech_language", self.speech_language);
        insert_opt_ref!("tts_provider", self.tts_provider);
        insert_opt_ref!("tts_voice", self.tts_voice);
        insert_opt_ref!("tts_voice_custom_id", self.tts_voice_custom_id);
        insert_opt_ref!("tts_model", self.tts_model);
        insert_opt_ref!("search_engine", self.search_engine);
        insert_opt_ref!("web_search_provider", self.web_search_provider);
        insert_opt!(
            "web_search_tavily_configured",
            self.web_search_tavily_api_key
                .as_ref()
                .map(|value| !value.trim().is_empty())
        );
        insert_opt!(
            "web_search_brave_configured",
            self.web_search_brave_api_key
                .as_ref()
                .map(|value| !value.trim().is_empty())
        );
        insert_opt!("cloud_sync_enabled", self.cloud_sync_enabled);
        insert_opt_ref!("cloud_sync_path", self.cloud_sync_path);
        insert_opt!("token_savings_tracking", self.token_savings_tracking);
        insert_opt_ref!("advisor_model", self.advisor_model);
        insert_opt_ref!("system_prompt", self.system_prompt);

        out
    }
}

fn sanitized_extra_map(extra: &HashMap<String, Value>) -> HashMap<String, Value> {
    extra
        .iter()
        .filter_map(|(key, value)| {
            if is_sensitive_key(key) {
                None
            } else {
                sanitized_extra_value(value).map(|value| (key.clone(), value))
            }
        })
        .collect()
}

fn sanitized_extra_value(value: &Value) -> Option<Value> {
    match value {
        Value::Object(map) => {
            let clean: serde_json::Map<String, Value> = map
                .iter()
                .filter_map(|(key, value)| {
                    if is_sensitive_key(key) {
                        None
                    } else {
                        sanitized_extra_value(value).map(|value| (key.clone(), value))
                    }
                })
                .collect();
            Some(Value::Object(clean))
        }
        Value::Array(items) => Some(Value::Array(
            items.iter().filter_map(sanitized_extra_value).collect(),
        )),
        other => Some(other.clone()),
    }
}

fn is_sensitive_key(key: &str) -> bool {
    let normalized = key
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    normalized.contains("apikey")
        || normalized.contains("token")
        || normalized.contains("secret")
        || normalized.contains("password")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_map_includes_phase1_fields_and_redacts_sensitive_values() {
        let settings = SettingsJson {
            language: Some("zh-CN".to_string()),
            proxy_enabled: Some(true),
            show_reasoning_details: Some(true),
            tts_api_key: Some("sk-secret".to_string()),
            web_search_provider: Some("tavily".to_string()),
            web_search_tavily_api_key: Some("tvly-secret".to_string()),
            web_search_brave_api_key: Some("brave-secret".to_string()),
            extra: HashMap::from([
                ("appMode".to_string(), json!("desktop")),
                ("apiToken".to_string(), json!("secret")),
                (
                    "nested".to_string(),
                    json!({
                        "safe": true,
                        "password": "hidden"
                    }),
                ),
            ]),
            ..Default::default()
        };

        let map = settings.settings_map();
        assert_eq!(map.get("language"), Some(&json!("zh-CN")));
        assert_eq!(map.get("proxy_enabled"), Some(&json!(true)));
        assert_eq!(map.get("show_reasoning_details"), Some(&json!(true)));
        assert_eq!(map.get("web_search_provider"), Some(&json!("tavily")));
        assert_eq!(map.get("web_search_tavily_configured"), Some(&json!(true)));
        assert_eq!(map.get("web_search_brave_configured"), Some(&json!(true)));
        assert!(!map.contains_key("tts_api_key"));
        assert!(!map.contains_key("web_search_tavily_api_key"));
        assert!(!map.contains_key("web_search_brave_api_key"));
        assert!(!map.contains_key("apiToken"));
        assert_eq!(map.get("appMode"), Some(&json!("desktop")));
        assert_eq!(map.get("nested"), Some(&json!({ "safe": true })));
    }
}
