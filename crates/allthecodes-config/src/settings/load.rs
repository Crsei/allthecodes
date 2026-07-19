use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::Value;

use super::effective::{EffectiveSettings, LoadedSettings};
use super::layers::{
    attach_effective_values, flatten_layer_entries, ConfigLayer, ConfigLayerDisabledReason,
    ConfigLayerEntry, ConfigLayerStatus,
};
use super::paths::{
    find_local_config, find_project_config, managed_settings_path, user_settings_path,
};
use super::providers::{normalize_api_provider, provider_env_key, API_PROVIDER_OPENAI_CODEX};
use super::raw::{GlobalConfig, MergedConfig, ProjectConfig, RawSettings};
use super::requirements::{evaluate_requirements, load_requirements};
use super::source::{SettingsSource, SourceMap};

// ---------------------------------------------------------------------------
// Loaders
// ---------------------------------------------------------------------------

fn load_raw_from(path: &Path) -> Result<Option<RawSettings>> {
    if !path.exists() {
        return Ok(None);
    }
    let contents = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    let raw: RawSettings = serde_json::from_str(&contents)
        .with_context(|| format!("Failed to parse {}", path.display()))?;
    Ok(Some(raw))
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ProjectTrustPolicy {
    #[default]
    TrustAll,
    TrustConfiguredOnly,
}

#[derive(Debug, Clone, Default)]
pub struct LoadOptions {
    pub profile: Option<String>,
    pub cli_overrides: Option<RawSettings>,
    pub runtime_overrides: Option<RawSettings>,
    pub trust_policy: ProjectTrustPolicy,
    pub enforce_requirements: bool,
    #[doc(hidden)]
    pub user_settings_override: Option<RawSettings>,
}

/// Load the user-level settings. Returns `Ok(RawSettings::default())` if
/// the file does not exist.
///
/// Convenience wrapper kept for callers that only want one layer; the full
/// stack is loaded via [`load_effective`].
pub fn load_global_config() -> Result<RawSettings> {
    Ok(load_raw_from(&user_settings_path())?.unwrap_or_default())
}

/// Load the project-level settings. Returns defaults if none is found.
pub fn load_project_config(cwd: &Path) -> Result<RawSettings> {
    match find_project_config(cwd) {
        Some(p) => Ok(load_raw_from(&p)?.unwrap_or_default()),
        None => Ok(RawSettings::default()),
    }
}

/// Load project-local overrides (`.allthecodes/settings.local.json`).
pub fn load_local_config(cwd: &Path) -> Result<RawSettings> {
    match find_local_config(cwd) {
        Some(p) => Ok(load_raw_from(&p)?.unwrap_or_default()),
        None => Ok(RawSettings::default()),
    }
}

/// Load managed / policy settings, if a managed settings file exists on
/// disk. Errors reading an existing file are surfaced; a missing file is
/// treated as "no managed layer".
pub fn load_managed_config() -> Result<RawSettings> {
    Ok(load_raw_from(&managed_settings_path())?.unwrap_or_default())
}

// ---------------------------------------------------------------------------
// Merge / env overrides
// ---------------------------------------------------------------------------

/// Merge exactly two layers (global then project). Preserved for
/// backward compatibility with earlier call sites.
pub fn merge_configs(global: &GlobalConfig, project: &ProjectConfig) -> MergedConfig {
    let mut acc = RawSettings::default();
    let mut sources = SourceMap::new();
    acc.merge_from(global.clone(), SettingsSource::User, &mut sources);
    acc.merge_from(project.clone(), SettingsSource::Project, &mut sources);
    let mut merged = EffectiveSettings::from_raw(acc);
    apply_active_auth_profile(&mut merged, &mut sources);
    apply_env_overrides(&mut merged, &mut sources);
    merged
}

pub(crate) fn apply_active_auth_profile(merged: &mut EffectiveSettings, sources: &mut SourceMap) {
    let Some(active) = merged
        .active_auth_profile
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
    else {
        return;
    };
    let Some(profile) = merged.auth_profiles.get(&active).cloned() else {
        return;
    };

    let profile_key = |field: &str| format!("authProfiles.{active}.{field}");
    let source = sources
        .get("authProfiles")
        .copied()
        .unwrap_or(SettingsSource::User);
    let normalized_profile_provider = profile
        .api_provider
        .as_deref()
        .and_then(normalize_api_provider);
    let is_codex = profile
        .backend
        .as_deref()
        .is_some_and(|backend| backend.eq_ignore_ascii_case("codex"))
        || profile.api_provider.as_deref().is_some_and(|provider| {
            normalize_api_provider(provider) == Some(API_PROVIDER_OPENAI_CODEX)
        })
        || active.eq_ignore_ascii_case("codex");

    if let Some(backend) = profile
        .backend
        .filter(|_| should_profile_override(sources, "backend", source))
    {
        merged.backend = Some(backend);
        sources.insert("backend".to_string(), source);
        sources.insert(profile_key("backend"), source);
    }
    if let Some(api_provider) = profile
        .api_provider
        .filter(|_| should_profile_override(sources, "apiProvider", source))
    {
        merged.api_provider = Some(api_provider);
        sources.insert("apiProvider".to_string(), source);
        sources.insert(profile_key("apiProvider"), source);
    }
    if let Some(model) = profile
        .model
        .filter(|_| should_profile_override(sources, "model", source))
    {
        merged.model = Some(model);
        sources.insert("model".to_string(), source);
        sources.insert(profile_key("model"), source);
    }
    if let Some(models) = profile.available_models {
        merged.available_models = models;
        sources.insert("availableModels".to_string(), source);
        sources.insert(profile_key("availableModels"), source);
    }
    if let Some(capabilities) = profile.model_capabilities {
        merged.model_capabilities = capabilities;
        sources.insert("modelCapabilities".to_string(), source);
        sources.insert(profile_key("modelCapabilities"), source);
    }
    if let Some(effort) = profile.model_reasoning_effort {
        merged.model_reasoning_effort = Some(effort);
        sources.insert("model_reasoning_effort".to_string(), source);
        sources.insert(profile_key("modelReasoningEffort"), source);
    }
    if let Some(value) = profile.request_max_retries {
        merged.request_max_retries = Some(value);
        sources.insert(profile_key("requestMaxRetries"), source);
    }
    if let Some(value) = profile.stream_max_retries {
        merged.stream_max_retries = Some(value);
        sources.insert(profile_key("streamMaxRetries"), source);
    }
    if let Some(value) = profile.stream_idle_timeout_ms {
        merged.stream_idle_timeout_ms = Some(value);
        sources.insert(profile_key("streamIdleTimeoutMs"), source);
    }
    if let Some(value) = profile.request_timeout_ms {
        merged.request_timeout_ms = Some(value);
        sources.insert(profile_key("requestTimeoutMs"), source);
    }
    if let Some(api_key) = profile
        .api_key
        .filter(|_| should_profile_override(sources, "apiKey", source))
    {
        merged.api_key = Some(api_key.clone());
        if let Some(env_key) = normalized_profile_provider.and_then(provider_env_key) {
            merged.env.insert(env_key.to_string(), api_key);
        }
        sources.insert("apiKey".to_string(), source);
        sources.insert(profile_key("apiKey"), source);
        sources.insert("env".to_string(), source);
    }
    if let Some(base_url) = profile
        .base_url
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
    {
        let env_key = match normalized_profile_provider {
            Some(API_PROVIDER_OPENAI_CODEX) => Some("OPENAI_CODEX_BASE_URL"),
            Some("anthropic") => Some("ANTHROPIC_BASE_URL"),
            Some("azure") => Some("AZURE_BASE_URL"),
            _ => None,
        };
        if let Some(env_key) = env_key {
            merged.env.insert(env_key.to_string(), base_url);
        }
        sources.insert("env".to_string(), source);
        sources.insert(profile_key("baseUrl"), source);
    }
    if is_codex {
        if let Some(model) = merged
            .model
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            merged
                .env
                .insert("OPENAI_CODEX_MODEL".to_string(), model.to_string());
            sources.insert("env".to_string(), source);
        }
    } else if normalized_profile_provider == Some("anthropic") {
        if let Some(model) = merged
            .model
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            merged
                .env
                .insert("ANTHROPIC_MODEL".to_string(), model.to_string());
            sources.insert("env".to_string(), source);
        }
    }
    if let Some(env) = profile.env {
        for (key, value) in env {
            merged.env.insert(key, value);
        }
        sources.insert("env".to_string(), source);
        sources.insert(profile_key("env"), source);
    }
}

fn should_profile_override(sources: &SourceMap, key: &str, source: SettingsSource) -> bool {
    sources
        .get(key)
        .copied()
        .map(|existing| existing.rank() <= source.rank())
        .unwrap_or(true)
}

/// Apply environment-variable overrides in place.
fn apply_env_overrides(merged: &mut EffectiveSettings, sources: &mut SourceMap) {
    let set_src = |key: &str, sources: &mut SourceMap| {
        sources.insert(key.to_string(), SettingsSource::Env);
    };

    if let Ok(model) = std::env::var("CLAUDE_MODEL") {
        merged.model = Some(model);
        set_src("model", sources);
    }
    if let Ok(backend) = std::env::var("CC_BACKEND").or_else(|_| std::env::var("CLAUDE_BACKEND")) {
        merged.backend = Some(backend);
        set_src("backend", sources);
    }
    if let Ok(provider) = std::env::var("CC_API_PROVIDER") {
        merged.api_provider = Some(provider);
        set_src("apiProvider", sources);
    }
    if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
        merged.api_key = Some(key);
        set_src("apiKey", sources);
    }
    if let Ok(v) = std::env::var("CLAUDE_VERBOSE") {
        merged.verbose = v == "1" || v.eq_ignore_ascii_case("true");
        set_src("verbose", sources);
    }
    if let Ok(mode) = std::env::var("CLAUDE_PERMISSION_MODE") {
        merged.permission_mode = Some(mode.clone());
        merged.permissions.default_mode = Some(mode);
        set_src("permissionMode", sources);
    }
    if let Ok(lang) = std::env::var("CLAUDE_LANGUAGE") {
        merged.language = Some(lang);
        set_src("language", sources);
    }
    if let Ok(style) = std::env::var("CLAUDE_OUTPUT_STYLE") {
        merged.output_style = Some(style);
        set_src("outputStyle", sources);
    }
    if let Ok(theme) = std::env::var("CLAUDE_THEME") {
        merged.theme = Some(theme);
        set_src("theme", sources);
    }
    apply_stream_idle_env_override(merged, sources);
}

fn apply_stream_idle_env_override(merged: &mut EffectiveSettings, sources: &mut SourceMap) {
    let stream_idle_override = std::env::var("ALLTHECODES_STREAM_IDLE_TIMEOUT_MS")
        .ok()
        .or_else(|| std::env::var("CC_RUST_STREAM_IDLE_TIMEOUT_MS").ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| (1..=super::PROVIDER_TIMEOUT_MS_MAX).contains(value));
    if let Some(value) = stream_idle_override {
        merged.stream_idle_timeout_ms = Some(value);
        sources.insert("streamIdleTimeoutMs".to_string(), SettingsSource::Env);
    }
}

fn collect_env_overrides_raw() -> Option<RawSettings> {
    let mut raw = RawSettings::default();
    let mut found = false;

    if let Ok(model) = std::env::var("CLAUDE_MODEL") {
        raw.model = Some(model);
        found = true;
    }
    if let Ok(backend) = std::env::var("CC_BACKEND").or_else(|_| std::env::var("CLAUDE_BACKEND")) {
        raw.backend = Some(backend);
        found = true;
    }
    if let Ok(provider) = std::env::var("CC_API_PROVIDER") {
        raw.api_provider = Some(provider);
        found = true;
    }
    if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
        raw.api_key = Some(key);
        found = true;
    }
    if let Ok(v) = std::env::var("CLAUDE_VERBOSE") {
        raw.verbose = Some(v == "1" || v.eq_ignore_ascii_case("true"));
        found = true;
    }
    if let Ok(mode) = std::env::var("CLAUDE_PERMISSION_MODE") {
        raw.permission_mode = Some(mode.clone());
        raw.permissions = Some(super::types::PermissionsSettings {
            default_mode: Some(mode),
            ..Default::default()
        });
        found = true;
    }
    if let Ok(lang) = std::env::var("CLAUDE_LANGUAGE") {
        raw.language = Some(lang);
        found = true;
    }
    if let Ok(style) = std::env::var("CLAUDE_OUTPUT_STYLE") {
        raw.output_style = Some(style);
        found = true;
    }
    if let Ok(theme) = std::env::var("CLAUDE_THEME") {
        raw.theme = Some(theme);
        found = true;
    }

    found.then_some(raw)
}

/// Re-read process environment overrides into an already-loaded settings
/// stack. Used after startup seeds [`RawSettings::env`] into the process.
pub fn refresh_process_env_overrides(loaded: &mut LoadedSettings) {
    apply_active_auth_profile(&mut loaded.effective, &mut loaded.sources);
    apply_env_overrides(&mut loaded.effective, &mut loaded.sources);
    if let Some(raw) = loaded.managed.as_ref() {
        apply_managed_non_overridable(&mut loaded.effective, &mut loaded.sources, raw);
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RuntimeEnvApplyReport {
    pub applied: usize,
    pub skipped: usize,
    pub overridden: usize,
}

/// Apply merged `settings.env` values to the process environment.
///
/// This only fills missing variables. Existing shell, `.env`, and CI
/// variables keep priority and are never overwritten.
pub fn apply_runtime_env(env: &HashMap<String, String>) -> Result<RuntimeEnvApplyReport> {
    apply_runtime_env_inner(env, false)
}

/// Apply merged `settings.env` values during allthecodes startup.
///
/// Most variables still only fill missing process env. Provider auth, endpoint,
/// and model variables are intentionally overridden when declared in allthecodes
/// settings so inherited shell state from other Claude/Codex installations does
/// not silently route this process to the wrong account or model.
pub fn apply_startup_runtime_env(env: &HashMap<String, String>) -> Result<RuntimeEnvApplyReport> {
    apply_runtime_env_inner(env, true)
}

fn apply_runtime_env_inner(
    env: &HashMap<String, String>,
    override_provider_env: bool,
) -> Result<RuntimeEnvApplyReport> {
    let mut report = RuntimeEnvApplyReport::default();
    let mut keys = env.keys().collect::<Vec<_>>();
    keys.sort();

    for key in keys {
        let value = env.get(key).map(String::as_str).unwrap_or_default();
        validate_runtime_env_pair(key, value)?;
        match std::env::var_os(key) {
            Some(existing)
                if override_provider_env
                    && should_override_startup_runtime_env_key(key)
                    && existing != std::ffi::OsStr::new(value) =>
            {
                std::env::set_var(key, value);
                report.overridden += 1;
            }
            Some(_) => {
                report.skipped += 1;
            }
            None => {
                std::env::set_var(key, value);
                report.applied += 1;
            }
        }
    }

    if report.applied > 0 || report.skipped > 0 || report.overridden > 0 {
        tracing::debug!(
            applied = report.applied,
            skipped = report.skipped,
            overridden = report.overridden,
            "settings.env applied to runtime environment"
        );
    }

    Ok(report)
}

fn should_override_startup_runtime_env_key(key: &str) -> bool {
    matches!(
        key,
        "ANTHROPIC_API_KEY"
            | "ANTHROPIC_AUTH_TOKEN"
            | "ANTHROPIC_BASE_URL"
            | "ANTHROPIC_BEDROCK_BASE_URL"
            | "ANTHROPIC_MODEL"
            | "ANTHROPIC_DEFAULT_SOTA_MODEL"
            | "ANTHROPIC_DEFAULT_MOTA_MODEL"
            | "ANTHROPIC_DEFAULT_FOTA_MODEL"
            | "ANTHROPIC_DEFAULT_OPUS_MODEL"
            | "ANTHROPIC_DEFAULT_SONNET_MODEL"
            | "ANTHROPIC_DEFAULT_HAIKU_MODEL"
            | "OPENAI_CODEX_AUTH_TOKEN"
            | "OPENAI_CODEX_BASE_URL"
            | "OPENAI_CODEX_MODEL"
    )
}

fn validate_runtime_env_pair(key: &str, value: &str) -> Result<()> {
    if key.is_empty() || key.contains('=') || key.contains('\0') {
        anyhow::bail!("settings.env contains invalid environment variable name");
    }
    if value.contains('\0') {
        anyhow::bail!(
            "settings.env contains invalid value for environment variable `{}`",
            key
        );
    }
    Ok(())
}

pub(crate) fn apply_managed_non_overridable(
    merged: &mut EffectiveSettings,
    sources: &mut SourceMap,
    managed: &RawSettings,
) {
    if let Some(managed_permissions) = managed.permissions.as_ref() {
        if let Some(mode) = managed_permissions
            .default_mode
            .as_ref()
            .or(managed.permission_mode.as_ref())
        {
            merged.permission_mode = Some(mode.clone());
            merged.permissions.default_mode = Some(mode.clone());
            sources.insert("permissionMode".to_string(), SettingsSource::Managed);
            sources.insert(
                "permissions.defaultMode".to_string(),
                SettingsSource::Managed,
            );
        }
        if let Some(value) = managed_permissions.enable_auto_mode {
            merged.permissions.enable_auto_mode = Some(value);
            sources.insert(
                "permissions.enableAutoMode".to_string(),
                SettingsSource::Managed,
            );
        }
        if let Some(value) = managed_permissions.enable_bypass_mode {
            merged.permissions.enable_bypass_mode = Some(value);
            sources.insert(
                "permissions.enableBypassMode".to_string(),
                SettingsSource::Managed,
            );
        }
    } else if let Some(mode) = managed.permission_mode.as_ref() {
        merged.permission_mode = Some(mode.clone());
        merged.permissions.default_mode = Some(mode.clone());
        sources.insert("permissionMode".to_string(), SettingsSource::Managed);
        sources.insert(
            "permissions.defaultMode".to_string(),
            SettingsSource::Managed,
        );
    }

    let Some(managed_sandbox) = managed.sandbox.as_ref() else {
        return;
    };

    if let Some(value) = managed_sandbox.allow_managed_read_paths_only {
        merged.sandbox.allow_managed_read_paths_only = Some(value);
        sources.insert(
            "sandbox.allowManagedReadPathsOnly".to_string(),
            SettingsSource::Managed,
        );
        if value {
            merged.sandbox.filesystem.allow_read = managed_sandbox.filesystem.allow_read.clone();
            sources.insert(
                "sandbox.filesystem.allowRead".to_string(),
                SettingsSource::Managed,
            );
        }
    }

    if let Some(value) = managed_sandbox.allow_managed_domains_only {
        merged.sandbox.allow_managed_domains_only = Some(value);
        sources.insert(
            "sandbox.allowManagedDomainsOnly".to_string(),
            SettingsSource::Managed,
        );
        if value {
            merged.sandbox.network.allowed_domains =
                managed_sandbox.network.allowed_domains.clone();
            sources.insert(
                "sandbox.network.allowedDomains".to_string(),
                SettingsSource::Managed,
            );
        }
    }
}

pub fn validate_user_settings_candidate(cwd: &Path, raw: RawSettings) -> Result<()> {
    load_effective_with_options(
        cwd,
        LoadOptions {
            enforce_requirements: true,
            user_settings_override: Some(raw),
            ..Default::default()
        },
    )
    .map(|_| ())
}

fn apply_layer(
    acc: &mut RawSettings,
    sources: &mut SourceMap,
    layers: &mut Vec<ConfigLayer>,
    entries: &mut Vec<ConfigLayerEntry>,
    raw: RawSettings,
    source: SettingsSource,
    path: Option<PathBuf>,
) {
    let value = serde_json::to_value(&raw).unwrap_or(Value::Null);
    entries.extend(flatten_layer_entries(&value, source, path.as_deref(), None));
    acc.merge_from(raw, source, sources);
    layers.push(ConfigLayer {
        source,
        source_path: path,
        status: ConfigLayerStatus::Applied,
        disabled_reason: None,
    });
}

fn record_disabled_layer(
    layers: &mut Vec<ConfigLayer>,
    entries: &mut Vec<ConfigLayerEntry>,
    raw: RawSettings,
    source: SettingsSource,
    path: PathBuf,
    reason: ConfigLayerDisabledReason,
) {
    let value = serde_json::to_value(&raw).unwrap_or(Value::Null);
    entries.extend(flatten_layer_entries(
        &value,
        source,
        Some(&path),
        Some(reason.clone()),
    ));
    layers.push(ConfigLayer {
        source,
        source_path: Some(path),
        status: ConfigLayerStatus::Disabled,
        disabled_reason: Some(reason),
    });
}

fn resolve_paths_for_file(mut raw: RawSettings, path: &Path) -> RawSettings {
    let base = path.parent().unwrap_or_else(|| Path::new("."));
    if let Some(cloud_sync_path) = raw.cloud_sync_path.as_mut() {
        *cloud_sync_path = resolve_config_path(base, cloud_sync_path);
    }
    if let Some(permissions) = raw.permissions.as_mut() {
        resolve_string_paths(base, &mut permissions.additional_directories);
    }
    if let Some(sandbox) = raw.sandbox.as_mut() {
        resolve_string_paths(base, &mut sandbox.filesystem.allow_read);
        resolve_string_paths(base, &mut sandbox.filesystem.deny_read);
        resolve_string_paths(base, &mut sandbox.filesystem.allow_write);
        resolve_string_paths(base, &mut sandbox.filesystem.deny_write);
    }
    raw
}

fn resolve_string_paths(base: &Path, values: &mut [String]) {
    for value in values {
        *value = resolve_config_path(base, value);
    }
}

fn resolve_config_path(base: &Path, value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return value.to_string();
    }
    if let Some(rest) = trimmed.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest).display().to_string();
        }
    }
    let path = Path::new(trimmed);
    if path.is_absolute() {
        trimmed.to_string()
    } else {
        base.join(path).display().to_string()
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum TrustedWorkspacesFile {
    List(Vec<String>),
    Wrapped { workspaces: Vec<String> },
}

fn is_workspace_trusted(config_path: &Path) -> bool {
    let workspace = config_path
        .parent()
        .and_then(Path::parent)
        .unwrap_or(config_path);
    let Ok(workspace) = workspace.canonicalize() else {
        return false;
    };
    let path = crate::paths::data_root().join("trusted-workspaces.json");
    let Ok(contents) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(file) = serde_json::from_str::<TrustedWorkspacesFile>(&contents) else {
        return false;
    };
    let entries = match file {
        TrustedWorkspacesFile::List(entries) => entries,
        TrustedWorkspacesFile::Wrapped { workspaces } => workspaces,
    };
    entries.iter().any(|entry| {
        let path = PathBuf::from(entry);
        path.canonicalize()
            .map(|candidate| candidate == workspace)
            .unwrap_or(false)
    })
}

fn apply_profile(
    acc: &mut RawSettings,
    sources: &mut SourceMap,
    layers: &mut Vec<ConfigLayer>,
    entries: &mut Vec<ConfigLayerEntry>,
    user: &RawSettings,
    profile: &str,
    source_path: Option<&Path>,
) -> Result<()> {
    let profiles = user.config_profiles.as_ref().with_context(|| {
        format!("config profile `{profile}` was requested but no configProfiles are configured")
    })?;
    let mut raw = profiles
        .get(profile)
        .cloned()
        .with_context(|| format!("config profile `{profile}` does not exist"))?;
    if let Some(path) = source_path {
        raw = resolve_paths_for_file(raw, path);
    }
    apply_layer(
        acc,
        sources,
        layers,
        entries,
        raw,
        SettingsSource::UserProfile,
        source_path.map(Path::to_path_buf),
    );
    Ok(())
}

fn enforce_loaded_requirements(loaded: &LoadedSettings) -> Result<()> {
    if loaded.requirement_violations.is_empty() {
        return Ok(());
    }
    let messages = loaded
        .requirement_violations
        .iter()
        .map(|violation| violation.message.as_str())
        .collect::<Vec<_>>()
        .join("; ");
    bail!("settings requirements violated: {messages}")
}

/// Load the full four-layer stack (managed/user/project/local) plus env.
///
/// This is the preferred entry point for new code. The legacy
/// [`load_and_merge`] wraps this and returns only [`EffectiveSettings`].
pub fn load_effective_with_options(cwd: &Path, options: LoadOptions) -> Result<LoadedSettings> {
    let mut acc = RawSettings::default();
    let mut sources = SourceMap::new();
    let mut loaded_paths = Vec::new();
    let mut layers = Vec::new();
    let mut entries = Vec::new();
    let mut managed = None;
    let mut user = None;
    let mut project = None;
    let mut local = None;

    // 1. managed
    let managed_path = managed_settings_path();
    if let Some(raw) = load_raw_from(&managed_path)? {
        let raw = resolve_paths_for_file(raw, &managed_path);
        apply_layer(
            &mut acc,
            &mut sources,
            &mut layers,
            &mut entries,
            raw.clone(),
            SettingsSource::Managed,
            Some(managed_path.clone()),
        );
        managed = Some(raw);
        loaded_paths.push((SettingsSource::Managed, managed_path));
    }

    // 2. user
    let user_path = user_settings_path();
    let user_raw = if let Some(raw) = options.user_settings_override.clone() {
        Some(raw)
    } else {
        load_raw_from(&user_path)?
    };
    if let Some(raw) = user_raw {
        let raw = resolve_paths_for_file(raw, &user_path);
        apply_layer(
            &mut acc,
            &mut sources,
            &mut layers,
            &mut entries,
            raw.clone(),
            SettingsSource::User,
            Some(user_path.clone()),
        );
        if let Some(profile) = options.profile.as_deref() {
            apply_profile(
                &mut acc,
                &mut sources,
                &mut layers,
                &mut entries,
                &raw,
                profile,
                Some(&user_path),
            )?;
        }
        user = Some(raw);
        loaded_paths.push((SettingsSource::User, user_path));
    } else if let Some(profile) = options.profile.as_deref() {
        bail!("config profile `{profile}` was requested but user settings do not exist");
    }

    // 3. project
    if let Some(p) = find_project_config(cwd) {
        if let Some(raw) = load_raw_from(&p)? {
            let raw = resolve_paths_for_file(raw, &p);
            if options.trust_policy == ProjectTrustPolicy::TrustConfiguredOnly
                && !is_workspace_trusted(&p)
            {
                record_disabled_layer(
                    &mut layers,
                    &mut entries,
                    raw.clone(),
                    SettingsSource::Project,
                    p.clone(),
                    ConfigLayerDisabledReason::ProjectNotTrusted,
                );
            } else {
                apply_layer(
                    &mut acc,
                    &mut sources,
                    &mut layers,
                    &mut entries,
                    raw.clone(),
                    SettingsSource::Project,
                    Some(p.clone()),
                );
            }
            project = Some(raw);
            loaded_paths.push((SettingsSource::Project, p));
        }
    }

    // 4. local
    if let Some(p) = find_local_config(cwd) {
        if let Some(raw) = load_raw_from(&p)? {
            let raw = resolve_paths_for_file(raw, &p);
            if options.trust_policy == ProjectTrustPolicy::TrustConfiguredOnly
                && !is_workspace_trusted(&p)
            {
                record_disabled_layer(
                    &mut layers,
                    &mut entries,
                    raw.clone(),
                    SettingsSource::Local,
                    p.clone(),
                    ConfigLayerDisabledReason::ProjectNotTrusted,
                );
            } else {
                apply_layer(
                    &mut acc,
                    &mut sources,
                    &mut layers,
                    &mut entries,
                    raw.clone(),
                    SettingsSource::Local,
                    Some(p.clone()),
                );
            }
            local = Some(raw);
            loaded_paths.push((SettingsSource::Local, p));
        }
    }

    if let Some(raw) = collect_env_overrides_raw() {
        apply_layer(
            &mut acc,
            &mut sources,
            &mut layers,
            &mut entries,
            raw,
            SettingsSource::Env,
            None,
        );
    }

    if let Some(raw) = options.cli_overrides.clone() {
        apply_layer(
            &mut acc,
            &mut sources,
            &mut layers,
            &mut entries,
            raw,
            SettingsSource::Cli,
            None,
        );
    }

    if let Some(raw) = options.runtime_overrides.clone() {
        apply_layer(
            &mut acc,
            &mut sources,
            &mut layers,
            &mut entries,
            raw,
            SettingsSource::Runtime,
            None,
        );
    }

    let effective_raw_value = serde_json::to_value(&acc).unwrap_or(Value::Null);
    let mut effective = EffectiveSettings::from_raw(acc);
    apply_active_auth_profile(&mut effective, &mut sources);
    apply_stream_idle_env_override(&mut effective, &mut sources);

    if let Some(raw) = managed.as_ref() {
        apply_managed_non_overridable(&mut effective, &mut sources, raw);
    }
    attach_effective_values(&mut entries, &effective_raw_value);

    let (_requirements_path, requirements) = load_requirements()?;
    let requirement_violations = evaluate_requirements(&effective, &requirements);

    let loaded = LoadedSettings {
        effective,
        sources,
        layers,
        entries,
        requirement_violations,
        managed,
        user,
        project,
        local,
        loaded_paths,
    };
    if options.enforce_requirements {
        enforce_loaded_requirements(&loaded)?;
    }
    Ok(loaded)
}

pub fn load_effective(cwd: &Path) -> Result<LoadedSettings> {
    load_effective_with_options(
        cwd,
        LoadOptions {
            trust_policy: ProjectTrustPolicy::TrustAll,
            enforce_requirements: true,
            ..Default::default()
        },
    )
}

/// Convenience wrapper — loads the full stack and returns the merged
/// runtime view.
pub fn load_and_merge(cwd: &str) -> Result<MergedConfig> {
    Ok(load_effective(Path::new(cwd))?.effective)
}
