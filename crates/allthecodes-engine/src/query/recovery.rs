//! Query-loop recovery strategies for model call and stream failures.

use std::sync::Arc;
use std::time::Duration;

use tracing::{debug, warn};

use crate::types::message::{AssistantMessage, ContentBlock, Message, StreamEvent};
use crate::types::state::QueryLoopState;
use crate::types::transitions::Continue;

use super::deps::QueryDeps;
use super::loop_helpers::make_user_message;

#[cfg(test)]
const DEFAULT_STREAM_IDLE_TIMEOUT: Duration = Duration::from_millis(50);
#[cfg(not(test))]
const DEFAULT_STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(120);

#[cfg(test)]
const DEFAULT_STREAM_STALL_TIMEOUT: Duration = Duration::from_millis(25);
#[cfg(not(test))]
const DEFAULT_STREAM_STALL_TIMEOUT: Duration = Duration::from_secs(60);

/// Maximum number of max_output_tokens recovery attempts.
pub(crate) const MAX_OUTPUT_TOKENS_RECOVERY_LIMIT: usize = 3;

/// Escalated max output tokens (8k -> 64k).
pub(crate) const ESCALATED_MAX_TOKENS: usize = 64_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModelCallFailureStage {
    RequestStart,
    StreamInterrupted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ModelCallFailureRecovery {
    PromptTooLong,
    Fallback { model: String },
    Terminal,
}

/// prompt_too_long recovery result.
pub(crate) enum PromptRecovery {
    Continue(Continue),
    Terminal,
}

/// max_output_tokens recovery result.
pub(crate) enum MaxTokensRecovery {
    Continue(Continue),
    Terminal,
}

pub(crate) fn is_prompt_too_long_error(error: &str) -> bool {
    error.contains("prompt_too_long") || error.contains("prompt is too long")
}

/// Return the configured fallback model when a capacity-style model failure can
/// be retried on a different model.
fn fallback_model_for_recoverable_model_error(
    fallback_model: Option<&str>,
    attempted_model: &str,
    error: &str,
) -> Option<String> {
    if !is_recoverable_model_capacity_error(error) {
        return None;
    }

    let fallback = fallback_model?.trim();
    if fallback.is_empty() || fallback == attempted_model {
        return None;
    }

    Some(fallback.to_string())
}

pub(crate) fn classify_model_call_failure(
    stage: ModelCallFailureStage,
    fallback_model: Option<&str>,
    attempted_model: &str,
    error: &str,
) -> ModelCallFailureRecovery {
    if stage == ModelCallFailureStage::RequestStart && is_prompt_too_long_error(error) {
        return ModelCallFailureRecovery::PromptTooLong;
    }

    if let Some(model) =
        fallback_model_for_recoverable_model_error(fallback_model, attempted_model, error)
    {
        return ModelCallFailureRecovery::Fallback { model };
    }

    ModelCallFailureRecovery::Terminal
}

/// Return a fallback request history without model-bound signature blocks.
///
/// Thinking, redacted-thinking, and connector-text blocks are signed against
/// the model/key that produced them, so cross-model fallback must not replay
/// them as context.
pub(crate) fn strip_fallback_signature_blocks(messages: &[Message]) -> Vec<Message> {
    messages
        .iter()
        .map(|message| match message {
            Message::Assistant(assistant) => {
                let mut assistant = assistant.clone();
                assistant.content.retain(|block| {
                    !matches!(
                        block,
                        ContentBlock::Thinking { .. }
                            | ContentBlock::RedactedThinking { .. }
                            | ContentBlock::ConnectorText { .. }
                    )
                });
                Message::Assistant(assistant)
            }
            _ => message.clone(),
        })
        .collect()
}

fn is_recoverable_model_capacity_error(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    lower.contains("529")
        || lower.contains("overloaded")
        || lower.contains("high demand")
        || lower.contains("capacity")
}

pub(crate) fn stream_idle_timeout() -> Duration {
    duration_from_env("ALLTHECODES_STREAM_IDLE_TIMEOUT_MS")
        .or_else(|| duration_from_env("CC_RUST_STREAM_IDLE_TIMEOUT_MS"))
        .unwrap_or(DEFAULT_STREAM_IDLE_TIMEOUT)
}

pub(crate) fn stream_stall_timeout() -> Duration {
    duration_from_env("ALLTHECODES_STREAM_STALL_TIMEOUT_MS")
        .or_else(|| duration_from_env("CC_RUST_STREAM_STALL_TIMEOUT_MS"))
        .unwrap_or(DEFAULT_STREAM_STALL_TIMEOUT)
}

fn duration_from_env(name: &str) -> Option<Duration> {
    let value = std::env::var(name).ok()?;
    let millis = value.trim().parse::<u64>().ok()?;
    if millis == 0 {
        return None;
    }
    Some(Duration::from_millis(millis))
}

pub(crate) fn is_stream_progress_event(event: &StreamEvent) -> bool {
    matches!(
        event,
        StreamEvent::ContentBlockStart { .. }
            | StreamEvent::ContentBlockDelta { .. }
            | StreamEvent::ContentBlockStop { .. }
            | StreamEvent::MessageDelta { .. }
            | StreamEvent::MessageStop
    )
}

/// Transport interruptions that are safe to retry only when the current
/// attempt has not produced assistant content or a tool call.
pub(crate) fn is_retryable_stream_interruption(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    (lower.contains("error reading") && lower.contains("response chunk"))
        || lower.contains("stream idle timeout")
        || lower.contains("stream stalled")
}

/// Handle prompt_too_long error recovery.
///
/// Three-step recovery:
/// 1. collapse drain -- remove oldest non-critical messages
/// 2. reactive compact -- emergency compaction
/// 3. unrecoverable -- return error
pub(crate) async fn handle_prompt_too_long(
    deps: &Arc<dyn QueryDeps>,
    state: &mut QueryLoopState,
    _error: &str,
) -> PromptRecovery {
    if !state.has_attempted_collapse_drain {
        debug!("prompt_too_long: attempting collapse drain");
        state.has_attempted_collapse_drain = true;

        match deps
            .collapse_drain(state.messages.clone(), state.auto_compact_tracking.clone())
            .await
        {
            Ok(Some(result)) => {
                state.messages = result.messages;
                state.auto_compact_tracking = Some(result.tracking);
                return PromptRecovery::Continue(Continue::CollapseDrainRetry { committed: 1 });
            }
            Ok(None) => {
                debug!("collapse drain returned None, trying reactive compact");
            }
            Err(e) => {
                warn!(error = %e, "collapse drain failed, trying reactive compact");
            }
        }
    }

    if !state.has_attempted_reactive_compact {
        debug!("prompt_too_long: attempting reactive compact");
        state.has_attempted_reactive_compact = true;

        match deps.reactive_compact(state.messages.clone()).await {
            Ok(Some(result)) => {
                state.messages = result.messages;
                state.auto_compact_tracking = Some(result.tracking);
                return PromptRecovery::Continue(Continue::ReactiveCompactRetry);
            }
            Ok(None) => {
                debug!("reactive compact returned None, cannot recover");
            }
            Err(e) => {
                warn!(error = %e, "reactive compact failed");
            }
        }
    }

    PromptRecovery::Terminal
}

/// Handle max_output_tokens recovery.
///
/// Three-step recovery:
/// 1. escalate -- increase max_output_tokens to ESCALATED_MAX_TOKENS
/// 2. recovery message -- inject "continue from where you left off"
/// 3. reached recovery limit -- terminate
pub(crate) fn handle_max_output_tokens(
    deps: &Arc<dyn QueryDeps>,
    state: &mut QueryLoopState,
    _assistant_message: &AssistantMessage,
) -> MaxTokensRecovery {
    if state.max_output_tokens_override.is_none() {
        debug!("max_output_tokens: escalating to {}", ESCALATED_MAX_TOKENS);
        state.max_output_tokens_override = Some(ESCALATED_MAX_TOKENS);
        state.transition = Some(Continue::MaxOutputTokensEscalate);
        return MaxTokensRecovery::Continue(Continue::MaxOutputTokensEscalate);
    }

    if state.max_output_tokens_recovery_count < MAX_OUTPUT_TOKENS_RECOVERY_LIMIT {
        state.max_output_tokens_recovery_count += 1;
        let attempt = state.max_output_tokens_recovery_count;
        debug!(attempt, "max_output_tokens: recovery attempt");

        let recovery_msg = make_user_message(
            deps,
            "Your response was cut off due to output length limits. Please continue from where you left off.",
            true,
        );
        state.messages.push(Message::User(recovery_msg));
        state.turn_count += 1;
        return MaxTokensRecovery::Continue(Continue::MaxOutputTokensRecovery { attempt });
    }

    debug!("max_output_tokens: recovery limit reached, terminating");
    MaxTokensRecovery::Terminal
}
