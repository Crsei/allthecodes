use std::collections::HashMap;

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::effective::EffectiveSettings;
use super::providers::{merge_provider_profile, ProviderProfileSettings};
use super::source::{SettingsSource, SourceMap};
use super::types::{
    AutoModeSettings, PermissionsSettings, SandboxFilesystemSettings, SandboxNetworkSettings,
    SandboxSettings, SpinnerTipsSettings, StatusLineSettings,
};

// ---------------------------------------------------------------------------
// RawSettings — on-disk shape of a single settings file
// ---------------------------------------------------------------------------

/// On-disk shape of a single `settings.json` (or `settings.local.json`,
/// managed settings, etc.). All fields are optional.
///
/// Unknown keys fall into [`RawSettings::extra`] to preserve forward
/// compatibility.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct RawSettings {
    // -- Core identity --------------------------------------------------
    pub model: Option<String>,
    pub backend: Option<String>,
    pub api_provider: Option<String>,
    pub active_auth_profile: Option<String>,
    pub auth_profiles: Option<HashMap<String, ProviderProfileSettings>>,
    pub config_profiles: Option<HashMap<String, RawSettings>>,
    pub theme: Option<String>,
    pub verbose: Option<bool>,

    // -- Permissions / sandbox -----------------------------------------
    /// Legacy top-level permission mode (e.g. "auto"). Prefer
    /// `permissions.defaultMode`. If both are present, nested wins.
    pub permission_mode: Option<String>,
    /// Legacy flat allowed-tools list. Prefer `permissions.allow`.
    pub allowed_tools: Option<Vec<String>>,
    pub permissions: Option<PermissionsSettings>,
    pub sandbox: Option<SandboxSettings>,

    // -- Hooks ----------------------------------------------------------
    /// Event → config value mapping (deserialized by tools/hooks).
    pub hooks: Option<HashMap<String, Value>>,

    // -- UI / UX --------------------------------------------------------
    pub status_line: Option<StatusLineSettings>,
    pub output_style: Option<String>,
    pub language: Option<String>,
    pub voice_enabled: Option<bool>,
    pub editor_mode: Option<String>,
    pub view_mode: Option<String>,
    pub spinner_tips: Option<SpinnerTipsSettings>,
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

    // -- Model / effort -------------------------------------------------
    /// Anthropic thinking toggle. Accepts request-shaped values such as
    /// `{ "type": "enabled" }` or `{ "type": "disabled" }`.
    pub thinking: Option<Value>,
    /// Anthropic `output_config`. `output_config.effort` is the Claude-side
    /// reasoning effort used by the Rust TUI effort picker and request builder;
    /// runtime request building maps aliases to the API-supported high/max set.
    #[serde(rename = "output_config", alias = "outputConfig")]
    pub output_config: Option<Value>,
    /// Default model used when neither CLI nor `model` selects one.
    pub default_model: Option<String>,
    /// Model used for recoverable model-call fallback retries.
    pub fallback_model: Option<String>,
    /// Model selected by `/fast` when the current model is not fast-compatible.
    pub fast_model: Option<String>,
    /// Model ID used when resolving the neutral `SOTA` alias.
    pub sota_model: Option<String>,
    /// Model ID used when resolving the neutral `MOTA` alias.
    pub mota_model: Option<String>,
    /// Model ID used when resolving the neutral `FOTA` alias.
    pub fota_model: Option<String>,
    pub available_models: Option<Vec<String>>,
    pub effort_level: Option<String>,
    /// Codex/OpenAI Responses reasoning effort. Serialized with the Codex CLI
    /// key name so users can reuse `model_reasoning_effort = "high"` muscle
    /// memory in allthecodes settings JSON.
    #[serde(rename = "model_reasoning_effort", alias = "modelReasoningEffort")]
    pub model_reasoning_effort: Option<String>,
    pub fast_mode: Option<bool>,
    pub fast_mode_per_session_opt_in: Option<bool>,
    /// Stronger secondary model used as an advisor (issue #33).
    /// Persisted under `advisorModel`. Only honored by providers that
    /// advertise advisor support; others log a warning and ignore it.
    pub advisor_model: Option<String>,
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

    // -- Modes / integrations ------------------------------------------
    pub teammate_mode: Option<bool>,
    #[serde(rename = "claudeInChromeDefaultEnabled")]
    pub claude_in_chrome_default_enabled: Option<bool>,

    // -- Memory (issue #45) --------------------------------------------
    /// Auto-memory toggle: when `true`, memories captured during a session
    /// are surfaced by `/memory` and injected into the prompt via
    /// `build_memory_context_with`. Default is `None` (off). The capture
    /// hook itself is not yet wired up — only the state is persisted.
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

    // -- Prompts --------------------------------------------------------
    pub system_prompt: Option<String>,

    // -- Credentials ----------------------------------------------------
    /// API key override. User-level only; strongly discouraged. Redacted in
    /// source-map output.
    pub api_key: Option<String>,

    // -- Runtime environment -------------------------------------------
    /// Environment variables to seed into the process during startup.
    ///
    /// Values are strings only. The startup bridge applies these without
    /// overwriting variables already provided by the shell, `.env`, or CI.
    pub env: Option<HashMap<String, String>>,

    // -- Arbitrary passthrough ------------------------------------------
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

impl RawSettings {
    /// Store an arbitrary extension value under `extra` using a dotted path.
    ///
    /// Known settings should prefer typed fields. This helper preserves
    /// forward compatibility for web UI settings that are not yet modeled.
    pub fn set_extra_path(&mut self, path: &str, value: Value) -> Result<()> {
        let parts = parse_extra_path(path)?;
        insert_extra_path(&mut self.extra, &parts, value);
        Ok(())
    }

    /// Merge `other` **on top of** `self`. Mutates `self` in place and
    /// records, in `sources`, every key that `other` provided.
    pub(crate) fn merge_from(
        &mut self,
        other: RawSettings,
        source: SettingsSource,
        sources: &mut SourceMap,
    ) {
        macro_rules! merge_opt {
            ($field:ident, $key:expr) => {
                if let Some(v) = other.$field {
                    self.$field = Some(v);
                    sources.insert($key.to_string(), source);
                }
            };
        }

        merge_opt!(model, "model");
        merge_opt!(backend, "backend");
        merge_opt!(api_provider, "apiProvider");
        merge_opt!(active_auth_profile, "activeAuthProfile");
        merge_opt!(theme, "theme");
        merge_opt!(verbose, "verbose");
        merge_opt!(permission_mode, "permissionMode");

        if let Some(profiles) = other.auth_profiles {
            let mut merged = self.auth_profiles.take().unwrap_or_default();
            for (name, profile) in profiles {
                if profile.is_effectively_empty() {
                    continue;
                }
                merged
                    .entry(name)
                    .and_modify(|existing| merge_provider_profile(existing, profile.clone()))
                    .or_insert(profile);
            }
            self.auth_profiles = Some(merged);
            sources.insert("authProfiles".to_string(), source);
        }

        if let Some(profiles) = other.config_profiles {
            let mut merged = self.config_profiles.take().unwrap_or_default();
            for (name, profile) in profiles {
                merged.insert(name, profile);
            }
            self.config_profiles = Some(merged);
            sources.insert("configProfiles".to_string(), source);
        }

        if let Some(list) = other.allowed_tools {
            let merged = merge_str_lists(self.allowed_tools.as_deref(), Some(&list));
            self.allowed_tools = Some(merged);
            sources.insert("allowedTools".to_string(), source);
        }

        if let Some(mut perms) = other.permissions {
            if source == SettingsSource::Project {
                perms.skip_dangerous_mode_permission_prompt = None;
            }
            if !perms.is_effectively_empty() {
                self.permissions = Some(merge_permissions(self.permissions.take(), perms));
                sources.insert("permissions".to_string(), source);
            }
        }

        if let Some(sbx) = other.sandbox {
            if !sbx.is_effectively_empty() {
                self.sandbox = Some(merge_sandbox(self.sandbox.take(), sbx));
                sources.insert("sandbox".to_string(), source);
            }
        }

        if let Some(hooks) = other.hooks {
            let mut merged = self.hooks.take().unwrap_or_default();
            for (k, v) in hooks {
                merged.insert(k, v);
            }
            self.hooks = Some(merged);
            sources.insert("hooks".to_string(), source);
        }

        merge_opt!(status_line, "statusLine");
        merge_opt!(output_style, "outputStyle");
        merge_opt!(language, "language");
        merge_opt!(voice_enabled, "voiceEnabled");
        merge_opt!(editor_mode, "editorMode");
        merge_opt!(view_mode, "viewMode");
        merge_opt!(spinner_tips, "spinnerTips");
        merge_opt!(terminal_progress_bar_enabled, "terminalProgressBarEnabled");
        merge_opt!(app_icon, "appIcon");
        merge_opt!(auto_start, "autoStart");
        merge_opt!(start_minimized, "startMinimized");
        merge_opt!(minimize_to_tray, "minimizeToTray");
        merge_opt!(close_to_tray, "closeToTray");
        merge_opt!(quick_chat_hide_on_blur, "quickChatHideOnBlur");
        merge_opt!(quick_chat_inject_screen, "quickChatInjectScreen");
        merge_opt!(quick_chat_ambient, "quickChatAmbient");
        merge_opt!(auto_approve_tools, "autoApproveTools");
        merge_opt!(analytics_enabled, "analyticsEnabled");
        merge_opt!(thinking, "thinking");
        merge_opt!(output_config, "output_config");
        merge_opt!(default_model, "defaultModel");
        merge_opt!(fallback_model, "fallbackModel");
        merge_opt!(fast_model, "fastModel");
        merge_opt!(sota_model, "sotaModel");
        merge_opt!(mota_model, "motaModel");
        merge_opt!(fota_model, "fotaModel");
        merge_opt!(available_models, "availableModels");
        merge_opt!(effort_level, "effortLevel");
        merge_opt!(model_reasoning_effort, "model_reasoning_effort");
        merge_opt!(fast_mode, "fastMode");
        merge_opt!(fast_mode_per_session_opt_in, "fastModePerSessionOptIn");
        merge_opt!(context_window, "contextWindow");
        merge_opt!(max_messages, "maxMessages");
        merge_opt!(auto_title, "autoTitle");
        merge_opt!(temperature, "temperature");
        merge_opt!(max_tokens, "maxTokens");
        merge_opt!(streaming, "streaming");
        merge_opt!(show_token_usage, "showTokenUsage");
        merge_opt!(show_reasoning_details, "showReasoningDetails");
        merge_opt!(markdown_rendering, "markdownRendering");
        merge_opt!(single_dollar_math, "singleDollarMath");
        merge_opt!(infographic, "infographic");
        merge_opt!(auto_collapse_reasoning, "autoCollapseReasoning");
        merge_opt!(quick_reply_suggestions, "quickReplySuggestions");
        merge_opt!(default_tool_selection, "defaultToolSelection");
        merge_opt!(default_skill_selection, "defaultSkillSelection");
        merge_opt!(sound_effects, "soundEffects");
        merge_opt!(auto_compact, "autoCompact");
        merge_opt!(compact_threshold, "compactThreshold");
        merge_opt!(keep_recent_messages, "keepRecentMessages");
        merge_opt!(hashline_mode, "hashlineMode");
        merge_opt!(teammate_mode, "teammateMode");
        merge_opt!(
            claude_in_chrome_default_enabled,
            "claudeInChromeDefaultEnabled"
        );
        merge_opt!(auto_memory_enabled, "autoMemoryEnabled");
        merge_opt!(memory_auto_retrieve, "memoryAutoRetrieve");
        merge_opt!(memory_query_rewriting, "memoryQueryRewriting");
        merge_opt!(memory_max_retrieved, "memoryMaxRetrieved");
        merge_opt!(memory_similarity_threshold, "memorySimilarityThreshold");
        merge_opt!(memory_auto_summarize, "memoryAutoSummarize");
        merge_opt!(memory_nightly, "memoryNightly");
        merge_opt!(memory_sleep_time, "memorySleepTime");
        merge_opt!(memory_temp_ttl, "memoryTempTtl");
        merge_opt!(memory_archive_retention, "memoryArchiveRetention");
        merge_opt!(memory_tool_model, "memoryToolModel");
        merge_opt!(memory_embedding_model, "memoryEmbeddingModel");
        merge_opt!(proxy_enabled, "proxyEnabled");
        merge_opt!(proxy_url, "proxyUrl");
        merge_opt!(prefer_ipv4, "preferIpv4");
        merge_opt!(request_timeout, "requestTimeout");
        merge_opt!(retry_attempts, "retryAttempts");
        merge_opt!(custom_user_agent, "customUserAgent");
        merge_opt!(speech_enabled, "speechEnabled");
        merge_opt!(speech_active_model, "speechActiveModel");
        merge_opt!(speech_language, "speechLanguage");
        merge_opt!(tts_provider, "ttsProvider");
        merge_opt!(tts_api_key, "ttsApiKey");
        merge_opt!(tts_voice, "ttsVoice");
        merge_opt!(tts_voice_custom_id, "ttsVoiceCustomId");
        merge_opt!(tts_model, "ttsModel");
        merge_opt!(search_engine, "searchEngine");
        merge_opt!(web_search_provider, "webSearchProvider");
        merge_opt!(web_search_tavily_api_key, "webSearchTavilyApiKey");
        merge_opt!(web_search_brave_api_key, "webSearchBraveApiKey");
        merge_opt!(cloud_sync_enabled, "cloudSyncEnabled");
        merge_opt!(cloud_sync_path, "cloudSyncPath");
        merge_opt!(token_savings_tracking, "tokenSavingsTracking");
        merge_opt!(advisor_model, "advisorModel");
        merge_opt!(system_prompt, "systemPrompt");
        merge_opt!(api_key, "apiKey");

        if let Some(env) = other.env {
            let mut merged = self.env.take().unwrap_or_default();
            for (k, v) in env {
                merged.insert(k, v);
            }
            self.env = Some(merged);
            sources.insert("env".to_string(), source);
        }

        for (k, v) in other.extra {
            merge_json_value(self.extra.entry(k.clone()).or_insert(Value::Null), v);
            sources.insert(k, source);
        }
    }
}

pub(crate) fn merge_json_value(base: &mut Value, over: Value) {
    match (base, over) {
        (Value::Object(base), Value::Object(over)) => {
            for (key, value) in over {
                merge_json_value(base.entry(key).or_insert(Value::Null), value);
            }
        }
        (slot, value) => *slot = value,
    }
}

fn parse_extra_path(path: &str) -> Result<Vec<&str>> {
    let parts: Vec<&str> = path.split('.').collect();
    if parts.is_empty() {
        bail!("settings path cannot be empty");
    }
    for part in &parts {
        if part.is_empty() {
            bail!("settings path cannot contain empty segments");
        }
        if !part
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
        {
            bail!("settings path contains unsupported characters");
        }
    }
    Ok(parts)
}

fn insert_extra_path(extra: &mut HashMap<String, Value>, parts: &[&str], value: Value) {
    if parts.len() == 1 {
        extra.insert(parts[0].to_string(), value);
        return;
    }

    let entry = extra
        .entry(parts[0].to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    insert_json_path(entry, &parts[1..], value);
}

fn insert_json_path(current: &mut Value, parts: &[&str], value: Value) {
    if parts.len() == 1 {
        if !current.is_object() {
            *current = Value::Object(Map::new());
        }
        if let Some(map) = current.as_object_mut() {
            map.insert(parts[0].to_string(), value);
        }
        return;
    }

    if !current.is_object() {
        *current = Value::Object(Map::new());
    }
    if let Some(map) = current.as_object_mut() {
        let entry = map
            .entry(parts[0].to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        insert_json_path(entry, &parts[1..], value);
    }
}

pub(crate) fn merge_permissions(
    base: Option<PermissionsSettings>,
    over: PermissionsSettings,
) -> PermissionsSettings {
    let mut out = base.unwrap_or_default();
    if over.default_mode.is_some() {
        out.default_mode = over.default_mode;
    }
    out.allow = merge_str_lists(Some(&out.allow), Some(&over.allow));
    out.ask = merge_str_lists(Some(&out.ask), Some(&over.ask));
    out.deny = merge_str_lists(Some(&out.deny), Some(&over.deny));
    out.additional_directories = merge_str_lists(
        Some(&out.additional_directories),
        Some(&over.additional_directories),
    );
    if over.enable_bypass_mode.is_some() {
        out.enable_bypass_mode = over.enable_bypass_mode;
    }
    if let Some(skip_prompt) = over.skip_dangerous_mode_permission_prompt {
        out.skip_dangerous_mode_permission_prompt =
            Some(out.skip_dangerous_mode_permission_prompt.unwrap_or(false) || skip_prompt);
    }
    if over.enable_auto_mode.is_some() {
        out.enable_auto_mode = over.enable_auto_mode;
    }
    if let Some(auto_mode) = over.auto_mode {
        out.auto_mode = Some(merge_auto_mode(out.auto_mode.take(), auto_mode));
    }
    for (k, v) in over.extra {
        merge_json_value(out.extra.entry(k).or_insert(Value::Null), v);
    }
    out
}

fn merge_auto_mode(base: Option<AutoModeSettings>, over: AutoModeSettings) -> AutoModeSettings {
    let mut out = base.unwrap_or_default();
    out.environment = merge_str_lists(Some(&out.environment), Some(&over.environment));
    out.allow = merge_str_lists(Some(&out.allow), Some(&over.allow));
    out.soft_deny = merge_str_lists(Some(&out.soft_deny), Some(&over.soft_deny));
    for (k, v) in over.extra {
        merge_json_value(out.extra.entry(k).or_insert(Value::Null), v);
    }
    out
}

/// Merge sandbox settings by overlaying scalar fields and concatenating
/// (deduped) list fields. This matches the Claude Code spec where
/// `allowWrite` / `denyWrite` / `allowRead` / `denyRead` / `allowedDomains`
/// / `excludedCommands` are merged across scopes rather than replaced.
fn merge_sandbox(base: Option<SandboxSettings>, over: SandboxSettings) -> SandboxSettings {
    let mut out = base.unwrap_or_default();
    if over.enabled.is_some() {
        out.enabled = over.enabled;
    }
    if over.mode.is_some() {
        out.mode = over.mode;
    }
    if over.fail_if_unavailable.is_some() {
        out.fail_if_unavailable = over.fail_if_unavailable;
    }
    if over.allow_unsandboxed_commands.is_some() {
        out.allow_unsandboxed_commands = over.allow_unsandboxed_commands;
    }
    if over.allow_managed_read_paths_only.is_some() {
        out.allow_managed_read_paths_only = over.allow_managed_read_paths_only;
    }
    if over.allow_managed_domains_only.is_some() {
        out.allow_managed_domains_only = over.allow_managed_domains_only;
    }
    out.excluded_commands =
        merge_str_lists(Some(&out.excluded_commands), Some(&over.excluded_commands));
    out.allowed_commands =
        merge_str_lists(Some(&out.allowed_commands), Some(&over.allowed_commands));
    out.filesystem = merge_sandbox_fs(out.filesystem, over.filesystem);
    out.network = merge_sandbox_net(out.network, over.network);
    for (k, v) in over.extra {
        merge_json_value(out.extra.entry(k).or_insert(Value::Null), v);
    }
    out
}

fn merge_sandbox_fs(
    base: SandboxFilesystemSettings,
    over: SandboxFilesystemSettings,
) -> SandboxFilesystemSettings {
    let mut out = base;
    out.allow_read = merge_str_lists(Some(&out.allow_read), Some(&over.allow_read));
    out.deny_read = merge_str_lists(Some(&out.deny_read), Some(&over.deny_read));
    out.allow_write = merge_str_lists(Some(&out.allow_write), Some(&over.allow_write));
    out.deny_write = merge_str_lists(Some(&out.deny_write), Some(&over.deny_write));
    for (k, v) in over.extra {
        merge_json_value(out.extra.entry(k).or_insert(Value::Null), v);
    }
    out
}

fn merge_sandbox_net(
    base: SandboxNetworkSettings,
    over: SandboxNetworkSettings,
) -> SandboxNetworkSettings {
    let mut out = base;
    if over.disabled.is_some() {
        out.disabled = over.disabled;
    }
    out.allowed_domains = merge_str_lists(Some(&out.allowed_domains), Some(&over.allowed_domains));
    if over.http_proxy_port.is_some() {
        out.http_proxy_port = over.http_proxy_port;
    }
    if over.socks_proxy_port.is_some() {
        out.socks_proxy_port = over.socks_proxy_port;
    }
    for (k, v) in over.extra {
        merge_json_value(out.extra.entry(k).or_insert(Value::Null), v);
    }
    out
}

pub(crate) fn merge_str_lists(base: Option<&[String]>, over: Option<&[String]>) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(b) = base {
        out.extend_from_slice(b);
    }
    if let Some(o) = over {
        for item in o {
            if !out.contains(item) {
                out.push(item.clone());
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Backward-compat type aliases
// ---------------------------------------------------------------------------

/// Legacy alias — global/user settings file shape.
///
/// Prefer [`RawSettings`] in new code. Kept so that historic call sites
/// (`use settings::GlobalConfig;`) continue to compile after the refactor.
pub type GlobalConfig = RawSettings;

/// Legacy alias — project settings file shape.
///
/// Prefer [`RawSettings`] in new code. Kept for the same reason as
/// [`GlobalConfig`].
pub type ProjectConfig = RawSettings;

/// Merged runtime configuration. See [`EffectiveSettings`] for the new,
/// source-aware form.
pub type MergedConfig = EffectiveSettings;
