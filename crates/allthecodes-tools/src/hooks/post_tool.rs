//! Post-tool, failure, stop, and event hook execution.

use anyhow::{anyhow, Result};
use serde_json::Value;
use tracing::{debug, warn};

use super::execution::{execute_command_hook, parse_hook_output};
use super::http_hook::exec_http_hook;
use super::{HookEntry, HookEventConfig, HookOutput, PostToolHookResult};
#[cfg(test)]
use crate::tool::ToolResult;
use allthecodes_types::hooks::{load_hook_configs, matches_tool};

/// Value-only variant of [`run_post_tool_hooks`] used by `ShellHookRunner`.
///
/// Takes the already-extracted tool_result data (`&Value`) instead of a
/// `&ToolResult`, so `cc-types` does not need to depend on the main crate's
/// tool types.
pub(crate) async fn run_post_tool_hooks_data(
    tool_name: &str,
    input: &Value,
    tool_result_data: &Value,
    hook_configs: &[HookEventConfig],
) -> Result<PostToolHookResult> {
    if hook_configs.is_empty() {
        debug!(tool = tool_name, "post-tool hooks: no hooks configured");
        return Ok(PostToolHookResult::Continue);
    }

    let stdin_json = serde_json::json!({
        "tool_name": tool_name,
        "tool_input": input,
        "tool_result": tool_result_data,
    });

    for config in hook_configs {
        if !matches_tool(config.matcher.as_deref(), tool_name) {
            continue;
        }

        for entry in &config.hooks {
            let hook_label = hook_entry_label(entry);
            debug!(tool = tool_name, hook = %hook_label, "running post-tool hook");

            match execute_event_entry(entry, "PostToolUse", &stdin_json).await {
                Ok(output) => {
                    if !output.should_continue {
                        let message = output
                            .stop_reason
                            .or(output.reason)
                            .unwrap_or_else(|| "Hook stopped continuation".to_string());
                        return Ok(PostToolHookResult::StopContinuation { message });
                    }

                    if let Some(ref stop_reason) = output.stop_reason {
                        return Ok(PostToolHookResult::StopContinuation {
                            message: stop_reason.clone(),
                        });
                    }
                }
                Err(e) => {
                    if config.critical {
                        return Err(anyhow!(
                            "critical post-tool hook failed for tool '{}' (hook '{}'): {}",
                            tool_name,
                            hook_label,
                            e
                        ));
                    } else {
                        warn!(
                            tool = tool_name,
                            hook = %hook_label,
                            error = %e,
                            "optional post-tool hook error, continuing"
                        );
                    }
                }
            }
        }
    }

    Ok(PostToolHookResult::Continue)
}

// ---------------------------------------------------------------------------
// Post-tool hooks
// ---------------------------------------------------------------------------

/// Run post-tool hooks after successful tool execution.
///
/// Corresponds to TypeScript: `runPostToolUseHooks()` in toolExecution.ts
///
/// Stdin includes `tool_result` field in addition to tool_name and tool_input.
/// If any hook returns `stop_reason`, returns `StopContinuation`.
#[cfg(test)]
async fn run_post_tool_hooks(
    tool_name: &str,
    input: &Value,
    result: &ToolResult,
    hook_configs: &[HookEventConfig],
) -> Result<PostToolHookResult> {
    // Delegate to the Value-only variant — keeps the logic in one place and
    // lets the HookRunner trait operate without pulling the `ToolResult` type
    // into `cc-types`.
    run_post_tool_hooks_data(tool_name, input, &result.data, hook_configs).await
}

/// Run post-tool failure hooks after failed tool execution.
///
/// Corresponds to TypeScript: `runPostToolUseFailureHooks()` in toolExecution.ts
///
/// Fire-and-forget: run hooks but don't change behavior based on output.
pub async fn run_post_tool_failure_hooks(
    tool_name: &str,
    input: &Value,
    error: &str,
    hook_configs: &[HookEventConfig],
) -> Result<()> {
    if hook_configs.is_empty() {
        debug!(
            tool = tool_name,
            "post-tool failure hooks: no hooks configured"
        );
        return Ok(());
    }

    let stdin_json = serde_json::json!({
        "tool_name": tool_name,
        "tool_input": input,
        "error": error,
    });

    for config in hook_configs {
        if !matches_tool(config.matcher.as_deref(), tool_name) {
            continue;
        }

        for entry in &config.hooks {
            let hook_label = hook_entry_label(entry);
            debug!(tool = tool_name, hook = %hook_label, "running post-tool failure hook");

            if let Err(e) = execute_event_entry(entry, "PostToolUseFailure", &stdin_json).await {
                if config.critical {
                    return Err(anyhow!(
                        "critical post-tool failure hook failed for tool '{}' (hook '{}'): {}",
                        tool_name,
                        hook_label,
                        e
                    ));
                } else {
                    warn!(
                        tool = tool_name,
                        hook = %hook_label,
                        error = %e,
                        "optional post-tool failure hook error, continuing"
                    );
                }
            }
        }
    }

    Ok(())
}

/// Run stop hooks (when the model stops generating).
///
/// Corresponds to TypeScript: `executeStopHooks()`
pub async fn run_stop_hooks(hook_configs: &[HookEventConfig]) -> Result<PostToolHookResult> {
    if hook_configs.is_empty() {
        return Ok(PostToolHookResult::Continue);
    }

    let stdin_json = serde_json::json!({
        "event": "Stop",
    });

    for config in hook_configs {
        // Stop hooks don't have a tool name to match against, so we only
        // run configs with matcher None or "*"
        if config.matcher.is_some() && config.matcher.as_deref() != Some("*") {
            continue;
        }

        for entry in &config.hooks {
            let hook_label = hook_entry_label(entry);
            debug!(hook = %hook_label, "running stop hook");

            match execute_event_entry(entry, "Stop", &stdin_json).await {
                Ok(output) => {
                    if !output.should_continue {
                        let message = output
                            .stop_reason
                            .or(output.reason)
                            .unwrap_or_else(|| "Stop hook halted continuation".to_string());
                        return Ok(PostToolHookResult::StopContinuation { message });
                    }

                    if let Some(ref stop_reason) = output.stop_reason {
                        return Ok(PostToolHookResult::StopContinuation {
                            message: stop_reason.clone(),
                        });
                    }
                }
                Err(e) => {
                    if config.critical {
                        return Err(anyhow!(
                            "critical stop hook failed (hook '{}'): {}",
                            hook_label,
                            e
                        ));
                    } else {
                        warn!(
                            hook = %hook_label,
                            error = %e,
                            "optional stop hook error, continuing"
                        );
                    }
                }
            }
        }
    }

    Ok(PostToolHookResult::Continue)
}

// ---------------------------------------------------------------------------
// Generic event hook dispatcher
// ---------------------------------------------------------------------------

/// Generic hook event runner for non-tool lifecycle events.
///
/// Fires all matching hooks for the given event. Returns the merged HookOutput.
/// Errors from individual hooks are logged and skipped (fire-and-forget).
///
/// Non-tool events only match configs with `None` or `"*"` matcher (tool-specific
/// matchers like `"Bash"` are skipped).
pub async fn run_event_hooks(
    event_name: &str,
    payload: &Value,
    hook_configs: &[HookEventConfig],
) -> Result<HookOutput> {
    if hook_configs.is_empty() {
        debug!(event = event_name, "event hooks: no hooks configured");
        return Ok(HookOutput::default());
    }

    let mut last_output = HookOutput::default();

    for config in hook_configs {
        // Non-tool events: only match configs with None or "*" matcher
        if config.matcher.is_some() && config.matcher.as_deref() != Some("*") {
            continue;
        }

        for entry in &config.hooks {
            let hook_label = hook_entry_label(entry);
            debug!(event = event_name, hook = %hook_label, "running event hook");

            match execute_event_entry(entry, event_name, payload).await {
                Ok(output) => {
                    last_output = output;
                }
                Err(e) => {
                    warn!(event = event_name, hook = %hook_label, error = %e, "event hook error");
                }
            }
        }
    }

    Ok(last_output)
}

async fn execute_event_entry(
    entry: &HookEntry,
    event_name: &str,
    payload: &Value,
) -> Result<HookOutput> {
    match entry {
        HookEntry::Command {
            command,
            timeout,
            shell,
            ..
        } => execute_command_hook(command, payload, *timeout, shell.as_deref()).await,
        HookEntry::Http {
            url,
            timeout,
            headers,
            allowed_env_vars,
            ..
        } => {
            let result = exec_http_hook(
                url,
                event_name,
                payload,
                headers.as_ref(),
                allowed_env_vars.as_deref(),
                None,
                Some(*timeout),
            )
            .await;
            if !result.ok {
                anyhow::bail!(
                    "HTTP hook failed{}{}",
                    result
                        .status_code
                        .map(|code| format!(" with status {code}"))
                        .unwrap_or_default(),
                    result
                        .error
                        .map(|error| format!(": {error}"))
                        .unwrap_or_default()
                );
            }
            parse_hook_output(&result.body)
        }
        HookEntry::Prompt { .. } | HookEntry::Agent { .. } => {
            anyhow::bail!("prompt and agent hooks are not wired into the runtime yet")
        }
    }
}

fn hook_entry_label(entry: &HookEntry) -> String {
    match entry {
        HookEntry::Command { command, .. } => format!("command:{command}"),
        HookEntry::Http { url, .. } => format!("http:{url}"),
        HookEntry::Prompt { .. } => "prompt".to_string(),
        HookEntry::Agent { .. } => "agent".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Convenience wrappers for non-tool lifecycle hooks
// ---------------------------------------------------------------------------

/// Fire a notification hook (convenience wrapper).
///
/// Sends `{ "title": ..., "body": ... }` to all hooks registered under the
/// `"Notification"` event key.  Fire-and-forget: errors are logged internally.
pub async fn fire_notification_hook(
    title: &str,
    body: &str,
    hooks_map: &std::collections::HashMap<String, serde_json::Value>,
) {
    let configs = load_hook_configs(hooks_map, "Notification");
    if configs.is_empty() {
        return;
    }
    let payload = serde_json::json!({
        "title": title,
        "body": body,
    });
    let _ = run_event_hooks("Notification", &payload, &configs).await;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Helper: make a HookEventConfig with a single command that matches all tools.
    #[cfg(not(windows))]
    fn make_hook_config(command: &str) -> HookEventConfig {
        HookEventConfig {
            matcher: Some("*".to_string()),
            critical: false,
            hooks: vec![HookEntry::Command {
                command: command.to_string(),
                timeout: 10,
                shell: None,
                if_condition: None,
            }],
        }
    }

    fn make_hook_config_with_timeout(
        matcher: Option<&str>,
        command: &str,
        timeout: u64,
        critical: bool,
    ) -> HookEventConfig {
        HookEventConfig {
            matcher: matcher.map(str::to_string),
            critical,
            hooks: vec![HookEntry::Command {
                command: command.to_string(),
                timeout,
                shell: None,
                if_condition: None,
            }],
        }
    }

    #[tokio::test]
    async fn test_run_post_tool_hooks_empty() {
        let tool_result = ToolResult {
            data: Value::String("ok".into()),
            new_messages: vec![],
            ..Default::default()
        };
        let result = run_post_tool_hooks("Bash", &json!({}), &tool_result, &[])
            .await
            .unwrap();
        assert!(matches!(result, PostToolHookResult::Continue));
    }

    #[tokio::test]
    async fn test_run_post_tool_failure_hooks_empty() {
        let result = run_post_tool_failure_hooks("Bash", &json!({}), "test error", &[]).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_run_stop_hooks_empty() {
        let result = run_stop_hooks(&[]).await.unwrap();
        assert!(matches!(result, PostToolHookResult::Continue));
    }

    #[tokio::test]
    async fn test_run_event_hooks_empty() {
        let result = run_event_hooks("SessionStart", &serde_json::json!({}), &[]).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.should_continue);
    }

    #[tokio::test]
    async fn test_run_event_hooks_skips_tool_specific_matchers() {
        // A config with matcher "Bash" should be skipped for non-tool events
        let configs = vec![HookEventConfig {
            matcher: Some("Bash".to_string()),
            critical: false,
            hooks: vec![HookEntry::Command {
                command: r#"echo '{"continue":false,"reason":"should not fire"}'"#.to_string(),
                timeout: 10,
                shell: None,
                if_condition: None,
            }],
        }];

        let result = run_event_hooks("SessionStart", &json!({}), &configs).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        // Should still be the default (continue=true) because the matcher was skipped
        assert!(output.should_continue);
    }

    #[tokio::test]
    async fn test_optional_post_tool_hook_error_continues() {
        let configs = vec![make_hook_config_with_timeout(
            Some("Bash"),
            "echo optional",
            0,
            false,
        )];
        let tool_result = ToolResult {
            data: json!("ok"),
            new_messages: vec![],
            ..Default::default()
        };

        let result = run_post_tool_hooks("Bash", &json!({"command": "ls"}), &tool_result, &configs)
            .await
            .unwrap();

        assert!(matches!(result, PostToolHookResult::Continue));
    }

    #[tokio::test]
    async fn test_critical_post_tool_hook_error_is_visible() {
        let configs = vec![make_hook_config_with_timeout(
            Some("Bash"),
            "echo critical",
            0,
            true,
        )];
        let tool_result = ToolResult {
            data: json!("ok"),
            new_messages: vec![],
            ..Default::default()
        };

        let error = run_post_tool_hooks("Bash", &json!({"command": "ls"}), &tool_result, &configs)
            .await
            .unwrap_err();

        let message = error.to_string();
        assert!(message.contains("critical post-tool hook failed"));
        assert!(message.contains("Bash"));
    }

    #[tokio::test]
    async fn test_optional_post_tool_failure_hook_error_continues() {
        let configs = vec![make_hook_config_with_timeout(
            Some("Bash"),
            "echo optional",
            0,
            false,
        )];

        let result =
            run_post_tool_failure_hooks("Bash", &json!({"command": "ls"}), "boom", &configs).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_critical_post_tool_failure_hook_error_is_visible() {
        let configs = vec![make_hook_config_with_timeout(
            Some("Bash"),
            "echo critical",
            0,
            true,
        )];

        let error =
            run_post_tool_failure_hooks("Bash", &json!({"command": "ls"}), "boom", &configs)
                .await
                .unwrap_err();

        let message = error.to_string();
        assert!(message.contains("critical post-tool failure hook failed"));
        assert!(message.contains("Bash"));
    }

    #[tokio::test]
    async fn test_critical_stop_hook_error_is_visible() {
        let configs = vec![make_hook_config_with_timeout(
            None,
            "echo critical",
            0,
            true,
        )];

        let error = run_stop_hooks(&configs).await.unwrap_err();

        assert!(error.to_string().contains("critical stop hook failed"));
    }

    // =========================================================================
    // Subprocess integration tests
    // =========================================================================

    #[tokio::test]
    #[cfg(not(windows))]
    async fn test_post_tool_hook_stops_continuation() {
        let configs = vec![make_hook_config(
            r#"echo '{"continue":false,"stop_reason":"audit complete, halt"}'"#,
        )];
        let tool_result = ToolResult {
            data: json!("file contents here"),
            new_messages: vec![],
            ..Default::default()
        };

        let result = run_post_tool_hooks(
            "Read",
            &json!({"file_path": "/etc/passwd"}),
            &tool_result,
            &configs,
        )
        .await
        .unwrap();

        match result {
            PostToolHookResult::StopContinuation { message } => {
                assert!(message.contains("audit complete"));
            }
            PostToolHookResult::Continue => {
                panic!("expected StopContinuation, got Continue");
            }
        }
    }

    #[tokio::test]
    #[cfg(not(windows))]
    async fn test_post_tool_hook_continues() {
        let configs = vec![make_hook_config(r#"echo '{"continue":true}'"#)];
        let tool_result = ToolResult {
            data: json!("ok"),
            new_messages: vec![],
            ..Default::default()
        };

        let result = run_post_tool_hooks("Bash", &json!({"command": "ls"}), &tool_result, &configs)
            .await
            .unwrap();

        assert!(matches!(result, PostToolHookResult::Continue));
    }

    #[tokio::test]
    #[cfg(not(windows))]
    async fn test_stop_hook_prevents_stop() {
        let configs = vec![HookEventConfig {
            matcher: None,
            critical: false,
            hooks: vec![HookEntry::Command {
                command: r#"echo '{"continue":false,"stop_reason":"not done yet"}'"#.to_string(),
                timeout: 10,
                shell: None,
                if_condition: None,
            }],
        }];

        let result = run_stop_hooks(&configs).await.unwrap();

        match result {
            PostToolHookResult::StopContinuation { message } => {
                assert!(message.contains("not done yet"));
            }
            PostToolHookResult::Continue => {
                panic!("expected StopContinuation, got Continue");
            }
        }
    }
}
