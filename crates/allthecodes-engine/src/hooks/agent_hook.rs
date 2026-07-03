//! Agent hook execution — runs an agent-based hook using a multi-turn LLM query.
//!
//! Port of TypeScript `execAgentHook.ts`.
//!
//! NOTE: This is a structural stub. Full implementation requires:
//!   - cc-engine's query loop (query()) for multi-turn agent execution
//!   - cc-api for LLM calls
//!   - StructuredOutputTool integration
//!   - Session hook integration for structured output enforcement

use allthecodes_types::hooks::HookEntry;

/// Result of an agent or prompt hook execution.
#[derive(Debug, Clone)]
pub struct HookResult {
    pub outcome: HookOutcome,
    pub message: Option<serde_json::Value>,
    pub blocking_error: Option<BlockingError>,
    pub prevent_continuation: bool,
    pub stop_reason: Option<String>,
}

/// Outcome of a hook execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookOutcome {
    Success,
    Blocking,
    Cancelled,
    NonBlockingError,
}

/// A blocking error from a hook.
#[derive(Debug, Clone)]
pub struct BlockingError {
    pub error: String,
    pub command: String,
}

impl HookResult {
    pub fn success() -> Self {
        Self {
            outcome: HookOutcome::Success,
            message: None,
            blocking_error: None,
            prevent_continuation: false,
            stop_reason: None,
        }
    }

    pub fn cancelled() -> Self {
        Self {
            outcome: HookOutcome::Cancelled,
            message: None,
            blocking_error: None,
            prevent_continuation: false,
            stop_reason: None,
        }
    }

    pub fn blocking(error: String, command: String) -> Self {
        Self {
            outcome: HookOutcome::Blocking,
            message: None,
            blocking_error: Some(BlockingError { error, command }),
            prevent_continuation: true,
            stop_reason: None,
        }
    }

    pub fn non_blocking_error(message: serde_json::Value) -> Self {
        Self {
            outcome: HookOutcome::NonBlockingError,
            message: Some(message),
            blocking_error: None,
            prevent_continuation: false,
            stop_reason: None,
        }
    }
}

/// Execute an agent-based hook using a multi-turn LLM query.
///
/// The agent hook spawns a sub-agent that has access to tools and can
/// perform multi-turn analysis.
pub async fn exec_agent_hook(
    _hook: &HookEntry,
    _hook_name: &str,
    _hook_event: &str,
    _json_input: &serde_json::Value,
    _signal: tokio::sync::watch::Receiver<bool>,
) -> HookResult {
    // TODO: Full implementation:
    //
    // 1. Build system prompt with transcript path
    // 2. Create user message with processed prompt ($ARGUMENTS substituted)
    // 3. Setup combined abort signal (parent + timeout)
    // 4. Filter tools (remove disallowed agent tools, add StructuredOutput tool)
    // 5. Execute multi-turn query:
    //    for await (const message of query({...})) { ... }
    // 6. Parse structured output for ok/reason
    // 7. Clean up session hooks
    // 8. Return result based on structured output

    HookResult::success()
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_types::hooks::HookEntry;

    #[tokio::test]
    async fn test_exec_agent_hook_stub() {
        let hook = HookEntry::Agent {
            prompt: "Test prompt $ARGUMENTS".into(),
            timeout: 60,
            model: None,
            if_condition: None,
        };

        let (tx, rx) = tokio::sync::watch::channel(false);
        let result = exec_agent_hook(
            &hook,
            "test-agent-hook",
            "Stop",
            &serde_json::json!({"test": true}),
            rx,
        )
        .await;

        // Stub returns success
        assert!(matches!(result.outcome, HookOutcome::Success));
        drop(tx);
    }

    // CS-002: Schema test — HookResult constructs with correct fields via factory methods.
    #[test]
    fn test_hook_result_success_schema() {
        let r = HookResult::success();
        assert!(matches!(r.outcome, HookOutcome::Success));
        assert!(r.message.is_none());
        assert!(r.blocking_error.is_none());
        assert!(!r.prevent_continuation);
        assert!(r.stop_reason.is_none());
    }

    #[test]
    fn test_hook_result_cancelled_schema() {
        let r = HookResult::cancelled();
        assert!(matches!(r.outcome, HookOutcome::Cancelled));
        assert!(r.message.is_none());
        assert!(r.blocking_error.is_none());
        assert!(!r.prevent_continuation);
    }

    #[test]
    fn test_hook_result_blocking_schema() {
        let r = HookResult::blocking("denied".into(), "echo block".into());
        assert!(matches!(r.outcome, HookOutcome::Blocking));
        assert!(r.prevent_continuation);
        let err = r.blocking_error.unwrap();
        assert_eq!(err.error, "denied");
        assert_eq!(err.command, "echo block");
    }

    #[test]
    fn test_hook_result_non_blocking_error_schema() {
        let r = HookResult::non_blocking_error(serde_json::json!({"msg": "oops"}));
        assert!(matches!(r.outcome, HookOutcome::NonBlockingError));
        assert!(!r.prevent_continuation);
        assert_eq!(r.message.unwrap(), serde_json::json!({"msg": "oops"}));
    }

    // CS-002: Verify the stub does not panic — structural panic check.
    #[tokio::test]
    async fn test_no_structural_panic_in_agent_stub() {
        let hook = HookEntry::Agent {
            prompt: "test $ARGUMENTS".into(),
            timeout: 10,
            model: None,
            if_condition: None,
        };

        let (tx, rx) = tokio::sync::watch::channel(true);
        let result = exec_agent_hook(
            &hook,
            "panic-check",
            "Stop",
            &serde_json::json!({"x": 1}),
            rx,
        )
        .await;

        // Stub should never panic; results are always a valid HookResult.
        assert!(matches!(result.outcome, HookOutcome::Success));
        drop(tx);
    }

    // CS-002: PreToolUse hook can deny (blocking outcome sets prevent_continuation).
    #[test]
    fn test_pre_tool_use_can_block() {
        let r = HookResult::blocking("policy forbid".into(), "/usr/bin/deny-hook".into());
        assert!(r.prevent_continuation);
        assert!(matches!(r.outcome, HookOutcome::Blocking));
    }

    // CS-002: PostToolUse-like outcome (success/non-blocking) does not set prevent_continuation.
    #[test]
    fn test_post_tool_use_cannot_block() {
        let r = HookResult::success();
        assert!(!r.prevent_continuation);
        assert!(r.blocking_error.is_none());

        let r2 = HookResult::non_blocking_error(serde_json::json!({"warn": true}));
        assert!(!r2.prevent_continuation);
    }
}
