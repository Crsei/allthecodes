use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

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
        merge_opt!(teammate_mode, "teammateMode");
        merge_opt!(
            claude_in_chrome_default_enabled,
            "claudeInChromeDefaultEnabled"
        );
        merge_opt!(auto_memory_enabled, "autoMemoryEnabled");
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
            self.extra.insert(k.clone(), v);
            sources.insert(k, source);
        }
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
        out.extra.insert(k, v);
    }
    out
}

fn merge_auto_mode(base: Option<AutoModeSettings>, over: AutoModeSettings) -> AutoModeSettings {
    let mut out = base.unwrap_or_default();
    out.environment = merge_str_lists(Some(&out.environment), Some(&over.environment));
    out.allow = merge_str_lists(Some(&out.allow), Some(&over.allow));
    out.soft_deny = merge_str_lists(Some(&out.soft_deny), Some(&over.soft_deny));
    for (k, v) in over.extra {
        out.extra.insert(k, v);
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
        out.extra.insert(k, v);
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
        out.extra.insert(k, v);
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
        out.extra.insert(k, v);
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
