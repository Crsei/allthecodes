pub(super) use super::*;
pub(super) use crate::api::providers::AnthropicEndpointKind;
pub(super) use crate::api::streaming::StreamAccumulator;
pub(super) use allthecodes_types::message::StreamEvent;
pub(super) use anyhow::Result;
pub(super) use futures::Stream;
pub(super) use std::collections::HashMap;
pub(super) use std::pin::Pin;
pub(super) use std::sync::atomic::{AtomicUsize, Ordering};
pub(super) use std::sync::{Arc, Mutex, OnceLock};
pub(super) use std::time::Duration;

pub(super) static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
pub(super) static TEST_KEYCHAIN: OnceLock<Mutex<HashMap<(String, String), Vec<u8>>>> =
    OnceLock::new();
pub(super) const ANTHROPIC_MODEL_ENV_KEYS: &[&str] = &[
    "ANTHROPIC_MODEL",
    ANTHROPIC_DEFAULT_SOTA_MODEL_ENV,
    ANTHROPIC_DEFAULT_MOTA_MODEL_ENV,
    ANTHROPIC_DEFAULT_FOTA_MODEL_ENV,
    ANTHROPIC_DEFAULT_OPUS_MODEL_ENV,
    ANTHROPIC_DEFAULT_SONNET_MODEL_ENV,
    ANTHROPIC_DEFAULT_HAIKU_MODEL_ENV,
];

pub(super) fn anthropic_config() -> ApiClientConfig {
    ApiClientConfig {
        provider: ApiProvider::Anthropic {
            auth: AnthropicAuth::ApiKey("sk-test-key-123".to_string()),
            base_url: None,
            endpoint_kind: AnthropicEndpointKind::DirectAnthropic,
        },
        default_model: "claude-sonnet-4-20250514".to_string(),
        max_retries: 3,
        timeout_secs: 60,
    }
}

pub(super) fn anthropic_config_custom_url() -> ApiClientConfig {
    ApiClientConfig {
        provider: ApiProvider::Anthropic {
            auth: AnthropicAuth::ApiKey("sk-test-key-456".to_string()),
            base_url: Some("https://custom.api.example.com".to_string()),
            endpoint_kind: AnthropicEndpointKind::CompatibleAnthropic,
        },
        default_model: "claude-sonnet-4-20250514".to_string(),
        max_retries: 2,
        timeout_secs: 30,
    }
}

pub(super) fn save_env(keys: &'static [&'static str]) -> Vec<(&'static str, Option<String>)> {
    keys.iter()
        .map(|key| (*key, std::env::var(key).ok()))
        .collect()
}

pub(super) fn restore_env(saved: Vec<(&'static str, Option<String>)>) {
    for (key, value) in saved {
        match value {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }
}

pub(super) fn clear_env(keys: &[&str]) {
    for key in keys {
        std::env::remove_var(key);
    }
}

pub(super) struct CwdGuard(std::path::PathBuf);

impl CwdGuard {
    pub(super) fn set(path: &std::path::Path) -> Self {
        let previous = std::env::current_dir().expect("current dir");
        std::env::set_current_dir(path).expect("set current dir");
        Self(previous)
    }
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.0);
    }
}

pub(super) fn fixture_json(name: &str) -> serde_json::Value {
    let raw = match name {
        "auth_header_expected" => AUTH_HEADER_EXPECTED,
        "base_url_expected" => BASE_URL_EXPECTED,
        "model_alias_expected" => MODEL_ALIAS_EXPECTED,
        "prompt_cache_body_expected" => PROMPT_CACHE_BODY_EXPECTED,
        other => panic!("unknown fixture: {other}"),
    };
    serde_json::from_str(raw).expect("fixture must be valid JSON")
}

pub(super) const AUTH_HEADER_EXPECTED: &str = r#"{
  "authorization": "Bearer anthropic-compatible-token",
  "absent": ["x-api-key"]
}"#;

pub(super) const BASE_URL_EXPECTED: &str = r#"{
  "base_url": "https://compatible.example.com",
  "messages_url": "https://compatible.example.com/v1/messages"
}"#;

pub(super) const MODEL_ALIAS_EXPECTED: &str = r#"{
  "alias": "MOTA",
  "wire_model": "claude-sonnet-4-6"
}"#;

pub(super) const PROMPT_CACHE_BODY_EXPECTED: &str = r#"{
  "model": "claude-sonnet-4-5-20250929",
  "messages": [
    {
      "role": "user",
      "content": "Hello"
    }
  ],
  "system": [
    {
      "type": "text",
      "text": "You are a coding assistant.",
      "cache_control": {
        "type": "ephemeral"
      }
    }
  ],
  "max_tokens": 1024,
  "tools": null,
  "stream": true
}"#;

pub(super) const STREAM_ERROR_EVENT_SSE: &str = r#"event: error
data: {"type":"error","status":529,"request_id":"req_sse","error":{"type":"overloaded_error","message":"Overloaded"}}

"#;

pub(super) fn save_and_clear_provider_keys() -> Vec<(&'static str, String)> {
    let saved: Vec<_> = crate::api::providers::PROVIDERS
        .iter()
        .filter_map(|p| std::env::var(p.env_key).ok().map(|v| (p.env_key, v)))
        .collect();
    for p in crate::api::providers::PROVIDERS {
        std::env::remove_var(p.env_key);
    }
    saved
}

pub(super) fn restore_provider_keys(saved: Vec<(&'static str, String)>) {
    for (key, value) in saved {
        std::env::set_var(key, value);
    }
}

#[derive(Debug)]
pub(super) struct PersistentTestCredential {
    service: String,
    user: String,
}

impl keyring::credential::CredentialApi for PersistentTestCredential {
    fn set_secret(&self, secret: &[u8]) -> keyring::Result<()> {
        TEST_KEYCHAIN
            .get_or_init(Default::default)
            .lock()
            .expect("test keychain poisoned")
            .insert((self.service.clone(), self.user.clone()), secret.to_vec());
        Ok(())
    }

    fn get_secret(&self) -> keyring::Result<Vec<u8>> {
        TEST_KEYCHAIN
            .get_or_init(Default::default)
            .lock()
            .expect("test keychain poisoned")
            .get(&(self.service.clone(), self.user.clone()))
            .cloned()
            .ok_or(keyring::Error::NoEntry)
    }

    fn delete_credential(&self) -> keyring::Result<()> {
        TEST_KEYCHAIN
            .get_or_init(Default::default)
            .lock()
            .expect("test keychain poisoned")
            .remove(&(self.service.clone(), self.user.clone()))
            .map(|_| ())
            .ok_or(keyring::Error::NoEntry)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

pub(super) struct PersistentTestCredentialBuilder;

impl keyring::credential::CredentialBuilderApi for PersistentTestCredentialBuilder {
    fn build(
        &self,
        _target: Option<&str>,
        service: &str,
        user: &str,
    ) -> keyring::Result<Box<keyring::Credential>> {
        Ok(Box::new(PersistentTestCredential {
            service: service.to_string(),
            user: user.to_string(),
        }))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn persistence(&self) -> keyring::credential::CredentialPersistence {
        keyring::credential::CredentialPersistence::ProcessOnly
    }
}

pub(super) fn use_persistent_test_keyring() {
    TEST_KEYCHAIN
        .get_or_init(Default::default)
        .lock()
        .expect("test keychain poisoned")
        .clear();
    keyring::set_default_credential_builder(Box::new(PersistentTestCredentialBuilder));
}

mod auth_env;
mod headers;
mod model_alias;
mod request_serialization;
mod sse_parsing;
mod streaming_retry;
mod url_building;
