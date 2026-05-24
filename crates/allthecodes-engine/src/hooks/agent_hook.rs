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
}
