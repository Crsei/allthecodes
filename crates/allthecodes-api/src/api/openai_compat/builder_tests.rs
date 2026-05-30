use super::*;
use crate::api::client::OPENAI_CODEX_PROVIDER_NAME;
use serde_json::{json, Value};

fn base_request(model: &str, messages: Vec<Value>) -> MessagesRequest {
    MessagesRequest {
        model: model.to_string(),
        messages,
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

#[test]
fn test_build_openai_request_basic() {
    let mut req = base_request("gpt-4o", vec![json!({"role": "user", "content": "Hello"})]);
    req.system = Some(vec![json!({"type": "text", "text": "Be helpful."})]);
    req.reasoning_effort = Some("high".to_string());
    let body = build_openai_request(&req, "openai");
    assert_eq!(body["model"], "gpt-4o");
}

#[test]
fn test_build_openai_request_strips_anthropic_cache_fields() {
    let mut req = base_request(
        "gpt-4o",
        vec![json!({
            "role": "user",
            "content": [{
                "type": "text",
                "text": "Hello",
                "cache_control": {"type": "ephemeral"}
            }]
        })],
    );
    req.system = Some(vec![json!({
        "type": "text",
        "text": "Be helpful.",
        "cache_control": {"type": "ephemeral"}
    })]);
    req.reasoning_effort = Some("high".to_string());

    let body = build_openai_request(&req, "openai");
    assert!(serde_json::to_string(&body)
        .unwrap()
        .find("cache_")
        .is_none());
    assert_eq!(body["stream"], true);
    assert_eq!(body["max_completion_tokens"], 1024);
    assert!(body.get("max_tokens").is_none() || body["max_tokens"].is_null());

    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["role"], "system");
    assert_eq!(messages[0]["content"], "Be helpful.");
    assert_eq!(messages[1]["role"], "user");
    assert_eq!(messages[1]["content"], "Hello");
}

#[test]
fn test_build_openai_request_codex_compatible_shape() {
    let mut req = base_request("gpt-5.4", vec![json!({"role": "user", "content": "Hello"})]);
    req.max_tokens = 4096;
    req.reasoning_effort = Some("high".to_string());

    let body = build_openai_request(&req, OPENAI_CODEX_PROVIDER_NAME);
    assert_eq!(body["model"], "gpt-5.4");
    assert_eq!(body["instructions"], "");
    assert_eq!(body["stream"], true);
    assert_eq!(body["store"], false);
    assert_eq!(body["tool_choice"], "auto");
    assert!(body.get("stream_options").is_none());
    assert!(body.get("messages").is_none());
    assert!(body.get("max_tokens").is_none());
    assert!(body.get("max_completion_tokens").is_none());
    assert_eq!(body["reasoning"], json!({"effort": "high"}));
    assert_eq!(body["include"], json!(["reasoning.encrypted_content"]));
    assert_eq!(body["input"][0]["type"], "message");
    assert_eq!(body["input"][0]["role"], "user");
    assert_eq!(body["input"][0]["content"][0]["type"], "input_text");
    assert_eq!(body["input"][0]["content"][0]["text"], "Hello");
}

#[test]
fn test_build_codex_responses_skips_invalid_function_schema_roots() {
    let mut req = base_request("gpt-5.4", vec![json!({"role": "user", "content": "Hello"})]);
    req.max_tokens = 4096;
    req.tools = Some(vec![
        json!({
            "name": "Valid",
            "description": "valid tool",
            "input_schema": {
                "type": "object",
                "properties": {"path": {"type": "string"}}
            }
        }),
        json!({
            "name": "Invalid",
            "description": "invalid tool",
            "input_schema": {
                "oneOf": [
                    {"type": "object", "properties": {}}
                ]
            }
        }),
    ]);

    let body = build_openai_request(&req, OPENAI_CODEX_PROVIDER_NAME);
    let tools = body["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["name"], "Valid");
}

#[test]
fn test_build_codex_responses_uses_custom_apply_patch_tool() {
    let mut req = base_request("gpt-5.4", vec![json!({"role": "user", "content": "Hello"})]);
    req.tools = Some(vec![json!({
        "name": "apply_patch",
        "description": "",
        "input_schema": {
            "type": "object",
            "properties": {"input": {"type": "string"}},
            "required": ["input"]
        }
    })]);

    let body = build_openai_request(&req, OPENAI_CODEX_PROVIDER_NAME);
    let tools = body["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["type"], "custom");
    assert_eq!(tools[0]["name"], "apply_patch");
    assert_eq!(tools[0]["format"]["type"], "grammar");
    assert_eq!(tools[0]["format"]["syntax"], "lark");
    assert!(tools[0]["format"]["definition"]
        .as_str()
        .unwrap()
        .contains("environment_id?"));
}

#[test]
fn test_build_openai_request_downgrades_apply_patch_to_json_function() {
    let mut req = base_request("gpt-4o", vec![json!({"role": "user", "content": "Hello"})]);
    req.tools = Some(vec![json!({
        "name": "apply_patch",
        "description": "Apply a patch",
        "input_schema": {
            "type": "object",
            "properties": {"input": {"type": "string"}},
            "required": ["input"]
        }
    })]);

    let body = build_openai_request(&req, "openai");
    let tools = body["tools"].as_array().unwrap();

    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["type"], "function");
    assert_eq!(tools[0]["function"]["name"], "apply_patch");
    assert_eq!(
        tools[0]["function"]["parameters"]["properties"]["input"]["type"],
        "string"
    );
    assert!(tools[0].get("format").is_none());
}

#[test]
fn test_build_openai_request_no_system() {
    let mut req = base_request(
        "deepseek-chat",
        vec![json!({"role": "user", "content": "Hi"})],
    );
    req.max_tokens = 512;
    let body = build_openai_request(&req, "deepseek");
    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(body["max_tokens"], 512);
}

#[test]
fn test_build_openai_request_content_blocks() {
    let req = base_request(
        "gpt-4o",
        vec![json!({
            "role": "user",
            "content": [
                {"type": "text", "text": "Hello"},
                {"type": "text", "text": "World"},
            ]
        })],
    );
    let body = build_openai_request(&req, "openai");
    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages[0]["content"], "Hello\nWorld");
}

#[test]
fn test_build_deepseek_request_preserves_empty_reasoning_content_for_tool_call() {
    let mut req = base_request(
        "deepseek-v4-pro",
        vec![json!({
            "role": "assistant",
            "content": [
                {"type": "thinking", "thinking": "", "signature": null},
                {
                    "type": "tool_use",
                    "id": "call_empty_reasoning",
                    "name": "Read",
                    "input": {"file_path": "Cargo.toml"}
                },
            ]
        })],
    );
    req.thinking = Some(json!({"type": "enabled"}));

    let body = build_openai_request(&req, "deepseek");
    let messages = body["messages"].as_array().unwrap();
    let assistant = messages[0].as_object().unwrap();

    assert!(assistant.contains_key("reasoning_content"));
    assert_eq!(assistant["reasoning_content"], "");
    assert_eq!(assistant["content"], "");
    assert_eq!(assistant["tool_calls"][0]["id"], "call_empty_reasoning");
}

#[test]
fn test_build_openai_request_does_not_send_deepseek_reasoning_to_other_providers() {
    let req = base_request(
        "gpt-4o",
        vec![json!({
            "role": "assistant",
            "content": [
                {"type": "thinking", "thinking": "", "signature": null},
                {
                    "type": "tool_use",
                    "id": "call_1",
                    "name": "Read",
                    "input": {"file_path": "Cargo.toml"}
                },
            ]
        })],
    );

    let body = build_openai_request(&req, "openai");
    let messages = body["messages"].as_array().unwrap();
    let assistant = messages[0].as_object().unwrap();

    assert!(!assistant.contains_key("reasoning_content"));
    assert!(!assistant.contains_key("content"));
    assert_eq!(assistant["tool_calls"][0]["id"], "call_1");
}
