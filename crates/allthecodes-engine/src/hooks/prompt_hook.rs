//! Prompt hook execution — runs a prompt-based hook using a single LLM call.
//!
//! Port of TypeScript `execPromptHook.ts`.
//!
//! NOTE: This is a structural stub. Full implementation requires:
//!   - cc-api for `queryModelWithoutStreaming()` equivalent
//!   - Hook response schema validation
//!   - Combined abort signal (parent + timeout)

use allthecodes_types::hooks::HookEntry;
use serde_json::Value;

use super::agent_hook::HookResult;

/// Execute a prompt-based hook using an LLM.
///
/// Unlike agent hooks, prompt hooks make a single LLM call and expect
/// a JSON response with `{ ok: boolean, reason?: string }`.
pub async fn exec_prompt_hook(
    _hook: &HookEntry,
    _hook_name: &str,
    _hook_event: &str,
    _json_input: &Value,
    _signal: tokio::sync::watch::Receiver<bool>,
    _messages: Option<&[Value]>,
) -> HookResult {
    // TODO: Full implementation:
    //
    // 1. Substitute $ARGUMENTS in prompt
    // 2. Build message array (prepend conversation history if provided)
    // 3. Call queryModelWithoutStreaming with JSON schema output format
    // 4. Parse response as JSON and validate against hookResponseSchema
    // 5. If ok: true -> return success
    //    If ok: false -> return blocking with reason
    //    If parse error -> return non_blocking_error

    HookResult::success()
}

#[cfg(test)]
mod tests {
    use super::super::agent_hook::HookOutcome;
    use super::*;

    #[tokio::test]
    async fn test_exec_prompt_hook_stub() {
        let hook = HookEntry::Prompt {
            prompt: "Check condition: $ARGUMENTS".into(),
            timeout: 30,
            model: None,
            if_condition: None,
        };

        let (tx, rx) = tokio::sync::watch::channel(false);
        let result = exec_prompt_hook(
            &hook,
            "test-prompt-hook",
            "Stop",
            &serde_json::json!({"input": "test"}),
            rx,
            None,
        )
        .await;

        assert!(matches!(result.outcome, HookOutcome::Success));
        drop(tx);
    }
}
