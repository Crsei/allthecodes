//! /compact command -- triggers conversation context compaction.
//!
//! Compacts the conversation by summarizing messages to reduce token usage.
//! Supports whole-conversation local compaction plus deterministic partial
//! compaction around a selected anchor.

use anyhow::Result;
use async_trait::async_trait;

use allthecodes_compact::{compaction, partial_compact, pipeline};
use allthecodes_types::message::{ContentBlock, Message, MessageContent};
use allthecodes_utils::tokens;

use crate::{CommandContext, CommandHandler, CommandResult};

/// Handler for the `/compact` slash command.
pub struct CompactHandler;

#[async_trait]
impl CommandHandler for CompactHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let args = args.trim();
        if let Some(parsed) = parse_partial_compact_args(args) {
            return Ok(execute_partial_compact(parsed, ctx));
        }

        let message_count = ctx.messages.len();

        if message_count == 0 {
            return Ok(CommandResult::Output(
                "Nothing to compact -- conversation is empty.".into(),
            ));
        }

        let custom_instructions = if args.is_empty() {
            None
        } else {
            Some(args.to_string())
        };

        // Estimate tokens before compaction
        let pre_tokens = tokens::estimate_messages_tokens(&ctx.messages);
        let model = &ctx.app_state.main_loop_model;

        // Run the local context management pipeline
        let pipeline_result =
            pipeline::run_context_pipeline(ctx.messages.clone(), None, model).await;

        let post_tokens = pipeline_result.estimated_tokens;

        if pipeline_result.compacted {
            // Pipeline made progress — apply the compacted messages
            let freed = pre_tokens.saturating_sub(post_tokens);
            let _new_count = pipeline_result.messages.len();

            // Build summary message for the compacted portion
            let summary = if let Some(ref instructions) = custom_instructions {
                format!("Conversation compacted with instructions: {}", instructions)
            } else {
                "Conversation compacted to reduce context usage.".to_string()
            };

            let config = compaction::CompactionConfig {
                model: model.clone(),
                session_id: String::new(),
                query_source: "compact".into(),
            };

            let post_messages =
                compaction::build_post_compact_messages(&summary, &ctx.messages, &config);

            let preserved_segment = compaction::create_preserved_segment(
                post_messages.first(),
                &pipeline_result.messages,
            );

            // Create the compact boundary marker
            let boundary = compaction::create_compact_boundary_with_preserved_segment(
                pre_tokens,
                post_tokens,
                Some(preserved_segment),
            );

            // Apply: replace conversation with compacted messages + boundary
            let mut new_messages = pipeline_result.messages;
            new_messages.push(boundary);
            new_messages.extend(post_messages);
            ctx.messages = new_messages;

            return Ok(CommandResult::Output(format!(
                "Compacted: ~{} → ~{} tokens ({} tokens freed)\n\
                 Messages: {} → {}",
                pre_tokens,
                post_tokens,
                freed,
                message_count,
                ctx.messages.len(),
            )));
        }

        // No compaction needed or possible
        Ok(CommandResult::Output(format!(
            "No compaction needed. Current conversation:\n\
             - {} messages, ~{} estimated tokens\n\
             - Token usage is within limits for model '{}'{}",
            message_count,
            pre_tokens,
            model,
            custom_instructions
                .map(|i| format!("\n\nNote: custom instructions '{}' will be used when full API compaction is available.", i))
                .unwrap_or_default(),
        )))
    }
}

#[derive(Debug, Clone)]
struct ParsedPartialCompact {
    direction: partial_compact::PartialCompactDirection,
    anchor: String,
    summary_override: Option<String>,
}

fn execute_partial_compact(
    parsed: Result<ParsedPartialCompact, String>,
    ctx: &mut CommandContext,
) -> CommandResult {
    let parsed = match parsed {
        Ok(parsed) => parsed,
        Err(message) => return CommandResult::Output(message),
    };

    if ctx.messages.is_empty() {
        return CommandResult::Output("Nothing to compact -- conversation is empty.".into());
    }

    if !allthecodes_compact::gates::CompactionFeatureGates::from_env().partial_compact {
        return CommandResult::Output(
            "Partial compact is disabled by ALLTHECODES_PARTIAL_COMPACT.".into(),
        );
    }

    let (anchor_index, anchor_uuid) = match resolve_partial_anchor(&ctx.messages, &parsed.anchor) {
        Ok(anchor) => anchor,
        Err(message) => return CommandResult::Output(message),
    };

    let pre_messages = ctx.messages.len();
    let summary = parsed.summary_override.unwrap_or_else(|| {
        build_partial_summary(&ctx.messages, parsed.direction, anchor_index, &anchor_uuid)
    });
    let config = partial_compact::PartialCompactConfig {
        anchor_uuid: anchor_uuid.clone(),
        direction: parsed.direction,
        summary,
    };

    let Some(result) = partial_compact::partial_compact(ctx.messages.clone(), &config) else {
        return CommandResult::Output(
            "Partial compact made no changes; the selected anchor leaves no compactable messages in that direction.".into(),
        );
    };

    let direction = partial_direction_label(parsed.direction);
    let compacted = result.compacted_end.saturating_sub(result.compacted_start);
    let tokens_freed = result.tokens_freed;
    let boundary_index = result.boundary_index + 1;
    ctx.messages = result.messages;

    CommandResult::Output(format!(
        "Partial compacted {direction} anchor {}: compacted {} messages, freed ~{} tokens.\n\
         Messages: {} -> {}\n\
         Boundary index: {}",
        short_uuid(&anchor_uuid),
        compacted,
        tokens_freed,
        pre_messages,
        ctx.messages.len(),
        boundary_index,
    ))
}

fn parse_partial_compact_args(args: &str) -> Option<Result<ParsedPartialCompact, String>> {
    let (command, rest) = split_first_token(args)?;
    let direction = match command {
        "up-to" | "up_to" | "upto" => partial_compact::PartialCompactDirection::UpTo,
        "from" => partial_compact::PartialCompactDirection::From,
        _ => return None,
    };

    let Some((anchor, rest)) = split_first_token(rest) else {
        return Some(Err(partial_compact_usage()));
    };

    let summary_override = rest
        .trim()
        .strip_prefix("--")
        .unwrap_or_else(|| rest.trim())
        .trim()
        .to_string();
    let summary_override = (!summary_override.is_empty()).then_some(summary_override);

    Some(Ok(ParsedPartialCompact {
        direction,
        anchor: anchor.to_string(),
        summary_override,
    }))
}

fn split_first_token(input: &str) -> Option<(&str, &str)> {
    let trimmed = input.trim_start();
    if trimmed.is_empty() {
        return None;
    }
    let end = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
    Some((&trimmed[..end], &trimmed[end..]))
}

fn partial_compact_usage() -> String {
    "Usage: /compact up-to <message-index-or-uuid-prefix> [summary]\n\
     Usage: /compact from <message-index-or-uuid-prefix> [summary]"
        .into()
}

fn resolve_partial_anchor(
    messages: &[Message],
    anchor: &str,
) -> std::result::Result<(usize, String), String> {
    if let Ok(index) = anchor.parse::<usize>() {
        if index == 0 {
            return Err("Message index must be 1-based.".into());
        }
        let visible = messages
            .iter()
            .enumerate()
            .filter(|(_, message)| is_visible_anchor_message(message))
            .collect::<Vec<_>>();
        let Some((message_index, message)) = visible.get(index - 1) else {
            return Err(format!(
                "Message index {} is out of range; there are {} visible messages.",
                index,
                visible.len()
            ));
        };
        return Ok((*message_index, message.uuid().to_string()));
    }

    let matches = messages
        .iter()
        .enumerate()
        .filter(|(_, message)| message.uuid().to_string().starts_with(anchor))
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [(index, message)] => Ok((*index, message.uuid().to_string())),
        [] => Err(format!("No message UUID starts with `{anchor}`.")),
        _ => Err(format!("Message UUID prefix `{anchor}` is ambiguous.")),
    }
}

fn is_visible_anchor_message(message: &Message) -> bool {
    matches!(message, Message::User(_) | Message::Assistant(_))
}

fn build_partial_summary(
    messages: &[Message],
    direction: partial_compact::PartialCompactDirection,
    anchor_index: usize,
    anchor_uuid: &str,
) -> String {
    let compacted = match direction {
        partial_compact::PartialCompactDirection::UpTo => &messages[..anchor_index],
        partial_compact::PartialCompactDirection::From => {
            messages.get(anchor_index + 1..).unwrap_or(&[])
        }
    };
    let estimated_tokens = tokens::estimate_messages_tokens(compacted);
    let mut lines = vec![format!(
        "Deterministic partial compact summary ({}) for anchor {}. Compacted {} messages, estimated at ~{} tokens.",
        partial_direction_label(direction),
        short_uuid(anchor_uuid),
        compacted.len(),
        estimated_tokens,
    )];

    let excerpts = compacted
        .iter()
        .filter_map(message_excerpt)
        .take(6)
        .collect::<Vec<_>>();
    if !excerpts.is_empty() {
        lines.push("Representative excerpts:".into());
        lines.extend(excerpts.into_iter().map(|excerpt| format!("- {excerpt}")));
    }

    lines.join("\n")
}

fn message_excerpt(message: &Message) -> Option<String> {
    let role = match message {
        Message::User(_) => "user",
        Message::Assistant(_) => "assistant",
        Message::System(_) => "system",
        Message::Progress(_) => "progress",
        Message::Attachment(_) => "attachment",
    };
    let text = match message {
        Message::User(user) => message_content_excerpt(&user.content),
        Message::Assistant(assistant) => assistant
            .content
            .iter()
            .filter_map(content_block_excerpt)
            .collect::<Vec<_>>()
            .join(" "),
        Message::System(system) => system.content.clone(),
        Message::Progress(progress) => format!("progress for {}", progress.tool_use_id),
        Message::Attachment(attachment) => format!("{:?}", attachment.attachment),
    };
    let text = compact_whitespace(&text);
    (!text.is_empty()).then(|| format!("{role}: {}", truncate_chars(&text, 180)))
}

fn message_content_excerpt(content: &MessageContent) -> String {
    match content {
        MessageContent::Text(text) => text.clone(),
        MessageContent::Blocks(blocks) => blocks
            .iter()
            .filter_map(content_block_excerpt)
            .collect::<Vec<_>>()
            .join(" "),
    }
}

fn content_block_excerpt(block: &ContentBlock) -> Option<String> {
    match block {
        ContentBlock::Text { text } => Some(text.clone()),
        ContentBlock::Thinking { thinking, .. } => Some(thinking.clone()),
        ContentBlock::ToolUse { name, input, .. }
        | ContentBlock::ServerToolUse { name, input, .. } => Some(format!(
            "tool_use {name} {}",
            truncate_chars(&input.to_string(), 80)
        )),
        ContentBlock::ToolResult { content, .. } => Some(match content {
            allthecodes_types::message::ToolResultContent::Text(text) => text.clone(),
            allthecodes_types::message::ToolResultContent::Blocks(blocks) => blocks
                .iter()
                .filter_map(content_block_excerpt)
                .collect::<Vec<_>>()
                .join(" "),
        }),
        _ => None,
    }
}

fn compact_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_chars(text: &str, limit: usize) -> String {
    let mut iter = text.chars();
    let truncated = iter.by_ref().take(limit).collect::<String>();
    if iter.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

fn short_uuid(uuid: &str) -> &str {
    uuid.get(..8).unwrap_or(uuid)
}

fn partial_direction_label(direction: partial_compact::PartialCompactDirection) -> &'static str {
    match direction {
        partial_compact::PartialCompactDirection::UpTo => "up-to",
        partial_compact::PartialCompactDirection::From => "from",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
    use allthecodes_types::message::{Message, MessageContent, SystemSubtype, UserMessage};
    use std::path::PathBuf;
    use uuid::Uuid;

    fn make_user_msg(text: &str) -> Message {
        make_user_msg_with_uuid(text, Uuid::new_v4())
    }

    fn make_user_msg_with_uuid(text: &str, uuid: Uuid) -> Message {
        Message::User(UserMessage {
            uuid,
            timestamp: 0,
            role: "user".into(),
            content: MessageContent::Text(text.into()),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        })
    }

    fn command_context(messages: Vec<Message>) -> CommandContext {
        CommandContext {
            messages,
            cwd: PathBuf::from("."),
            app_state: Default::default(),
            session_id: SessionId::new(),
        }
    }

    fn contains_user_text(messages: &[Message], expected: &str) -> bool {
        messages.iter().any(|message| {
            matches!(
                message,
                Message::User(UserMessage {
                    content: MessageContent::Text(text),
                    is_meta: false,
                    ..
                }) if text.contains(expected)
            )
        })
    }

    fn contains_compact_boundary(messages: &[Message]) -> bool {
        messages.iter().any(|message| {
            matches!(
                message,
                Message::System(system)
                    if matches!(&system.subtype, SystemSubtype::CompactBoundary { .. })
            )
        })
    }

    #[tokio::test]
    async fn test_compact_empty_conversation() {
        let handler = CompactHandler;
        let mut ctx = command_context(Vec::new());

        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("empty"));
            }
            _ => panic!("Expected Output result"),
        }
    }

    #[tokio::test]
    async fn test_compact_small_conversation_no_compaction_needed() {
        let handler = CompactHandler;
        let mut ctx = command_context(vec![make_user_msg("hello"), make_user_msg("world")]);

        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("No compaction needed") || text.contains("Compacted"));
                assert!(text.contains("2 messages"));
            }
            _ => panic!("Expected Output result"),
        }
    }

    #[tokio::test]
    async fn test_compact_with_custom_instructions() {
        let handler = CompactHandler;
        let mut ctx = command_context(vec![make_user_msg("hello")]);

        let result = handler
            .execute("focus on code changes", &mut ctx)
            .await
            .unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("focus on code changes"));
            }
            _ => panic!("Expected Output result"),
        }
    }

    #[tokio::test]
    async fn test_compact_boundary_includes_preserved_segment() {
        let handler = CompactHandler;
        let messages = (0..205)
            .map(|idx| make_user_msg(&format!("turn {idx} {}", "x".repeat(80))))
            .collect();
        let mut ctx = command_context(messages);

        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Compacted"));
            }
            _ => panic!("Expected Output result"),
        }

        let segment = ctx
            .messages
            .iter()
            .find_map(|message| {
                if let Message::System(system) = message {
                    if let SystemSubtype::CompactBoundary {
                        compact_metadata: Some(metadata),
                    } = &system.subtype
                    {
                        return metadata.preserved_segment.as_ref();
                    }
                }
                None
            })
            .expect("expected compact boundary preserved segment");

        assert!(segment.summary_message_uuid.is_some());
        assert!(!segment.preserved_message_uuids.is_empty());
    }

    #[tokio::test]
    async fn test_partial_compact_up_to_by_visible_index() {
        let handler = CompactHandler;
        let anchor = make_user_msg("anchor message");
        let anchor_uuid = anchor.uuid().to_string();
        let mut ctx = command_context(vec![
            make_user_msg(&format!("old one {}", "x ".repeat(500))),
            make_user_msg(&format!("old two {}", "y ".repeat(500))),
            anchor,
            make_user_msg("tail message"),
        ]);

        let result = handler.execute("up-to 3", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Partial compacted up-to"));
            }
            _ => panic!("Expected Output result"),
        }

        assert!(contains_compact_boundary(&ctx.messages));
        assert!(contains_user_text(&ctx.messages, "anchor message"));
        assert!(contains_user_text(&ctx.messages, "tail message"));
        assert!(!contains_user_text(&ctx.messages, "old one"));
        assert!(ctx
            .messages
            .iter()
            .any(|message| message.uuid().to_string() == anchor_uuid));
    }

    #[tokio::test]
    async fn test_partial_compact_from_by_uuid_prefix() {
        let handler = CompactHandler;
        let anchor = make_user_msg("anchor message");
        let anchor_uuid = anchor.uuid().to_string();
        let prefix = &anchor_uuid[..8];
        let mut ctx = command_context(vec![
            make_user_msg("start message"),
            anchor,
            make_user_msg(&format!("later one {}", "x ".repeat(500))),
            make_user_msg(&format!("later two {}", "y ".repeat(500))),
        ]);

        let result = handler
            .execute(&format!("from {prefix}"), &mut ctx)
            .await
            .unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Partial compacted from"));
            }
            _ => panic!("Expected Output result"),
        }

        assert!(contains_compact_boundary(&ctx.messages));
        assert!(contains_user_text(&ctx.messages, "start message"));
        assert!(contains_user_text(&ctx.messages, "anchor message"));
        assert!(!contains_user_text(&ctx.messages, "later one"));
        assert!(ctx
            .messages
            .iter()
            .any(|message| message.uuid().to_string() == anchor_uuid));
    }

    #[tokio::test]
    async fn test_partial_compact_reports_ambiguous_uuid_prefix() {
        let handler = CompactHandler;
        let first = Uuid::parse_str("aaaaaaaa-0000-0000-0000-000000000001").expect("uuid fixture");
        let second = Uuid::parse_str("aaaaaaaa-0000-0000-0000-000000000002").expect("uuid fixture");
        let mut ctx = command_context(vec![
            make_user_msg_with_uuid("first", first),
            make_user_msg_with_uuid("second", second),
            make_user_msg("tail"),
        ]);

        let result = handler.execute("up-to aaaaaaaa", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("ambiguous"));
            }
            _ => panic!("Expected Output result"),
        }
        assert!(!contains_compact_boundary(&ctx.messages));
    }
}
