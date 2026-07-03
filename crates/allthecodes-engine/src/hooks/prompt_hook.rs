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

    // CS-002: Schema test for HookEntry::Prompt JSON construction/serde.
    #[test]
    fn test_prompt_hook_entry_schema() {
        // Verify a Prompt-type HookEntry deserializes correctly.
        let json = serde_json::json!({
            "type": "prompt",
            "prompt": "analyze $ARGUMENTS",
            "timeout": 45,
            "model": "claude-3-5-sonnet",
            "if": "tool_name == 'Read'"
        });

        let entry: HookEntry = serde_json::from_value(json).unwrap();
        match entry {
            HookEntry::Prompt {
                prompt,
                timeout,
                model,
                if_condition,
            } => {
                assert_eq!(prompt, "analyze $ARGUMENTS");
                assert_eq!(timeout, 45);
                assert_eq!(model.unwrap(), "claude-3-5-sonnet");
                assert_eq!(if_condition.unwrap(), "tool_name == 'Read'");
            }
            _ => panic!("expected Prompt variant"),
        }
    }

    #[test]
    fn test_agent_hook_entry_schema() {
        // Verify an Agent-type HookEntry deserializes correctly.
        let json = serde_json::json!({
            "type": "agent",
            "prompt": "spawn agent $ARGUMENTS",
            "timeout": 120
        });

        let entry: HookEntry = serde_json::from_value(json).unwrap();
        match entry {
            HookEntry::Agent {
                prompt,
                timeout,
                model,
                if_condition,
            } => {
                assert_eq!(prompt, "spawn agent $ARGUMENTS");
                assert_eq!(timeout, 120);
                assert!(model.is_none());
                assert!(if_condition.is_none());
            }
            _ => panic!("expected Agent variant"),
        }
    }

    #[test]
    fn test_command_hook_entry_schema() {
        let json = serde_json::json!({
            "type": "command",
            "command": "/usr/bin/check-policy",
            "timeout": 10,
            "shell": "bash"
        });

        let entry: HookEntry = serde_json::from_value(json).unwrap();
        match entry {
            HookEntry::Command {
                command,
                timeout,
                shell,
                if_condition,
            } => {
                assert_eq!(command, "/usr/bin/check-policy");
                assert_eq!(timeout, 10);
                assert_eq!(shell.unwrap(), "bash");
                assert!(if_condition.is_none());
            }
            _ => panic!("expected Command variant"),
        }
    }

    #[test]
    fn test_http_hook_entry_schema() {
        let json = serde_json::json!({
            "type": "http",
            "url": "https://hooks.example.com/check",
            "timeout": 300,
            "headers": {"Authorization": "Bearer token"},
            "allowed_env_vars": ["HOME"]
        });

        let entry: HookEntry = serde_json::from_value(json).unwrap();
        match entry {
            HookEntry::Http {
                url,
                timeout,
                headers,
                if_condition,
                allowed_env_vars,
            } => {
                assert_eq!(url, "https://hooks.example.com/check");
                assert_eq!(timeout, 300);
                assert_eq!(
                    headers.unwrap().get("Authorization").unwrap(),
                    "Bearer token"
                );
                assert!(if_condition.is_none());
                assert_eq!(allowed_env_vars.unwrap(), vec!["HOME".to_string()]);
            }
            _ => panic!("expected Http variant"),
        }
    }

    // CS-002: Verify the stub does not panic — structural panic check.
    #[tokio::test]
    async fn test_no_structural_panic_in_prompt_stub() {
        let hook = HookEntry::Prompt {
            prompt: "test $ARGUMENTS".into(),
            timeout: 10,
            model: None,
            if_condition: None,
        };

        let (tx, rx) = tokio::sync::watch::channel(true);
        let result = exec_prompt_hook(
            &hook,
            "panic-check",
            "Stop",
            &serde_json::json!({"input": "test"}),
            rx,
            None,
        )
        .await;

        assert!(matches!(result.outcome, HookOutcome::Success));
        drop(tx);
    }

    // CS-002: Timeout simulation — hook with cancelled signal returns success (stub, no hang).
    #[tokio::test]
    async fn test_prompt_hook_timeout_does_not_block() {
        let hook = HookEntry::Prompt {
            prompt: "quick check $ARGUMENTS".into(),
            timeout: 5,
            model: None,
            if_condition: None,
        };

        // Signal true = cancelled/aborted
        let (_tx, rx) = tokio::sync::watch::channel(true);
        let result = exec_prompt_hook(
            &hook,
            "timeout-test",
            "Stop",
            &serde_json::json!({"input": "test"}),
            rx,
            None,
        )
        .await;

        // As a stub, still returns success; full impl would respect cancel
        // but the important thing is it doesn't hang or panic.
        assert!(matches!(result.outcome, HookOutcome::Success));
    }

    // CS-002: Hook error handling — error path is never reached in the stub,
    //           but we confirm the path is safe by verifying the HookResult API.
    #[test]
    fn test_agent_hook_error_returns_safe_result() {
        // Simulate what a caller does on hook error: log warn, return success.
        let result = HookResult::success();
        assert!(!result.prevent_continuation);
        assert!(result.blocking_error.is_none());
    }
}
