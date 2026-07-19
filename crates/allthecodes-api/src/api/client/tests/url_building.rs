use super::*;

// --- URL building ---

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
        recovery_policy: None,
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
        recovery_policy: None,
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
        recovery_policy: None,
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
        recovery_policy: None,
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
        recovery_policy: None,
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
        recovery_policy: None,
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
        recovery_policy: None,
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
        recovery_policy: None,
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
        recovery_policy: None,
    };
    let client = ApiClient::new(config);
    let url = client.build_url();
    assert_eq!(url, "https://chatgpt.com/backend-api/codex/responses");
}
