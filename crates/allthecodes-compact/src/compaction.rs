//! Full conversation compaction — `compactConversation()`.
//!
//! This is the heavy-weight compaction that calls the model to generate
//! a summary of the conversation history. It is triggered when token usage
//! exceeds the auto-compact threshold.
//!
//! Pipeline:
//!   1. Initialization — snapshot messages, estimate tokens
//!   2. Prompt assembly — build compaction prompt with conversation context
//!   3. Summary generation — call model to produce summary
//!   4. PTL retry loop — if summary prompt itself is too long, truncate and retry
//!   5. Validation — check summary is reasonable
//!   6. Post-compact message assembly:
//!      a. Summary message
//!      b. Recover last 5 files (each capped at 5K tokens)
//!      c. Re-inject skill instructions (25K budget, 5 skills, 5K each)
//!      d. Hook results
//!   7. Boundary generation — compact boundary system message
//!   8. Cleanup — update tracking state

use tracing::debug;
use uuid::Uuid;

use allthecodes_types::message::{
    CompactMetadata, ContentBlock, Message, PreservedSegment, SystemMessage, SystemSubtype,
};
use allthecodes_types::state::AutoCompactTracking;
use allthecodes_utils::tokens;

use super::auto_compact;
use super::messages as compact_messages;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum consecutive compaction failures before circuit breaker trips.
const MAX_CONSECUTIVE_FAILURES: usize = 3;

/// Maximum number of recent files to recover in post-compact messages.
const MAX_RECOVERED_FILES: usize = 5;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Result of a full compaction operation.
#[derive(Debug)]
pub struct CompactionResult {
    /// Post-compact messages (summary + recovered context).
    pub messages: Vec<Message>,
    /// Updated tracking state.
    pub tracking: AutoCompactTracking,
    /// Tokens before compaction.
    pub pre_compact_tokens: u64,
    /// Tokens after compaction.
    pub post_compact_tokens: u64,
    /// Compact boundary message to yield.
    pub boundary_message: Message,
}

/// Configuration for the compaction operation.
#[derive(Debug, Clone)]
pub struct CompactionConfig {
    /// Model name (for context window size).
    pub model: String,
    /// Session ID.
    pub session_id: String,
    /// Current query source.
    pub query_source: String,
}

// ---------------------------------------------------------------------------
// Decision logic
// ---------------------------------------------------------------------------

/// Check if auto-compaction should be triggered.
///
/// Returns false (skip compaction) if:
/// - Query source is 'compact' or 'session_memory' (recursion guard)
/// - Auto-compact is disabled
/// - Circuit breaker has tripped
/// - Token count is below threshold
pub fn should_auto_compact(
    messages: &[Message],
    model: &str,
    query_source: &str,
    tracking: Option<&AutoCompactTracking>,
    snip_tokens_freed: u64,
) -> bool {
    if !crate::gates::CompactionFeatureGates::from_env().auto_compact {
        return false;
    }

    // Recursion guard
    if query_source == "compact" || query_source == "session_memory" {
        return false;
    }

    // Circuit breaker
    if let Some(t) = tracking {
        if t.consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
            debug!(
                failures = t.consecutive_failures,
                "auto-compact circuit breaker: skipping"
            );
            return false;
        }
    }

    // Token threshold check
    let token_count = tokens::estimate_messages_tokens(messages).saturating_sub(snip_tokens_freed);

    auto_compact::should_auto_compact(token_count, model)
}

// ---------------------------------------------------------------------------
// Post-compact message building
// ---------------------------------------------------------------------------

/// Build post-compact messages from a summary.
///
/// Produces:
/// 1. A user message containing the compaction summary
/// 2. Recovered file references (up to MAX_RECOVERED_FILES)
pub fn build_post_compact_messages(
    summary: &str,
    pre_compact_messages: &[Message],
    _config: &CompactionConfig,
) -> Vec<Message> {
    let mut result = Vec::new();

    // 1. Summary as a user message
    let summary_msg = compact_messages::create_user_message(
        &format!(
            "<context_compaction>\nThe conversation was compacted to save context space. \
             Here is a summary of what happened so far:\n\n{}\n</context_compaction>",
            summary
        ),
        true, // is_meta
    );
    result.push(summary_msg);

    // 2. Recovered file references
    // Extract the last N unique file paths mentioned in tool calls
    let file_paths = extract_recent_file_paths(pre_compact_messages, MAX_RECOVERED_FILES);
    if !file_paths.is_empty() {
        let files_text = file_paths
            .iter()
            .map(|p| format!("- {}", p))
            .collect::<Vec<_>>()
            .join("\n");
        let files_msg = compact_messages::create_user_message(
            &format!(
                "<recovered_context>\nRecently accessed files:\n{}\n</recovered_context>",
                files_text
            ),
            true,
        );
        result.push(files_msg);
    }

    result
}

/// Build post-compact messages and prepend a compact boundary.
///
/// This is used by model-summary auto compaction where the post-compact
/// conversation is only the generated summary plus recovered context.
pub fn build_post_compact_messages_with_boundary(
    summary: &str,
    pre_compact_messages: &[Message],
    config: &CompactionConfig,
    pre_compact_tokens: u64,
) -> Vec<Message> {
    let mut post_messages = build_post_compact_messages(summary, pre_compact_messages, config);
    let post_compact_tokens = tokens::estimate_messages_tokens(&post_messages);
    let preserved_segment =
        create_preserved_segment(post_messages.first(), post_messages.get(1..).unwrap_or(&[]));
    let boundary = create_compact_boundary_with_preserved_segment(
        pre_compact_tokens,
        post_compact_tokens,
        Some(preserved_segment),
    );

    let mut result = Vec::with_capacity(post_messages.len() + 1);
    result.push(boundary);
    result.append(&mut post_messages);
    result
}

/// Create a compact boundary system message.
pub fn create_compact_boundary(pre_compact_tokens: u64, post_compact_tokens: u64) -> Message {
    create_compact_boundary_with_preserved_segment(pre_compact_tokens, post_compact_tokens, None)
}

/// Create a compact boundary system message with preserved segment metadata.
pub fn create_compact_boundary_with_preserved_segment(
    pre_compact_tokens: u64,
    post_compact_tokens: u64,
    preserved_segment: Option<PreservedSegment>,
) -> Message {
    Message::System(SystemMessage {
        uuid: Uuid::new_v4(),
        timestamp: chrono::Utc::now().timestamp_millis(),
        subtype: SystemSubtype::CompactBoundary {
            compact_metadata: Some(CompactMetadata {
                pre_compact_token_count: pre_compact_tokens,
                post_compact_token_count: post_compact_tokens,
                preserved_segment,
            }),
        },
        content: format!(
            "Context compacted: {} → {} tokens",
            pre_compact_tokens, post_compact_tokens
        ),
    })
}

/// Build preserved-segment metadata for a compact boundary.
pub fn create_preserved_segment(
    summary_message: Option<&Message>,
    preserved_messages: &[Message],
) -> PreservedSegment {
    PreservedSegment {
        summary_message_uuid: summary_message.map(|message| message.uuid().to_string()),
        preserved_message_uuids: preserved_messages
            .iter()
            .map(|message| message.uuid().to_string())
            .collect(),
    }
}

// ---------------------------------------------------------------------------
// Tracking state management
// ---------------------------------------------------------------------------

/// Update tracking state after a successful compaction.
pub fn tracking_on_success(
    _prev: Option<&AutoCompactTracking>,
    turn_id: &str,
) -> AutoCompactTracking {
    AutoCompactTracking {
        compacted: true,
        turn_counter: 0,
        turn_id: turn_id.to_string(),
        consecutive_failures: 0,
    }
}

/// Update tracking state after a failed compaction.
pub fn tracking_on_failure(prev: Option<&AutoCompactTracking>) -> AutoCompactTracking {
    let failures = prev.map_or(1, |t| t.consecutive_failures + 1);
    AutoCompactTracking {
        compacted: false,
        turn_counter: prev.map_or(0, |t| t.turn_counter),
        turn_id: prev.map_or_else(String::new, |t| t.turn_id.clone()),
        consecutive_failures: failures,
    }
}

/// Increment the turn counter (called each iteration when not compacting).
pub fn tracking_increment_turn(tracking: &AutoCompactTracking) -> AutoCompactTracking {
    AutoCompactTracking {
        compacted: false,
        turn_counter: tracking.turn_counter + 1,
        turn_id: tracking.turn_id.clone(),
        consecutive_failures: tracking.consecutive_failures,
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract unique file paths from recent tool calls.
fn extract_recent_file_paths(messages: &[Message], max: usize) -> Vec<String> {
    let mut paths = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // Walk messages in reverse to get the most recent files first
    for msg in messages.iter().rev() {
        if let Message::Assistant(assistant) = msg {
            for block in &assistant.content {
                if let ContentBlock::ToolUse { input, name, .. } = block {
                    // Extract file_path from tool input
                    if let Some(path) = input.get("file_path").and_then(|v| v.as_str()) {
                        if seen.insert(path.to_string()) {
                            paths.push(path.to_string());
                            if paths.len() >= max {
                                return paths;
                            }
                        }
                    }
                    // Extract pattern from Glob
                    if name == "Glob" {
                        if let Some(pattern) = input.get("pattern").and_then(|v| v.as_str()) {
                            if seen.insert(pattern.to_string()) {
                                paths.push(pattern.to_string());
                                if paths.len() >= max {
                                    return paths;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    paths
}

// ---------------------------------------------------------------------------
// Compaction prompt template
// ---------------------------------------------------------------------------

/// Build the system prompt for the compaction model call.
pub fn build_compaction_prompt() -> String {
    "You are a conversation summarizer. Summarize the following conversation \
     between a user and an AI assistant working on code. Focus on:\n\
     1. What task(s) the user requested\n\
     2. What files were read, created, or modified\n\
     3. What tools were used and their outcomes\n\
     4. Any important decisions or conclusions\n\
     5. Current state of the task (completed, in progress, blocked)\n\n\
     Be concise but comprehensive. Preserve file paths, function names, \
     and technical details that would be needed to continue the conversation."
        .to_string()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_types::message::AssistantMessage;

    fn make_user(text: &str) -> Message {
        compact_messages::create_user_message(text, false)
    }

    fn make_assistant_with_tool(file_path: &str) -> Message {
        Message::Assistant(AssistantMessage {
            uuid: Uuid::new_v4(),
            timestamp: 0,
            role: "assistant".into(),
            content: vec![ContentBlock::ToolUse {
                id: "tu_1".into(),
                name: "Read".into(),
                input: serde_json::json!({"file_path": file_path}),
            }],
            usage: None,
            stop_reason: Some("tool_use".into()),
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        })
    }

    #[test]
    fn test_should_auto_compact_recursion_guard() {
        let messages = vec![make_user("hello")];
        assert!(!should_auto_compact(
            &messages,
            "claude-sonnet",
            "compact",
            None,
            0
        ));
        assert!(!should_auto_compact(
            &messages,
            "claude-sonnet",
            "session_memory",
            None,
            0
        ));
    }

    #[test]
    fn test_should_auto_compact_circuit_breaker() {
        let messages = vec![make_user("hello")];
        let tracking = AutoCompactTracking {
            compacted: false,
            turn_counter: 0,
            turn_id: String::new(),
            consecutive_failures: 3,
        };
        assert!(!should_auto_compact(
            &messages,
            "claude-sonnet",
            "repl",
            Some(&tracking),
            0
        ));
    }

    #[test]
    fn test_tracking_on_success() {
        let t = tracking_on_success(None, "turn_1");
        assert!(t.compacted);
        assert_eq!(t.consecutive_failures, 0);
        assert_eq!(t.turn_id, "turn_1");
    }

    #[test]
    fn test_tracking_on_failure_increments() {
        let prev = AutoCompactTracking {
            compacted: false,
            turn_counter: 5,
            turn_id: "t1".into(),
            consecutive_failures: 1,
        };
        let t = tracking_on_failure(Some(&prev));
        assert_eq!(t.consecutive_failures, 2);
        assert_eq!(t.turn_counter, 5);
    }

    #[test]
    fn test_extract_recent_file_paths() {
        let messages = vec![
            make_assistant_with_tool("/tmp/foo.rs"),
            make_assistant_with_tool("/tmp/bar.rs"),
            make_assistant_with_tool("/tmp/foo.rs"), // duplicate
        ];
        let paths = extract_recent_file_paths(&messages, 5);
        assert_eq!(paths.len(), 2); // deduplicated
    }

    #[test]
    fn test_build_post_compact_messages() {
        let config = CompactionConfig {
            model: "claude-sonnet".into(),
            session_id: "test".into(),
            query_source: "repl".into(),
        };
        let messages = build_post_compact_messages("Summary text", &[], &config);
        assert!(!messages.is_empty());
    }

    #[test]
    fn test_build_post_compact_messages_with_boundary_tracks_preserved_segment() {
        let config = CompactionConfig {
            model: "claude-sonnet".into(),
            session_id: "test".into(),
            query_source: "repl".into(),
        };
        let pre_messages = vec![make_assistant_with_tool("/tmp/foo.rs")];
        let messages =
            build_post_compact_messages_with_boundary("Summary text", &pre_messages, &config, 100);

        assert!(matches!(&messages[0], Message::System(_)));
        assert!(matches!(&messages[1], Message::User(_)));

        let Message::System(system) = &messages[0] else {
            panic!("expected compact boundary");
        };
        let SystemSubtype::CompactBoundary {
            compact_metadata: Some(metadata),
        } = &system.subtype
        else {
            panic!("expected compact metadata");
        };
        let segment = metadata.preserved_segment.as_ref().unwrap();
        assert_eq!(
            segment.summary_message_uuid,
            Some(messages[1].uuid().to_string())
        );
        assert_eq!(
            segment.preserved_message_uuids,
            messages[2..]
                .iter()
                .map(|message| message.uuid().to_string())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_create_compact_boundary() {
        let boundary = create_compact_boundary(100_000, 5_000);
        if let Message::System(sys) = &boundary {
            assert!(matches!(sys.subtype, SystemSubtype::CompactBoundary { .. }));
            assert!(sys.content.contains("100000"));
        } else {
            panic!("expected system message");
        }
    }

    #[test]
    fn test_create_preserved_segment() {
        let summary = make_user("summary");
        let preserved = vec![make_user("recent")];
        let segment = create_preserved_segment(Some(&summary), &preserved);
        assert_eq!(
            segment.summary_message_uuid,
            Some(summary.uuid().to_string())
        );
        assert_eq!(
            segment.preserved_message_uuids,
            vec![preserved[0].uuid().to_string()]
        );
    }

    #[test]
    fn test_build_compaction_prompt() {
        let prompt = build_compaction_prompt();
        assert!(prompt.contains("summarizer"));
        assert!(prompt.contains("file paths"));
    }
}
