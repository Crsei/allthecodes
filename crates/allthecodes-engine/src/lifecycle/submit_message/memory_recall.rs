use std::time::Duration;

use crate::types::config::QueryEngineConfig;
use crate::types::message::{AssistantMessage, ContentBlock};

use tracing::debug;

fn model_assisted_memory_recall_enabled() -> bool {
    std::env::var("ALLTHECODES_MODEL_ASSISTED_MEMORY_RECALL")
        .or_else(|_| std::env::var("CC_RUST_MODEL_ASSISTED_MEMORY_RECALL"))
        .or_else(|_| std::env::var("ALLTHECODES_MODEL_MEMORY_RECALL"))
        .or_else(|_| std::env::var("CC_RUST_MODEL_MEMORY_RECALL"))
        .map(|value| is_truthy_model_assisted_memory_recall_value(&value))
        .unwrap_or(false)
}

fn is_truthy_model_assisted_memory_recall_value(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn model_assisted_memory_recall_timeout() -> Duration {
    let millis = std::env::var("ALLTHECODES_MODEL_ASSISTED_MEMORY_RECALL_TIMEOUT_MS")
        .or_else(|_| std::env::var("CC_RUST_MODEL_ASSISTED_MEMORY_RECALL_TIMEOUT_MS"))
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|millis| *millis > 0)
        .unwrap_or(1500);
    Duration::from_millis(millis)
}

fn deterministic_memory_context(
    cwd: &str,
    include_auto_memory: bool,
    memory_query_text: &str,
    recent_tool_names: &[String],
    already_surfaced_memory_keys: &std::collections::HashSet<String>,
) -> (Option<String>, Vec<String>) {
    match allthecodes_session::memdir::build_relevant_memory_context_with(
        std::path::Path::new(cwd),
        include_auto_memory,
        memory_query_text,
        recent_tool_names,
        already_surfaced_memory_keys,
        allthecodes_session::memdir::MODEL_ASSISTED_RECALL_MAX_RESULTS,
    ) {
        Ok((context, surfaced)) => (Some(context), surfaced),
        Err(error) => {
            debug!(
                error = %error,
                "failed to build deterministic memory context; falling back to full memory context"
            );
            (None, Vec::new())
        }
    }
}

async fn build_model_assisted_memory_context(
    cwd: &str,
    include_auto_memory: bool,
    memory_query_text: &str,
    recent_tool_names: &[String],
    already_surfaced_memory_keys: &std::collections::HashSet<String>,
    backend_name: &str,
    model_name: &str,
) -> anyhow::Result<Option<(String, Vec<String>)>> {
    let candidates = allthecodes_session::memdir::recall_relevant_memories(
        std::path::Path::new(cwd),
        include_auto_memory,
        memory_query_text,
        recent_tool_names,
        already_surfaced_memory_keys,
        allthecodes_session::memdir::MODEL_ASSISTED_RECALL_CANDIDATE_LIMIT,
    )?;
    if candidates.is_empty() {
        return Ok(Some((String::new(), Vec::new())));
    }

    let api_client = match allthecodes_api::api::client::ApiClient::from_backend(Some(backend_name))
    {
        Some(client) => client,
        None => return Ok(None),
    };
    let prompt = allthecodes_session::memdir::build_model_assisted_recall_prompt(
        memory_query_text,
        &candidates,
        allthecodes_session::memdir::MODEL_ASSISTED_RECALL_MAX_RESULTS,
    );
    let request = allthecodes_api::api::client::MessagesRequest {
        model: model_name.to_string(),
        messages: vec![serde_json::json!({
            "role": "user",
            "content": prompt,
        })],
        system: Some(vec![serde_json::json!({
            "type": "text",
            "text": "Rank memory candidates for relevance. Return only valid JSON.",
        })]),
        max_tokens: 512,
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

    let response = tokio::time::timeout(
        model_assisted_memory_recall_timeout(),
        api_client.messages(request),
    )
    .await
    .map_err(|_| anyhow::anyhow!("model-assisted memory recall timed out"))??;
    let response_text = assistant_message_text(&response);
    let identities = allthecodes_session::memdir::parse_model_assisted_recall_selection(
        &response_text,
        &candidates,
        allthecodes_session::memdir::MODEL_ASSISTED_RECALL_MAX_RESULTS,
    );

    if identities.is_empty() {
        if response_text.contains("[]") {
            return Ok(Some((String::new(), Vec::new())));
        }
        anyhow::bail!("model-assisted memory recall returned no recognized memory identities");
    }

    let selected = allthecodes_session::memdir::select_relevant_memories_by_identity(
        &candidates,
        &identities,
        allthecodes_session::memdir::MODEL_ASSISTED_RECALL_MAX_RESULTS,
    );
    if selected.is_empty() {
        anyhow::bail!("model-assisted memory recall selected no usable candidates");
    }

    let surfaced = selected
        .iter()
        .map(|memory| memory.identity.clone())
        .collect::<Vec<_>>();
    Ok(Some((
        allthecodes_session::memdir::format_relevant_memory_context(&selected),
        surfaced,
    )))
}

fn assistant_message_text(message: &AssistantMessage) -> String {
    message
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn is_model_assisted_memory_recall_enabled() -> bool {
    model_assisted_memory_recall_enabled()
}

#[expect(
    clippy::too_many_arguments,
    reason = "memory recall selection keeps the independent policy inputs explicit"
)]
pub(super) async fn resolve_memory_context_override(
    config: &QueryEngineConfig,
    include_auto_memory: bool,
    memory_query_text: &str,
    recent_tool_names: &[String],
    already_surfaced_memory_keys: &std::collections::HashSet<String>,
    backend_name: &str,
    model_name: &str,
    model_assisted_memory_recall: bool,
    ignore_memory: bool,
) -> (Option<String>, Vec<String>) {
    if ignore_memory {
        debug!("memory recall skipped because the user requested memory ignore");
        return (None, Vec::new());
    }

    if !model_assisted_memory_recall {
        debug!("using deterministic memory recall");
        return deterministic_memory_context(
            &config.cwd,
            include_auto_memory,
            memory_query_text,
            recent_tool_names,
            already_surfaced_memory_keys,
        );
    }

    match build_model_assisted_memory_context(
        &config.cwd,
        include_auto_memory,
        memory_query_text,
        recent_tool_names,
        already_surfaced_memory_keys,
        backend_name,
        model_name,
    )
    .await
    {
        Ok(Some((context, surfaced))) => {
            debug!(
                surfaced_count = surfaced.len(),
                "model-assisted memory recall completed"
            );
            (Some(context), surfaced)
        }
        Ok(None) => {
            debug!("model-assisted memory recall unavailable; using deterministic recall");
            deterministic_memory_context(
                &config.cwd,
                include_auto_memory,
                memory_query_text,
                recent_tool_names,
                already_surfaced_memory_keys,
            )
        }
        Err(error) => {
            debug!(
                error = %error,
                "model-assisted memory recall failed; using deterministic recall"
            );
            deterministic_memory_context(
                &config.cwd,
                include_auto_memory,
                memory_query_text,
                recent_tool_names,
                already_surfaced_memory_keys,
            )
        }
    }
}

#[cfg(test)]
mod model_assisted_memory_recall_tests {
    use super::*;

    #[test]
    fn truthy_model_assisted_memory_recall_values_are_explicit() {
        for value in ["1", "true", "TRUE", " yes ", "on"] {
            assert!(is_truthy_model_assisted_memory_recall_value(value));
        }

        for value in ["", "0", "false", "off", "enabled"] {
            assert!(!is_truthy_model_assisted_memory_recall_value(value));
        }
    }
}
