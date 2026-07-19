use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    load_global_config, provider_runtime_support, user_settings_path, write_settings_file,
    ProviderProfileSettings, ProviderRuntimeSupport, RawSettings,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderProfileStoreError {
    EmptyId,
    AlreadyExists(String),
    NotFound(String),
    ActiveProfile(String),
    UnsupportedProvider(String),
    InvalidRecoveryPolicy(String),
}

impl fmt::Display for ProviderProfileStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyId => write!(f, "provider profile id cannot be empty"),
            Self::AlreadyExists(id) => write!(f, "provider profile `{id}` already exists"),
            Self::NotFound(id) => write!(f, "provider profile `{id}` was not found"),
            Self::ActiveProfile(id) => write!(
                f,
                "provider profile `{id}` is active; activate another profile before deleting it"
            ),
            Self::UnsupportedProvider(provider) => write!(
                f,
                "provider `{provider}` is not supported for runtime activation"
            ),
            Self::InvalidRecoveryPolicy(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for ProviderProfileStoreError {}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", content = "value", rename_all = "snake_case")]
pub enum SecretUpdate<T> {
    #[default]
    Keep,
    Set(T),
    Clear,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ProviderSecretUpdates {
    pub api_key: SecretUpdate<String>,
    pub env: SecretUpdate<HashMap<String, String>>,
    pub auth_source: SecretUpdate<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderProfileReplacement {
    pub profile: ProviderProfileSettings,
    #[serde(default)]
    pub secrets: ProviderSecretUpdates,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RedactedProviderProfile {
    pub id: String,
    pub active: bool,
    pub runtime_support: String,
    pub backend: Option<String>,
    pub api_provider: Option<String>,
    pub model: Option<String>,
    pub available_models: Option<Vec<String>>,
    pub model_capabilities: Option<HashMap<String, super::ModelCapabilitySettings>>,
    pub model_reasoning_effort: Option<String>,
    pub request_max_retries: Option<u8>,
    pub stream_max_retries: Option<u8>,
    pub stream_idle_timeout_ms: Option<u64>,
    pub request_timeout_ms: Option<u64>,
    pub base_url: Option<String>,
    pub api_key_configured: bool,
    pub env_keys: Vec<String>,
    pub auth_source_configured: bool,
    pub extra: HashMap<String, Value>,
}

#[derive(Debug, Clone)]
pub struct ProviderProfileStore {
    path: PathBuf,
}

impl Default for ProviderProfileStore {
    fn default() -> Self {
        Self::global()
    }
}

impl ProviderProfileStore {
    pub fn global() -> Self {
        Self {
            path: user_settings_path(),
        }
    }

    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Result<RawSettings> {
        if self.path == user_settings_path() {
            return load_global_config();
        }
        if !self.path.exists() {
            return Ok(RawSettings::default());
        }
        let contents = std::fs::read_to_string(&self.path)
            .with_context(|| format!("failed to read {}", self.path.display()))?;
        serde_json::from_str(&contents)
            .with_context(|| format!("failed to parse {}", self.path.display()))
    }

    fn save(&self, settings: &RawSettings) -> Result<()> {
        write_settings_file(&self.path, settings)
    }

    pub fn list(&self) -> Result<Vec<RedactedProviderProfile>> {
        let settings = self.load()?;
        let active = settings.active_auth_profile.as_deref();
        let mut profiles = settings
            .auth_profiles
            .unwrap_or_default()
            .into_iter()
            .map(|(id, profile)| redact_profile(id, profile, active))
            .collect::<Vec<_>>();
        profiles.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(profiles)
    }

    pub fn get(&self, id: &str) -> Result<RedactedProviderProfile> {
        let settings = self.load()?;
        let active = settings.active_auth_profile.as_deref();
        let profile = settings
            .auth_profiles
            .unwrap_or_default()
            .remove(id)
            .ok_or_else(|| ProviderProfileStoreError::NotFound(id.to_string()))?;
        Ok(redact_profile(id.to_string(), profile, active))
    }

    pub fn get_raw(&self, id: &str) -> Result<ProviderProfileSettings> {
        self.load()?
            .auth_profiles
            .unwrap_or_default()
            .remove(id)
            .ok_or_else(|| ProviderProfileStoreError::NotFound(id.to_string()).into())
    }

    pub fn create(&self, id: &str, profile: ProviderProfileSettings) -> Result<()> {
        let id = validate_id(id)?;
        validate_recovery_policy(&profile)?;
        let mut settings = self.load()?;
        let profiles = settings.auth_profiles.get_or_insert_with(HashMap::new);
        if profiles.contains_key(id) {
            return Err(ProviderProfileStoreError::AlreadyExists(id.to_string()).into());
        }
        profiles.insert(id.to_string(), profile);
        self.save(&settings)
    }

    pub fn update<F>(&self, id: &str, update: F) -> Result<()>
    where
        F: FnOnce(&mut ProviderProfileSettings),
    {
        let mut settings = self.load()?;
        let profile = settings
            .auth_profiles
            .get_or_insert_with(HashMap::new)
            .get_mut(id)
            .ok_or_else(|| ProviderProfileStoreError::NotFound(id.to_string()))?;
        update(profile);
        validate_recovery_policy(profile)?;
        self.save(&settings)
    }

    pub fn update_and_rename<F>(&self, id: &str, new_id: Option<&str>, update: F) -> Result<String>
    where
        F: FnOnce(&mut ProviderProfileSettings),
    {
        let target_id = match new_id {
            Some(value) => validate_id(value)?.to_string(),
            None => id.to_string(),
        };
        let mut settings = self.load()?;
        let profiles = settings.auth_profiles.get_or_insert_with(HashMap::new);
        if target_id != id && profiles.contains_key(&target_id) {
            return Err(ProviderProfileStoreError::AlreadyExists(target_id).into());
        }
        let mut profile = profiles
            .remove(id)
            .ok_or_else(|| ProviderProfileStoreError::NotFound(id.to_string()))?;
        update(&mut profile);
        validate_recovery_policy(&profile)?;
        if settings.active_auth_profile.as_deref() == Some(id) {
            ensure_activatable(&target_id, &profile)?;
            settings.active_auth_profile = Some(target_id.clone());
        }
        profiles.insert(target_id.clone(), profile);
        self.save(&settings)?;
        Ok(target_id)
    }

    pub fn replace(&self, id: &str, replacement: ProviderProfileReplacement) -> Result<()> {
        self.replace_and_rename(id, id, replacement)
    }

    pub fn replace_and_rename(
        &self,
        id: &str,
        new_id: &str,
        mut replacement: ProviderProfileReplacement,
    ) -> Result<()> {
        let new_id = validate_id(new_id)?.to_string();
        let mut settings = self.load()?;
        let profiles = settings.auth_profiles.get_or_insert_with(HashMap::new);
        if new_id != id && profiles.contains_key(&new_id) {
            return Err(ProviderProfileStoreError::AlreadyExists(new_id).into());
        }
        let existing = profiles
            .remove(id)
            .ok_or_else(|| ProviderProfileStoreError::NotFound(id.to_string()))?;

        apply_secret_update(
            &mut replacement.profile.api_key,
            existing.api_key,
            replacement.secrets.api_key,
        );
        apply_secret_update(
            &mut replacement.profile.env,
            existing.env,
            replacement.secrets.env,
        );
        apply_secret_update(
            &mut replacement.profile.auth_source,
            existing.auth_source,
            replacement.secrets.auth_source,
        );
        validate_recovery_policy(&replacement.profile)?;

        if settings.active_auth_profile.as_deref() == Some(id) {
            ensure_activatable(&new_id, &replacement.profile)?;
            settings.active_auth_profile = Some(new_id.clone());
        }
        profiles.insert(new_id, replacement.profile);
        self.save(&settings)
    }

    pub fn rename(&self, id: &str, new_id: &str) -> Result<()> {
        let new_id = validate_id(new_id)?;
        let mut settings = self.load()?;
        let profiles = settings.auth_profiles.get_or_insert_with(HashMap::new);
        if id != new_id && profiles.contains_key(new_id) {
            return Err(ProviderProfileStoreError::AlreadyExists(new_id.to_string()).into());
        }
        let profile = profiles
            .remove(id)
            .ok_or_else(|| ProviderProfileStoreError::NotFound(id.to_string()))?;
        profiles.insert(new_id.to_string(), profile);
        if settings.active_auth_profile.as_deref() == Some(id) {
            settings.active_auth_profile = Some(new_id.to_string());
        }
        self.save(&settings)
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        let mut settings = self.load()?;
        if settings.active_auth_profile.as_deref() == Some(id) {
            return Err(ProviderProfileStoreError::ActiveProfile(id.to_string()).into());
        }
        let removed = settings
            .auth_profiles
            .get_or_insert_with(HashMap::new)
            .remove(id);
        if removed.is_none() {
            return Err(ProviderProfileStoreError::NotFound(id.to_string()).into());
        }
        self.save(&settings)
    }

    pub fn activate(&self, id: &str) -> Result<()> {
        let mut settings = self.load()?;
        let profile = settings
            .auth_profiles
            .as_ref()
            .and_then(|profiles| profiles.get(id))
            .ok_or_else(|| ProviderProfileStoreError::NotFound(id.to_string()))?;
        ensure_activatable(id, profile)?;
        validate_recovery_policy(profile)?;
        settings.active_auth_profile = Some(id.to_string());
        self.save(&settings)
    }
}

fn validate_id(id: &str) -> Result<&str> {
    let id = id.trim();
    if id.is_empty() {
        Err(ProviderProfileStoreError::EmptyId.into())
    } else {
        Ok(id)
    }
}

fn ensure_activatable(id: &str, profile: &ProviderProfileSettings) -> Result<()> {
    let provider = profile.api_provider.as_deref().unwrap_or(id);
    match provider_runtime_support(provider) {
        ProviderRuntimeSupport::Supported => Ok(()),
        ProviderRuntimeSupport::Unsupported | ProviderRuntimeSupport::Unknown => {
            Err(ProviderProfileStoreError::UnsupportedProvider(provider.to_string()).into())
        }
    }
}

pub fn validate_recovery_policy(profile: &ProviderProfileSettings) -> Result<()> {
    if profile
        .request_max_retries
        .is_some_and(|value| value > super::PROVIDER_RETRY_LIMIT_MAX)
    {
        return Err(ProviderProfileStoreError::InvalidRecoveryPolicy(
            "requestMaxRetries must be between 0 and 100".to_string(),
        )
        .into());
    }
    if profile
        .stream_max_retries
        .is_some_and(|value| value > super::PROVIDER_RETRY_LIMIT_MAX)
    {
        return Err(ProviderProfileStoreError::InvalidRecoveryPolicy(
            "streamMaxRetries must be between 0 and 100".to_string(),
        )
        .into());
    }
    for (name, value) in [
        ("streamIdleTimeoutMs", profile.stream_idle_timeout_ms),
        ("requestTimeoutMs", profile.request_timeout_ms),
    ] {
        if value.is_some_and(|value| !(1..=super::PROVIDER_TIMEOUT_MS_MAX).contains(&value)) {
            return Err(ProviderProfileStoreError::InvalidRecoveryPolicy(format!(
                "{name} must be between 1 and {}",
                super::PROVIDER_TIMEOUT_MS_MAX
            ))
            .into());
        }
    }
    Ok(())
}

fn apply_secret_update<T>(target: &mut Option<T>, existing: Option<T>, update: SecretUpdate<T>) {
    *target = match update {
        SecretUpdate::Keep => existing,
        SecretUpdate::Set(value) => Some(value),
        SecretUpdate::Clear => None,
    };
}

fn redact_profile(
    id: String,
    profile: ProviderProfileSettings,
    active: Option<&str>,
) -> RedactedProviderProfile {
    let provider = profile.api_provider.as_deref().unwrap_or(&id);
    let runtime_support = match provider_runtime_support(provider) {
        ProviderRuntimeSupport::Supported => "supported",
        ProviderRuntimeSupport::Unsupported => "unsupported",
        ProviderRuntimeSupport::Unknown => "unknown",
    }
    .to_string();
    let mut env_keys = profile
        .env
        .as_ref()
        .map(|env| env.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    env_keys.sort();
    RedactedProviderProfile {
        active: active == Some(id.as_str()),
        id,
        runtime_support,
        backend: profile.backend,
        api_provider: profile.api_provider,
        model: profile.model,
        available_models: profile.available_models,
        model_capabilities: profile.model_capabilities,
        model_reasoning_effort: profile.model_reasoning_effort,
        request_max_retries: profile.request_max_retries,
        stream_max_retries: profile.stream_max_retries,
        stream_idle_timeout_ms: profile.stream_idle_timeout_ms,
        request_timeout_ms: profile.request_timeout_ms,
        base_url: profile.base_url,
        api_key_configured: profile
            .api_key
            .as_ref()
            .is_some_and(|value| !value.is_empty()),
        env_keys,
        auth_source_configured: profile.auth_source.is_some(),
        extra: redact_map(profile.extra),
    }
}

fn redact_map(mut values: HashMap<String, Value>) -> HashMap<String, Value> {
    for (key, value) in &mut values {
        redact_value(key, value);
    }
    values
}

fn redact_value(path: &str, value: &mut Value) {
    if is_sensitive(path) {
        *value = Value::String("[redacted]".to_string());
        return;
    }
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                redact_value(key, child);
            }
        }
        Value::Array(items) => {
            for child in items {
                redact_value(path, child);
            }
        }
        _ => {}
    }
}

fn is_sensitive(key: &str) -> bool {
    let key = key
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    [
        "apikey",
        "token",
        "secret",
        "password",
        "credential",
        "privatekey",
    ]
    .iter()
    .any(|needle| key.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::VALID_API_PROVIDERS;
    use serde_json::json;

    fn store() -> (tempfile::TempDir, ProviderProfileStore) {
        let root = tempfile::tempdir().unwrap();
        let store = ProviderProfileStore::at(root.path().join("settings.json"));
        (root, store)
    }

    fn profile(provider: &str, key: &str) -> ProviderProfileSettings {
        ProviderProfileSettings {
            api_provider: Some(provider.to_string()),
            model: Some("model-a".to_string()),
            api_key: Some(key.to_string()),
            extra: HashMap::from([("futureSecret".to_string(), json!("hidden"))]),
            ..ProviderProfileSettings::default()
        }
    }

    #[test]
    fn crud_is_lossless_and_reads_are_redacted() {
        let (_root, store) = store();
        store
            .create("Local OpenAI", profile("openai", "secret"))
            .unwrap();
        let detail = store.get("Local OpenAI").unwrap();
        assert!(detail.api_key_configured);
        assert_eq!(detail.extra["futureSecret"], "[redacted]");
        assert_eq!(
            store.get_raw("Local OpenAI").unwrap().api_key.as_deref(),
            Some("secret")
        );
        store.rename("Local OpenAI", "Local GPT").unwrap();
        assert_eq!(
            store.get_raw("Local GPT").unwrap().model.as_deref(),
            Some("model-a")
        );
        store.delete("Local GPT").unwrap();
        assert!(store.list().unwrap().is_empty());
    }

    #[test]
    fn replacement_clears_non_secrets_and_keeps_secret_by_default() {
        let (_root, store) = store();
        store.create("p", profile("openai", "secret")).unwrap();
        store
            .replace(
                "p",
                ProviderProfileReplacement {
                    profile: ProviderProfileSettings {
                        api_provider: Some("openai".to_string()),
                        ..ProviderProfileSettings::default()
                    },
                    secrets: ProviderSecretUpdates::default(),
                },
            )
            .unwrap();
        let saved = store.get_raw("p").unwrap();
        assert_eq!(saved.api_key.as_deref(), Some("secret"));
        assert!(saved.model.is_none());
    }

    #[test]
    fn replace_and_rename_is_atomic_and_updates_active_pointer() {
        let (_root, store) = store();
        store.create("active", profile("openai", "secret")).unwrap();
        store
            .create("occupied", profile("deepseek", "other"))
            .unwrap();
        store.activate("active").unwrap();

        let replacement = ProviderProfileReplacement {
            profile: ProviderProfileSettings {
                api_provider: Some("deepseek".to_string()),
                model: Some("model-b".to_string()),
                ..ProviderProfileSettings::default()
            },
            secrets: ProviderSecretUpdates {
                api_key: SecretUpdate::Clear,
                ..ProviderSecretUpdates::default()
            },
        };
        assert!(store
            .replace_and_rename("active", "occupied", replacement.clone())
            .is_err());
        assert_eq!(
            store.get_raw("active").unwrap().api_key.as_deref(),
            Some("secret")
        );
        assert_eq!(
            store.load().unwrap().active_auth_profile.as_deref(),
            Some("active")
        );

        store
            .replace_and_rename("active", "renamed", replacement)
            .unwrap();
        let saved = store.get_raw("renamed").unwrap();
        assert_eq!(saved.model.as_deref(), Some("model-b"));
        assert!(saved.api_key.is_none());
        assert!(store.get_raw("active").is_err());
        assert_eq!(
            store.load().unwrap().active_auth_profile.as_deref(),
            Some("renamed")
        );
    }

    #[test]
    fn active_profile_cannot_be_deleted_and_rename_updates_pointer() {
        let (_root, store) = store();
        store.create("p", profile("deepseek", "secret")).unwrap();
        store.activate("p").unwrap();
        assert!(store
            .delete("p")
            .unwrap_err()
            .to_string()
            .contains("is active"));
        store.rename("p", "renamed").unwrap();
        assert_eq!(
            store.load().unwrap().active_auth_profile.as_deref(),
            Some("renamed")
        );
    }

    #[test]
    fn activation_rejects_foundry_and_unknown_but_allows_supported_matrix() {
        let (_root, store) = store();
        for provider in VALID_API_PROVIDERS
            .iter()
            .copied()
            .filter(|p| *p != "azure-foundry")
        {
            let id = format!("profile-{provider}");
            store.create(&id, profile(provider, "secret")).unwrap();
            store.activate(&id).unwrap();
        }
        store
            .create("foundry", profile("azure-foundry", "secret"))
            .unwrap();
        assert!(store.activate("foundry").is_err());
        store
            .create("custom", profile("custom-acp", "secret"))
            .unwrap();
        assert!(store.activate("custom").is_err());
    }
}
