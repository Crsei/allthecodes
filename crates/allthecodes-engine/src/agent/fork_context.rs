//! Context construction for explicit AgentTool forks.

use std::collections::HashSet;

use allthecodes_types::agent_types::{
    ForkContextMode, ForkLaunchMetadata, FORK_ACTIVE_TOOL_PLACEHOLDER, FORK_BOILERPLATE_TAG,
};
use anyhow::Result;
use uuid::Uuid;

use crate::types::config::ThinkingConfig;
use crate::types::message::{
    AssistantMessage, ContentBlock, Message, MessageContent, ToolResultContent, UserMessage,
};
use crate::types::tool::{ToolUseContext, Tools};

use super::live_parent_context;

/// Fully prepared fork state. All spawn paths consume this same value so
/// context behavior cannot drift between synchronous, background, and
/// worktree agents.
#[derive(Clone)]
pub(super) struct PreparedForkLaunch {
    pub initial_messages: Vec<Message>,
    pub child_prompt: String,
    pub metadata: ForkLaunchMetadata,
    pub persistent_reminder: Option<String>,
    pub parent_tools: Tools,
    pub thinking_config: Option<ThinkingConfig>,
}

pub(super) fn prepare_fork_launch(
    mode: ForkContextMode,
    ctx: &ToolUseContext,
    current_assistant: &AssistantMessage,
    agent_id: &str,
    requested_task: &str,
) -> Result<PreparedForkLaunch> {
    let full_snapshot = build_full_snapshot(&ctx.messages, current_assistant);
    let parent_runtime = ctx.parent_runtime_snapshot();
    let app_state = (ctx.get_app_state)();
    let thinking_config = app_state.thinking_enabled.map(|enabled| {
        if enabled {
            ThinkingConfig::Adaptive
        } else {
            ThinkingConfig::Disabled
        }
    });
    let mut metadata = ForkLaunchMetadata {
        is_fork: true,
        context: mode,
        live_channel: None,
    };

    let (initial_messages, persistent_reminder) = match mode {
        ForkContextMode::FullSnapshot => (full_snapshot, None),
        ForkContextMode::LastOutput => (
            latest_parent_output(&ctx.messages, current_assistant)
                .into_iter()
                .collect(),
            None,
        ),
        ForkContextMode::LiveReadonly => {
            let rendered = render_parent_context(&full_snapshot);
            let paths = live_parent_context::create_channel(&ctx.session_id, agent_id, &rendered)?;
            let reminder = format!(
                "This fork has a read-only live parent context channel. Before later model/tool turns, read {} or {} when current parent state matters. Do not write to these files.",
                paths.snapshot, paths.updates
            );
            metadata.live_channel = Some(paths);
            (Vec::new(), Some(reminder))
        }
    };

    let child_prompt = build_fork_directive(
        &metadata,
        requested_task,
        parent_runtime.query_source.as_deref(),
    );
    Ok(PreparedForkLaunch {
        initial_messages,
        child_prompt,
        metadata,
        persistent_reminder,
        parent_tools: ctx.available_tools.clone(),
        thinking_config,
    })
}

pub(super) fn build_full_snapshot(
    parent_messages: &[Message],
    current_assistant: &AssistantMessage,
) -> Vec<Message> {
    let mut messages = filter_incomplete_history(parent_messages);
    if !messages
        .iter()
        .any(|message| message.uuid() == current_assistant.uuid)
    {
        messages.push(Message::Assistant(current_assistant.clone()));
    }

    let tool_use_ids = tool_use_ids(&messages);
    let result_ids = tool_result_ids(&messages);
    let missing: Vec<_> = tool_use_ids
        .into_iter()
        .filter(|tool_use_id| !result_ids.contains(tool_use_id))
        .collect();
    if !missing.is_empty() {
        messages.push(Message::User(UserMessage {
            uuid: Uuid::new_v4(),
            timestamp: chrono::Utc::now().timestamp(),
            role: "user".to_string(),
            content: MessageContent::Blocks(
                missing
                    .into_iter()
                    .map(|tool_use_id| ContentBlock::ToolResult {
                        tool_use_id,
                        content: ToolResultContent::Text(FORK_ACTIVE_TOOL_PLACEHOLDER.to_string()),
                        is_error: false,
                    })
                    .collect(),
            ),
            is_meta: true,
            tool_use_result: Some(FORK_ACTIVE_TOOL_PLACEHOLDER.to_string()),
            source_tool_assistant_uuid: Some(current_assistant.uuid),
        }));
    }
    messages
}

fn filter_incomplete_history(parent_messages: &[Message]) -> Vec<Message> {
    let known_tool_uses: HashSet<_> = tool_use_ids(parent_messages).into_iter().collect();
    parent_messages
        .iter()
        .filter_map(|message| match message {
            Message::Progress(_) | Message::Attachment(_) => None,
            Message::User(user) => match &user.content {
                MessageContent::Blocks(blocks) => {
                    let filtered: Vec<_> = blocks
                        .iter()
                        .filter(|block| match block {
                            ContentBlock::ToolResult { tool_use_id, .. } => {
                                known_tool_uses.contains(tool_use_id)
                            }
                            _ => true,
                        })
                        .cloned()
                        .collect();
                    if filtered.is_empty() {
                        None
                    } else {
                        let mut cloned = user.clone();
                        cloned.content = MessageContent::Blocks(filtered);
                        Some(Message::User(cloned))
                    }
                }
                MessageContent::Text(_) => Some(message.clone()),
            },
            _ => Some(message.clone()),
        })
        .collect()
}

fn latest_parent_output(
    parent_messages: &[Message],
    current_assistant: &AssistantMessage,
) -> Option<Message> {
    let current_text = assistant_visible_text(current_assistant);
    if !current_text.trim().is_empty() {
        return Some(text_only_assistant(current_assistant, current_text));
    }

    parent_messages
        .iter()
        .rev()
        .find_map(|message| match message {
            Message::Assistant(assistant) => {
                let text = assistant_visible_text(assistant);
                (!text.trim().is_empty()).then(|| text_only_assistant(assistant, text))
            }
            _ => None,
        })
}

fn text_only_assistant(source: &AssistantMessage, text: String) -> Message {
    let mut assistant = source.clone();
    assistant.content = vec![ContentBlock::Text { text }];
    Message::Assistant(assistant)
}

fn tool_use_ids(messages: &[Message]) -> Vec<String> {
    messages
        .iter()
        .filter_map(|message| match message {
            Message::Assistant(assistant) => Some(&assistant.content),
            _ => None,
        })
        .flatten()
        .filter_map(|block| match block {
            ContentBlock::ToolUse { id, .. } | ContentBlock::ServerToolUse { id, .. } => {
                Some(id.clone())
            }
            _ => None,
        })
        .collect()
}

fn tool_result_ids(messages: &[Message]) -> HashSet<String> {
    messages
        .iter()
        .filter_map(|message| match message {
            Message::User(user) => match &user.content {
                MessageContent::Blocks(blocks) => Some(blocks),
                MessageContent::Text(_) => None,
            },
            _ => None,
        })
        .flatten()
        .filter_map(|block| match block {
            ContentBlock::ToolResult { tool_use_id, .. } => Some(tool_use_id.clone()),
            _ => None,
        })
        .collect()
}

fn build_fork_directive(
    metadata: &ForkLaunchMetadata,
    requested_task: &str,
    parent_query_source: Option<&str>,
) -> String {
    let inheritance = match metadata.context {
        ForkContextMode::FullSnapshot => {
            "You inherited a snapshot of the full parent conversation at spawn time.".to_string()
        }
        ForkContextMode::LastOutput => {
            "You inherited only the parent's latest visible assistant output.".to_string()
        }
        ForkContextMode::LiveReadonly => {
            let paths = metadata
                .live_channel
                .as_ref()
                .expect("live readonly metadata must include channel paths");
            format!(
                "You have a read-only live parent channel. Read snapshot `{}` and append-only updates `{}` when you need current parent state. Latest sequence at launch: {}.",
                paths.snapshot, paths.updates, paths.latest_seq
            )
        }
    };

    let source = parent_query_source
        .map(|source| format!(" Parent query source at spawn: {source}."))
        .unwrap_or_default();
    format!(
        "<{tag}>\nYou are an explicit fork worker. {inheritance}{source}\nComplete the requested task independently and return a concise result.\n\nRequested task:\n{requested_task}\n</{tag}>",
        tag = FORK_BOILERPLATE_TAG
    )
}

pub(super) fn render_parent_context(messages: &[Message]) -> String {
    let mut rendered = String::from("# Parent context snapshot\n\n");
    for message in messages {
        let (role, text) = match message {
            Message::User(user) => ("user", message_content_text(&user.content)),
            Message::Assistant(assistant) => ("assistant", assistant_visible_text(assistant)),
            Message::System(system) => ("system", system.content.clone()),
            Message::Progress(_) | Message::Attachment(_) => continue,
        };
        if !text.trim().is_empty() {
            rendered.push_str(&format!("## {role}\n\n{text}\n\n"));
        }
    }
    rendered
}

pub(crate) fn assistant_visible_text(assistant: &AssistantMessage) -> String {
    assistant
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn message_content_text(content: &MessageContent) -> String {
    match content {
        MessageContent::Text(text) => text.clone(),
        MessageContent::Blocks(blocks) => blocks
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text } => Some(text.clone()),
                ContentBlock::ToolResult { content, .. } => Some(match content {
                    ToolResultContent::Text(text) => text.clone(),
                    ToolResultContent::Blocks(_) => "[structured tool result]".to_string(),
                }),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assistant(text: &str, tool_id: Option<&str>) -> AssistantMessage {
        let mut content = vec![ContentBlock::Text {
            text: text.to_string(),
        }];
        if let Some(id) = tool_id {
            content.push(ContentBlock::ToolUse {
                id: id.to_string(),
                name: "Agent".to_string(),
                input: serde_json::json!({}),
            });
        }
        AssistantMessage {
            uuid: Uuid::new_v4(),
            timestamp: 1,
            role: "assistant".to_string(),
            content,
            usage: None,
            stop_reason: None,
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        }
    }

    #[test]
    fn full_snapshot_keeps_history_and_completes_active_tools() {
        let prior = assistant("prior", None);
        let current = assistant("current", Some("active"));
        let snapshot = build_full_snapshot(&[Message::Assistant(prior)], &current);
        assert_eq!(snapshot.len(), 3);
        assert!(tool_result_ids(&snapshot).contains("active"));
    }

    #[test]
    fn last_output_excludes_earlier_history() {
        let prior = assistant("prior", None);
        let current = assistant("current", None);
        let inherited = latest_parent_output(&[Message::Assistant(prior)], &current).unwrap();
        let Message::Assistant(inherited) = inherited else {
            panic!("expected assistant output")
        };
        assert_eq!(assistant_visible_text(&inherited), "current");
    }

    #[test]
    fn live_directive_injects_channel_paths_and_sequence() {
        let metadata = ForkLaunchMetadata {
            is_fork: true,
            context: ForkContextMode::LiveReadonly,
            live_channel: Some(allthecodes_types::agent_types::LiveParentContextPaths {
                directory: "/tmp/fork".to_string(),
                snapshot: "/tmp/fork/parent-context.md".to_string(),
                updates: "/tmp/fork/parent-updates.ndjson".to_string(),
                latest_diff: Some("/tmp/fork/parent-context.diff".to_string()),
                latest_seq: 7,
            }),
        };
        let directive = build_fork_directive(&metadata, "inspect state", Some("interactive"));
        assert!(directive.contains("parent-context.md"));
        assert!(directive.contains("parent-updates.ndjson"));
        assert!(directive.contains("Latest sequence at launch: 7"));
        assert!(directive.contains("<fork-boilerplate>"));
    }
}
