use super::*;

// --- MessagesRequest serialization ---

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
    assert!(!serde_json::to_string(&body).unwrap().contains("cache_"));
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
