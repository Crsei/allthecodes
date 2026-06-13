//! Real provider API connectivity tests.
//!
//! These tests actually call third-party provider APIs and require valid
//! API keys set as environment variables. They are marked `#[ignore]` so
//! they only run when explicitly invoked.
//!
//! Run with:
//!
//! ```bash
//! DEEPSEEK_API_KEY="sk-..." cargo test test_provider_connectivity -- --ignored --nocapture
//! ```

use super::*;

/// Smoke-test the DeepSeek chat completions endpoint via the OpenAI-compatible
/// provider path.
///
/// Verifies:
/// - HTTP transport to api.deepseek.com succeeds
/// - The API key is accepted
/// - A non-streaming chat response is returned with content
/// - Token usage counters are present
#[tokio::test]
#[ignore = "requires DEEPSEEK_API_KEY env var set"]
async fn test_deepseek_openai_compat_chat() {
    let api_key =
        std::env::var("DEEPSEEK_API_KEY").expect("set DEEPSEEK_API_KEY=sk-... to run this test");

    let provider = ApiProvider::OpenAiCompat {
        name: "deepseek".to_string(),
        api_key: api_key.clone(),
        base_url: "https://api.deepseek.com/v1".to_string(),
        default_model: "deepseek-v4-pro".to_string(),
    };

    let client = ApiClient::new(ApiClientConfig {
        provider,
        default_model: "deepseek-v4-pro".to_string(),
        max_retries: 1,
        timeout_secs: 60,
    });

    let request = MessagesRequest {
        model: "deepseek-v4-pro".to_string(),
        messages: vec![
            serde_json::json!({"role": "user", "content": "Reply with only the word OK."}),
        ],
        system: None,
        max_tokens: 256,
        tools: None,
        stream: false,
        metadata: None,
        service_tier: None,
        stop_sequences: None,
        temperature: Some(0.0),
        top_p: None,
        top_k: None,
        context_management: None,
        thinking: None,
        output_config: None,
        tool_choice: None,
        reasoning_effort: None,
        advisor_model: None,
    };

    let response = client
        .messages(request)
        .await
        .expect("DeepSeek chat completion should succeed");

    // The assistant should have produced at least one content block
    assert!(
        !response.content.is_empty(),
        "expected at least one content block, got empty"
    );

    // Ensure the response is text, not tool_use
    let text_content: Vec<&str> = response
        .content
        .iter()
        .filter_map(|block| match block {
            allthecodes_types::message::ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        !text_content.is_empty(),
        "expected at least one text block, got: {:?}",
        response.content
    );

    // Verify token usage is populated
    let usage = response.usage.expect("usage should be present");
    assert!(
        usage.input_tokens > 0,
        "expected >0 input tokens, got {}",
        usage.input_tokens
    );
    assert!(
        usage.output_tokens > 0,
        "expected >0 output tokens, got {}",
        usage.output_tokens
    );

    eprintln!(
        "✓ DeepSeek responded with {} content block(s), {} in / {} out tokens",
        response.content.len(),
        usage.input_tokens,
        usage.output_tokens,
    );
    eprintln!("  → {}", text_content[0]);
}

/// Verify that the DeepSeek model list endpoint is reachable and returns
/// known model IDs. This is a lightweight connectivity check that does not
/// consume token quota.
#[tokio::test]
#[ignore = "requires DEEPSEEK_API_KEY env var set"]
async fn test_deepseek_model_list() {
    let api_key =
        std::env::var("DEEPSEEK_API_KEY").expect("set DEEPSEEK_API_KEY=sk-... to run this test");

    let client = reqwest::Client::new();
    let resp = client
        .get("https://api.deepseek.com/v1/models")
        .header("Authorization", format!("Bearer {}", api_key))
        .send()
        .await
        .expect("GET /v1/models should succeed");

    assert_eq!(resp.status(), 200, "expected 200 OK, got {}", resp.status());

    let body: serde_json::Value = resp.json().await.expect("response should be valid JSON");

    let models = body["data"]
        .as_array()
        .expect("response should have a 'data' array");

    assert!(!models.is_empty(), "expected at least one model");

    let model_ids: Vec<&str> = models.iter().filter_map(|m| m["id"].as_str()).collect();

    assert!(
        model_ids.contains(&"deepseek-chat") || model_ids.contains(&"deepseek-v4-pro"),
        "expected 'deepseek-chat' or 'deepseek-v4-pro' in model list, got: {:?}",
        model_ids
    );

    eprintln!("✓ DeepSeek reports {} models", models.len());
    eprintln!("  models: {:?}", model_ids);
}
