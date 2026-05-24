use allthecodes_config::paths;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
    pub persistence: GatewayPersistence,
    pub limits: GatewayLimits,
    #[serde(default)]
    pub security: GatewaySecurityConfig,
    #[serde(default)]
    pub adapters: GatewayAdaptersConfig,
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GatewayAdaptersConfig {
    #[serde(default)]
    pub lark: LarkAdapterConfig,
    #[serde(default)]
    pub telegram: TelegramAdapterConfig,
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
}

impl Default for TelegramAdapterConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            api_base_url: default_telegram_api_base_url(),
            test_chat_allowlist: Vec::new(),
            probe_updates: false,
            bot_token: None,
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

    struct EnvGuard {
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(value: &str) -> Self {
            let previous = std::env::var("CC_RUST_HOME").ok();
            std::env::set_var("CC_RUST_HOME", value);
            Self { previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var("CC_RUST_HOME", value),
                None => std::env::remove_var("CC_RUST_HOME"),
            }
        }
    }

    #[test]
    fn config_defaults_use_gateway_paths_under_cc_rust_home() {
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
}
