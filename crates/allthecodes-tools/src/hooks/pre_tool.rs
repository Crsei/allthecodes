//! Pre-tool hook execution.

use anyhow::Result;
use serde_json::Value;
use tracing::{debug, warn};

use super::execution::{execute_command_hook, parse_hook_output};
use super::http_hook::exec_http_hook;
use super::{HookEntry, HookEventConfig, PermissionOverride, PreToolHookResult};
use allthecodes_types::hooks::matches_tool;

// ---------------------------------------------------------------------------
// Pre-tool hooks
// ---------------------------------------------------------------------------

/// Run pre-tool hooks for a tool invocation.
///
/// Corresponds to TypeScript: `runPreToolUseHooks()` in toolExecution.ts
///
/// For each matching hook config:
/// - If any hook returns `should_continue: false` -> Stop
/// - If any hook returns `permission_decision: "allow"` or `"deny"` -> override
/// - If any hook returns `updated_input` -> merge into result
pub async fn run_pre_tool_hooks(
    tool_name: &str,
    input: &Value,
    hook_configs: &[HookEventConfig],
) -> Result<PreToolHookResult> {
    if hook_configs.is_empty() {
        debug!(tool = tool_name, "pre-tool hooks: no hooks configured");
        return Ok(PreToolHookResult::Continue {
            updated_input: None,
            permission_override: None,
        });
    }

    let stdin_json = serde_json::json!({
        "tool_name": tool_name,
        "tool_input": input,
    });

    let mut final_updated_input: Option<Value> = None;
    let mut final_permission_override: Option<PermissionOverride> = None;

    for config in hook_configs {
        if !matches_tool(config.matcher.as_deref(), tool_name) {
            continue;
        }

        for entry in &config.hooks {
            let hook_label = hook_entry_label(entry);
            debug!(tool = tool_name, hook = %hook_label, "running pre-tool hook");

            match execute_pre_tool_entry(entry, &stdin_json).await {
                Ok(output) => {
                    // Check for stop
                    if !output.should_continue {
                        let message = output
                            .reason
                            .or(output.stop_reason)
                            .unwrap_or_else(|| "Hook stopped execution".to_string());
                        return Ok(PreToolHookResult::Stop { message });
                    }

                    // Check for permission override
                    if let Some(ref perm) = output.permission_decision {
                        match perm.as_str() {
                            "allow" => {
                                final_permission_override = Some(PermissionOverride::Allow);
                            }
                            "deny" => {
                                let reason = output
                                    .reason
                                    .clone()
                                    .unwrap_or_else(|| "Denied by hook".to_string());
                                final_permission_override =
                                    Some(PermissionOverride::Deny { reason });
                            }
                            other => {
                                debug!(
                                    decision = other,
                                    "unknown permission_decision from hook, ignoring"
                                );
                            }
                        }
                    }

                    // Check for updated input
                    if let Some(updated) = output.updated_input {
                        final_updated_input = Some(updated);
                    }
                }
                Err(e) => {
                    if config.critical {
                        return Err(anyhow::anyhow!(
                            "critical pre-tool hook failed for tool '{}' (hook '{}'): {}",
                            tool_name,
                            hook_label,
                            e
                        ));
                    } else {
                        warn!(
                            tool = tool_name,
                            hook = %hook_label,
                            error = %e,
                            "optional pre-tool hook error, continuing"
                        );
                    }
                }
            }
        }
    }

    Ok(PreToolHookResult::Continue {
        updated_input: final_updated_input,
        permission_override: final_permission_override,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_run_pre_tool_hooks_empty() {
        let result = run_pre_tool_hooks("Bash", &json!({}), &[]).await.unwrap();
        assert!(matches!(
            result,
            PreToolHookResult::Continue {
                updated_input: None,
                permission_override: None,
            }
        ));
    }

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

    /// Helper: make a HookEventConfig matching a specific tool name.
    fn make_hook_config_for_tool(tool: &str, command: &str) -> HookEventConfig {
        HookEventConfig {
            matcher: Some(tool.to_string()),
            critical: false,
            hooks: vec![HookEntry::Command {
                command: command.to_string(),
                timeout: 10,
                shell: None,
                if_condition: None,
            }],
        }
    }

    fn make_hook_config_for_tool_with_timeout(
        tool: &str,
        command: &str,
        timeout: u64,
        critical: bool,
    ) -> HookEventConfig {
        HookEventConfig {
            matcher: Some(tool.to_string()),
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
    #[cfg(not(windows))]
    async fn test_pre_tool_hook_stops_execution() {
        let configs = vec![make_hook_config(
            r#"echo '{"continue":false,"reason":"blocked by policy"}'"#,
        )];

        let result = run_pre_tool_hooks("Bash", &json!({"command": "rm -rf /"}), &configs)
            .await
            .unwrap();

        match result {
            PreToolHookResult::Stop { message } => {
                assert!(message.contains("blocked by policy"));
            }
            other => panic!("expected Stop, got: {:?}", other),
        }
    }

    #[tokio::test]
    #[cfg(not(windows))]
    async fn test_pre_tool_hook_denies_permission() {
        let configs = vec![make_hook_config(
            r#"echo '{"continue":true,"permission_decision":"deny","reason":"not in allowlist"}'"#,
        )];

        let result = run_pre_tool_hooks("Bash", &json!({"command": "ls"}), &configs)
            .await
            .unwrap();

        match result {
            PreToolHookResult::Continue {
                permission_override: Some(PermissionOverride::Deny { reason }),
                ..
            } => {
                assert!(reason.contains("not in allowlist"));
            }
            other => panic!("expected Deny override, got: {:?}", other),
        }
    }

    #[tokio::test]
    #[cfg(not(windows))]
    async fn test_pre_tool_hook_allows_permission() {
        let configs = vec![make_hook_config(
            r#"echo '{"continue":true,"permission_decision":"allow"}'"#,
        )];

        let result = run_pre_tool_hooks("Bash", &json!({"command": "ls"}), &configs)
            .await
            .unwrap();

        assert!(matches!(
            result,
            PreToolHookResult::Continue {
                permission_override: Some(PermissionOverride::Allow),
                ..
            }
        ));
    }

    #[tokio::test]
    #[cfg(not(windows))]
    async fn test_pre_tool_hook_modifies_input() {
        let configs = vec![make_hook_config(
            r#"echo '{"continue":true,"updated_input":{"command":"ls -la --safe"}}'"#,
        )];

        let result = run_pre_tool_hooks("Bash", &json!({"command": "rm -rf /"}), &configs)
            .await
            .unwrap();

        match result {
            PreToolHookResult::Continue {
                updated_input: Some(new_input),
                ..
            } => {
                assert_eq!(new_input, json!({"command": "ls -la --safe"}));
            }
            other => panic!("expected updated_input, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_pre_tool_hook_matcher_skips_non_matching() {
        let configs = vec![make_hook_config_for_tool(
            "Read",
            r#"echo '{"continue":false,"reason":"should not fire"}'"#,
        )];

        // Call with "Bash" — should skip the "Read" matcher without spawning
        let result = run_pre_tool_hooks("Bash", &json!({}), &configs)
            .await
            .unwrap();

        assert!(matches!(
            result,
            PreToolHookResult::Continue {
                updated_input: None,
                permission_override: None,
            }
        ));
    }

    #[tokio::test]
    async fn test_optional_pre_tool_hook_error_continues() {
        let configs = vec![make_hook_config_for_tool_with_timeout(
            "Bash",
            "echo optional",
            0,
            false,
        )];

        let result = run_pre_tool_hooks("Bash", &json!({"command": "ls"}), &configs)
            .await
            .unwrap();

        assert!(matches!(
            result,
            PreToolHookResult::Continue {
                updated_input: None,
                permission_override: None,
            }
        ));
    }

    #[tokio::test]
    async fn test_critical_pre_tool_hook_error_blocks() {
        let configs = vec![make_hook_config_for_tool_with_timeout(
            "Bash",
            "echo critical",
            0,
            true,
        )];

        let error = run_pre_tool_hooks("Bash", &json!({"command": "ls"}), &configs)
            .await
            .unwrap_err();

        let message = error.to_string();
        assert!(message.contains("critical pre-tool hook failed"));
        assert!(message.contains("Bash"));
    }
}

async fn execute_pre_tool_entry(
    entry: &HookEntry,
    stdin_json: &Value,
) -> Result<super::HookOutput> {
    match entry {
        HookEntry::Command {
            command,
            timeout,
            shell,
            ..
        } => execute_command_hook(command, stdin_json, *timeout, shell.as_deref()).await,
        HookEntry::Http {
            url,
            timeout,
            headers,
            allowed_env_vars,
            ..
        } => {
            let result = exec_http_hook(
                url,
                "PreToolUse",
                stdin_json,
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
