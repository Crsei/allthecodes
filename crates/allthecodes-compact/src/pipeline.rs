//! Context management pipeline — orchestrates all compaction steps.
//!
//! Pipeline order (each query loop iteration):
//!   1. applyToolResultBudget — persist oversized tool results to disk
//!   2. snipCompact — trim old turns beyond max_turns limit
//!   3. microcompact — remove cached/redundant tool results
//!   4. contextCollapse — fold old segments into summaries (Phase 2+)
//!   5. autoCompact — full summarization when nearing token limit

use tracing::{debug, info};

use allthecodes_types::message::Message;
use allthecodes_types::state::AutoCompactTracking;
use allthecodes_utils::tokens;

use super::auto_compact;
use super::context_collapse;
use super::microcompact;
use super::snip;
use super::tool_result_budget;

// ---------------------------------------------------------------------------
// Pipeline result types
// ---------------------------------------------------------------------------

/// Result of running the context management pipeline.
#[derive(Debug)]
pub struct PipelineResult {
    /// Processed messages (may be shorter than input).
    pub messages: Vec<Message>,
    /// Updated auto-compact tracking state.
    pub tracking: Option<AutoCompactTracking>,
    /// Whether any compaction was actually performed.
    pub compacted: bool,
    /// Whether this pipeline pass crossed the auto-compact threshold.
    pub auto_compact_triggered: bool,
    /// Estimated tokens after compaction.
    pub estimated_tokens: u64,
    /// Estimated tokens used for the auto-compact threshold after this
    /// pipeline's local compaction has already been applied.
    pub auto_compact_estimated_tokens: u64,
    /// Estimated tokens freed by history snipping.
    pub snip_tokens_freed: u64,
    /// Estimated tokens freed by microcompacting old tool results.
    pub microcompact_tokens_freed: u64,
    /// Estimated tokens freed by context collapse.
    pub context_collapse_tokens_freed: u64,
    /// Total estimated tokens freed before auto-compact is considered.
    pub total_tokens_freed: u64,
}

/// Result of a reactive compaction attempt.
#[derive(Debug, Clone)]
pub struct ReactiveCompactResult {
    pub messages: Vec<Message>,
    pub tracking: AutoCompactTracking,
    pub tokens_freed: u64,
}

/// Default maximum turns before snipping kicks in.
const DEFAULT_SNIP_MAX_TURNS: usize = 200;

/// Emergency snip target for reactive compaction.
const REACTIVE_SNIP_MAX_TURNS: usize = 5;

// ---------------------------------------------------------------------------
// Main pipeline (async — includes tool result budget disk I/O)
// ---------------------------------------------------------------------------

/// Run the full context management pipeline (steps 1-5).
///
/// # Arguments
/// * `messages` - Current conversation history
/// * `tracking` - Auto-compact tracking state from previous iteration
/// * `model` - Model name (for context window size lookup)
pub async fn run_context_pipeline(
    messages: Vec<Message>,
    tracking: Option<AutoCompactTracking>,
    model: &str,
) -> PipelineResult {
    let mut current = messages;
    let mut compacted = false;
    let initial_tokens = tokens::estimate_messages_tokens(&current);

    // ── Step 1: Tool result budget (async — saves oversized results to disk) ──
    let mut replacement_state = tool_result_budget::ContentReplacementState::default();
    let budgeted = tool_result_budget::apply_tool_result_budget(
        current,
        &mut replacement_state,
        tool_result_budget::DEFAULT_MAX_SIZE_CHARS,
    )
    .await;
    if !replacement_state.replacements.is_empty() {
        compacted = true;
        debug!(
            replacements = replacement_state.replacements.len(),
            "tool result budget: persisted oversized results"
        );
    }
    current = budgeted;

    // ── Step 2: Snip compact ────────────────────────────────────────
    let snip_result = snip::snip_compact_if_needed(current, DEFAULT_SNIP_MAX_TURNS);
    if snip_result.tokens_freed > 0 {
        compacted = true;
        debug!(
            freed = snip_result.tokens_freed,
            "snip compact: trimmed old turns"
        );
    }
    let snip_tokens_freed = snip_result.tokens_freed;
    current = snip_result.messages;

    // ── Step 3: Microcompact ────────────────────────────────────────
    let micro_result = microcompact::microcompact_messages(current);
    if micro_result.tokens_freed > 0 {
        compacted = true;
        debug!(
            freed = micro_result.tokens_freed,
            "microcompact: trimmed old tool results"
        );
    }
    let microcompact_tokens_freed = micro_result.tokens_freed;
    current = micro_result.messages;

    // ── Step 4: Context collapse (Phase 2+) ─────────────────────────
    let collapse_result = context_collapse::context_collapse_if_needed(current, model);
    if collapse_result.boundary_message.is_some() {
        compacted = true;
        debug!(
            freed = collapse_result.tokens_freed,
            collapsed_messages = collapse_result.collapsed_messages,
            "context collapse: folded old turns into summary"
        );
    }
    let context_collapse_tokens_freed = collapse_result.tokens_freed;
    current = collapse_result.messages;

    // ── Step 5: Auto compact check ──────────────────────────────────
    let estimated = tokens::estimate_messages_tokens(&current);
    let total_tokens_freed = snip_tokens_freed
        .saturating_add(microcompact_tokens_freed)
        .saturating_add(context_collapse_tokens_freed);
    let auto_compact_estimated_tokens = estimated;
    let auto_compact_triggered =
        auto_compact::should_auto_compact(auto_compact_estimated_tokens, model);
    let updated_tracking = if auto_compact_triggered {
        info!(
            estimated_tokens = auto_compact_estimated_tokens,
            raw_estimated_tokens = estimated,
            pre_autocompact_tokens_freed = total_tokens_freed,
            model = model,
            "auto compact triggered (>80% of context window)"
        );
        let base = tracking.unwrap_or(AutoCompactTracking {
            compacted: false,
            turn_counter: 0,
            turn_id: String::new(),
            consecutive_failures: 0,
        });
        Some(AutoCompactTracking {
            compacted: true,
            turn_counter: base.turn_counter + 1,
            turn_id: base.turn_id,
            consecutive_failures: base.consecutive_failures,
        })
    } else {
        tracking
    };

    if compacted {
        info!(
            before_tokens = initial_tokens,
            after_tokens = estimated,
            auto_compact_estimated_tokens = auto_compact_estimated_tokens,
            snip_tokens_freed = snip_tokens_freed,
            microcompact_tokens_freed = microcompact_tokens_freed,
            context_collapse_tokens_freed = context_collapse_tokens_freed,
            messages = current.len(),
            "compaction pipeline completed",
        );
    }

    PipelineResult {
        messages: current,
        tracking: updated_tracking,
        compacted,
        auto_compact_triggered,
        estimated_tokens: estimated,
        auto_compact_estimated_tokens,
        snip_tokens_freed,
        microcompact_tokens_freed,
        context_collapse_tokens_freed,
        total_tokens_freed,
    }
}

/// Attempt reactive compaction after a prompt_too_long error.
///
/// This is more aggressive than the normal pipeline — it aggressively
/// budgets tool results, snips to a very small number of turns, and
/// microcompacts.
///
/// Returns `None` if compaction is not possible (already small enough).
pub async fn try_reactive_compact(
    messages: Vec<Message>,
    model: &str,
) -> Option<ReactiveCompactResult> {
    if !crate::gates::CompactionFeatureGates::from_env().reactive_compact {
        return None;
    }

    let initial_tokens = tokens::estimate_messages_tokens(&messages);
    let target = (auto_compact::get_context_window_size(model) as f64 * 0.6) as u64;

    if initial_tokens <= target {
        return None; // Already within limits
    }

    // First: budget oversized tool results
    let mut replacement_state = tool_result_budget::ContentReplacementState::default();
    let current = tool_result_budget::apply_tool_result_budget(
        messages,
        &mut replacement_state,
        tool_result_budget::DEFAULT_MAX_SIZE_CHARS,
    )
    .await;

    if !replacement_state.replacements.is_empty() {
        debug!(
            replacements = replacement_state.replacements.len(),
            "reactive compact: budgeted oversized tool results"
        );
    }

    // Aggressive strategy: snip to keep only the last few turns
    let snip_result = snip::snip_compact_if_needed(current, REACTIVE_SNIP_MAX_TURNS);
    if snip_result.tokens_freed == 0 && replacement_state.replacements.is_empty() {
        // Snipping didn't help — try microcompact alone
        let micro = microcompact::microcompact_messages(snip_result.messages);
        if micro.tokens_freed == 0 {
            return None;
        }
        let final_tokens = tokens::estimate_messages_tokens(&micro.messages);
        return Some(ReactiveCompactResult {
            messages: micro.messages,
            tracking: AutoCompactTracking {
                compacted: true,
                turn_counter: 0,
                turn_id: String::new(),
                consecutive_failures: 0,
            },
            tokens_freed: initial_tokens.saturating_sub(final_tokens),
        });
    }

    // Then microcompact the result
    let micro_result = microcompact::microcompact_messages(snip_result.messages);

    let final_tokens = tokens::estimate_messages_tokens(&micro_result.messages);
    let tokens_freed = initial_tokens.saturating_sub(final_tokens);

    if tokens_freed == 0 {
        return None;
    }

    Some(ReactiveCompactResult {
        messages: micro_result.messages,
        tracking: AutoCompactTracking {
            compacted: true,
            turn_counter: 0,
            turn_id: String::new(),
            consecutive_failures: 0,
        },
        tokens_freed,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_types::message::{AssistantMessage, ContentBlock, MessageContent, UserMessage};
    use uuid::Uuid;

    fn make_user(text: &str) -> Message {
        Message::User(UserMessage {
            uuid: Uuid::new_v4(),
            timestamp: 0,
            role: "user".into(),
            content: MessageContent::Text(text.into()),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        })
    }

    fn make_assistant(text: &str) -> Message {
        Message::Assistant(AssistantMessage {
            uuid: Uuid::new_v4(),
            timestamp: 0,
            role: "assistant".into(),
            content: vec![ContentBlock::Text { text: text.into() }],
            usage: None,
            stop_reason: Some("end_turn".into()),
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        })
    }

    fn make_tool_use_assistant(id: &str) -> Message {
        Message::Assistant(AssistantMessage {
            uuid: Uuid::new_v4(),
            timestamp: 0,
            role: "assistant".into(),
            content: vec![ContentBlock::ToolUse {
                id: id.into(),
                name: "bash".into(),
                input: serde_json::json!({ "command": "echo test" }),
            }],
            usage: None,
            stop_reason: Some("tool_use".into()),
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        })
    }

    #[tokio::test]
    async fn test_pipeline_no_changes_small_conversation() {
        let messages = vec![make_user("Hello"), make_assistant("Hi!")];
        let result = run_context_pipeline(messages, None, "claude-sonnet-4-20250514").await;
        assert_eq!(result.messages.len(), 2);
        assert_eq!(result.snip_tokens_freed, 0);
        assert_eq!(result.microcompact_tokens_freed, 0);
        assert_eq!(result.context_collapse_tokens_freed, 0);
        assert_eq!(result.total_tokens_freed, 0);
    }

    #[tokio::test]
    async fn test_pipeline_returns_estimated_tokens() {
        let messages = vec![make_user("Hello"), make_assistant("Hi!")];
        let result = run_context_pipeline(messages, None, "claude-sonnet-4-20250514").await;
        assert!(result.estimated_tokens > 0);
    }

    #[tokio::test]
    async fn test_pipeline_applies_context_collapse_before_autocompact() {
        let mut messages = vec![make_user("initial context")];
        for turn in 0..50 {
            messages.push(make_user(&format!("question {turn} {}", "x".repeat(120))));
            messages.push(make_assistant(&format!(
                "answer {turn} {}",
                "y".repeat(120)
            )));
        }

        let result = run_context_pipeline(messages, None, "claude-sonnet-4-20250514").await;

        assert!(result.compacted);
        assert!(result.context_collapse_tokens_freed > 0);
        assert_eq!(
            result.total_tokens_freed,
            result.snip_tokens_freed
                + result.microcompact_tokens_freed
                + result.context_collapse_tokens_freed
        );
        assert!(
            result
                .messages
                .iter()
                .any(|message| matches!(message, Message::System(system) if system.content.contains("<context_collapse>"))),
            "pipeline should insert a context collapse boundary"
        );
    }

    #[tokio::test]
    async fn test_pipeline_does_not_double_credit_local_freed_tokens_before_autocompact_threshold()
    {
        let model = "claude-sonnet-4-20250514";
        let mut messages = vec![make_user("initial context")];
        let large_tool_result = "x".repeat(75_000);
        for index in 0..16 {
            let tool_use_id = format!("toolu_{index}");
            messages.push(make_tool_use_assistant(&tool_use_id));
            messages.push(crate::messages::create_tool_result_message(
                &tool_use_id,
                &large_tool_result,
                false,
            ));
        }

        let result = run_context_pipeline(messages, None, model).await;

        assert!(result.microcompact_tokens_freed > 0);
        assert!(result.estimated_tokens > 160_000);
        assert_eq!(
            result.auto_compact_estimated_tokens,
            result.estimated_tokens
        );
        assert!(result.tracking.is_some());
        assert!(result.auto_compact_triggered);
    }

    #[tokio::test]
    async fn test_pipeline_keeps_previous_tracking_without_new_auto_compact_trigger() {
        let messages = vec![make_user("Hello"), make_assistant("Hi!")];
        let tracking = AutoCompactTracking {
            compacted: true,
            turn_counter: 0,
            turn_id: "previous".into(),
            consecutive_failures: 0,
        };

        let result =
            run_context_pipeline(messages, Some(tracking.clone()), "claude-sonnet-4-20250514")
                .await;

        assert!(!result.auto_compact_triggered);
        assert!(result.tracking.is_some());
        assert_eq!(result.tracking.unwrap().turn_id, tracking.turn_id);
    }

    #[tokio::test]
    async fn test_reactive_compact_small_conversation_returns_none() {
        let messages = vec![make_user("Hello"), make_assistant("Hi!")];
        let result = try_reactive_compact(messages, "claude-sonnet-4-20250514").await;
        assert!(result.is_none());
    }
}
