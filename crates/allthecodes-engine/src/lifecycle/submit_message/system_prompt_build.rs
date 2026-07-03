use std::sync::Arc;

use crate::runtime_services::RuntimeServices;
use crate::system_prompt;
use crate::types::message::{ContentBlock, Message, MessageContent};

use super::super::QueryEngineState;
use super::memory_recall::{
    is_model_assisted_memory_recall_enabled, resolve_memory_context_override,
};

pub(super) struct SubmitSystemPrompt {
    pub(super) system_prompt_parts: Vec<String>,
    pub(super) user_context: std::collections::HashMap<String, String>,
    pub(super) system_context: std::collections::HashMap<String, String>,
}

#[expect(
    clippy::too_many_arguments,
    reason = "prompt assembly combines config, live state, hooks, tools, and model metadata"
)]
pub(super) async fn build_submit_system_prompt(
    prompt: &str,
    config: &crate::types::config::QueryEngineConfig,
    session_id: &crate::bootstrap::SessionId,
    state_ref: &Arc<parking_lot::RwLock<QueryEngineState>>,
    hook_runner: &Arc<dyn allthecodes_types::hooks::HookRunner>,
    tools_snapshot: &crate::types::tool::Tools,
    model_name: &str,
    backend_name: &str,
    runtime_services: &Arc<RuntimeServices>,
) -> SubmitSystemPrompt {
    // Pull live language/output_style off AppState so /config set takes effect
    // on the next submit without restarting the engine.
    let (
        cfg_language,
        cfg_output_style,
        cfg_system_prompt,
        include_auto_memory,
        session_memory_context,
        memory_query_text,
        recent_tool_names,
        already_surfaced_memory_keys,
        model_assisted_memory_recall,
    ) = {
        let state = state_ref.read();
        (
            state.app_state.settings.language.clone(),
            state.app_state.settings.output_style.clone(),
            state.app_state.settings.system_prompt.clone(),
            state
                .app_state
                .settings
                .auto_memory_enabled
                .unwrap_or(false),
            state
                .runtime
                .session_memory
                .format_memory_context_for_workspace_excluding_session(
                    5,
                    Some(std::path::Path::new(&config.cwd)),
                    Some(session_id.as_str()),
                ),
            latest_user_query_text(&state.transcript.messages)
                .unwrap_or_else(|| prompt.to_string()),
            recent_tool_names(&state.transcript.messages, 8),
            state.app_state.surfaced_memory_keys.clone(),
            is_model_assisted_memory_recall_enabled(),
        )
    };

    let ignore_memory =
        allthecodes_session::memdir::query_requests_memory_ignore(&memory_query_text);
    let session_memory_context = if ignore_memory {
        None
    } else {
        session_memory_context
    };
    let (memory_context_override, newly_surfaced_memory_keys) = resolve_memory_context_override(
        config,
        include_auto_memory,
        &memory_query_text,
        &recent_tool_names,
        &already_surfaced_memory_keys,
        backend_name,
        model_name,
        runtime_services.model_client_factory.as_ref(),
        model_assisted_memory_recall,
        ignore_memory,
    )
    .await;

    if !newly_surfaced_memory_keys.is_empty() {
        state_ref
            .write()
            .app_state
            .surfaced_memory_keys
            .extend(newly_surfaced_memory_keys);
    }

    let custom_system_prompt = system_prompt::select_custom_system_prompt(
        config.custom_system_prompt.as_deref(),
        cfg_system_prompt.as_deref(),
    );

    let (system_prompt_parts, user_context, system_context) =
        system_prompt::build_system_prompt_with_memory_contexts(
            custom_system_prompt,
            config.append_system_prompt.as_deref(),
            tools_snapshot,
            model_name,
            &config.cwd,
            cfg_language.as_deref(),
            cfg_output_style.as_deref(),
            include_auto_memory,
            memory_context_override.as_deref(),
            session_memory_context.as_deref(),
        );

    fire_instructions_loaded_hook(state_ref, hook_runner, &system_prompt_parts, &config.cwd).await;

    SubmitSystemPrompt {
        system_prompt_parts,
        user_context,
        system_context,
    }
}

async fn fire_instructions_loaded_hook(
    state_ref: &Arc<parking_lot::RwLock<QueryEngineState>>,
    hook_runner: &Arc<dyn allthecodes_types::hooks::HookRunner>,
    system_prompt_parts: &[String],
    cwd: &str,
) {
    let content_length: usize = system_prompt_parts.iter().map(|part| part.len()).sum();
    if content_length == 0 {
        return;
    }

    let hooks_map = state_ref.read().app_state.hooks.clone();
    let configs = hook_runner.load_hook_configs(&hooks_map, "InstructionsLoaded");
    if !configs.is_empty() {
        let payload = serde_json::json!({
            "source": "system_prompt",
            "content_length": content_length,
            "cwd": cwd,
        });
        let _ = hook_runner
            .run_event_hooks("InstructionsLoaded", &payload, &configs)
            .await;
    }
}

fn latest_user_query_text(messages: &[Message]) -> Option<String> {
    messages.iter().rev().find_map(|message| {
        let Message::User(user) = message else {
            return None;
        };
        if user.is_meta {
            return None;
        }
        user_message_text(&user.content).filter(|text| !text.trim().is_empty())
    })
}

fn user_message_text(content: &MessageContent) -> Option<String> {
    match content {
        MessageContent::Text(text) => Some(text.clone()),
        MessageContent::Blocks(blocks) => {
            let text = blocks
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            (!text.trim().is_empty()).then_some(text)
        }
    }
}

fn recent_tool_names(messages: &[Message], limit: usize) -> Vec<String> {
    let mut names = Vec::new();
    for message in messages.iter().rev() {
        let Message::Assistant(assistant) = message else {
            continue;
        };
        for block in assistant.content.iter().rev() {
            let name = match block {
                ContentBlock::ToolUse { name, .. } | ContentBlock::ServerToolUse { name, .. } => {
                    name
                }
                _ => continue,
            };
            if !names.iter().any(|existing| existing == name) {
                names.push(name.clone());
                if names.len() >= limit {
                    return names;
                }
            }
        }
    }
    names
}
