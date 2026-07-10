use allthecodes_config::paths;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{AdapterProvider, GatewayDiagnostic, GatewayError};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum PatchField<T> {
    #[default]
    Missing,
    Clear,
    Set(T),
}

fn deserialize_patch_field<'de, T, D>(deserializer: D) -> Result<PatchField<T>, D::Error>
where
    T: Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    Ok(match Option::<T>::deserialize(deserializer)? {
        Some(value) => PatchField::Set(value),
        None => PatchField::Clear,
    })
}

/// Gateway-local persistence locations. All defaults stay under
/// `allthecodes_config::paths::data_root()` so allthecodes never writes to upstream
/// Claude/Codex directories.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayPersistence {
    pub gateway_dir: PathBuf,
    pub runs_dir: PathBuf,
    pub adapters_dir: PathBuf,
    pub webhooks_dir: PathBuf,
}

impl Default for GatewayPersistence {
    fn default() -> Self {
        Self {
            gateway_dir: paths::gateway_dir(),
            runs_dir: paths::gateway_runs_dir(),
            adapters_dir: paths::gateway_adapters_dir(),
            webhooks_dir: paths::gateway_webhooks_dir(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayLimits {
    pub max_payload_bytes: usize,
    pub max_metadata_entries: usize,
}

impl Default for GatewayLimits {
    fn default() -> Self {
        Self {
            max_payload_bytes: 256 * 1024,
            max_metadata_entries: 64,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GatewayConfig {
    pub enabled: bool,
    #[serde(default)]
    pub persistence: GatewayPersistence,
    #[serde(default)]
    pub limits: GatewayLimits,
    #[serde(default)]
    pub security: GatewaySecurityConfig,
    #[serde(default)]
    pub adapters: GatewayAdaptersConfig,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
    #[serde(skip)]
    token_extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GatewaySecurityConfig {
    #[serde(default)]
    pub allowed_origins: Vec<String>,
    #[serde(default, skip_serializing)]
    pub remote_token: Option<String>,
}

impl GatewayConfig {
    pub fn default_config_path() -> PathBuf {
        paths::gateway_dir().join("config.json")
    }

    pub fn tokens_path() -> PathBuf {
        paths::gateway_dir().join("tokens.json")
    }

    pub fn load() -> Result<Self, GatewayError> {
        Self::load_from(&Self::default_config_path())
    }

    pub fn load_from(path: &Path) -> Result<Self, GatewayError> {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => {
                return Err(GatewayError::io(
                    "gateway_config_read_failed",
                    "The gateway configuration could not be read.",
                    "Check permissions for the allthecodes gateway config file.",
                    path,
                    &error,
                ));
            }
        };
        let mut config: Self = serde_json::from_slice(&bytes).map_err(|error| {
            GatewayError::new(
                GatewayDiagnostic::new(
                    "gateway_config_invalid",
                    "The gateway configuration is invalid.",
                    "Fix the gateway config JSON and retry.",
                )
                .with_context(format!("path={}, error={}", path.display(), error)),
            )
        })?;

        let tokens_path = tokens_path_for(path);
        let token_bytes = match fs::read(&tokens_path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(config),
            Err(error) => {
                return Err(GatewayError::io(
                    "gateway_tokens_read_failed",
                    "The gateway channel credentials could not be read.",
                    "Check permissions for the allthecodes gateway credentials file.",
                    &tokens_path,
                    &error,
                ));
            }
        };
        let tokens: GatewayTokenFile = serde_json::from_slice(&token_bytes).map_err(|error| {
            GatewayError::new(
                GatewayDiagnostic::new(
                    "gateway_tokens_invalid",
                    "The gateway channel credentials file is invalid.",
                    "Fix the gateway credentials JSON and retry.",
                )
                .with_context(format!(
                    "path={}, error={}",
                    tokens_path.display(),
                    error
                )),
            )
        })?;
        config.security.remote_token = tokens.remote_token;
        config.adapters.telegram.bot_token = tokens.telegram_bot_token;
        config.adapters.lark.app_id = tokens.lark_app_id;
        config.adapters.lark.app_secret = tokens.lark_app_secret;
        config.adapters.lark.outbound_webhook_url = tokens.lark_outbound_webhook_url;
        config.token_extra = tokens.extra;
        Ok(config)
    }

    pub fn save(&self) -> Result<(), GatewayError> {
        self.save_to(&Self::default_config_path())
    }

    pub fn save_to(&self, path: &Path) -> Result<(), GatewayError> {
        self.persistence.validate_layout().map_err(|error| {
            GatewayError::new(
                GatewayDiagnostic::new(
                    "gateway_config_invalid",
                    "The gateway persistence layout is invalid.",
                    "Keep gateway persistence paths under the gateway directory.",
                )
                .with_context(error),
            )
        })?;
        let parent = path.parent().ok_or_else(|| {
            GatewayError::new(GatewayDiagnostic::new(
                "gateway_config_write_failed",
                "The gateway configuration path has no parent directory.",
                "Use a config path inside the allthecodes data directory.",
            ))
        })?;
        fs::create_dir_all(parent).map_err(|error| {
            GatewayError::io(
                "gateway_config_write_failed",
                "The gateway configuration directory could not be created.",
                "Check permissions for the allthecodes gateway directory.",
                parent,
                &error,
            )
        })?;

        let config_bytes = serde_json::to_vec_pretty(self).map_err(|error| {
            GatewayError::new(
                GatewayDiagnostic::new(
                    "gateway_config_encode_failed",
                    "The gateway configuration could not be encoded.",
                    "Check the channel configuration values and retry.",
                )
                .with_context(format!("error={}", error)),
            )
        })?;
        let mut tokens = GatewayTokenFile {
            remote_token: self.security.remote_token.clone(),
            telegram_bot_token: self.adapters.telegram.bot_token.clone(),
            lark_app_id: self.adapters.lark.app_id.clone(),
            lark_app_secret: self.adapters.lark.app_secret.clone(),
            lark_outbound_webhook_url: self.adapters.lark.outbound_webhook_url.clone(),
            extra: self.token_extra.clone(),
        };
        remove_empty_token_fields(&mut tokens);
        let tokens_bytes = serde_json::to_vec_pretty(&tokens).map_err(|error| {
            GatewayError::new(
                GatewayDiagnostic::new(
                    "gateway_tokens_encode_failed",
                    "The gateway channel credentials could not be encoded.",
                    "Check the channel credential values and retry.",
                )
                .with_context(format!("error={}", error)),
            )
        })?;
        atomic_write_json(
            &tokens_path_for(path),
            &tokens_bytes,
            "gateway_tokens_write_failed",
        )?;
        atomic_write_json(path, &config_bytes, "gateway_config_write_failed")
    }

    pub fn channels_config(&self) -> GatewayChannelsConfigResponse {
        GatewayChannelsConfigResponse {
            providers: vec![
                self.channel_snapshot(AdapterProvider::Telegram),
                self.channel_snapshot(AdapterProvider::Lark),
            ],
        }
    }

    pub fn channel_snapshot(&self, provider: AdapterProvider) -> GatewayChannelConfigSnapshot {
        match provider {
            AdapterProvider::Telegram => {
                let configured = non_empty(self.adapters.telegram.bot_token.as_deref());
                GatewayChannelConfigSnapshot {
                    provider,
                    enabled: self.adapters.telegram.enabled,
                    configured,
                    has_secret: configured,
                    api_base_url: self.adapters.telegram.api_base_url.clone(),
                    test_chat_allowlist: Some(self.adapters.telegram.test_chat_allowlist.clone()),
                    test_target_allowlist: None,
                    probe_updates: Some(self.adapters.telegram.probe_updates),
                }
            }
            AdapterProvider::Lark => {
                let has_webhook = non_empty(self.adapters.lark.outbound_webhook_url.as_deref());
                let has_app_credentials = non_empty(self.adapters.lark.app_id.as_deref())
                    && non_empty(self.adapters.lark.app_secret.as_deref());
                GatewayChannelConfigSnapshot {
                    provider,
                    enabled: self.adapters.lark.enabled,
                    configured: has_webhook || has_app_credentials,
                    has_secret: has_webhook || non_empty(self.adapters.lark.app_secret.as_deref()),
                    api_base_url: self.adapters.lark.api_base_url.clone(),
                    test_chat_allowlist: None,
                    test_target_allowlist: Some(self.adapters.lark.test_target_allowlist.clone()),
                    probe_updates: None,
                }
            }
        }
    }

    pub fn update_channel(
        &mut self,
        provider: AdapterProvider,
        patch: GatewayChannelConfigPatch,
    ) -> Result<(), GatewayError> {
        match provider {
            AdapterProvider::Telegram => {
                if !matches!(&patch.app_id, PatchField::Missing)
                    || !matches!(&patch.app_secret, PatchField::Missing)
                    || !matches!(&patch.outbound_webhook_url, PatchField::Missing)
                    || patch.test_target_allowlist.is_some()
                {
                    return Err(invalid_channel_patch(
                        "Telegram updates only accept botToken, enabled, apiBaseUrl, testAllowlist, and probeUpdates.",
                    ));
                }
                let mut config = self.adapters.telegram.clone();
                apply_common_patch(
                    &mut config.enabled,
                    &mut config.api_base_url,
                    &mut config.test_chat_allowlist,
                    patch.enabled,
                    patch.api_base_url,
                    patch.test_allowlist.or(patch.test_chat_allowlist),
                )?;
                if let Some(probe_updates) = patch.probe_updates {
                    config.probe_updates = probe_updates;
                }
                let token = combine_secret_patch(patch.bot_token, patch.secret, "botToken")?;
                let clears_required_credential = clears_secret(&token);
                apply_secret_patch(&mut config.bot_token, token);
                if config.enabled && !non_empty(config.bot_token.as_deref()) {
                    if clears_required_credential {
                        config.enabled = false;
                    } else {
                        return Err(channel_not_configured(AdapterProvider::Telegram));
                    }
                }
                self.adapters.telegram = config;
            }
            AdapterProvider::Lark => {
                if patch.probe_updates.is_some()
                    || matches!(&patch.bot_token, PatchField::Set(_))
                    || matches!(&patch.bot_token, PatchField::Clear)
                {
                    return Err(invalid_channel_patch(
                        "Lark updates only accept appId, appSecret, outboundWebhookUrl, enabled, apiBaseUrl, and testAllowlist.",
                    ));
                }
                let mut config = self.adapters.lark.clone();
                apply_common_patch(
                    &mut config.enabled,
                    &mut config.api_base_url,
                    &mut config.test_target_allowlist,
                    patch.enabled,
                    patch.api_base_url,
                    patch.test_allowlist.or(patch.test_target_allowlist),
                )?;
                let clears_app_id = clears_secret(&patch.app_id);
                let clears_webhook = clears_secret(&patch.outbound_webhook_url);
                apply_secret_patch(&mut config.app_id, patch.app_id);
                let app_secret = combine_secret_patch(patch.app_secret, patch.secret, "appSecret")?;
                let clears_app_secret = clears_secret(&app_secret);
                apply_secret_patch(&mut config.app_secret, app_secret);
                apply_secret_patch(&mut config.outbound_webhook_url, patch.outbound_webhook_url);
                let has_webhook = non_empty(config.outbound_webhook_url.as_deref());
                let has_app_credentials =
                    non_empty(config.app_id.as_deref()) && non_empty(config.app_secret.as_deref());
                if config.enabled && !has_webhook && !has_app_credentials {
                    if clears_app_id || clears_app_secret || clears_webhook {
                        config.enabled = false;
                    } else {
                        return Err(channel_not_configured(AdapterProvider::Lark));
                    }
                }
                self.adapters.lark = config;
            }
        }
        Ok(())
    }
}

fn apply_common_patch(
    enabled: &mut bool,
    api_base_url: &mut String,
    test_allowlist: &mut Vec<String>,
    enabled_patch: Option<bool>,
    api_base_url_patch: Option<String>,
    test_allowlist_patch: Option<Vec<String>>,
) -> Result<(), GatewayError> {
    if let Some(value) = enabled_patch {
        *enabled = value;
    }
    if let Some(value) = api_base_url_patch {
        if value.trim().is_empty() {
            return Err(invalid_channel_patch("apiBaseUrl must not be empty."));
        }
        *api_base_url = value;
    }
    if let Some(value) = test_allowlist_patch {
        *test_allowlist = value;
    }
    Ok(())
}

fn combine_secret_patch(
    primary: PatchField<String>,
    alias: PatchField<String>,
    field: &'static str,
) -> Result<PatchField<String>, GatewayError> {
    if !matches!(primary, PatchField::Missing) && !matches!(alias, PatchField::Missing) {
        return Err(invalid_channel_patch(&format!(
            "Specify either {field} or secret, not both."
        )));
    }
    Ok(if matches!(primary, PatchField::Missing) {
        alias
    } else {
        primary
    })
}

fn apply_secret_patch(secret: &mut Option<String>, patch: PatchField<String>) {
    match patch {
        PatchField::Missing => {}
        PatchField::Clear => *secret = None,
        PatchField::Set(value) if value.trim().is_empty() => *secret = None,
        PatchField::Set(value) => *secret = Some(value),
    }
}

fn clears_secret(patch: &PatchField<String>) -> bool {
    matches!(patch, PatchField::Clear)
        || matches!(patch, PatchField::Set(value) if value.trim().is_empty())
}

fn invalid_channel_patch(message: &str) -> GatewayError {
    GatewayError::new(GatewayDiagnostic::new(
        "invalid_channel_config",
        message,
        "Send a channel config patch matching the provider schema.",
    ))
}

fn channel_not_configured(provider: AdapterProvider) -> GatewayError {
    GatewayError::new(GatewayDiagnostic::new(
        "channel_not_configured",
        format!("{} credentials are not configured.", provider.as_str()),
        "Persist the required channel credentials before enabling the adapter.",
    ))
}

fn non_empty(value: Option<&str>) -> bool {
    value.is_some_and(|value| !value.trim().is_empty())
}

fn tokens_path_for(config_path: &Path) -> PathBuf {
    config_path.with_file_name("tokens.json")
}

fn remove_empty_token_fields(tokens: &mut GatewayTokenFile) {
    if !non_empty(tokens.remote_token.as_deref()) {
        tokens.remote_token = None;
    }
    if !non_empty(tokens.telegram_bot_token.as_deref()) {
        tokens.telegram_bot_token = None;
    }
    if !non_empty(tokens.lark_app_id.as_deref()) {
        tokens.lark_app_id = None;
    }
    if !non_empty(tokens.lark_app_secret.as_deref()) {
        tokens.lark_app_secret = None;
    }
    if !non_empty(tokens.lark_outbound_webhook_url.as_deref()) {
        tokens.lark_outbound_webhook_url = None;
    }
}

fn atomic_write_json(path: &Path, bytes: &[u8], code: &'static str) -> Result<(), GatewayError> {
    let parent = path.parent().ok_or_else(|| {
        GatewayError::new(GatewayDiagnostic::new(
            code,
            "The gateway persistence path has no parent directory.",
            "Use a gateway persistence path inside the allthecodes data directory.",
        ))
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        GatewayError::io(
            code,
            "The gateway persistence directory could not be created.",
            "Check permissions for the allthecodes gateway directory.",
            parent,
            &error,
        )
    })?;
    let (tmp_path, mut file) = create_private_temp(parent, path).map_err(|error| {
        GatewayError::io(
            code,
            "A private gateway persistence file could not be created.",
            "Check permissions and temporary files in the gateway directory.",
            path,
            &error,
        )
    })?;
    if let Err(error) = file.write_all(bytes).and_then(|_| file.sync_all()) {
        let _ = fs::remove_file(&tmp_path);
        return Err(GatewayError::io(
            code,
            "The gateway persistence file could not be written.",
            "Check permissions and available disk space for the gateway directory.",
            &tmp_path,
            &error,
        ));
    }
    drop(file);
    if let Err(error) = set_private_permissions(&tmp_path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(GatewayError::io(
            code,
            "The gateway persistence file permissions could not be set.",
            "Check permissions for the gateway directory.",
            &tmp_path,
            &error,
        ));
    }
    if let Err(error) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(GatewayError::io(
            code,
            "The gateway persistence file could not be published atomically.",
            "Check permissions and filesystem state for the gateway directory.",
            path,
            &error,
        ));
    }
    Ok(())
}

static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

fn create_private_temp(parent: &Path, target: &Path) -> std::io::Result<(PathBuf, fs::File)> {
    let file_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("gateway.json");
    for _ in 0..32 {
        let suffix = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp_path = parent.join(format!(".{file_name}.tmp-{}-{suffix}", std::process::id()));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&tmp_path) {
            Ok(file) => return Ok((tmp_path, file)),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(std::io::Error::new(
        ErrorKind::AlreadyExists,
        "could not allocate a unique private gateway temporary file",
    ))
}

fn set_private_permissions(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayChannelsConfigResponse {
    pub providers: Vec<GatewayChannelConfigSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayChannelConfigSnapshot {
    pub provider: AdapterProvider,
    pub enabled: bool,
    pub configured: bool,
    pub has_secret: bool,
    pub api_base_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_chat_allowlist: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_target_allowlist: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub probe_updates: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GatewayChannelConfigPatch {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub api_base_url: Option<String>,
    #[serde(default)]
    pub test_allowlist: Option<Vec<String>>,
    #[serde(default)]
    pub test_chat_allowlist: Option<Vec<String>>,
    #[serde(default)]
    pub test_target_allowlist: Option<Vec<String>>,
    #[serde(default)]
    pub probe_updates: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_patch_field")]
    pub secret: PatchField<String>,
    #[serde(default, deserialize_with = "deserialize_patch_field")]
    pub bot_token: PatchField<String>,
    #[serde(default, deserialize_with = "deserialize_patch_field")]
    pub app_id: PatchField<String>,
    #[serde(default, deserialize_with = "deserialize_patch_field")]
    pub app_secret: PatchField<String>,
    #[serde(default, deserialize_with = "deserialize_patch_field")]
    pub outbound_webhook_url: PatchField<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct GatewayTokenFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    remote_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    telegram_bot_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    lark_app_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    lark_app_secret: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    lark_outbound_webhook_url: Option<String>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GatewayAdaptersConfig {
    #[serde(default)]
    pub lark: LarkAdapterConfig,
    #[serde(default)]
    pub telegram: TelegramAdapterConfig,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TelegramAdapterConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_telegram_api_base_url")]
    pub api_base_url: String,
    #[serde(default)]
    pub test_chat_allowlist: Vec<String>,
    #[serde(default)]
    pub probe_updates: bool,
    #[serde(default, skip_serializing)]
    pub bot_token: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Default for TelegramAdapterConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            api_base_url: default_telegram_api_base_url(),
            test_chat_allowlist: Vec::new(),
            probe_updates: false,
            bot_token: None,
            extra: BTreeMap::new(),
        }
    }
}

fn default_telegram_api_base_url() -> String {
    "https://api.telegram.org".to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LarkAdapterConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_lark_api_base_url")]
    pub api_base_url: String,
    #[serde(default)]
    pub test_target_allowlist: Vec<String>,
    #[serde(default, skip_serializing)]
    pub app_id: Option<String>,
    #[serde(default, skip_serializing)]
    pub app_secret: Option<String>,
    #[serde(default, skip_serializing)]
    pub outbound_webhook_url: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Default for LarkAdapterConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            api_base_url: default_lark_api_base_url(),
            test_target_allowlist: Vec::new(),
            app_id: None,
            app_secret: None,
            outbound_webhook_url: None,
            extra: BTreeMap::new(),
        }
    }
}

fn default_lark_api_base_url() -> String {
    "https://open.larksuite.com".to_string()
}

impl GatewayPersistence {
    pub fn validate_layout(&self) -> Result<(), String> {
        validate_child("runs_dir", &self.runs_dir, &self.gateway_dir)?;
        validate_child("adapters_dir", &self.adapters_dir, &self.gateway_dir)?;
        validate_child("webhooks_dir", &self.webhooks_dir, &self.gateway_dir)?;
        Ok(())
    }
}

fn validate_child(
    label: &str,
    child: &std::path::Path,
    parent: &std::path::Path,
) -> Result<(), String> {
    if child.starts_with(parent) {
        Ok(())
    } else {
        Err(format!(
            "{label} must stay under gateway_dir; child={}, gateway_dir={}",
            child.display(),
            parent.display()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALLTHECODES_HOME_ENV: &str = "ALLTHECODES_HOME";

    struct EnvGuard {
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(value: &str) -> Self {
            let previous = std::env::var(ALLTHECODES_HOME_ENV).ok();
            std::env::set_var(ALLTHECODES_HOME_ENV, value);
            Self { previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var(ALLTHECODES_HOME_ENV, value),
                None => std::env::remove_var(ALLTHECODES_HOME_ENV),
            }
        }
    }

    #[test]
    fn config_defaults_use_gateway_paths_under_allthecodes_home() {
        let tmp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set(tmp.path().to_str().unwrap());

        let config = GatewayConfig::default();
        assert_eq!(config.persistence.gateway_dir, tmp.path().join("gateway"));
        assert_eq!(
            config.persistence.runs_dir,
            tmp.path().join("gateway").join("runs")
        );
        assert_eq!(
            config.persistence.adapters_dir,
            tmp.path().join("gateway").join("adapters")
        );
        assert_eq!(
            config.persistence.webhooks_dir,
            tmp.path().join("gateway").join("webhooks")
        );
        assert_eq!(
            GatewayConfig::default_config_path(),
            tmp.path().join("gateway").join("config.json")
        );
        assert_eq!(
            GatewayConfig::tokens_path(),
            tmp.path().join("gateway").join("tokens.json")
        );
    }

    #[test]
    fn config_serializes_in_camel_case() {
        let json = serde_json::to_value(GatewayConfig::default()).unwrap();
        assert!(json.get("enabled").is_some());
        assert!(json.get("maxPayloadBytes").is_none());
        assert!(json
            .get("limits")
            .and_then(|limits| limits.get("maxPayloadBytes"))
            .is_some());
    }

    #[test]
    fn config_does_not_serialize_telegram_token() {
        let mut config = GatewayConfig::default();
        config.adapters.telegram.bot_token = Some("123456:raw-secret-token".to_string());

        let json = serde_json::to_string(&config).unwrap();
        assert!(!json.contains("raw-secret-token"));
        assert!(!json.contains("123456:"));
    }

    #[test]
    fn config_does_not_serialize_lark_credentials() {
        let mut config = GatewayConfig::default();
        config.adapters.lark.app_id = Some("cli_a_secret_app".to_string());
        config.adapters.lark.app_secret = Some("raw-lark-secret".to_string());
        config.adapters.lark.outbound_webhook_url =
            Some("https://open.larksuite.com/open-apis/bot/v2/hook/raw-webhook-secret".to_string());

        let json = serde_json::to_string(&config).unwrap();
        assert!(!json.contains("cli_a_secret_app"));
        assert!(!json.contains("raw-lark-secret"));
        assert!(!json.contains("raw-webhook-secret"));
    }

    #[test]
    fn config_does_not_serialize_remote_token() {
        let mut config = GatewayConfig::default();
        config.security.remote_token = Some("raw-remote-token".to_string());

        let json = serde_json::to_string(&config).unwrap();

        assert!(!json.contains("raw-remote-token"));
    }

    #[test]
    fn config_round_trip_preserves_unknown_fields_and_persists_secrets_separately() {
        let tmp = tempfile::tempdir().unwrap();
        let config_path = tmp.path().join("gateway").join("config.json");
        std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        std::fs::write(
            &config_path,
            serde_json::json!({
                "enabled": false,
                "unknownGatewaySetting": {"kept": true},
                "adapters": {
                    "unknownAdaptersSetting": {"kept": true},
                    "telegram": {"enabled": true, "unknownAdapterSetting": "kept"}
                }
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            config_path.with_file_name("tokens.json"),
            serde_json::json!({"telegramBotToken": "test-only-not-a-real-credential"}).to_string(),
        )
        .unwrap();

        let mut config = GatewayConfig::load_from(&config_path).unwrap();
        assert_eq!(
            config.adapters.telegram.bot_token.as_deref(),
            Some("test-only-not-a-real-credential")
        );
        config.security.remote_token = Some("test-only-remote-token".to_string());
        config.adapters.telegram.enabled = false;
        config.save_to(&config_path).unwrap();

        let saved_config: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&config_path).unwrap()).unwrap();
        assert_eq!(saved_config["unknownGatewaySetting"]["kept"], true);
        assert_eq!(
            saved_config["adapters"]["unknownAdaptersSetting"]["kept"],
            true
        );
        assert_eq!(
            saved_config["adapters"]["telegram"]["unknownAdapterSetting"],
            "kept"
        );
        assert!(!saved_config
            .to_string()
            .contains("test-only-not-a-real-credential"));

        let saved_tokens: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(config_path.with_file_name("tokens.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            saved_tokens["telegramBotToken"],
            "test-only-not-a-real-credential"
        );
        assert_eq!(saved_tokens["remoteToken"], "test-only-remote-token");
        assert_eq!(
            GatewayConfig::load_from(&config_path)
                .unwrap()
                .security
                .remote_token
                .as_deref(),
            Some("test-only-remote-token")
        );
    }

    #[cfg(unix)]
    #[test]
    fn config_save_does_not_follow_precreated_temporary_symlink() {
        use std::os::unix::fs::symlink;

        let tmp = tempfile::tempdir().unwrap();
        let config_path = tmp.path().join("gateway").join("config.json");
        std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        let victim = tmp.path().join("victim.txt");
        std::fs::write(&victim, b"unchanged").unwrap();
        symlink(&victim, config_path.with_extension("json.tmp")).unwrap();

        GatewayConfig::default().save_to(&config_path).unwrap();

        assert_eq!(std::fs::read(&victim).unwrap(), b"unchanged");
    }

    #[test]
    fn channel_patch_distinguishes_omitted_secret_from_explicit_clear() {
        let omitted: GatewayChannelConfigPatch =
            serde_json::from_value(serde_json::json!({})).unwrap();
        assert!(matches!(omitted.bot_token, PatchField::Missing));

        let cleared: GatewayChannelConfigPatch =
            serde_json::from_value(serde_json::json!({"botToken": null})).unwrap();
        assert!(matches!(cleared.bot_token, PatchField::Clear));
    }

    #[test]
    fn channel_cannot_be_enabled_without_required_credentials() {
        let mut config = GatewayConfig::default();
        let enable_only: GatewayChannelConfigPatch =
            serde_json::from_value(serde_json::json!({"enabled": true})).unwrap();

        let error = config
            .update_channel(AdapterProvider::Telegram, enable_only)
            .unwrap_err();

        assert_eq!(error.diagnostic().code, "channel_not_configured");
        assert!(!config.adapters.telegram.enabled);

        let configure_and_enable: GatewayChannelConfigPatch =
            serde_json::from_value(serde_json::json!({
                "enabled": true,
                "botToken": "test-only-not-a-real-credential"
            }))
            .unwrap();
        config
            .update_channel(AdapterProvider::Telegram, configure_and_enable)
            .unwrap();
        assert!(config.adapters.telegram.enabled);

        let clear_required_credential: GatewayChannelConfigPatch =
            serde_json::from_value(serde_json::json!({"botToken": null})).unwrap();
        config
            .update_channel(AdapterProvider::Telegram, clear_required_credential)
            .unwrap();
        assert!(!config.adapters.telegram.enabled);
        assert!(config.adapters.telegram.bot_token.is_none());
    }
}
