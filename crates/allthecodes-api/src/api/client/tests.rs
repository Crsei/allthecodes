use super::*;
use crate::api::providers::AnthropicEndpointKind;
use crate::api::streaming::StreamAccumulator;
use allthecodes_types::message::StreamEvent;
use anyhow::Result;
use futures::Stream;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
static TEST_KEYCHAIN: OnceLock<Mutex<HashMap<(String, String), Vec<u8>>>> = OnceLock::new();
const ANTHROPIC_MODEL_ENV_KEYS: &[&str] = &[
    "ANTHROPIC_MODEL",
    ANTHROPIC_DEFAULT_SOTA_MODEL_ENV,
    ANTHROPIC_DEFAULT_MOTA_MODEL_ENV,
    ANTHROPIC_DEFAULT_FOTA_MODEL_ENV,
    ANTHROPIC_DEFAULT_OPUS_MODEL_ENV,
    ANTHROPIC_DEFAULT_SONNET_MODEL_ENV,
    ANTHROPIC_DEFAULT_HAIKU_MODEL_ENV,
];

fn anthropic_config() -> ApiClientConfig {
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

fn anthropic_config_custom_url() -> ApiClientConfig {
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

fn save_env(keys: &'static [&'static str]) -> Vec<(&'static str, Option<String>)> {
    keys.iter()
        .map(|key| (*key, std::env::var(key).ok()))
        .collect()
}

fn restore_env(saved: Vec<(&'static str, Option<String>)>) {
    for (key, value) in saved {
        match value {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }
}

fn clear_env(keys: &[&str]) {
    for key in keys {
        std::env::remove_var(key);
    }
}

struct CwdGuard(std::path::PathBuf);

impl CwdGuard {
    fn set(path: &std::path::Path) -> Self {
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

fn fixture_json(name: &str) -> serde_json::Value {
    let raw = match name {
        "auth_header_expected" => AUTH_HEADER_EXPECTED,
        "base_url_expected" => BASE_URL_EXPECTED,
        "model_alias_expected" => MODEL_ALIAS_EXPECTED,
        "prompt_cache_body_expected" => PROMPT_CACHE_BODY_EXPECTED,
        other => panic!("unknown fixture: {other}"),
    };
    serde_json::from_str(raw).expect("fixture must be valid JSON")
}

const AUTH_HEADER_EXPECTED: &str = r#"{
  "authorization": "Bearer anthropic-compatible-token",
  "absent": ["x-api-key"]
}"#;

const BASE_URL_EXPECTED: &str = r#"{
  "base_url": "https://compatible.example.com",
  "messages_url": "https://compatible.example.com/v1/messages"
}"#;

const MODEL_ALIAS_EXPECTED: &str = r#"{
  "alias": "MOTA",
  "wire_model": "claude-sonnet-4-6"
}"#;

const PROMPT_CACHE_BODY_EXPECTED: &str = r#"{
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

const STREAM_ERROR_EVENT_SSE: &str = r#"event: error
data: {"type":"error","status":529,"request_id":"req_sse","error":{"type":"overloaded_error","message":"Overloaded"}}

"#;

fn save_and_clear_provider_keys() -> Vec<(&'static str, String)> {
    let saved: Vec<_> = crate::api::providers::PROVIDERS
        .iter()
        .filter_map(|p| std::env::var(p.env_key).ok().map(|v| (p.env_key, v)))
        .collect();
    for p in crate::api::providers::PROVIDERS {
        std::env::remove_var(p.env_key);
    }
    saved
}

fn restore_provider_keys(saved: Vec<(&'static str, String)>) {
    for (key, value) in saved {
        std::env::set_var(key, value);
    }
}

#[derive(Debug)]
struct PersistentTestCredential {
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

struct PersistentTestCredentialBuilder;

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

fn use_persistent_test_keyring() {
    TEST_KEYCHAIN
        .get_or_init(Default::default)
        .lock()
        .expect("test keychain poisoned")
        .clear();
    keyring::set_default_credential_builder(Box::new(PersistentTestCredentialBuilder));
}

// -----------------------------------------------------------------------
// URL building
// -----------------------------------------------------------------------

#[test]
fn test_build_url_anthropic() {
    let client = ApiClient::new(anthropic_config());
    let url = client.build_url();
    assert_eq!(url, "https://api.anthropic.com/v1/messages");
}

#[test]
fn test_build_url_anthropic_custom_base() {
    let client = ApiClient::new(anthropic_config_custom_url());
    let url = client.build_url();
    assert_eq!(url, "https://custom.api.example.com/v1/messages");
}

#[test]
fn test_build_url_anthropic_trailing_slash() {
    let config = ApiClientConfig {
        provider: ApiProvider::Anthropic {
            auth: AnthropicAuth::ApiKey("key".to_string()),
            base_url: Some("https://example.com/".to_string()),
            endpoint_kind: AnthropicEndpointKind::CompatibleAnthropic,
        },
        default_model: "model".to_string(),
        max_retries: 1,
        timeout_secs: 30,
    };
    let client = ApiClient::new(config);
    let url = client.build_url();
    assert_eq!(url, "https://example.com/v1/messages");
}

#[test]
fn test_build_url_bedrock_returns_aws_endpoint() {
    let config = ApiClientConfig {
        provider: ApiProvider::Bedrock {
            region: "us-east-1".to_string(),
            auth: crate::api::bedrock::BedrockAuth::BearerToken("dummy".to_string()),
            base_url_override: None,
        },
        default_model: "claude-sonnet-4-5-20250929".to_string(),
        max_retries: 3,
        timeout_secs: 60,
    };
    let client = ApiClient::new(config);
    let url = client.build_url();
    assert!(
        url.starts_with("https://bedrock-runtime.us-east-1.amazonaws.com/model/"),
        "unexpected URL: {url}"
    );
    assert!(
        url.ends_with("/invoke-with-response-stream"),
        "unexpected URL: {url}"
    );
    // Default model gets translated to its Bedrock ID.
    assert!(
        url.contains("us.anthropic.claude-sonnet-4-5-20250929-v1"),
        "URL missing translated Bedrock model: {url}"
    );
}

#[test]
fn test_build_url_bedrock_with_override() {
    let config = ApiClientConfig {
        provider: ApiProvider::Bedrock {
            region: "us-east-1".to_string(),
            auth: crate::api::bedrock::BedrockAuth::BearerToken("dummy".to_string()),
            base_url_override: Some("https://proxy.example.com".to_string()),
        },
        default_model: "claude-sonnet-4-5-20250929".to_string(),
        max_retries: 3,
        timeout_secs: 60,
    };
    let client = ApiClient::new(config);
    let url = client.build_url();
    assert!(
        url.starts_with("https://proxy.example.com/model/"),
        "override should be used, got: {url}"
    );
    assert!(
        url.ends_with("/invoke-with-response-stream"),
        "unexpected URL: {url}"
    );
}

#[test]
fn test_build_url_vertex_returns_streamrawpredict() {
    let config = ApiClientConfig {
        provider: ApiProvider::Vertex {
            project_id: "my-project".to_string(),
            region: "us-east5".to_string(),
            access_token: crate::api::vertex::VertexAccessToken("dummy".to_string()),
        },
        default_model: "claude-sonnet-4-5-20250929".to_string(),
        max_retries: 3,
        timeout_secs: 60,
    };
    let client = ApiClient::new(config);
    let url = client.build_url();
    assert_eq!(
        url,
        "https://us-east5-aiplatform.googleapis.com/v1/projects/my-project/locations/us-east5/publishers/anthropic/models/claude-sonnet-4-5@20250929:streamRawPredict"
    );
}

#[test]
fn test_build_url_vertex_uses_per_model_region_override() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    let saved = save_env(&["VERTEX_REGION_CLAUDE_HAIKU_4_5"]);
    std::env::set_var("VERTEX_REGION_CLAUDE_HAIKU_4_5", "us-central1");

    let config = ApiClientConfig {
        provider: ApiProvider::Vertex {
            project_id: "my-project".to_string(),
            region: "us-east5".to_string(),
            access_token: crate::api::vertex::VertexAccessToken("dummy".to_string()),
        },
        default_model: "claude-haiku-4-5-20251001".to_string(),
        max_retries: 3,
        timeout_secs: 60,
    };
    let client = ApiClient::new(config);
    let url = client.build_url();
    assert_eq!(
        url,
        "https://us-central1-aiplatform.googleapis.com/v1/projects/my-project/locations/us-central1/publishers/anthropic/models/claude-haiku-4-5@20251001:streamRawPredict"
    );

    restore_env(saved);
}

#[test]
fn test_build_url_azure() {
    let config = ApiClientConfig {
        provider: ApiProvider::Azure {
            endpoint: "https://my-azure-endpoint.com".to_string(),
            api_key: "az-key".to_string(),
        },
        default_model: "model".to_string(),
        max_retries: 3,
        timeout_secs: 60,
    };
    let client = ApiClient::new(config);
    let url = client.build_url();
    assert_eq!(url, "https://my-azure-endpoint.com/v1/messages");
}

#[test]
fn test_build_url_openai_compat() {
    let config = ApiClientConfig {
        provider: ApiProvider::OpenAiCompat {
            name: "deepseek".to_string(),
            api_key: "sk-test".to_string(),
            base_url: "https://api.deepseek.com/v1".to_string(),
            default_model: "deepseek-chat".to_string(),
        },
        default_model: "deepseek-chat".to_string(),
        max_retries: 3,
        timeout_secs: 60,
    };
    let client = ApiClient::new(config);
    let url = client.build_url();
    assert_eq!(url, "https://api.deepseek.com/v1/chat/completions");
}

#[test]
fn test_build_url_openai_compat_trailing_slash() {
    let config = ApiClientConfig {
        provider: ApiProvider::OpenAiCompat {
            name: "openai".to_string(),
            api_key: "sk-test".to_string(),
            base_url: "https://api.openai.com/v1/".to_string(),
            default_model: "gpt-4o".to_string(),
        },
        default_model: "gpt-4o".to_string(),
        max_retries: 3,
        timeout_secs: 60,
    };
    let client = ApiClient::new(config);
    let url = client.build_url();
    assert_eq!(url, "https://api.openai.com/v1/chat/completions");
}

#[test]
fn test_build_url_openai_codex() {
    let config = ApiClientConfig {
        provider: ApiProvider::OpenAiCompat {
            name: OPENAI_CODEX_PROVIDER_NAME.to_string(),
            api_key: "token-test".to_string(),
            base_url: "https://chatgpt.com/backend-api/".to_string(),
            default_model: "gpt-5.4".to_string(),
        },
        default_model: "gpt-5.4".to_string(),
        max_retries: 3,
        timeout_secs: 60,
    };
    let client = ApiClient::new(config);
    let url = client.build_url();
    assert_eq!(url, "https://chatgpt.com/backend-api/codex/responses");
}

// -----------------------------------------------------------------------
// Header building
// -----------------------------------------------------------------------

#[test]
fn test_build_headers_has_required() {
    let client = ApiClient::new(anthropic_config());
    let headers = client.build_headers_map();

    assert_eq!(headers.get("content-type").unwrap(), "application/json");
    assert_eq!(
        headers.get("user-agent").unwrap(),
        &allthecodes_config::user_agent::api_user_agent()
    );
    assert_eq!(headers.get("anthropic-version").unwrap(), "2023-06-01");
    assert_eq!(headers.get("x-api-key").unwrap(), "sk-test-key-123");
    assert!(headers
        .get("anthropic-beta")
        .unwrap()
        .contains("interleaved-thinking"));
    assert!(headers
        .get("anthropic-beta")
        .unwrap()
        .contains("prompt-caching"));
}

#[test]
fn test_build_headers_raw_header_map_has_required() {
    let client = ApiClient::new(anthropic_config());
    let headers = client.build_headers();

    assert_eq!(
        headers
            .get(reqwest::header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap(),
        "application/json"
    );
    assert_eq!(
        headers.get("anthropic-version").unwrap().to_str().unwrap(),
        "2023-06-01"
    );
    assert_eq!(
        headers
            .get(reqwest::header::USER_AGENT)
            .unwrap()
            .to_str()
            .unwrap(),
        allthecodes_config::user_agent::api_user_agent()
    );
    assert_eq!(
        headers.get("x-api-key").unwrap().to_str().unwrap(),
        "sk-test-key-123"
    );
}

#[test]
fn compatible_anthropic_headers_omit_beta_extensions() {
    let body = serde_json::json!({
        "model": "deepseek-v4-pro",
        "system": [{"type": "text", "text": "sys", "cache_control": {"type": "ephemeral"}}],
        "messages": [{"role": "user", "content": "hello"}],
    });
    let headers = build_anthropic_headers_for_body_with_beta_policy(
        &AnthropicAuth::BearerToken("compatible-token".to_string()),
        true,
        &body,
        false,
    )
    .expect("headers build");

    assert_eq!(
        headers.get("anthropic-version").unwrap().to_str().unwrap(),
        "2023-06-01"
    );
    assert_eq!(
        headers.get("Authorization").unwrap().to_str().unwrap(),
        "Bearer compatible-token"
    );
    assert!(!headers.contains_key("anthropic-beta"));
}

#[test]
fn anthropic_headers_include_effort_beta_for_output_config_effort() {
    let body = serde_json::json!({
        "model": "claude-sonnet-4-20250514",
        "messages": [{"role": "user", "content": "hello"}],
        "output_config": {"effort": "high"}
    });
    let headers = build_anthropic_headers_for_body(
        &AnthropicAuth::ApiKey("sk-test-key-123".to_string()),
        false,
        &body,
    )
    .expect("headers build");

    assert!(headers
        .get("anthropic-beta")
        .unwrap()
        .to_str()
        .unwrap()
        .contains(allthecodes_config::constants::api::EFFORT_BETA));
}

#[test]
fn test_build_headers_azure_has_api_key() {
    let config = ApiClientConfig {
        provider: ApiProvider::Azure {
            endpoint: "https://azure.example.com".to_string(),
            api_key: "az-secret".to_string(),
        },
        default_model: "model".to_string(),
        max_retries: 1,
        timeout_secs: 30,
    };
    let client = ApiClient::new(config);
    let headers = client.build_headers_map();
    assert_eq!(headers.get("x-api-key").unwrap(), "az-secret");
}

#[test]
fn test_build_headers_openai_compat_bearer() {
    let config = ApiClientConfig {
        provider: ApiProvider::OpenAiCompat {
            name: "openai".to_string(),
            api_key: "sk-my-key".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            default_model: "gpt-4o".to_string(),
        },
        default_model: "gpt-4o".to_string(),
        max_retries: 1,
        timeout_secs: 30,
    };
    let client = ApiClient::new(config);
    let headers = client.build_headers_map();
    assert_eq!(headers.get("Authorization").unwrap(), "Bearer sk-my-key");
    assert!(!headers.contains_key("x-api-key"));
    assert!(!headers.contains_key("anthropic-version"));
}

#[test]
fn test_build_headers_google_no_auth_header() {
    let config = ApiClientConfig {
        provider: ApiProvider::Google {
            api_key: "AIza-test".to_string(),
            base_url: "https://generativelanguage.googleapis.com/v1beta".to_string(),
        },
        default_model: "gemini-2.0-flash".to_string(),
        max_retries: 1,
        timeout_secs: 30,
    };
    let client = ApiClient::new(config);
    let headers = client.build_headers_map();
    assert_eq!(headers.get("content-type").unwrap(), "application/json");
    assert!(!headers.contains_key("x-api-key"));
    assert!(!headers.contains_key("Authorization"));
}

#[test]
fn test_build_headers_bedrock_no_api_key() {
    let config = ApiClientConfig {
        provider: ApiProvider::Bedrock {
            region: "us-east-1".to_string(),
            auth: crate::api::bedrock::BedrockAuth::BearerToken("dummy".to_string()),
            base_url_override: None,
        },
        default_model: "model".to_string(),
        max_retries: 1,
        timeout_secs: 30,
    };
    let client = ApiClient::new(config);
    let headers = client.build_headers_map();
    // Generic header map deliberately does not include Bedrock auth 鈥?the
    // Bedrock provider sets Bearer/SigV4 headers per-request in its stream
    // implementation.
    assert!(!headers.contains_key("x-api-key"));
    assert!(!headers.contains_key("authorization"));
    assert_eq!(headers.get("content-type").unwrap(), "application/json");
}

#[test]
fn regression_anthropic_auth_token_uses_authorization_bearer() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    let saved = save_env(&[
        "ANTHROPIC_AUTH_TOKEN",
        "ANTHROPIC_BASE_URL",
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    let saved_keys = save_and_clear_provider_keys();
    clear_env(&[
        "ANTHROPIC_BASE_URL",
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    std::env::set_var("ANTHROPIC_AUTH_TOKEN", "anthropic-compatible-token");

    let fixture = fixture_json("auth_header_expected");
    let client = ApiClient::from_auth_result()
        .expect("auth resolution should not error")
        .expect("auth token should build a client");
    let headers = client.build_headers_map();

    assert_eq!(
        headers.get("Authorization").map(String::as_str),
        fixture["authorization"].as_str()
    );
    for absent in fixture["absent"].as_array().unwrap() {
        assert!(!headers.contains_key(absent.as_str().unwrap()));
    }

    restore_provider_keys(saved_keys);
    restore_env(saved);
}

#[test]
fn regression_anthropic_base_url_env_currently_loses_compatible_routing() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    let saved = save_env(&[
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_MODEL",
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    let saved_keys = save_and_clear_provider_keys();
    clear_env(&[
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    let fixture = fixture_json("base_url_expected");
    std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-api03-compatible-routing-test");
    std::env::set_var("ANTHROPIC_BASE_URL", fixture["base_url"].as_str().unwrap());
    std::env::set_var("ANTHROPIC_MODEL", "compatible-explicit-model");

    let client = ApiClient::from_auth_result()
        .expect("auth resolution should not error")
        .expect("API key should build a client");

    // Phase 0 risk: env-provider detection currently routes through provider
    // metadata and drops ANTHROPIC_BASE_URL for compatible endpoints.
    assert_eq!(
        client.build_url(),
        fixture["messages_url"].as_str().unwrap()
    );
    assert_eq!(
        client.config().provider.endpoint_kind(),
        Some(AnthropicEndpointKind::CompatibleAnthropic)
    );

    restore_provider_keys(saved_keys);
    restore_env(saved);
}

#[test]
fn anthropic_auth_token_with_non_official_base_url_selects_compatible_messages_endpoint() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    let saved = save_env(&[
        "ANTHROPIC_AUTH_TOKEN",
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_MODEL",
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    let saved_keys = save_and_clear_provider_keys();
    clear_env(&[
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    let fixture = fixture_json("base_url_expected");
    std::env::set_var("ANTHROPIC_AUTH_TOKEN", "compatible-secret-token");
    std::env::set_var("ANTHROPIC_BASE_URL", fixture["base_url"].as_str().unwrap());
    std::env::set_var("ANTHROPIC_MODEL", "compatible-explicit-model");

    let client = ApiClient::from_auth_result()
        .expect("auth resolution should not error")
        .expect("auth token should build a client");

    assert_eq!(
        client.build_url(),
        fixture["messages_url"].as_str().unwrap()
    );
    assert_eq!(
        client.config().provider.endpoint_kind(),
        Some(AnthropicEndpointKind::CompatibleAnthropic)
    );
    assert!(matches!(
        client.config().provider,
        ApiProvider::Anthropic {
            auth: AnthropicAuth::BearerToken(_),
            ..
        }
    ));
    let headers = client.build_headers_map();
    assert_eq!(
        headers.get("Authorization").map(String::as_str),
        Some("Bearer compatible-secret-token")
    );
    assert!(!headers.contains_key("x-api-key"));

    restore_provider_keys(saved_keys);
    restore_env(saved);
}

#[test]
fn provider_diagnostic_includes_endpoint_kind_and_host_without_secret() {
    let config = ApiClientConfig {
        provider: ApiProvider::Anthropic {
            auth: AnthropicAuth::BearerToken("secret-token-must-not-leak".to_string()),
            base_url: Some("https://compatible.example.com/anthropic".to_string()),
            endpoint_kind: AnthropicEndpointKind::CompatibleAnthropic,
        },
        default_model: "claude-sonnet-4-20250514".to_string(),
        max_retries: 1,
        timeout_secs: 30,
    };
    let client = ApiClient::new(config);
    let diagnostic = client.provider_diagnostic();

    assert_eq!(
        diagnostic.endpoint_kind,
        Some(AnthropicEndpointKind::CompatibleAnthropic)
    );
    assert_eq!(
        diagnostic.base_url_host.as_deref(),
        Some("compatible.example.com")
    );
    let serialized = serde_json::to_string(&diagnostic).unwrap();
    assert!(!serialized.contains("secret-token-must-not-leak"));
}

#[test]
fn regression_anthropic_model_alias_currently_not_resolved_for_wire_model() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    let saved = save_env(&[
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_MODEL",
        ANTHROPIC_DEFAULT_SOTA_MODEL_ENV,
        ANTHROPIC_DEFAULT_MOTA_MODEL_ENV,
        ANTHROPIC_DEFAULT_FOTA_MODEL_ENV,
        ANTHROPIC_DEFAULT_OPUS_MODEL_ENV,
        ANTHROPIC_DEFAULT_SONNET_MODEL_ENV,
        ANTHROPIC_DEFAULT_HAIKU_MODEL_ENV,
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    let saved_keys = save_and_clear_provider_keys();
    clear_env(&[
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
        "ANTHROPIC_BASE_URL",
        ANTHROPIC_DEFAULT_SOTA_MODEL_ENV,
        ANTHROPIC_DEFAULT_MOTA_MODEL_ENV,
        ANTHROPIC_DEFAULT_FOTA_MODEL_ENV,
        ANTHROPIC_DEFAULT_OPUS_MODEL_ENV,
        ANTHROPIC_DEFAULT_SONNET_MODEL_ENV,
        ANTHROPIC_DEFAULT_HAIKU_MODEL_ENV,
    ]);
    let fixture = fixture_json("model_alias_expected");
    std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-api03-model-alias-test");
    std::env::set_var("ANTHROPIC_MODEL", fixture["alias"].as_str().unwrap());

    let client = ApiClient::from_auth_result()
        .expect("auth resolution should not error")
        .expect("API key should build a client");

    assert_eq!(
        client.config().default_model,
        fixture["wire_model"].as_str().unwrap()
    );

    restore_provider_keys(saved_keys);
    restore_env(saved);
}

#[test]
fn anthropic_alias_uses_new_default_model_env_before_official_fallback() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    let saved = save_env(&[
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_MODEL",
        ANTHROPIC_DEFAULT_MOTA_MODEL_ENV,
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    let saved_keys = save_and_clear_provider_keys();
    clear_env(&[
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-api03-model-env-test");
    std::env::set_var("ANTHROPIC_MODEL", "MOTA");
    std::env::set_var(ANTHROPIC_DEFAULT_MOTA_MODEL_ENV, "compatible-sonnet-model");

    let client = ApiClient::from_auth_result()
        .expect("auth resolution should not error")
        .expect("API key should build a client");

    assert_eq!(client.config().default_model, "compatible-sonnet-model");

    restore_provider_keys(saved_keys);
    restore_env(saved);
}

#[test]
fn anthropic_alias_uses_legacy_default_model_env_as_fallback() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    let saved = save_env(&[
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_MODEL",
        ANTHROPIC_DEFAULT_MOTA_MODEL_ENV,
        ANTHROPIC_DEFAULT_SONNET_MODEL_ENV,
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    let saved_keys = save_and_clear_provider_keys();
    clear_env(&[
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
        ANTHROPIC_DEFAULT_MOTA_MODEL_ENV,
    ]);
    std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-api03-legacy-model-env-test");
    std::env::set_var("ANTHROPIC_MODEL", "MOTA");
    std::env::set_var(ANTHROPIC_DEFAULT_SONNET_MODEL_ENV, "legacy-sonnet-model");

    let client = ApiClient::from_auth_result()
        .expect("auth resolution should not error")
        .expect("API key should build a client");

    assert_eq!(client.config().default_model, "legacy-sonnet-model");

    restore_provider_keys(saved_keys);
    restore_env(saved);
}

#[test]
fn anthropic_compatible_alias_without_provider_default_is_config_error() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    let saved = save_env(&[
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_MODEL",
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    let saved_model_env = save_env(ANTHROPIC_MODEL_ENV_KEYS);
    let saved_keys = save_and_clear_provider_keys();
    clear_env(&[
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    clear_env(ANTHROPIC_MODEL_ENV_KEYS);
    std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-api03-compatible-model-error");
    std::env::set_var("ANTHROPIC_BASE_URL", "https://compatible.example.com");
    std::env::set_var("ANTHROPIC_MODEL", "FOTA");

    let err = match ApiClient::from_auth_result() {
        Err(error) => error,
        Ok(_) => panic!("compatible Anthropic alias without fallback must fail"),
    };
    let msg = err.to_string();
    assert!(msg.contains("Anthropic-compatible provider"));
    assert!(msg.contains(ANTHROPIC_DEFAULT_FOTA_MODEL_ENV));
    assert!(!msg.contains("claude-haiku-4-5-20251001"));

    restore_provider_keys(saved_keys);
    restore_env(saved_model_env);
    restore_env(saved);
}

#[test]
fn anthropic_compatible_explicit_model_id_wins_over_alias_defaults() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    let saved = save_env(&[
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_MODEL",
        ANTHROPIC_DEFAULT_MOTA_MODEL_ENV,
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    let saved_keys = save_and_clear_provider_keys();
    clear_env(&[
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-api03-compatible-explicit");
    std::env::set_var("ANTHROPIC_BASE_URL", "https://compatible.example.com");
    std::env::set_var("ANTHROPIC_MODEL", "provider-explicit-model");
    std::env::set_var(ANTHROPIC_DEFAULT_MOTA_MODEL_ENV, "provider-mota-model");

    let client = ApiClient::from_auth_result()
        .expect("auth resolution should not error")
        .expect("API key should build a client");

    assert_eq!(client.config().default_model, "provider-explicit-model");

    restore_provider_keys(saved_keys);
    restore_env(saved);
}

#[test]
fn settings_runtime_env_is_visible_to_anthropic_provider_detection() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    let saved = save_env(&[
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_MODEL",
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    let saved_keys = save_and_clear_provider_keys();
    clear_env(&[
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_MODEL",
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    let env = std::collections::HashMap::from([
        (
            "ANTHROPIC_API_KEY".to_string(),
            "sk-ant-api03-settings-runtime".to_string(),
        ),
        (
            "ANTHROPIC_BASE_URL".to_string(),
            "https://compatible.example.com".to_string(),
        ),
        ("ANTHROPIC_MODEL".to_string(), "deepseek-v4-pro".to_string()),
    ]);

    let report =
        allthecodes_config::settings::apply_runtime_env(&env).expect("settings env applies");
    let client = ApiClient::from_auth_result()
        .expect("auth resolution should not error")
        .expect("settings env should build a client");

    assert_eq!(report.applied, 3);
    assert_eq!(client.config().default_model, "deepseek-v4-pro");
    assert_eq!(
        client.config().provider.endpoint_kind(),
        Some(AnthropicEndpointKind::CompatibleAnthropic)
    );

    restore_provider_keys(saved_keys);
    restore_env(saved);
}

#[test]
fn startup_settings_env_overrides_inherited_anthropic_provider_env() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    let saved = save_env(&[
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_MODEL",
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    let saved_keys = save_and_clear_provider_keys();
    clear_env(&[
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-api03-old-process-key");
    std::env::set_var("ANTHROPIC_BASE_URL", "https://old.example.com");
    std::env::set_var("ANTHROPIC_MODEL", "claude-opus-4-20250514");
    let env = std::collections::HashMap::from([
        (
            "ANTHROPIC_API_KEY".to_string(),
            "sk-ant-api03-settings-runtime".to_string(),
        ),
        (
            "ANTHROPIC_BASE_URL".to_string(),
            "https://compatible.example.com".to_string(),
        ),
        ("ANTHROPIC_MODEL".to_string(), "deepseek-v4-pro".to_string()),
    ]);

    let report = allthecodes_config::settings::apply_startup_runtime_env(&env)
        .expect("settings env applies");
    let client = ApiClient::from_auth_result()
        .expect("auth resolution should not error")
        .expect("settings env should build a client");

    assert_eq!(report.overridden, 3);
    assert_eq!(client.config().default_model, "deepseek-v4-pro");
    assert_eq!(
        client.build_url(),
        "https://compatible.example.com/v1/messages"
    );

    restore_provider_keys(saved_keys);
    restore_env(saved);
}

#[test]
fn settings_runtime_env_supports_anthropic_legacy_model_alias_fallback() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    let saved = save_env(&[
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_MODEL",
        ANTHROPIC_DEFAULT_MOTA_MODEL_ENV,
        ANTHROPIC_DEFAULT_SONNET_MODEL_ENV,
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    let saved_keys = save_and_clear_provider_keys();
    clear_env(&[
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_MODEL",
        ANTHROPIC_DEFAULT_MOTA_MODEL_ENV,
        ANTHROPIC_DEFAULT_SONNET_MODEL_ENV,
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    let env = std::collections::HashMap::from([
        (
            "ANTHROPIC_API_KEY".to_string(),
            "sk-ant-api03-settings-runtime-alias".to_string(),
        ),
        (
            "ANTHROPIC_BASE_URL".to_string(),
            "https://compatible.example.com".to_string(),
        ),
        ("ANTHROPIC_MODEL".to_string(), "MOTA".to_string()),
        (
            ANTHROPIC_DEFAULT_SONNET_MODEL_ENV.to_string(),
            "deepseek-v4-pro".to_string(),
        ),
    ]);

    allthecodes_config::settings::apply_runtime_env(&env).expect("settings env applies");
    let client = ApiClient::from_auth_result()
        .expect("auth resolution should not error")
        .expect("settings env should build a client");

    assert_eq!(client.config().default_model, "deepseek-v4-pro");

    restore_provider_keys(saved_keys);
    restore_env(saved);
}

#[test]
fn settings_runtime_env_is_visible_to_codex_backend_auth() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    let saved = save_env(&[
        OPENAI_CODEX_TOKEN_ENV,
        OPENAI_CODEX_BASE_URL_ENV,
        OPENAI_CODEX_MODEL_ENV,
    ]);
    clear_env(&[
        OPENAI_CODEX_TOKEN_ENV,
        OPENAI_CODEX_BASE_URL_ENV,
        OPENAI_CODEX_MODEL_ENV,
    ]);
    let env = std::collections::HashMap::from([
        (
            OPENAI_CODEX_TOKEN_ENV.to_string(),
            "codex-settings-token".to_string(),
        ),
        (
            OPENAI_CODEX_BASE_URL_ENV.to_string(),
            "https://example.com/codex".to_string(),
        ),
        (
            OPENAI_CODEX_MODEL_ENV.to_string(),
            "gpt-5.3-codex-spark".to_string(),
        ),
    ]);

    allthecodes_config::settings::apply_runtime_env(&env).expect("settings env applies");
    let client = ApiClient::from_backend(Some("codex")).expect("codex settings env auth");

    match &client.config().provider {
        ApiProvider::OpenAiCompat {
            name,
            api_key,
            base_url,
            default_model,
        } => {
            assert_eq!(name, OPENAI_CODEX_PROVIDER_NAME);
            assert_eq!(api_key, "codex-settings-token");
            assert_eq!(base_url, "https://example.com/codex");
            assert_eq!(default_model, "gpt-5.3-codex-spark");
        }
        other => panic!("expected OpenAiCompat provider, got {:?}", other),
    }

    restore_env(saved);
}

#[test]
fn active_codex_profile_env_builds_codex_client() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    let temp = tempfile::tempdir().expect("tempdir");
    let saved = save_env(&[
        "ALLTHECODES_HOME",
        OPENAI_CODEX_TOKEN_ENV,
        OPENAI_CODEX_BASE_URL_ENV,
        OPENAI_CODEX_MODEL_ENV,
    ]);
    clear_env(&[
        OPENAI_CODEX_TOKEN_ENV,
        OPENAI_CODEX_BASE_URL_ENV,
        OPENAI_CODEX_MODEL_ENV,
    ]);
    std::env::set_var("ALLTHECODES_HOME", temp.path());
    allthecodes_config::settings::write_user_settings(&allthecodes_config::settings::RawSettings {
        active_auth_profile: Some("codex".to_string()),
        auth_profiles: Some(HashMap::from([(
            "codex".to_string(),
            allthecodes_config::settings::ProviderProfileSettings {
                backend: Some("codex".to_string()),
                api_provider: Some(
                    allthecodes_config::settings::API_PROVIDER_OPENAI_CODEX.to_string(),
                ),
                model: Some("gpt-5.4".to_string()),
                base_url: Some("https://example.com/codex/".to_string()),
                api_key: Some("codex-profile-token".to_string()),
                ..Default::default()
            },
        )])),
        ..Default::default()
    })
    .unwrap();
    let loaded = allthecodes_config::settings::load_effective(temp.path()).unwrap();
    allthecodes_config::settings::apply_startup_runtime_env(&loaded.effective.env)
        .expect("profile env applies");

    let client = ApiClient::from_backend(Some("codex")).expect("codex profile client");

    match &client.config().provider {
        ApiProvider::OpenAiCompat {
            name,
            api_key,
            base_url,
            default_model,
        } => {
            assert_eq!(name, OPENAI_CODEX_PROVIDER_NAME);
            assert_eq!(api_key, "codex-profile-token");
            assert_eq!(base_url, "https://example.com/codex");
            assert_eq!(default_model, "gpt-5.4");
        }
        other => panic!("expected OpenAiCompat provider, got {:?}", other),
    }

    restore_env(saved);
}

#[test]
fn active_custom_profile_env_builds_anthropic_compatible_client() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    let temp = tempfile::tempdir().expect("tempdir");
    let saved = save_env(&[
        "ALLTHECODES_HOME",
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_MODEL",
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    clear_env(&[
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_MODEL",
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    std::env::set_var("ALLTHECODES_HOME", temp.path());
    allthecodes_config::settings::write_user_settings(&allthecodes_config::settings::RawSettings {
        active_auth_profile: Some("custom".to_string()),
        auth_profiles: Some(HashMap::from([(
            "custom".to_string(),
            allthecodes_config::settings::ProviderProfileSettings {
                backend: Some("native".to_string()),
                api_provider: Some(
                    allthecodes_config::settings::API_PROVIDER_ANTHROPIC.to_string(),
                ),
                model: Some("deepseek-v4-pro".to_string()),
                base_url: Some("https://compatible.example.com/anthropic".to_string()),
                env: Some(HashMap::from([(
                    "ANTHROPIC_AUTH_TOKEN".to_string(),
                    "custom-profile-token".to_string(),
                )])),
                ..Default::default()
            },
        )])),
        ..Default::default()
    })
    .unwrap();
    let loaded = allthecodes_config::settings::load_effective(temp.path()).unwrap();
    allthecodes_config::settings::apply_startup_runtime_env(&loaded.effective.env)
        .expect("profile env applies");

    let client = ApiClient::from_auth_result()
        .expect("auth resolution should not error")
        .expect("custom profile client");

    match &client.config().provider {
        ApiProvider::Anthropic {
            auth,
            base_url,
            endpoint_kind,
        } => {
            assert_eq!(
                auth,
                &AnthropicAuth::BearerToken("custom-profile-token".to_string())
            );
            assert_eq!(
                base_url.as_deref(),
                Some("https://compatible.example.com/anthropic")
            );
            assert_eq!(endpoint_kind, &AnthropicEndpointKind::CompatibleAnthropic);
            assert_eq!(client.config().default_model, "deepseek-v4-pro");
        }
        other => panic!("expected Anthropic provider, got {:?}", other),
    }

    restore_env(saved);
}

// -----------------------------------------------------------------------
// from_provider_info
// -----------------------------------------------------------------------

#[test]
fn test_from_provider_info_anthropic() {
    use crate::api::providers::get_provider;
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    let saved = save_env(ANTHROPIC_MODEL_ENV_KEYS);
    clear_env(ANTHROPIC_MODEL_ENV_KEYS);
    let info = get_provider("anthropic").unwrap();
    let client = ApiClient::from_provider_info(info, "sk-test");
    assert!(matches!(
        client.config().provider,
        ApiProvider::Anthropic { .. }
    ));
    assert_eq!(client.config().default_model, "claude-sonnet-4-6");
    restore_env(saved);
}

#[test]
fn test_from_provider_info_deepseek() {
    use crate::api::providers::get_provider;
    let info = get_provider("deepseek").unwrap();
    let client = ApiClient::from_provider_info(info, "sk-ds-key");
    match &client.config().provider {
        ApiProvider::OpenAiCompat { name, base_url, .. } => {
            assert_eq!(name, "deepseek");
            assert_eq!(base_url, "https://api.deepseek.com/v1");
        }
        _ => panic!("expected OpenAiCompat"),
    }
}

#[test]
fn test_from_provider_info_google() {
    use crate::api::providers::get_provider;
    let info = get_provider("google").unwrap();
    let client = ApiClient::from_provider_info(info, "AIza-test");
    assert!(matches!(
        client.config().provider,
        ApiProvider::Google { .. }
    ));
    assert_eq!(client.config().default_model, "gemini-2.0-flash");
}

// -----------------------------------------------------------------------
// from_env / from_auth
// -----------------------------------------------------------------------

#[test]
fn test_from_env_with_anthropic_key() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    let saved_flags = save_env(&[
        "ANTHROPIC_BASE_URL",
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
    ]);
    clear_env(&[
        "ANTHROPIC_BASE_URL",
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
    ]);

    // Temporarily set the env var for this test
    let key = "sk-ant-api03-test-from-env-key";
    std::env::set_var("ANTHROPIC_API_KEY", key);

    let client = ApiClient::from_env();
    assert!(
        client.is_some(),
        "from_env should return Some when ANTHROPIC_API_KEY is set"
    );

    let client = client.unwrap();
    match &client.config().provider {
        ApiProvider::Anthropic {
            auth,
            endpoint_kind,
            ..
        } => {
            assert_eq!(auth, &AnthropicAuth::ApiKey(key.to_string()));
            assert_eq!(endpoint_kind, &AnthropicEndpointKind::DirectAnthropic);
        }
        other => panic!("expected Anthropic provider, got {:?}", other),
    }

    // Clean up
    std::env::remove_var("ANTHROPIC_API_KEY");
    restore_env(saved_flags);
}

#[test]
fn test_from_env_no_keys() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    let saved_flags = save_env(&["ALLTHECODES_USE_BEDROCK", "ALLTHECODES_USE_VERTEX"]);
    clear_env(&["ALLTHECODES_USE_BEDROCK", "ALLTHECODES_USE_VERTEX"]);

    // Save and clear all provider keys
    let saved: Vec<_> = crate::api::providers::PROVIDERS
        .iter()
        .filter_map(|p| std::env::var(p.env_key).ok().map(|v| (p.env_key, v)))
        .collect();
    for p in crate::api::providers::PROVIDERS {
        std::env::remove_var(p.env_key);
    }

    let client = ApiClient::from_env();
    assert!(
        client.is_none(),
        "from_env should return None when no provider key is set"
    );

    // Restore
    for (key, val) in saved {
        std::env::set_var(key, val);
    }
    restore_env(saved_flags);
}

#[test]
fn test_from_auth_with_env() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    let saved_flags = save_env(&["ALLTHECODES_USE_BEDROCK", "ALLTHECODES_USE_VERTEX"]);
    clear_env(&["ALLTHECODES_USE_BEDROCK", "ALLTHECODES_USE_VERTEX"]);

    let key = "sk-ant-api03-test-from-auth-key";
    std::env::set_var("ANTHROPIC_API_KEY", key);

    let client = ApiClient::from_auth();
    assert!(client.is_some(), "from_auth should find the env var");

    // Clean up
    std::env::remove_var("ANTHROPIC_API_KEY");
    restore_env(saved_flags);
}

#[test]
fn test_from_codex_auth_with_env() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    std::env::set_var(OPENAI_CODEX_TOKEN_ENV, "codex-token-test");
    std::env::set_var(OPENAI_CODEX_BASE_URL_ENV, "https://example.com/codex/");
    std::env::set_var(OPENAI_CODEX_MODEL_ENV, "gpt-5.3-codex-spark");

    let client = ApiClient::from_codex_auth().expect("from_codex_auth should return Some");
    match &client.config().provider {
        ApiProvider::OpenAiCompat {
            name,
            api_key,
            base_url,
            default_model,
        } => {
            assert_eq!(name, OPENAI_CODEX_PROVIDER_NAME);
            assert_eq!(api_key, "codex-token-test");
            assert_eq!(base_url, "https://example.com/codex");
            assert_eq!(default_model, "gpt-5.3-codex-spark");
        }
        other => panic!("expected OpenAiCompat provider, got {:?}", other),
    }
    assert_eq!(client.config().default_model, "gpt-5.3-codex-spark");

    std::env::remove_var(OPENAI_CODEX_TOKEN_ENV);
    std::env::remove_var(OPENAI_CODEX_BASE_URL_ENV);
    std::env::remove_var(OPENAI_CODEX_MODEL_ENV);
}

#[test]
fn test_from_env_prefers_bedrock_when_flag_set() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    let saved_extra = save_env(&[
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "AWS_BEARER_TOKEN_BEDROCK",
        "AWS_REGION",
    ]);
    // Save + clear all provider keys so Anthropic-API-key detection doesn't shadow.
    let saved_keys: Vec<_> = crate::api::providers::PROVIDERS
        .iter()
        .filter_map(|p| std::env::var(p.env_key).ok().map(|v| (p.env_key, v)))
        .collect();
    for p in crate::api::providers::PROVIDERS {
        std::env::remove_var(p.env_key);
    }

    std::env::set_var("ALLTHECODES_USE_BEDROCK", "1");
    std::env::set_var("AWS_BEARER_TOKEN_BEDROCK", "bedrock-123");
    std::env::set_var("AWS_REGION", "us-west-2");

    let client = ApiClient::from_env().expect("Bedrock flag should produce a client");
    match &client.config().provider {
        ApiProvider::Bedrock { region, .. } => assert_eq!(region, "us-west-2"),
        other => panic!("expected Bedrock provider, got {:?}", other),
    }

    for (k, v) in saved_keys {
        std::env::set_var(k, v);
    }
    restore_env(saved_extra);
}

#[test]
fn test_from_env_prefers_vertex_when_flag_set() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    let saved_extra = save_env(&[
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ANTHROPIC_VERTEX_PROJECT_ID",
        "ALLTHECODES_VERTEX_ACCESS_TOKEN",
        "CLOUD_ML_REGION",
    ]);
    let saved_keys: Vec<_> = crate::api::providers::PROVIDERS
        .iter()
        .filter_map(|p| std::env::var(p.env_key).ok().map(|v| (p.env_key, v)))
        .collect();
    for p in crate::api::providers::PROVIDERS {
        std::env::remove_var(p.env_key);
    }

    std::env::set_var("ALLTHECODES_USE_VERTEX", "true");
    std::env::set_var("ANTHROPIC_VERTEX_PROJECT_ID", "proj-42");
    std::env::set_var("ALLTHECODES_VERTEX_ACCESS_TOKEN", "ya29.test");
    std::env::set_var("CLOUD_ML_REGION", "europe-west4");

    let client = ApiClient::from_env().expect("Vertex flag should produce a client");
    match &client.config().provider {
        ApiProvider::Vertex {
            project_id, region, ..
        } => {
            assert_eq!(project_id, "proj-42");
            assert_eq!(region, "europe-west4");
        }
        other => panic!("expected Vertex provider, got {:?}", other),
    }

    for (k, v) in saved_keys {
        std::env::set_var(k, v);
    }
    restore_env(saved_extra);
}

#[test]
fn test_from_bedrock_env_returns_none_without_auth() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    // Ensure neither auth method is available.
    let saved_bearer = std::env::var("AWS_BEARER_TOKEN_BEDROCK").ok();
    let saved_ak = std::env::var("AWS_ACCESS_KEY_ID").ok();
    let saved_sk = std::env::var("AWS_SECRET_ACCESS_KEY").ok();
    std::env::remove_var("AWS_BEARER_TOKEN_BEDROCK");
    std::env::remove_var("AWS_ACCESS_KEY_ID");
    std::env::remove_var("AWS_SECRET_ACCESS_KEY");

    assert!(
        ApiClient::from_bedrock_env_result().is_err(),
        "from_bedrock_env_result should reject missing AWS creds"
    );

    if let Some(v) = saved_bearer {
        std::env::set_var("AWS_BEARER_TOKEN_BEDROCK", v);
    }
    if let Some(v) = saved_ak {
        std::env::set_var("AWS_ACCESS_KEY_ID", v);
    }
    if let Some(v) = saved_sk {
        std::env::set_var("AWS_SECRET_ACCESS_KEY", v);
    }
}

#[test]
fn test_from_env_result_errors_for_explicit_bedrock_without_auth() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    let saved = save_env(&[
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "AWS_BEARER_TOKEN_BEDROCK",
        "AWS_ACCESS_KEY_ID",
        "AWS_SECRET_ACCESS_KEY",
        "ANTHROPIC_API_KEY",
    ]);
    clear_env(&[
        "ALLTHECODES_USE_VERTEX",
        "AWS_BEARER_TOKEN_BEDROCK",
        "AWS_ACCESS_KEY_ID",
        "AWS_SECRET_ACCESS_KEY",
    ]);
    std::env::set_var("ALLTHECODES_USE_BEDROCK", "1");
    std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-api03-should-not-fallback");

    let err = match ApiClient::from_env_result() {
        Err(error) => error,
        Ok(_) => panic!("Bedrock config must fail early"),
    };
    let msg = err.to_string();
    assert!(msg.contains("ALLTHECODES_USE_BEDROCK"));
    assert!(msg.contains("AWS_BEARER_TOKEN_BEDROCK"));

    restore_env(saved);
}

#[test]
fn test_from_env_result_errors_for_explicit_foundry() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    let saved = save_env(&[
        "ALLTHECODES_USE_FOUNDRY",
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ANTHROPIC_API_KEY",
    ]);
    clear_env(&["ALLTHECODES_USE_BEDROCK", "ALLTHECODES_USE_VERTEX"]);
    std::env::set_var("ALLTHECODES_USE_FOUNDRY", "1");
    std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-api03-should-not-fallback");

    let err = match ApiClient::from_env_result() {
        Err(error) => error,
        Ok(_) => panic!("Foundry config must fail early"),
    };
    let msg = err.to_string();
    assert!(msg.contains("Foundry"));
    assert!(msg.contains("no Foundry request/auth adapter"));

    restore_env(saved);
}

#[test]
fn test_from_env_result_errors_for_explicit_vertex_without_project() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    let saved = save_env(&[
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ANTHROPIC_VERTEX_PROJECT_ID",
        "GOOGLE_CLOUD_PROJECT",
        "GCLOUD_PROJECT",
        "ALLTHECODES_VERTEX_ACCESS_TOKEN",
        "ANTHROPIC_API_KEY",
    ]);
    clear_env(&[
        "ALLTHECODES_USE_BEDROCK",
        "ANTHROPIC_VERTEX_PROJECT_ID",
        "GOOGLE_CLOUD_PROJECT",
        "GCLOUD_PROJECT",
    ]);
    std::env::set_var("ALLTHECODES_USE_VERTEX", "1");
    std::env::set_var("ALLTHECODES_VERTEX_ACCESS_TOKEN", "vertex-token");
    std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-api03-should-not-fallback");

    let err = match ApiClient::from_env_result() {
        Err(error) => error,
        Ok(_) => panic!("Vertex config must fail early"),
    };
    let msg = err.to_string();
    assert!(msg.contains("ALLTHECODES_USE_VERTEX"));
    assert!(msg.contains("ANTHROPIC_VERTEX_PROJECT_ID"));

    restore_env(saved);
}

#[test]
fn test_from_backend_codex_prefers_codex_auth() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    std::env::set_var(OPENAI_CODEX_TOKEN_ENV, "codex-token-backend");
    std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-api03-should-not-win");

    let client = ApiClient::from_backend(Some("codex")).expect("from_backend should return Some");
    match &client.config().provider {
        ApiProvider::OpenAiCompat { name, api_key, .. } => {
            assert_eq!(name, OPENAI_CODEX_PROVIDER_NAME);
            assert_eq!(api_key, "codex-token-backend");
        }
        other => panic!("expected OpenAiCompat provider, got {:?}", other),
    }

    std::env::remove_var(OPENAI_CODEX_TOKEN_ENV);
    std::env::remove_var("ANTHROPIC_API_KEY");
}

#[test]
fn test_from_auth_uses_openai_keychain_when_api_provider_is_openai() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    use_persistent_test_keyring();
    let temp = tempfile::tempdir().expect("tempdir");
    let saved = save_env(&[
        "ALLTHECODES_HOME",
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "OPENAI_API_KEY",
        OPENAI_CODEX_TOKEN_ENV,
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    clear_env(&[
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "OPENAI_API_KEY",
        OPENAI_CODEX_TOKEN_ENV,
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    std::env::set_var("ALLTHECODES_HOME", temp.path());
    let _cwd = CwdGuard::set(temp.path());
    allthecodes_auth::api_key::remove_api_key().unwrap();
    allthecodes_auth::api_key::remove_openai_api_key().unwrap();
    allthecodes_auth::api_key::store_openai_api_key("sk-proj-keychain-openai-123456").unwrap();
    allthecodes_config::settings::write_user_settings(&allthecodes_config::settings::RawSettings {
        api_provider: Some(allthecodes_config::settings::API_PROVIDER_OPENAI.to_string()),
        backend: Some("native".to_string()),
        ..Default::default()
    })
    .unwrap();

    let client = ApiClient::from_auth_result()
        .expect("auth resolution should not error")
        .expect("openai keychain client");
    match &client.config().provider {
        ApiProvider::OpenAiCompat { name, api_key, .. } => {
            assert_eq!(name, OPENAI_PROVIDER_NAME);
            assert_eq!(api_key, "sk-proj-keychain-openai-123456");
        }
        other => panic!("expected OpenAI-compatible provider, got {:?}", other),
    }

    restore_env(saved);
}

#[test]
fn test_from_auth_anthropic_provider_does_not_read_openai_keychain() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    use_persistent_test_keyring();
    let temp = tempfile::tempdir().expect("tempdir");
    let saved = save_env(&[
        "ALLTHECODES_HOME",
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "OPENAI_API_KEY",
        OPENAI_CODEX_TOKEN_ENV,
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    clear_env(&[
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "OPENAI_API_KEY",
        OPENAI_CODEX_TOKEN_ENV,
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    std::env::set_var("ALLTHECODES_HOME", temp.path());
    let _cwd = CwdGuard::set(temp.path());
    allthecodes_auth::api_key::remove_api_key().unwrap();
    allthecodes_auth::api_key::remove_openai_api_key().unwrap();
    allthecodes_auth::api_key::store_openai_api_key("sk-proj-keychain-openai-abcdef").unwrap();
    allthecodes_config::settings::write_user_settings(&allthecodes_config::settings::RawSettings {
        api_provider: Some(allthecodes_config::settings::API_PROVIDER_ANTHROPIC.to_string()),
        backend: Some("native".to_string()),
        ..Default::default()
    })
    .unwrap();

    let client = ApiClient::from_auth_result().expect("auth resolution should not error");
    assert!(
        client.is_none(),
        "anthropic provider selection must not consume OpenAI keychain"
    );

    restore_env(saved);
}

#[test]
fn test_from_auth_settings_provider_takes_priority_over_other_env_key() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    use_persistent_test_keyring();
    let temp = tempfile::tempdir().expect("tempdir");
    let saved = save_env(&[
        "ALLTHECODES_HOME",
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "OPENAI_API_KEY",
        OPENAI_CODEX_TOKEN_ENV,
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    clear_env(&[
        "ANTHROPIC_AUTH_TOKEN",
        "OPENAI_API_KEY",
        OPENAI_CODEX_TOKEN_ENV,
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    std::env::set_var("ALLTHECODES_HOME", temp.path());
    let _cwd = CwdGuard::set(temp.path());
    std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-api03-env-priority-key");
    allthecodes_auth::api_key::remove_openai_api_key().unwrap();
    allthecodes_auth::api_key::store_openai_api_key("sk-proj-keychain-openai-priority").unwrap();
    allthecodes_config::settings::write_user_settings(&allthecodes_config::settings::RawSettings {
        api_provider: Some(allthecodes_config::settings::API_PROVIDER_OPENAI.to_string()),
        backend: Some("native".to_string()),
        ..Default::default()
    })
    .unwrap();

    let client = ApiClient::from_auth_result()
        .expect("auth resolution should not error")
        .expect("settings-selected provider client");
    match &client.config().provider {
        ApiProvider::OpenAiCompat { name, api_key, .. } => {
            assert_eq!(name, OPENAI_PROVIDER_NAME);
            assert_eq!(api_key, "sk-proj-keychain-openai-priority");
        }
        other => panic!("expected OpenAI provider, got {:?}", other),
    }

    restore_env(saved);
}

#[test]
fn test_active_anthropic_profile_ignores_inherited_codex_token() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    let temp = tempfile::tempdir().expect("tempdir");
    let saved = save_env(&[
        "ALLTHECODES_HOME",
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_MODEL",
        OPENAI_CODEX_TOKEN_ENV,
        OPENAI_CODEX_BASE_URL_ENV,
        OPENAI_CODEX_MODEL_ENV,
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    clear_env(&[
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_MODEL",
        OPENAI_CODEX_TOKEN_ENV,
        OPENAI_CODEX_BASE_URL_ENV,
        OPENAI_CODEX_MODEL_ENV,
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    std::env::set_var("ALLTHECODES_HOME", temp.path());
    let _cwd = CwdGuard::set(temp.path());
    std::env::set_var(OPENAI_CODEX_TOKEN_ENV, "inherited-codex-token");
    allthecodes_config::settings::write_user_settings(&allthecodes_config::settings::RawSettings {
        active_auth_profile: Some("claude_code".to_string()),
        auth_profiles: Some(HashMap::from([(
            "claude_code".to_string(),
            allthecodes_config::settings::ProviderProfileSettings {
                backend: Some("native".to_string()),
                api_provider: Some(
                    allthecodes_config::settings::API_PROVIDER_ANTHROPIC.to_string(),
                ),
                model: Some("deepseek-v4-pro".to_string()),
                base_url: Some("https://compatible.example.com/anthropic".to_string()),
                env: Some(HashMap::from([(
                    "ANTHROPIC_AUTH_TOKEN".to_string(),
                    "anthropic-profile-token".to_string(),
                )])),
                ..Default::default()
            },
        )])),
        ..Default::default()
    })
    .unwrap();
    let loaded = allthecodes_config::settings::load_effective(temp.path()).unwrap();
    allthecodes_config::settings::apply_startup_runtime_env(&loaded.effective.env)
        .expect("profile env applies");

    let client = ApiClient::from_auth_result()
        .expect("auth resolution should not error")
        .expect("anthropic profile client");

    match &client.config().provider {
        ApiProvider::Anthropic {
            auth,
            base_url,
            endpoint_kind,
        } => {
            assert_eq!(
                auth,
                &AnthropicAuth::BearerToken("anthropic-profile-token".to_string())
            );
            assert_eq!(
                base_url.as_deref(),
                Some("https://compatible.example.com/anthropic")
            );
            assert_eq!(endpoint_kind, &AnthropicEndpointKind::CompatibleAnthropic);
            assert_eq!(client.config().default_model, "deepseek-v4-pro");
        }
        other => panic!("expected Anthropic provider, got {:?}", other),
    }

    restore_env(saved);
}

// -----------------------------------------------------------------------
// SSE line parsing
// -----------------------------------------------------------------------

#[test]
fn test_sse_line_parsing_message_start() {
    let sse_text = "\
event: message_start\n\
data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":100,\"output_tokens\":0}}}\n\
\n";

    let events = parse_sse_text(sse_text).unwrap();
    assert_eq!(events.len(), 1);
    match &events[0] {
        StreamEvent::MessageStart { usage } => {
            assert_eq!(usage.input_tokens, 100);
            assert_eq!(usage.output_tokens, 0);
        }
        other => panic!("expected MessageStart, got {:?}", other),
    }
}

#[test]
fn test_sse_line_parsing_content_block_start() {
    let sse_text = "\
event: content_block_start\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\
\n";

    let events = parse_sse_text(sse_text).unwrap();
    assert_eq!(events.len(), 1);
    match &events[0] {
        StreamEvent::ContentBlockStart {
            index,
            content_block: _content_block,
        } => {
            assert_eq!(*index, 0);
        }
        other => panic!("expected ContentBlockStart, got {:?}", other),
    }
}

#[test]
fn test_sse_line_parsing_multiple_events() {
    let sse_text = "\
event: message_start\n\
data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":50,\"output_tokens\":0}}}\n\
\n\
event: content_block_start\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\
\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\
\n\
event: content_block_stop\n\
data: {\"type\":\"content_block_stop\",\"index\":0}\n\
\n\
event: message_delta\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":10}}\n\
\n\
event: message_stop\n\
data: {\"type\":\"message_stop\"}\n\
\n";

    let events = parse_sse_text(sse_text).unwrap();
    assert_eq!(events.len(), 6);

    assert!(matches!(events[0], StreamEvent::MessageStart { .. }));
    assert!(matches!(events[1], StreamEvent::ContentBlockStart { .. }));
    assert!(matches!(events[2], StreamEvent::ContentBlockDelta { .. }));
    assert!(matches!(events[3], StreamEvent::ContentBlockStop { .. }));
    assert!(matches!(events[4], StreamEvent::MessageDelta { .. }));
    assert!(matches!(events[5], StreamEvent::MessageStop));
}

#[test]
fn test_sse_line_parsing_ping_ignored() {
    let sse_text = "\
event: ping\n\
data: {}\n\
\n\
event: message_stop\n\
data: {\"type\":\"message_stop\"}\n\
\n";

    let events = parse_sse_text(sse_text).unwrap();
    // ping should be ignored, only message_stop should come through
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], StreamEvent::MessageStop));
}

#[test]
fn test_sse_line_parsing_accumulator_integration() {
    let sse_text = "\
event: message_start\n\
data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":42,\"output_tokens\":0}}}\n\
\n\
event: content_block_start\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\
\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello, world!\"}}\n\
\n\
event: content_block_stop\n\
data: {\"type\":\"content_block_stop\",\"index\":0}\n\
\n\
event: message_delta\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":5}}\n\
\n\
event: message_stop\n\
data: {\"type\":\"message_stop\"}\n\
\n";

    let events = parse_sse_text(sse_text).unwrap();
    let mut acc = StreamAccumulator::new();
    for event in &events {
        acc.process_event(event);
    }

    let msg = acc.build("claude-sonnet-4-20250514");
    assert_eq!(msg.role, "assistant");
    assert_eq!(msg.content.len(), 1);
    assert_eq!(msg.stop_reason.as_deref(), Some("end_turn"));

    if let allthecodes_types::message::ContentBlock::Text { text } = &msg.content[0] {
        assert_eq!(text, "Hello, world!");
    } else {
        panic!("expected Text content block");
    }

    assert_eq!(msg.usage.as_ref().unwrap().input_tokens, 42);
    assert_eq!(msg.usage.as_ref().unwrap().output_tokens, 5);
}

#[test]
fn test_sse_line_parsing_no_trailing_newline() {
    // SSE text without a trailing blank line should still parse
    let sse_text = "\
event: message_stop\n\
data: {\"type\":\"message_stop\"}";

    let events = parse_sse_text(sse_text).unwrap();
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], StreamEvent::MessageStop));
}

#[test]
fn test_sse_line_parsing_empty_text() {
    let events = parse_sse_text("").unwrap();
    assert!(events.is_empty());
}

#[test]
fn regression_anthropic_stream_error_event_currently_ignored() {
    // Phase 0 risk: event:error is currently discarded, so overload/auth
    // failures can disappear from the stream parser instead of surfacing.
    let err =
        parse_sse_text(STREAM_ERROR_EVENT_SSE).expect_err("error event should not be ignored");
    assert!(err.to_string().contains("overloaded_error"));
}

#[test]
fn anthropic_stream_error_event_preserves_available_metadata() {
    let sse_text = "\
event: error\n\
data: {\"type\":\"error\",\"status\":529,\"request_id\":\"req_sse\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}\n\
\n";

    let err = parse_sse_text(sse_text).expect_err("error event should surface");
    let normalized = err
        .downcast_ref::<crate::api::streaming::NormalizedApiError>()
        .expect("stream error should use normalized API error");

    assert_eq!(normalized.provider, "anthropic");
    assert_eq!(normalized.status, Some(529));
    assert_eq!(normalized.request_id.as_deref(), Some("req_sse"));
    assert_eq!(normalized.error_type.as_deref(), Some("overloaded_error"));
    assert_eq!(normalized.message, "Overloaded");
}

struct FlakyStreamProvider {
    calls: Arc<AtomicUsize>,
    fail_times: usize,
    error: &'static str,
}

struct StaticStreamProvider {
    events: Vec<StreamEvent>,
}

struct PartialThenErrorStreamProvider;

#[async_trait::async_trait]
impl crate::api::stream_provider::StreamProvider for StaticStreamProvider {
    async fn stream(
        &self,
        _http: &reqwest::Client,
        _request: &MessagesRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        Ok(Box::pin(futures::stream::iter(
            self.events.clone().into_iter().map(Ok),
        )))
    }
}

#[async_trait::async_trait]
impl crate::api::stream_provider::StreamProvider for PartialThenErrorStreamProvider {
    async fn stream(
        &self,
        _http: &reqwest::Client,
        _request: &MessagesRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        let events = vec![
            Ok(StreamEvent::MessageStart {
                usage: allthecodes_types::message::Usage {
                    input_tokens: 11,
                    output_tokens: 0,
                    reasoning_output_tokens: 0,
                    cache_read_input_tokens: 0,
                    cache_creation_input_tokens: 0,
                },
            }),
            Ok(StreamEvent::ContentBlockStart {
                index: 0,
                content_block: allthecodes_types::message::ContentBlock::Text {
                    text: String::new(),
                },
            }),
            Ok(StreamEvent::ContentBlockDelta {
                index: 0,
                delta: serde_json::json!({"type": "text_delta", "text": "partial"}),
            }),
            Err(anyhow::anyhow!(crate::api::streaming::NormalizedApiError {
                provider: "anthropic".to_string(),
                status: Some(529),
                request_id: Some("req_partial".to_string()),
                error_type: Some("overloaded_error".to_string()),
                message: "Overloaded".to_string(),
            })),
        ];
        Ok(Box::pin(futures::stream::iter(events)))
    }
}

#[async_trait::async_trait]
impl crate::api::stream_provider::StreamProvider for FlakyStreamProvider {
    async fn stream(
        &self,
        _http: &reqwest::Client,
        _request: &MessagesRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        let call_index = self.calls.fetch_add(1, Ordering::SeqCst);
        if call_index < self.fail_times {
            anyhow::bail!("{}", self.error);
        }

        Ok(Box::pin(futures::stream::empty::<Result<StreamEvent>>()))
    }
}

fn minimal_stream_request() -> MessagesRequest {
    MessagesRequest {
        model: "claude-sonnet-4-20250514".to_string(),
        messages: vec![serde_json::json!({"role": "user", "content": "Hello"})],
        system: None,
        max_tokens: 1024,
        tools: None,
        stream: true,
        metadata: None,
        service_tier: None,
        stop_sequences: None,
        temperature: None,
        top_p: None,
        top_k: None,
        context_management: None,
        thinking: None,
        output_config: None,
        tool_choice: None,
        reasoning_effort: None,
        advisor_model: None,
    }
}

fn retry_test_config(max_retries: usize) -> crate::api::retry::RetryConfig {
    crate::api::retry::RetryConfig {
        max_retries,
        initial_delay_ms: 0,
        max_delay_ms: 0,
        backoff_multiplier: 1.0,
        retryable_status_codes: vec![429, 500, 502, 503, 504, 529],
    }
}

#[tokio::test]
async fn messages_stream_retries_retryable_stream_start_errors() {
    let calls = Arc::new(AtomicUsize::new(0));
    let client = ApiClient {
        config: anthropic_config(),
        http: reqwest::Client::new(),
        stream_provider: Box::new(FlakyStreamProvider {
            calls: calls.clone(),
            fail_times: 1,
            error: "Provider qwen error (HTTP 500): upstream unavailable",
        }),
    };
    let observed_delays = Arc::new(Mutex::new(Vec::new()));
    let observed_delays_for_sleep = observed_delays.clone();

    let result = client
        .messages_stream_with_backoff(
            minimal_stream_request(),
            retry_test_config(2),
            move |delay| {
                observed_delays_for_sleep.lock().unwrap().push(delay);
                std::future::ready(())
            },
        )
        .await;

    assert!(result.is_ok());
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        *observed_delays.lock().unwrap(),
        vec![Duration::from_millis(0)]
    );
}

#[tokio::test]
async fn messages_stream_does_not_retry_nonretryable_stream_start_errors() {
    let calls = Arc::new(AtomicUsize::new(0));
    let client = ApiClient {
        config: anthropic_config(),
        http: reqwest::Client::new(),
        stream_provider: Box::new(FlakyStreamProvider {
            calls: calls.clone(),
            fail_times: 1,
            error: "API error (HTTP 400): prompt is too long",
        }),
    };
    let observed_delays = Arc::new(Mutex::new(Vec::new()));
    let observed_delays_for_sleep = observed_delays.clone();

    let result = client
        .messages_stream_with_backoff(
            minimal_stream_request(),
            retry_test_config(2),
            move |delay| {
                observed_delays_for_sleep.lock().unwrap().push(delay);
                std::future::ready(())
            },
        )
        .await;

    assert!(result.is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(observed_delays.lock().unwrap().is_empty());
}

#[tokio::test]
async fn messages_collects_stream_events_into_assistant_message() {
    let client = ApiClient {
        config: anthropic_config(),
        http: reqwest::Client::new(),
        stream_provider: Box::new(StaticStreamProvider {
            events: vec![
                StreamEvent::MessageStart {
                    usage: allthecodes_types::message::Usage {
                        input_tokens: 11,
                        output_tokens: 0,
                        reasoning_output_tokens: 0,
                        cache_read_input_tokens: 0,
                        cache_creation_input_tokens: 0,
                    },
                },
                StreamEvent::ContentBlockStart {
                    index: 0,
                    content_block: allthecodes_types::message::ContentBlock::Text {
                        text: String::new(),
                    },
                },
                StreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: serde_json::json!({"type": "text_delta", "text": "Hello"}),
                },
                StreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: serde_json::json!({"type": "text_delta", "text": ", world"}),
                },
                StreamEvent::ContentBlockStop { index: 0 },
                StreamEvent::MessageDelta {
                    delta: allthecodes_types::message::MessageDelta {
                        stop_reason: Some("end_turn".to_string()),
                    },
                    usage: Some(allthecodes_types::message::Usage {
                        input_tokens: 0,
                        output_tokens: 7,
                        reasoning_output_tokens: 0,
                        cache_read_input_tokens: 0,
                        cache_creation_input_tokens: 0,
                    }),
                },
                StreamEvent::MessageStop,
            ],
        }),
    };

    let message = client.messages(minimal_stream_request()).await.unwrap();

    assert_eq!(message.role, "assistant");
    assert_eq!(message.stop_reason.as_deref(), Some("end_turn"));
    assert_eq!(message.usage.as_ref().unwrap().input_tokens, 11);
    assert_eq!(message.usage.as_ref().unwrap().output_tokens, 7);
    match &message.content[0] {
        allthecodes_types::message::ContentBlock::Text { text } => assert_eq!(text, "Hello, world"),
        other => panic!("expected text content, got {:?}", other),
    }
}

#[tokio::test]
async fn messages_propagates_partial_stream_error_instead_of_fake_success() {
    let client = ApiClient {
        config: anthropic_config(),
        http: reqwest::Client::new(),
        stream_provider: Box::new(PartialThenErrorStreamProvider),
    };

    let err = client
        .messages(minimal_stream_request())
        .await
        .expect_err("partial stream error must not become an assistant message");
    let msg = err.to_string();

    assert!(msg.contains("provider=anthropic"));
    assert!(msg.contains("status=529"));
    assert!(msg.contains("request_id=req_partial"));
    assert!(msg.contains("type=overloaded_error"));
}

#[test]
fn normalizes_anthropic_error_body_with_request_metadata() {
    let err = crate::api::streaming::normalize_api_error_body(
        "anthropic",
        Some(400),
        r#"{"type":"error","request_id":"req_123","error":{"type":"invalid_request_error","message":"bad request"}}"#,
        None,
    );

    assert_eq!(err.provider, "anthropic");
    assert_eq!(err.status, Some(400));
    assert_eq!(err.request_id.as_deref(), Some("req_123"));
    assert_eq!(err.error_type.as_deref(), Some("invalid_request_error"));
    assert_eq!(err.message, "bad request");
    assert_eq!(
        err.to_string(),
        "API error provider=anthropic status=400 request_id=req_123 type=invalid_request_error: bad request"
    );
}

// -----------------------------------------------------------------------
// MessagesRequest serialization
// -----------------------------------------------------------------------

#[test]
fn test_messages_request_serialization() {
    let req = MessagesRequest {
        model: "claude-sonnet-4-20250514".to_string(),
        messages: vec![serde_json::json!({"role": "user", "content": "Hello"})],
        system: None,
        max_tokens: 1024,
        tools: None,
        stream: true,
        metadata: None,
        service_tier: None,
        stop_sequences: None,
        temperature: None,
        top_p: None,
        top_k: None,
        context_management: None,
        thinking: None,
        output_config: None,
        tool_choice: None,
        reasoning_effort: None,
        advisor_model: None,
    };

    let json = serde_json::to_value(&req).unwrap();
    assert_eq!(json["model"], "claude-sonnet-4-20250514");
    assert_eq!(json["max_tokens"], 1024);
    assert_eq!(json["stream"], true);
    // thinking, output_config, tool_choice and advisor_model should be omitted when None
    assert!(json.get("thinking").is_none());
    assert!(json.get("output_config").is_none());
    assert!(json.get("tool_choice").is_none());
    assert!(json.get("advisor_model").is_none());
    assert!(json.get("metadata").is_none());
    assert!(json.get("service_tier").is_none());
    assert!(json.get("stop_sequences").is_none());
    assert!(json.get("temperature").is_none());
    assert!(json.get("top_p").is_none());
    assert!(json.get("top_k").is_none());
    assert!(json.get("context_management").is_none());
}

#[test]
fn test_messages_request_optional_fields_serialize_when_present() {
    let req = MessagesRequest {
        model: "claude-sonnet-4-20250514".to_string(),
        messages: vec![serde_json::json!({"role": "user", "content": "Hello"})],
        system: None,
        max_tokens: 1024,
        tools: None,
        stream: true,
        metadata: Some(serde_json::json!({"user_id": "user-123"})),
        service_tier: Some("auto".to_string()),
        stop_sequences: Some(vec!["STOP".to_string()]),
        temperature: Some(0.25),
        top_p: Some(0.9),
        top_k: Some(50),
        context_management: Some(serde_json::json!({
            "edits": [{"type": "clear_tool_uses_20250919"}]
        })),
        thinking: None,
        output_config: Some(serde_json::json!({"effort": "high"})),
        tool_choice: None,
        reasoning_effort: None,
        advisor_model: None,
    };

    let json = serde_json::to_value(&req).unwrap();
    assert_eq!(json["metadata"]["user_id"], "user-123");
    assert_eq!(json["service_tier"], "auto");
    assert_eq!(json["stop_sequences"][0], "STOP");
    assert_eq!(json["temperature"], 0.25);
    assert_eq!(json["top_p"], 0.9);
    assert_eq!(json["top_k"], 50);
    assert_eq!(
        json["context_management"]["edits"][0]["type"],
        "clear_tool_uses_20250919"
    );
    assert_eq!(json["output_config"]["effort"], "high");
}

#[test]
fn regression_prompt_cache_marker_serializes_in_anthropic_body() {
    let req = MessagesRequest {
        model: "claude-sonnet-4-5-20250929".to_string(),
        messages: vec![serde_json::json!({"role": "user", "content": "Hello"})],
        system: Some(vec![serde_json::json!({
            "type": "text",
            "text": "You are a coding assistant.",
            "cache_control": {"type": "ephemeral"}
        })]),
        max_tokens: 1024,
        tools: None,
        stream: true,
        metadata: None,
        service_tier: None,
        stop_sequences: None,
        temperature: None,
        top_p: None,
        top_k: None,
        context_management: None,
        thinking: None,
        output_config: None,
        tool_choice: None,
        reasoning_effort: None,
        advisor_model: None,
    };

    let body = serde_json::to_value(&req).unwrap();
    let expected = fixture_json("prompt_cache_body_expected");

    assert_eq!(body, expected);
}

#[test]
fn test_prompt_cache_policy_defaults_do_not_add_ttl_or_global() {
    let _guard = ENV_LOCK.lock().unwrap();
    let saved = save_env(&[
        "ALLTHECODES_PROMPT_CACHE_TTL",
        "ALLTHECODES_PROMPT_CACHE_GLOBAL",
    ]);
    clear_env(&[
        "ALLTHECODES_PROMPT_CACHE_TTL",
        "ALLTHECODES_PROMPT_CACHE_GLOBAL",
    ]);

    let mut body = serde_json::json!({
        "system": [{"type": "text", "text": "sys", "cache_control": {"type": "ephemeral"}}],
    });
    apply_prompt_cache_policy_to_body(
        &mut body,
        PromptCacheCapability {
            explicit_markers: true,
            ttl_1h: true,
            global_scope: true,
            direct_official_anthropic: true,
        },
    );

    assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");
    assert!(body["system"][0]["cache_control"].get("ttl").is_none());
    assert!(body["system"][0]["cache_control"].get("scope").is_none());
    restore_env(saved);
}

#[test]
fn test_compatible_anthropic_body_strips_cache_and_thinking_extensions() {
    let mut body = serde_json::json!({
        "model": "deepseek-v4-pro",
        "thinking": {"type": "enabled", "budget_tokens": 1024},
        "context_management": {"edits": [{"type": "clear_tool_uses_20250919"}]},
        "system": [{"type": "text", "text": "sys", "cache_control": {"type": "ephemeral"}}],
        "messages": [{
            "role": "assistant",
            "content": [
                {"type": "thinking", "thinking": "private", "signature": "signed"},
                {"type": "redacted_thinking", "data": "redacted"},
                {"type": "text", "text": "hello", "cache_reference": "abc"}
            ]
        }, {
            "role": "user",
            "content": [{"type": "tool_result", "tool_use_id": "toolu_1", "content": {"thinking": "payload field stays"}}]
        }]
    });

    strip_anthropic_compatible_only_fields(&mut body);

    assert!(body.get("thinking").is_none());
    assert!(body.get("context_management").is_none());
    assert!(body["system"][0].get("cache_control").is_none());
    assert!(body["messages"][0]["content"][0]
        .get("cache_reference")
        .is_none());
    assert_eq!(body["messages"][0]["content"].as_array().unwrap().len(), 1);
    assert_eq!(body["messages"][0]["content"][0]["type"], "text");
    assert_eq!(
        body["messages"][1]["content"][0]["content"]["thinking"],
        "payload field stays"
    );
}

#[test]
fn test_prompt_cache_policy_adds_ttl_and_global_only_when_capable() {
    let _guard = ENV_LOCK.lock().unwrap();
    let saved = save_env(&[
        "ALLTHECODES_PROMPT_CACHE_TTL",
        "ALLTHECODES_PROMPT_CACHE_GLOBAL",
    ]);
    std::env::set_var("ALLTHECODES_PROMPT_CACHE_TTL", "1h");
    std::env::set_var("ALLTHECODES_PROMPT_CACHE_GLOBAL", "1");

    let mut body = serde_json::json!({
        "system": [{"type": "text", "text": "sys", "cache_control": {"type": "ephemeral"}}],
    });
    apply_prompt_cache_policy_to_body(
        &mut body,
        PromptCacheCapability {
            explicit_markers: true,
            ttl_1h: true,
            global_scope: true,
            direct_official_anthropic: true,
        },
    );
    assert_eq!(body["system"][0]["cache_control"]["ttl"], "1h");
    assert_eq!(body["system"][0]["cache_control"]["scope"], "global");

    let mut body = serde_json::json!({
        "system": [{"type": "text", "text": "sys", "cache_control": {"type": "ephemeral"}}],
    });
    apply_prompt_cache_policy_to_body(
        &mut body,
        PromptCacheCapability {
            explicit_markers: true,
            ttl_1h: false,
            global_scope: true,
            direct_official_anthropic: false,
        },
    );
    assert!(body["system"][0]["cache_control"].get("ttl").is_none());
    assert!(body["system"][0]["cache_control"].get("scope").is_none());
    restore_env(saved);
}

#[test]
fn test_strip_anthropic_cache_fields_recursively() {
    let mut body = serde_json::json!({
        "messages": [{
            "role": "user",
            "content": [{
                "type": "text",
                "text": "hello",
                "cache_control": {"type": "ephemeral"},
                "cache_reference": "x"
            }]
        }],
        "cache_edits": []
    });
    strip_anthropic_cache_fields(&mut body);
    assert!(serde_json::to_string(&body)
        .unwrap()
        .find("cache_")
        .is_none());
    assert_eq!(body["messages"][0]["content"][0]["text"], "hello");
}

#[test]
fn test_messages_request_with_thinking() {
    let req = MessagesRequest {
        model: "claude-sonnet-4-20250514".to_string(),
        messages: vec![],
        system: Some(vec![
            serde_json::json!({"type": "text", "text": "You are helpful."}),
        ]),
        max_tokens: 4096,
        tools: None,
        stream: true,
        metadata: None,
        service_tier: None,
        stop_sequences: None,
        temperature: None,
        top_p: None,
        top_k: None,
        context_management: None,
        thinking: Some(serde_json::json!({"type": "enabled", "budget_tokens": 2048})),
        output_config: None,
        tool_choice: None,
        reasoning_effort: None,
        advisor_model: None,
    };

    let json = serde_json::to_value(&req).unwrap();
    assert!(json.get("thinking").is_some());
    assert_eq!(json["thinking"]["type"], "enabled");
    assert!(json.get("system").is_some());
}

#[test]
fn test_anthropic_count_tokens_body_omits_generation_only_fields() {
    let req = MessagesRequest {
        model: "claude-sonnet-4-20250514".to_string(),
        messages: vec![serde_json::json!({"role": "user", "content": "Hello"})],
        system: Some(vec![serde_json::json!({
            "type": "text",
            "text": "Be brief.",
            "cache_control": {"type": "ephemeral"}
        })]),
        max_tokens: 1024,
        tools: Some(vec![serde_json::json!({
            "name": "Read",
            "description": "",
            "input_schema": {"type": "object"}
        })]),
        stream: true,
        metadata: None,
        service_tier: None,
        stop_sequences: None,
        temperature: None,
        top_p: None,
        top_k: None,
        context_management: None,
        thinking: Some(serde_json::json!({"type": "enabled", "budget_tokens": 1024})),
        output_config: None,
        tool_choice: None,
        reasoning_effort: None,
        advisor_model: Some("advisor".to_string()),
    };

    let body = build_anthropic_count_tokens_body(&req);

    assert_eq!(body["model"], "claude-sonnet-4-20250514");
    assert_eq!(body["messages"][0]["content"], "Hello");
    assert_eq!(body["system"][0]["text"], "Be brief.");
    assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");
    assert!(body.get("tools").is_some());
    assert!(body.get("thinking").is_some());
    assert!(body.get("stream").is_none());
    assert!(body.get("max_tokens").is_none());
    assert!(body.get("advisor_model").is_none());
}

#[test]
fn test_exact_token_count_support_matrix() {
    let anthropic = ApiClient::new(anthropic_config());
    assert!(anthropic.supports_exact_token_count());

    let compatible_anthropic = ApiClient::new(ApiClientConfig {
        provider: ApiProvider::Anthropic {
            auth: AnthropicAuth::BearerToken("compatible-token".to_string()),
            base_url: Some("https://compatible.example.com/anthropic".to_string()),
            endpoint_kind: AnthropicEndpointKind::CompatibleAnthropic,
        },
        default_model: "claude-sonnet-4-20250514".to_string(),
        max_retries: 3,
        timeout_secs: 60,
    });
    assert!(compatible_anthropic.supports_exact_token_count());

    let azure = ApiClient::new(ApiClientConfig {
        provider: ApiProvider::Azure {
            endpoint: "https://azure.example.com".to_string(),
            api_key: "az-key".to_string(),
        },
        default_model: "claude-sonnet-4-20250514".to_string(),
        max_retries: 3,
        timeout_secs: 60,
    });
    assert!(azure.supports_exact_token_count());

    let google = ApiClient::new(ApiClientConfig {
        provider: ApiProvider::Google {
            api_key: "google-key".to_string(),
            base_url: "https://generativelanguage.googleapis.com/v1beta".to_string(),
        },
        default_model: "gemini-2.0-flash".to_string(),
        max_retries: 3,
        timeout_secs: 60,
    });
    assert!(google.supports_exact_token_count());

    let bedrock = ApiClient::new(ApiClientConfig {
        provider: ApiProvider::Bedrock {
            region: "us-east-1".to_string(),
            auth: crate::api::bedrock::BedrockAuth::BearerToken("bedrock-key".to_string()),
            base_url_override: None,
        },
        default_model: "claude-sonnet-4-5-20250929".to_string(),
        max_retries: 3,
        timeout_secs: 60,
    });
    assert!(bedrock.supports_exact_token_count());

    let vertex = ApiClient::new(ApiClientConfig {
        provider: ApiProvider::Vertex {
            project_id: "project".to_string(),
            region: "us-east5".to_string(),
            access_token: crate::api::vertex::VertexAccessToken("token".to_string()),
        },
        default_model: "claude-sonnet-4-5-20250929".to_string(),
        max_retries: 3,
        timeout_secs: 60,
    });
    assert!(vertex.supports_exact_token_count());

    let openai = ApiClient::new(ApiClientConfig {
        provider: ApiProvider::OpenAiCompat {
            name: "openai".to_string(),
            api_key: "sk-test".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            default_model: "gpt-4o".to_string(),
        },
        default_model: "gpt-4o".to_string(),
        max_retries: 3,
        timeout_secs: 60,
    });
    assert!(!openai.supports_exact_token_count());

    let openai_compatible = ApiClient::new(ApiClientConfig {
        provider: ApiProvider::OpenAiCompat {
            name: "deepseek".to_string(),
            api_key: "sk-test".to_string(),
            base_url: "https://api.deepseek.com/v1".to_string(),
            default_model: "deepseek-chat".to_string(),
        },
        default_model: "deepseek-chat".to_string(),
        max_retries: 3,
        timeout_secs: 60,
    });
    assert!(!openai_compatible.supports_exact_token_count());
}

#[test]
fn test_messages_request_advisor_model_serializes_when_set() {
    let req = MessagesRequest {
        model: "claude-sonnet-4-20250514".to_string(),
        messages: vec![],
        system: None,
        max_tokens: 1024,
        tools: None,
        stream: true,
        metadata: None,
        service_tier: None,
        stop_sequences: None,
        temperature: None,
        top_p: None,
        top_k: None,
        context_management: None,
        thinking: None,
        output_config: None,
        tool_choice: None,
        reasoning_effort: None,
        advisor_model: Some("claude-opus-4-20250514".to_string()),
    };

    let json = serde_json::to_value(&req).unwrap();
    assert_eq!(json["advisor_model"], "claude-opus-4-20250514");
}

#[test]
fn test_provider_supports_advisor_matrix() {
    use crate::api::client::{provider_supports_advisor, ApiProvider};
    assert!(provider_supports_advisor(&ApiProvider::Anthropic {
        auth: AnthropicAuth::ApiKey("k".into()),
        base_url: None,
        endpoint_kind: AnthropicEndpointKind::DirectAnthropic,
    }));
    assert!(provider_supports_advisor(&ApiProvider::Azure {
        endpoint: "e".into(),
        api_key: "k".into(),
    }));
    assert!(!provider_supports_advisor(&ApiProvider::OpenAiCompat {
        name: "openai".into(),
        api_key: "k".into(),
        base_url: "u".into(),
        default_model: "m".into(),
    }));
    assert!(!provider_supports_advisor(&ApiProvider::Google {
        api_key: "k".into(),
        base_url: "u".into(),
    }));
}
