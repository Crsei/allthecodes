use std::collections::HashSet;

use serde_json::{json, Value};

use crate::api::client::{strip_anthropic_cache_fields, MessagesRequest};

const APPLY_PATCH_LARK_GRAMMAR: &str = r#"start: begin_patch environment_id? hunk+ end_patch
begin_patch: "*** Begin Patch" LF
environment_id: "*** Environment ID: " filename LF
end_patch: "*** End Patch" LF?

hunk: add_hunk | delete_hunk | update_hunk
add_hunk: "*** Add File: " filename LF add_line+
delete_hunk: "*** Delete File: " filename LF
update_hunk: "*** Update File: " filename LF change_move? change?

filename: /(.+)/
add_line: "+" /(.*)/ LF -> line

change_move: "*** Move to: " filename LF
change: (change_context | change_line)+ eof_line?
change_context: ("@@" | "@@ " /(.+)/) LF
change_line: ("+" | "-" | " ") /(.*)/ LF
eof_line: "*** End of File" LF

%import common.LF
"#;

fn is_responses_freeform_tool_name(name: &str) -> bool {
    name == "apply_patch"
}

fn responses_freeform_tool_spec(name: &str, description: &str) -> Option<Value> {
    if !is_responses_freeform_tool_name(name) {
        return None;
    }
    Some(json!({
        "type": "custom",
        "name": name,
        "description": if description.is_empty() {
            "Use the `apply_patch` tool to edit files. This is a FREEFORM tool, so do not wrap the patch in JSON."
        } else {
            description
        },
        "format": {
            "type": "grammar",
            "syntax": "lark",
            "definition": APPLY_PATCH_LARK_GRAMMAR,
        },
    }))
}

fn freeform_input_text(input: Option<&Value>) -> String {
    match input {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Object(map)) => map
            .get("input")
            .or_else(|| map.get("patch"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| Value::Object(map.clone()).to_string()),
        Some(value) => value.to_string(),
        None => String::new(),
    }
}

fn collect_freeform_tool_call_ids(messages: &[Value]) -> HashSet<String> {
    let mut ids = HashSet::new();
    for msg in messages {
        if msg.get("role").and_then(|v| v.as_str()) != Some("assistant") {
            continue;
        }
        let Some(Value::Array(blocks)) = msg.get("content") else {
            continue;
        };
        for block in blocks {
            let Some(name) = block.get("name").and_then(|v| v.as_str()) else {
                continue;
            };
            if !is_responses_freeform_tool_name(name) {
                continue;
            }
            if let Some(id) = block.get("id").and_then(|v| v.as_str()) {
                ids.insert(id.to_string());
            }
        }
    }
    ids
}

pub(super) fn reasoning_output_tokens_from_usage(usage: &Value) -> u64 {
    usage
        .get("completion_tokens_details")
        .or_else(|| usage.get("output_tokens_details"))
        .and_then(|details| details.get("reasoning_tokens"))
        .and_then(|value| value.as_u64())
        .unwrap_or(0)
}

/// Extract text from Anthropic system prompt blocks.
///
/// System blocks look like: `[{"type": "text", "text": "Be helpful."}]`
pub(super) fn extract_system_text(system: &[Value]) -> String {
    system
        .iter()
        .filter_map(|block| {
            if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                block
                    .get("text")
                    .and_then(|t| t.as_str())
                    .map(|s| s.to_string())
            } else {
                block.as_str().map(|s| s.to_string())
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Flatten Anthropic content (array of blocks or string) to a single string.
///
/// Anthropic: `{"content": [{"type":"text","text":"Hello"}]}` or `{"content": "Hello"}`
/// OpenAI:    `{"content": "Hello"}`
pub(super) fn flatten_content(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|block| match block.get("type").and_then(|t| t.as_str()) {
                Some("text") => block
                    .get("text")
                    .and_then(|t| t.as_str())
                    .map(|s| s.to_string()),
                Some("tool_result") => block
                    .get("content")
                    .and_then(|c| c.as_str())
                    .map(|s| s.to_string()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

pub(super) fn content_items_for_responses(content: Option<&Value>, output: bool) -> Vec<Value> {
    let item_type = if output { "output_text" } else { "input_text" };
    let text = flatten_content(content);
    if text.is_empty() {
        Vec::new()
    } else {
        vec![json!({ "type": item_type, "text": text })]
    }
}

pub(super) fn build_responses_input(request: &MessagesRequest) -> Vec<Value> {
    let mut messages = Value::Array(request.messages.clone());
    strip_anthropic_cache_fields(&mut messages);
    let messages = messages.as_array().cloned().unwrap_or_default();
    let freeform_tool_call_ids = collect_freeform_tool_call_ids(&messages);
    let mut input = Vec::new();

    for msg in &messages {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("user");
        let content = msg.get("content");

        if role == "assistant" {
            if let Some(Value::Array(blocks)) = content {
                let mut text_parts = Vec::new();
                for block in blocks {
                    match block.get("type").and_then(|t| t.as_str()) {
                        Some("text") => {
                            if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                if !text.is_empty() {
                                    text_parts.push(text.to_string());
                                }
                            }
                        }
                        Some("tool_use") => {
                            let call_id = block
                                .get("id")
                                .and_then(|i| i.as_str())
                                .unwrap_or("call_unknown");
                            let name = block
                                .get("name")
                                .and_then(|n| n.as_str())
                                .unwrap_or("unknown");
                            if is_responses_freeform_tool_name(name) {
                                input.push(json!({
                                    "type": "custom_tool_call",
                                    "name": name,
                                    "input": freeform_input_text(block.get("input")),
                                    "call_id": call_id,
                                }));
                            } else {
                                let arguments = block
                                    .get("input")
                                    .cloned()
                                    .unwrap_or_else(|| json!({}))
                                    .to_string();
                                input.push(json!({
                                    "type": "function_call",
                                    "name": name,
                                    "arguments": arguments,
                                    "call_id": call_id,
                                }));
                            }
                        }
                        _ => {}
                    }
                }
                if !text_parts.is_empty() {
                    input.push(json!({
                        "type": "message",
                        "role": "assistant",
                        "content": [{
                            "type": "output_text",
                            "text": text_parts.join("\n"),
                        }],
                    }));
                }
            } else {
                let content = content_items_for_responses(content, true);
                if !content.is_empty() {
                    input.push(json!({
                        "type": "message",
                        "role": "assistant",
                        "content": content,
                    }));
                }
            }
        } else if role == "user" {
            if let Some(Value::Array(blocks)) = content {
                let mut text_parts = Vec::new();
                for block in blocks {
                    match block.get("type").and_then(|t| t.as_str()) {
                        Some("tool_result") => {
                            let call_id = block
                                .get("tool_use_id")
                                .and_then(|i| i.as_str())
                                .unwrap_or("call_unknown");
                            let output = block
                                .get("content")
                                .map(|c| match c {
                                    Value::String(s) => s.clone(),
                                    Value::Array(arr) => arr
                                        .iter()
                                        .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                                        .collect::<Vec<_>>()
                                        .join("\n"),
                                    other => other.to_string(),
                                })
                                .unwrap_or_default();
                            if freeform_tool_call_ids.contains(call_id) {
                                input.push(json!({
                                    "type": "custom_tool_call_output",
                                    "call_id": call_id,
                                    "output": output,
                                }));
                            } else {
                                input.push(json!({
                                    "type": "function_call_output",
                                    "call_id": call_id,
                                    "output": output,
                                }));
                            }
                        }
                        Some("text") => {
                            if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                if !text.is_empty() {
                                    text_parts.push(text.to_string());
                                }
                            }
                        }
                        _ => {}
                    }
                }
                if !text_parts.is_empty() {
                    input.push(json!({
                        "type": "message",
                        "role": "user",
                        "content": [{
                            "type": "input_text",
                            "text": text_parts.join("\n"),
                        }],
                    }));
                }
            } else {
                let content = content_items_for_responses(content, false);
                if !content.is_empty() {
                    input.push(json!({
                        "type": "message",
                        "role": "user",
                        "content": content,
                    }));
                }
            }
        } else {
            let content = content_items_for_responses(content, false);
            if !content.is_empty() {
                input.push(json!({
                    "type": "message",
                    "role": role,
                    "content": content,
                }));
            }
        }
    }

    input
}

pub(super) fn build_responses_tools(request: &MessagesRequest) -> Vec<Value> {
    let mut tools = request.tools.clone().map(Value::Array);
    if let Some(value) = tools.as_mut() {
        strip_anthropic_cache_fields(value);
    }
    let tools = tools.and_then(|value| value.as_array().cloned());
    tools
        .unwrap_or_default()
        .into_iter()
        .filter_map(|tool| {
            let name = tool.get("name")?.as_str()?;
            let description = tool
                .get("description")
                .and_then(|d| d.as_str())
                .unwrap_or("");
            if let Some(custom_tool) = responses_freeform_tool_spec(name, description) {
                return Some(custom_tool);
            }
            let parameters = sanitize_responses_function_parameters(
                tool.get("input_schema")
                    .cloned()
                    .unwrap_or_else(|| json!({"type": "object", "properties": {}})),
            )?;
            Some(json!({
                "type": "function",
                "name": name,
                "description": description,
                "parameters": parameters,
            }))
        })
        .collect()
}

pub(super) fn sanitize_responses_function_parameters(parameters: Value) -> Option<Value> {
    let object = parameters.as_object()?;
    if object.get("type").and_then(|v| v.as_str()) != Some("object") {
        return None;
    }
    if ["oneOf", "anyOf", "allOf", "enum", "not"]
        .iter()
        .any(|key| object.contains_key(*key))
    {
        return None;
    }
    Some(parameters)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_system_text_blocks() {
        let system = vec![
            json!({"type": "text", "text": "You are helpful."}),
            json!({"type": "text", "text": "Be concise."}),
        ];
        assert_eq!(
            extract_system_text(&system),
            "You are helpful.\nBe concise."
        );
    }

    #[test]
    fn test_extract_system_text_empty() {
        let system: Vec<Value> = vec![];
        assert_eq!(extract_system_text(&system), "");
    }

    #[test]
    fn test_flatten_content_string() {
        let content = Value::String("Hello world".to_string());
        assert_eq!(flatten_content(Some(&content)), "Hello world");
    }

    #[test]
    fn test_flatten_content_text_blocks() {
        let content = json!([
            {"type": "text", "text": "Hello"},
            {"type": "text", "text": " world"},
        ]);
        assert_eq!(flatten_content(Some(&content)), "Hello\n world");
    }

    #[test]
    fn test_flatten_content_none() {
        assert_eq!(flatten_content(None), "");
    }

    #[test]
    fn test_flatten_content_tool_result() {
        let content = json!([
            {"type": "tool_result", "tool_use_id": "id1", "content": "result text"},
        ]);
        assert_eq!(flatten_content(Some(&content)), "result text");
    }

    #[test]
    fn test_build_responses_input_round_trips_apply_patch_as_custom() {
        let request = MessagesRequest {
            model: "gpt-5.4".to_string(),
            messages: vec![
                json!({
                    "role": "assistant",
                    "content": [{
                        "type": "tool_use",
                        "id": "call_patch",
                        "name": "apply_patch",
                        "input": {"input": "*** Begin Patch\n*** End Patch"}
                    }]
                }),
                json!({
                    "role": "user",
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": "call_patch",
                        "content": "Applied patch"
                    }]
                }),
            ],
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

        let input = build_responses_input(&request);
        assert_eq!(input[0]["type"], "custom_tool_call");
        assert_eq!(input[0]["name"], "apply_patch");
        assert_eq!(input[0]["input"], "*** Begin Patch\n*** End Patch");
        assert_eq!(input[1]["type"], "custom_tool_call_output");
        assert_eq!(input[1]["call_id"], "call_patch");
    }
}
