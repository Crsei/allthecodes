use super::*;

// --- Header building ---

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
        recovery_policy: None,
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
        recovery_policy: None,
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
        recovery_policy: None,
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
        recovery_policy: None,
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
        recovery_policy: None,
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
