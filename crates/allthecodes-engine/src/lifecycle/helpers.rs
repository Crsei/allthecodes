//! Helper functions for the QueryEngine lifecycle.

use crate::types::message::Message;

/// Convert a conversation history into a text representation suitable for
/// model-based summarization. Extracts user messages, assistant text, and
/// tool use/result information.
pub(crate) fn format_conversation_for_summary(messages: &[Message]) -> String {
    let mut parts = Vec::new();

    for msg in messages {
        match msg {
            Message::User(u) => {
                let text = match &u.content {
                    crate::types::message::MessageContent::Text(t) => t.clone(),
                    crate::types::message::MessageContent::Blocks(blocks) => blocks
                        .iter()
                        .filter_map(|b| match b {
                            crate::types::message::ContentBlock::Text { text } => {
                                Some(text.clone())
                            }
                            crate::types::message::ContentBlock::ToolResult {
                                tool_use_id,
                                content,
                                is_error,
                            } => {
                                let result_text = match content {
                                    crate::types::message::ToolResultContent::Text(t) => {
                                        if t.len() > 500 {
                                            format!("{}...[truncated]", &t[..500])
                                        } else {
                                            t.clone()
                                        }
                                    }
                                    crate::types::message::ToolResultContent::Blocks(_) => {
                                        "[complex result]".to_string()
                                    }
                                };
                                Some(format!(
                                    "[Tool Result ({}{}): {}]",
                                    tool_use_id,
                                    if *is_error { ", ERROR" } else { "" },
                                    result_text
                                ))
                            }
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                };
                if !text.is_empty() && !u.is_meta {
                    parts.push(format!("User: {}", text));
                }
            }
            Message::Assistant(a) => {
                for block in &a.content {
                    match block {
                        crate::types::message::ContentBlock::Text { text } => {
                            if text.len() > 1000 {
                                parts.push(format!("Assistant: {}...[truncated]", &text[..1000]));
                            } else {
                                parts.push(format!("Assistant: {}", text));
                            }
                        }
                        crate::types::message::ContentBlock::ToolUse { name, input, .. } => {
                            let input_preview = {
                                let s = input.to_string();
                                if s.len() > 200 {
                                    format!("{}...", &s[..200])
                                } else {
                                    s
                                }
                            };
                            parts.push(format!("[Tool Use: {} ({})]", name, input_preview));
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    // Cap total length to avoid exceeding context for the summarization call
    let joined = parts.join("\n\n");
    if joined.len() > 400_000 {
        format!(
            "{}...\n\n[conversation truncated for summarization]",
            &joined[..400_000]
        )
    } else {
        joined
    }
}

/// Build a `MessagesRequest` from the generic `ModelCallParams`.
///
/// This translates the engine's internal representation into the wire format
/// expected by `api::client::ApiClient`.
pub(crate) fn build_messages_request(
    params: &allthecodes_engine::query::deps::ModelCallParams,
) -> allthecodes_api::api::client::MessagesRequest {
    use crate::types::message::{Attachment, Message, MessageContent};

    // Convert Message list to API JSON format
    let mut api_messages: Vec<serde_json::Value> = params
        .messages
        .iter()
        .filter_map(|msg| match msg {
            Message::User(u) => {
                let content = match &u.content {
                    MessageContent::Text(t) => serde_json::json!(t),
                    MessageContent::Blocks(blocks) => {
                        serde_json::to_value(blocks).unwrap_or_default()
                    }
                };
                Some(serde_json::json!({
                    "role": "user",
                    "content": content,
                }))
            }
            Message::Assistant(a) => {
                let content = serde_json::to_value(&a.content).unwrap_or_default();
                Some(serde_json::json!({
                    "role": "assistant",
                    "content": content,
                }))
            }
            Message::Attachment(attachment) => match &attachment.attachment {
                Attachment::QueuedCommand { prompt, .. } => Some(serde_json::json!({
                    "role": "user",
                    "content": prompt,
                })),
                Attachment::NestedMemory { path, content } => Some(serde_json::json!({
                    "role": "user",
                    "content": format!("[Memory from {}]:\n{}", path, content),
                })),
                Attachment::EditedTextFile { .. }
                | Attachment::MaxTurnsReached { .. }
                | Attachment::StructuredOutput { .. }
                | Attachment::HookStoppedContinuation
                | Attachment::SkillDiscovery { .. } => None,
            },
            // System and Progress messages are not sent to the API.
            _ => None,
        })
        .collect();

    // Convert system prompt parts into API format
    let (system, system_marker_count) = build_system_prompt_blocks(&params.system_prompt);
    let message_marker_budget = 4usize.saturating_sub(system_marker_count).min(1);
    if message_marker_budget == 0 {
        prompt_cache_diagnostic("message cache marker omitted: marker budget exhausted by system");
    } else {
        add_message_cache_marker(&mut api_messages, params.skip_cache_write == Some(true));
    }

    // Convert tools to API JSON format.
    let tools: Option<Vec<serde_json::Value>> = if params.tools.is_empty() {
        None
    } else {
        Some(
            params
                .tools
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "name": t.name(),
                        "description": "",
                        "input_schema": t.input_json_schema(),
                    })
                })
                .collect(),
        )
    };

    // Build thinking, output_config, and reasoning_effort from the
    // provider-aware resolved effort when present. The legacy scalar fields
    // (`effort_value`, `output_config`, `model_reasoning_effort`) are kept as
    // a compatibility fallback for callers that have not been migrated to
    // populate `resolved_effort` yet; they should never be set in production
    // once Phase B is fully landed.
    let (thinking, output_config, reasoning_effort) = match &params.resolved_effort {
        Some(resolved) => {
            // Thinking budget is only derived for the Anthropic fixed-budget
            // transport. Codex / passthrough never carry a local budget.
            let thinking = params.thinking_enabled.and_then(|enabled| {
                if enabled {
                    let budget = resolved.local_budget_tokens.unwrap_or_else(|| {
                        let max_tokens_fallback = params
                            .max_output_tokens
                            .map(|n| n.min(u32::MAX as usize) as u32);
                        crate::effort::resolve_thinking_budget(
                            params.effort_value.as_deref(),
                            max_tokens_fallback,
                        )
                    });
                    Some(serde_json::json!({
                        "type": "enabled",
                        "budget_tokens": budget,
                    }))
                } else {
                    None
                }
            });

            // output_config.effort is the Anthropic-compat wire field. Only
            // emit it for the Anthropic transport; emitting it for Codex is
            // the historical cross-contamination this refactor fixes.
            let output_config = if resolved.writes_output_config_effort() {
                let mut object: serde_json::Map<String, serde_json::Value> = params
                    .output_config
                    .clone()
                    .and_then(|value| match value {
                        serde_json::Value::Object(map) => Some(map),
                        _ => None,
                    })
                    .unwrap_or_default();
                object.insert(
                    "effort".to_string(),
                    serde_json::Value::String(resolved.level.clone()),
                );
                Some(serde_json::Value::Object(object))
            } else {
                // Preserve any non-effort keys the caller may have placed in
                // output_config (e.g. future Anthropic flags) without letting
                // effort leak onto the Codex wire.
                params.output_config.clone().and_then(|value| match value {
                    serde_json::Value::Object(map)
                        if !map.contains_key("effort") && !map.is_empty() =>
                    {
                        Some(serde_json::Value::Object(map))
                    }
                    _ => None,
                })
            };

            // reasoning_effort (Codex `reasoning.effort`) is only emitted for
            // the Codex transport.
            let reasoning_effort = if resolved.writes_reasoning_effort()
                && resolved.level != crate::effort::ResolvedEffort::AUTO_LABEL
            {
                Some(resolved.level.clone())
            } else {
                None
            };

            (thinking, output_config, reasoning_effort)
        }
        None => {
            // Legacy fallback path (no resolved_effort provided).
            let thinking = params.thinking_enabled.and_then(|enabled| {
                if enabled {
                    let max_tokens_fallback = params
                        .max_output_tokens
                        .map(|n| n.min(u32::MAX as usize) as u32);
                    let budget = crate::effort::resolve_thinking_budget(
                        params.effort_value.as_deref(),
                        max_tokens_fallback,
                    );
                    Some(serde_json::json!({
                        "type": "enabled",
                        "budget_tokens": budget,
                    }))
                } else {
                    None
                }
            });
            let output_config =
                build_output_config(params.output_config.clone(), params.effort_value.as_deref());
            let reasoning_effort = resolve_model_reasoning_effort(
                params.model_reasoning_effort.as_deref(),
                params.effort_value.as_deref(),
            );
            (thinking, output_config, reasoning_effort)
        }
    };

    let resolved_model = params
        .model
        .clone()
        .unwrap_or_else(allthecodes_types::models::default_fallback_model_id);

    allthecodes_api::api::client::MessagesRequest {
        max_tokens: clamp_max_tokens_for_model(
            params.max_output_tokens.unwrap_or(16384),
            &resolved_model,
        ),
        model: resolved_model,
        messages: api_messages,
        system,
        tools,
        stream: true,
        metadata: None,
        service_tier: None,
        stop_sequences: None,
        temperature: None,
        top_p: None,
        top_k: None,
        context_management: None,
        thinking,
        output_config,
        tool_choice: None,
        reasoning_effort,
        advisor_model: params.advisor_model.clone(),
    }
}

fn resolve_model_reasoning_effort(
    explicit: Option<&str>,
    effort_value: Option<&str>,
) -> Option<String> {
    explicit
        .and_then(normalize_model_reasoning_effort)
        .or_else(|| codex_effort_from_effort_level(effort_value))
}

fn normalize_model_reasoning_effort(value: &str) -> Option<String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max" => {
            Some(value.trim().to_ascii_lowercase())
        }
        _ => None,
    }
}

fn codex_effort_from_effort_level(value: Option<&str>) -> Option<String> {
    match value?.trim().to_ascii_lowercase().as_str() {
        "low" => Some("low".to_string()),
        "medium" | "med" => Some("medium".to_string()),
        "high" => Some("high".to_string()),
        "max" => Some("xhigh".to_string()),
        _ => None,
    }
}

fn build_output_config(
    configured: Option<serde_json::Value>,
    effort_value: Option<&str>,
) -> Option<serde_json::Value> {
    let mut object = configured
        .and_then(|value| match value {
            serde_json::Value::Object(map) => Some(map),
            _ => None,
        })
        .unwrap_or_default();

    if let Some(existing) = object.get("effort") {
        match crate::effort::normalize_output_effort_json(existing) {
            Some(effort) => {
                object.insert("effort".to_string(), serde_json::Value::String(effort));
            }
            None => {
                object.remove("effort");
            }
        }
    }

    if let Some(effort) = effort_value.and_then(crate::effort::normalize_output_effort_value) {
        object.insert("effort".to_string(), serde_json::Value::String(effort));
    }

    (!object.is_empty()).then_some(serde_json::Value::Object(object))
}

fn default_cache_marker() -> serde_json::Value {
    allthecodes_api::api::client::prompt_cache_marker_value(
        allthecodes_api::api::client::PromptCachePolicy {
            enabled: true,
            ttl_1h: false,
            global_scope: false,
        },
    )
}

fn build_system_prompt_blocks(parts: &[String]) -> (Option<Vec<serde_json::Value>>, usize) {
    if parts.is_empty() {
        return (None, 0);
    }

    let mut prefix = Vec::new();
    let mut suffix = Vec::new();
    let mut in_dynamic_suffix = false;
    for part in parts {
        if part == crate::prompt_sections::DYNAMIC_BOUNDARY {
            in_dynamic_suffix = true;
            continue;
        }
        if in_dynamic_suffix {
            suffix.push(part.as_str());
        } else {
            prefix.push(part.as_str());
        }
    }

    let mut blocks = Vec::new();
    let mut marker_count = 0usize;
    let prefix_text = prefix.join("\n\n");
    if !prefix_text.is_empty() {
        let mut block = serde_json::json!({"type": "text", "text": prefix_text});
        block["cache_control"] = default_cache_marker();
        marker_count = 1;
        blocks.push(block);
    }
    let suffix_text = suffix.join("\n\n");
    if !suffix_text.is_empty() {
        blocks.push(serde_json::json!({"type": "text", "text": suffix_text}));
    }
    if blocks.is_empty() {
        (None, marker_count)
    } else {
        (Some(blocks), marker_count)
    }
}

fn add_message_cache_marker(messages: &mut [serde_json::Value], skip_cache_write: bool) {
    let eligible: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter_map(|(idx, message)| {
            (message.get("role").and_then(serde_json::Value::as_str) == Some("user")
                && message_content_is_cache_eligible(message.get("content")))
            .then_some(idx)
        })
        .collect();

    let target = if skip_cache_write {
        if eligible.len() < 2 {
            prompt_cache_diagnostic(
                "message cache marker omitted: skip_cache_write requires two eligible user messages",
            );
            return;
        }
        eligible[eligible.len() - 2]
    } else {
        match eligible.last().copied() {
            Some(idx) => idx,
            None => {
                prompt_cache_diagnostic("message cache marker omitted: no eligible user message");
                return;
            }
        }
    };

    if !add_cache_marker_to_message_content(&mut messages[target]) {
        prompt_cache_diagnostic("message cache marker omitted: eligible message had no text block");
    }
}

fn message_content_is_cache_eligible(content: Option<&serde_json::Value>) -> bool {
    match content {
        Some(serde_json::Value::String(text)) => !text.is_empty(),
        Some(serde_json::Value::Array(blocks)) => blocks.iter().any(|block| {
            block.get("type").and_then(serde_json::Value::as_str) == Some("text")
                && block
                    .get("text")
                    .and_then(serde_json::Value::as_str)
                    .map(|text| !text.is_empty())
                    .unwrap_or(false)
        }),
        _ => false,
    }
}

fn add_cache_marker_to_message_content(message: &mut serde_json::Value) -> bool {
    let Some(content) = message.get_mut("content") else {
        return false;
    };
    match content {
        serde_json::Value::String(text) => {
            let text = std::mem::take(text);
            *content = serde_json::Value::Array(vec![serde_json::json!({
                "type": "text",
                "text": text,
                "cache_control": default_cache_marker(),
            })]);
            true
        }
        serde_json::Value::Array(blocks) => {
            let Some(target) = blocks.iter_mut().rfind(|block| {
                block.get("type").and_then(serde_json::Value::as_str) == Some("text")
                    && block
                        .get("text")
                        .and_then(serde_json::Value::as_str)
                        .map(|text| !text.is_empty())
                        .unwrap_or(false)
            }) else {
                return false;
            };
            target["cache_control"] = default_cache_marker();
            true
        }
        _ => false,
    }
}

fn prompt_cache_diagnostic(message: &'static str) {
    if allthecodes_api::api::client::is_env_truthy("ALLTHECODES_PROMPT_CACHE_BREAK_DETECTION")
        || allthecodes_api::api::client::is_env_truthy("CC_RUST_PROMPT_CACHE_BREAK_DETECTION")
    {
        tracing::debug!(message, "prompt cache break detection");
    }
}

/// Some OpenAI-compatible providers cap `max_tokens` below allthecodes' default
/// 16384. Rather than rely on provider-side errors surfacing as a blown
/// response, clamp at build time so the first request also succeeds.
pub(crate) fn clamp_max_tokens_for_model(requested: usize, model: &str) -> usize {
    // Keep the rules here narrow and documented; only add an entry when a
    // provider has confirmed, consistent behaviour.
    let lower = model.to_ascii_lowercase();
    let cap: Option<usize> = if lower.starts_with("deepseek") {
        // Confirmed via https://api.deepseek.com: valid range is [1, 8192].
        Some(8192)
    } else {
        None
    };
    match cap {
        Some(c) => requested.min(c),
        None => requested,
    }
}

#[cfg(test)]
mod clamp_tests {
    use super::clamp_max_tokens_for_model;

    #[test]
    fn deepseek_is_capped_at_8192() {
        assert_eq!(clamp_max_tokens_for_model(16384, "deepseek-chat"), 8192);
        assert_eq!(clamp_max_tokens_for_model(20000, "DeepSeek-Reasoner"), 8192);
    }

    #[test]
    fn deepseek_below_cap_unchanged() {
        assert_eq!(clamp_max_tokens_for_model(4096, "deepseek-chat"), 4096);
    }

    #[test]
    fn non_deepseek_unchanged() {
        assert_eq!(
            clamp_max_tokens_for_model(16384, "claude-sonnet-4-20250514"),
            16384
        );
        assert_eq!(clamp_max_tokens_for_model(32000, "gpt-4o"), 32000);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::message::{
        Attachment, AttachmentMessage, Message, MessageContent, UserMessage,
    };
    use allthecodes_engine::query::deps::ModelCallParams;
    use uuid::Uuid;

    fn base_params() -> ModelCallParams {
        ModelCallParams {
            messages: vec![],
            system_prompt: vec!["sys".into()],
            tools: vec![],
            model: Some("claude-sonnet-4-20250514".into()),
            max_output_tokens: Some(16_384),
            skip_cache_write: None,
            thinking_enabled: Some(true),
            effort_value: None,
            output_config: None,
            model_reasoning_effort: None,
            resolved_effort: None,
            advisor_model: None,
        }
    }

    fn user_text(text: &str) -> Message {
        Message::User(UserMessage {
            uuid: Uuid::nil(),
            timestamp: 0,
            role: "user".to_string(),
            content: MessageContent::Text(text.to_string()),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        })
    }

    fn marker_count(value: &serde_json::Value) -> usize {
        match value {
            serde_json::Value::Object(map) => {
                usize::from(map.contains_key("cache_control"))
                    + map.values().map(marker_count).sum::<usize>()
            }
            serde_json::Value::Array(values) => values.iter().map(marker_count).sum(),
            _ => 0,
        }
    }

    #[test]
    fn system_prompt_split_marks_static_prefix_only() {
        let mut p = base_params();
        p.system_prompt = vec![
            "static".into(),
            crate::prompt_sections::DYNAMIC_BOUNDARY.into(),
            "dynamic".into(),
        ];
        p.messages = vec![user_text("hello")];

        let req = build_messages_request(&p);
        let system = req.system.unwrap();
        assert_eq!(system.len(), 2);
        assert_eq!(system[0]["text"], "static");
        assert_eq!(system[0]["cache_control"]["type"], "ephemeral");
        assert_eq!(system[1]["text"], "dynamic");
        assert!(system[1].get("cache_control").is_none());
    }

    #[test]
    fn skip_cache_write_marks_second_to_last_eligible_user_message() {
        let mut p = base_params();
        p.system_prompt.clear();
        p.messages = vec![user_text("first"), user_text("second"), user_text("third")];
        p.skip_cache_write = Some(true);

        let req = build_messages_request(&p);
        assert!(req.messages[0]["content"][0].get("cache_control").is_none());
        assert_eq!(
            req.messages[1]["content"][0]["cache_control"]["type"],
            "ephemeral"
        );
        assert!(req.messages[2]["content"][0].get("cache_control").is_none());
    }

    #[test]
    fn skip_cache_write_omits_message_marker_with_one_eligible_message() {
        let mut p = base_params();
        p.system_prompt.clear();
        p.messages = vec![user_text("only")];
        p.skip_cache_write = Some(true);

        let req = build_messages_request(&p);
        let body = serde_json::to_value(&req).unwrap();
        assert_eq!(marker_count(&body), 0);
    }

    #[test]
    fn marker_budget_stays_at_system_plus_one_message() {
        let mut p = base_params();
        p.system_prompt = vec![
            "static".into(),
            crate::prompt_sections::DYNAMIC_BOUNDARY.into(),
            "dynamic".into(),
        ];
        p.messages = vec![user_text("first"), user_text("second")];

        let body = serde_json::to_value(build_messages_request(&p)).unwrap();
        assert_eq!(marker_count(&body), 2);
    }

    #[test]
    fn thinking_budget_uses_effort_high_over_max_tokens() {
        let mut p = base_params();
        p.effort_value = Some("high".into());
        p.max_output_tokens = Some(99_999);

        let req = build_messages_request(&p);
        let thinking = req.thinking.expect("thinking config present");
        assert_eq!(thinking["type"], "enabled");
        assert_eq!(thinking["budget_tokens"], 24_576);
    }

    #[test]
    fn thinking_budget_falls_back_to_max_tokens_when_effort_missing() {
        let mut p = base_params();
        p.effort_value = None;
        p.max_output_tokens = Some(8_000);

        let req = build_messages_request(&p);
        let thinking = req.thinking.expect("thinking config present");
        assert_eq!(thinking["budget_tokens"], 8_000);
    }

    #[test]
    fn thinking_budget_accepts_numeric_effort_override() {
        let mut p = base_params();
        p.effort_value = Some("12345".into());
        p.max_output_tokens = Some(8_000);

        let req = build_messages_request(&p);
        let thinking = req.thinking.expect("thinking config present");
        assert_eq!(thinking["budget_tokens"], 12_345);
    }

    #[test]
    fn output_config_effort_uses_runtime_effort_value() {
        let mut p = base_params();
        p.effort_value = Some("max".into());

        let req = build_messages_request(&p);
        let output_config = req.output_config.expect("output_config present");
        assert_eq!(output_config["effort"], "max");
    }

    #[test]
    fn output_config_effort_preserves_settings_when_no_runtime_override() {
        let mut p = base_params();
        p.effort_value = None;
        p.output_config = Some(serde_json::json!({
            "effort": "high",
            "format": {"type": "json_schema"}
        }));

        let req = build_messages_request(&p);
        let output_config = req.output_config.expect("output_config present");
        assert_eq!(output_config["effort"], "high");
        assert_eq!(output_config["format"]["type"], "json_schema");
    }

    #[test]
    fn output_config_effort_runtime_value_overrides_settings() {
        let mut p = base_params();
        p.effort_value = Some("low".into());
        p.output_config = Some(serde_json::json!({
            "effort": "high",
            "format": {"type": "json_schema"}
        }));

        let req = build_messages_request(&p);
        let output_config = req.output_config.expect("output_config present");
        assert_eq!(output_config["effort"], "high");
        assert_eq!(output_config["format"]["type"], "json_schema");
    }

    #[test]
    fn output_config_effort_maps_settings_aliases() {
        let mut p = base_params();
        p.effort_value = None;
        p.output_config = Some(serde_json::json!({"effort": "medium"}));
        let req = build_messages_request(&p);
        assert_eq!(req.output_config.unwrap()["effort"], "high");

        p.output_config = Some(serde_json::json!({"effort": "xhigh"}));
        let req = build_messages_request(&p);
        assert_eq!(req.output_config.unwrap()["effort"], "max");

        p.output_config = Some(serde_json::json!({"effort": "ultra"}));
        let req = build_messages_request(&p);
        assert_eq!(req.output_config.unwrap()["effort"], "max");
    }

    #[test]
    fn codex_reasoning_effort_prefers_explicit_setting() {
        let mut p = base_params();
        p.model_reasoning_effort = Some("max".into());
        p.effort_value = Some("low".into());

        let req = build_messages_request(&p);
        assert_eq!(req.reasoning_effort.as_deref(), Some("max"));
    }

    #[test]
    fn codex_reasoning_effort_falls_back_to_effort_label() {
        let mut p = base_params();
        p.effort_value = Some("max".into());

        let req = build_messages_request(&p);
        assert_eq!(req.reasoning_effort.as_deref(), Some("xhigh"));
    }

    #[test]
    fn codex_reasoning_effort_ignores_numeric_budget() {
        let mut p = base_params();
        p.effort_value = Some("12345".into());

        let req = build_messages_request(&p);
        assert!(req.reasoning_effort.is_none());
    }

    #[test]
    fn thinking_omitted_when_disabled() {
        let mut p = base_params();
        p.thinking_enabled = Some(false);
        p.effort_value = Some("high".into());

        let req = build_messages_request(&p);
        assert!(req.thinking.is_none());
    }

    #[test]
    fn thinking_omitted_when_unset() {
        let mut p = base_params();
        p.thinking_enabled = None;
        p.effort_value = Some("high".into());

        let req = build_messages_request(&p);
        assert!(req.thinking.is_none());
    }

    #[test]
    fn attachment_policy_sends_only_model_context_attachments_to_api() {
        let mut p = base_params();
        p.messages = vec![
            Message::Attachment(AttachmentMessage {
                uuid: uuid::Uuid::new_v4(),
                timestamp: 0,
                attachment: Attachment::QueuedCommand {
                    prompt: "run this next".to_string(),
                    source_uuid: Some("source".to_string()),
                },
            }),
            Message::Attachment(AttachmentMessage {
                uuid: uuid::Uuid::new_v4(),
                timestamp: 0,
                attachment: Attachment::NestedMemory {
                    path: "CLAUDE.md".to_string(),
                    content: "remember this".to_string(),
                },
            }),
            Message::Attachment(AttachmentMessage {
                uuid: uuid::Uuid::new_v4(),
                timestamp: 0,
                attachment: Attachment::EditedTextFile {
                    path: "src/main.rs".to_string(),
                },
            }),
            Message::Attachment(AttachmentMessage {
                uuid: uuid::Uuid::new_v4(),
                timestamp: 0,
                attachment: Attachment::SkillDiscovery {
                    skills: vec!["rust".to_string()],
                },
            }),
        ];

        let req = build_messages_request(&p);

        assert_eq!(req.messages.len(), 2);
        assert_eq!(req.messages[0]["role"], "user");
        assert_eq!(req.messages[0]["content"], "run this next");
        assert_eq!(
            req.messages[1]["content"][0]["text"],
            "[Memory from CLAUDE.md]:\nremember this"
        );
        assert_eq!(
            req.messages[1]["content"][0]["cache_control"]["type"],
            "ephemeral"
        );
    }
}
