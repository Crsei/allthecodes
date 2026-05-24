use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};

use super::effective::{EffectiveSettings, LoadedSettings};
use super::paths::{
    find_local_config, find_project_config, managed_settings_path, user_settings_path,
};
use super::providers::{normalize_api_provider, API_PROVIDER_OPENAI_CODEX};
use super::raw::{GlobalConfig, MergedConfig, ProjectConfig, RawSettings};
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
    let is_codex = profile
        .backend
        .as_deref()
        .is_some_and(|backend| backend.eq_ignore_ascii_case("codex"))
        || profile.api_provider.as_deref().is_some_and(|provider| {
            normalize_api_provider(provider) == Some(API_PROVIDER_OPENAI_CODEX)
        })
        || active.eq_ignore_ascii_case("codex");

    if let Some(backend) = profile.backend {
        merged.backend = Some(backend);
        sources.insert("backend".to_string(), source);
        sources.insert(profile_key("backend"), source);
    }
    if let Some(api_provider) = profile.api_provider {
        merged.api_provider = Some(api_provider);
        sources.insert("apiProvider".to_string(), source);
        sources.insert(profile_key("apiProvider"), source);
    }
    if let Some(model) = profile.model {
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
    if let Some(api_key) = profile.api_key {
        merged.api_key = Some(api_key.clone());
        let env_key = if is_codex {
            "OPENAI_CODEX_AUTH_TOKEN"
        } else {
            "ANTHROPIC_API_KEY"
        };
        merged.env.insert(env_key.to_string(), api_key);
        sources.insert("apiKey".to_string(), source);
        sources.insert(profile_key("apiKey"), source);
        sources.insert("env".to_string(), source);
    }
    if let Some(base_url) = profile
        .base_url
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
    {
        let env_key = if is_codex {
            "OPENAI_CODEX_BASE_URL"
        } else {
            "ANTHROPIC_BASE_URL"
        };
        merged.env.insert(env_key.to_string(), base_url);
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
    } else if let Some(model) = merged
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
    if let Some(env) = profile.env {
        for (key, value) in env {
            merged.env.insert(key, value);
        }
        sources.insert("env".to_string(), source);
        sources.insert(profile_key("env"), source);
    }
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

/// Load the full four-layer stack (managed/user/project/local) plus env.
///
/// This is the preferred entry point for new code. The legacy
/// [`load_and_merge`] wraps this and returns only [`EffectiveSettings`].
pub fn load_effective(cwd: &Path) -> Result<LoadedSettings> {
    let mut acc = RawSettings::default();
    let mut sources = SourceMap::new();
    let mut loaded_paths = Vec::new();
    let mut managed = None;
    let mut user = None;
    let mut project = None;
    let mut local = None;

    // 1. managed
    let managed_path = managed_settings_path();
    if let Some(raw) = load_raw_from(&managed_path)? {
        acc.merge_from(raw.clone(), SettingsSource::Managed, &mut sources);
        managed = Some(raw);
        loaded_paths.push((SettingsSource::Managed, managed_path));
    }

    // 2. user
    let user_path = user_settings_path();
    if let Some(raw) = load_raw_from(&user_path)? {
        acc.merge_from(raw.clone(), SettingsSource::User, &mut sources);
        user = Some(raw);
        loaded_paths.push((SettingsSource::User, user_path));
    }

    // 3. project
    if let Some(p) = find_project_config(cwd) {
        if let Some(raw) = load_raw_from(&p)? {
            acc.merge_from(raw.clone(), SettingsSource::Project, &mut sources);
            project = Some(raw);
            loaded_paths.push((SettingsSource::Project, p));
        }
    }

    // 4. local
    if let Some(p) = find_local_config(cwd) {
        if let Some(raw) = load_raw_from(&p)? {
            acc.merge_from(raw.clone(), SettingsSource::Local, &mut sources);
            local = Some(raw);
            loaded_paths.push((SettingsSource::Local, p));
        }
    }

    let mut effective = EffectiveSettings::from_raw(acc);
    apply_active_auth_profile(&mut effective, &mut sources);

    // 5. env
    apply_env_overrides(&mut effective, &mut sources);
    if let Some(raw) = managed.as_ref() {
        apply_managed_non_overridable(&mut effective, &mut sources, raw);
    }

    Ok(LoadedSettings {
        effective,
        sources,
        managed,
        user,
        project,
        local,
        loaded_paths,
    })
}

/// Convenience wrapper — loads the full stack and returns the merged
/// runtime view.
pub fn load_and_merge(cwd: &str) -> Result<MergedConfig> {
    Ok(load_effective(Path::new(cwd))?.effective)
}
