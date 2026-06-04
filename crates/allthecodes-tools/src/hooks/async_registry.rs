//! Async hook registry — tracks pending asynchronous hooks, polls for
//! completion, and collects responses.
//!
//! Port of TypeScript `AsyncHookRegistry.ts`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

use chrono::Utc;
use serde_json::Value;
use tracing::debug;

use allthecodes_types::hooks::PendingAsyncHook;

/// Global registry of pending async hooks.
static PENDING_HOOKS: LazyLock<Mutex<HashMap<String, PendingHookState>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Internal state for a pending hook beyond the shared types.
struct PendingHookState {
    info: PendingAsyncHook,
    /// Whether the hook process has completed.
    completed: Arc<AtomicBool>,
    /// Collected stdout/stderr so far.
    stdout: String,
    stderr: String,
    /// Exit code (set when completed).
    exit_code: Option<i32>,
    /// Stop progress interval function.
    stop_progress: Option<Box<dyn Fn() + Send>>,
}

/// Response collected from a completed async hook.
#[derive(Debug, Clone)]
pub struct AsyncHookResponse {
    pub process_id: String,
    pub response: Value,
    pub hook_name: String,
    pub hook_event: String,
    pub tool_name: Option<String>,
    pub plugin_id: Option<String>,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
}

/// Register a new pending async hook.
pub fn register_pending_async_hook(
    process_id: &str,
    hook_id: &str,
    hook_name: &str,
    hook_event: &str,
    command: &str,
    timeout_secs: u64,
    tool_name: Option<String>,
    plugin_id: Option<String>,
    stop_progress: Option<Box<dyn Fn() + Send>>,
) {
    let info = PendingAsyncHook {
        process_id: process_id.to_string(),
        hook_id: hook_id.to_string(),
        hook_name: hook_name.to_string(),
        hook_event: hook_event.to_string(),
        tool_name,
        plugin_id,
        start_time: Utc::now(),
        timeout: timeout_secs,
        command: command.to_string(),
        response_attachment_sent: false,
    };

    debug!("Hooks: Registering async hook {process_id} ({hook_name}) with timeout {timeout_secs}s");

    let mut hooks = PENDING_HOOKS.lock().expect("PENDING_HOOKS lock poisoned");
    hooks.insert(
        process_id.to_string(),
        PendingHookState {
            info,
            completed: Arc::new(AtomicBool::new(false)),
            stdout: String::new(),
            stderr: String::new(),
            exit_code: None,
            stop_progress,
        },
    );
}

/// Get all pending async hooks that haven't had their response sent yet.
pub fn get_pending_async_hooks() -> Vec<PendingAsyncHook> {
    let hooks = PENDING_HOOKS.lock().expect("PENDING_HOOKS lock poisoned");
    hooks
        .values()
        .filter(|h| !h.info.response_attachment_sent)
        .map(|h| h.info.clone())
        .collect()
}

/// Mark a pending hook as completed with the given output.
pub fn complete_async_hook(process_id: &str, stdout: &str, stderr: &str, exit_code: i32) {
    let mut hooks = PENDING_HOOKS.lock().expect("PENDING_HOOKS lock poisoned");
    if let Some(hook) = hooks.get_mut(process_id) {
        hook.stdout = stdout.to_string();
        hook.stderr = stderr.to_string();
        hook.exit_code = Some(exit_code);
        hook.completed.store(true, Ordering::Relaxed);
    }
}

/// Check for completed async hooks and collect their responses.
pub fn check_for_async_hook_responses(max_hooks: usize) -> Vec<AsyncHookResponse> {
    let mut responses = Vec::new();
    let mut to_remove = Vec::new();

    let mut hooks = PENDING_HOOKS.lock().expect("PENDING_HOOKS lock poisoned");

    for (process_id, hook) in hooks.iter_mut() {
        if responses.len() >= max_hooks {
            break;
        }

        if hook.info.response_attachment_sent {
            continue;
        }

        // Check if the hook has timed out
        let elapsed = Utc::now() - hook.info.start_time;
        let timeout_chrono = chrono::Duration::seconds(hook.info.timeout as i64);
        let timed_out = elapsed > timeout_chrono;

        if !hook.completed.load(Ordering::Relaxed) && !timed_out {
            continue;
        }

        // If timed out without completion, mark as cancelled
        let exit_code = hook.exit_code.unwrap_or(1);

        // Parse first JSON line from stdout as response
        let response = parse_hook_json_response(&hook.stdout);

        // Mark as sent
        hook.info.response_attachment_sent = true;

        // Call stop progress
        if let Some(stop) = hook.stop_progress.take() {
            stop();
        }

        debug!(
            "Hooks: Found response from {process_id} ({}), exit_code: {exit_code}",
            hook.info.hook_name
        );

        responses.push(AsyncHookResponse {
            process_id: process_id.clone(),
            response,
            hook_name: hook.info.hook_name.clone(),
            hook_event: hook.info.hook_event.clone(),
            tool_name: hook.info.tool_name.clone(),
            plugin_id: hook.info.plugin_id.clone(),
            stdout: hook.stdout.clone(),
            stderr: hook.stderr.clone(),
            exit_code: Some(exit_code),
        });

        to_remove.push(process_id.clone());
    }

    drop(hooks);

    // Remove processed hooks
    let mut hooks = PENDING_HOOKS.lock().expect("PENDING_HOOKS lock poisoned");
    for id in to_remove {
        hooks.remove(&id);
    }

    responses
}

/// Remove delivered async hooks by process IDs.
pub fn remove_delivered_async_hooks(process_ids: &[String]) {
    let mut hooks = PENDING_HOOKS.lock().expect("PENDING_HOOKS lock poisoned");
    for id in process_ids {
        if let Some(hook) = hooks.get(id) {
            if hook.info.response_attachment_sent {
                if let Some(stop) = hooks.remove(id).and_then(|h| h.stop_progress) {
                    stop();
                }
            }
        }
    }
}

/// Finalize all pending async hooks (called on shutdown).
pub async fn finalize_pending_async_hooks() {
    let hooks: Vec<String> = {
        let hooks = PENDING_HOOKS.lock().expect("PENDING_HOOKS lock poisoned");
        hooks.keys().cloned().collect()
    };

    for id in hooks {
        let mut hooks = PENDING_HOOKS.lock().expect("PENDING_HOOKS lock poisoned");
        if let Some(hook) = hooks.remove(&id) {
            if let Some(stop) = hook.stop_progress {
                stop();
            }
            debug!("Hooks: Finalized pending hook {id}");
        }
    }
}

/// Clear all async hooks (test utility).
pub fn clear_all_async_hooks() {
    let mut hooks = PENDING_HOOKS.lock().expect("PENDING_HOOKS lock poisoned");
    for (_, hook) in hooks.drain() {
        if let Some(stop) = hook.stop_progress {
            stop();
        }
    }
}

fn parse_hook_json_response(stdout: &str) -> Value {
    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('{') {
            if let Ok(val) = serde_json::from_str::<Value>(trimmed) {
                // Exclude async responses (they have an "async" field)
                if !val.get("async").is_some() {
                    return val;
                }
            }
        }
    }
    Value::Object(Default::default()) // empty object: {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[serial_test::serial]
    fn test_register_and_check_empty() {
        clear_all_async_hooks();

        // No hooks registered
        let pending = get_pending_async_hooks();
        assert!(pending.is_empty());

        let responses = check_for_async_hook_responses(10);
        assert!(responses.is_empty());
    }

    #[test]
    fn test_parse_json_response() {
        let stdout = r#"{"continue":false,"reason":"blocked"}"#;
        let parsed = parse_hook_json_response(stdout);
        assert_eq!(parsed["reason"], "blocked");

        // Plain text should return empty object
        let parsed = parse_hook_json_response("plain text output");
        assert!(parsed.as_object().unwrap().is_empty());
    }

    #[test]
    fn test_parse_json_skips_async_responses() {
        let stdout = r#"{"async":true,"asyncTimeout":5000}"#;
        let parsed = parse_hook_json_response(stdout);
        assert!(parsed.as_object().unwrap().is_empty());
    }

    #[test]
    #[serial_test::serial]
    fn test_get_pending_after_register() {
        clear_all_async_hooks();
        register_pending_async_hook(
            "proc-1",
            "hook-1",
            "test-hook",
            "Stop",
            "echo ok",
            30,
            None,
            None,
            None,
        );

        let pending = get_pending_async_hooks();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].process_id, "proc-1");
    }

    #[test]
    #[serial_test::serial]
    fn test_complete_async_hook() {
        clear_all_async_hooks();
        register_pending_async_hook(
            "proc-2",
            "hook-2",
            "test-hook-2",
            "PreToolUse",
            "echo json",
            30,
            Some("Bash".into()),
            None,
            None,
        );

        complete_async_hook("proc-2", r#"{"continue":true}"#, "", 0);

        let responses = check_for_async_hook_responses(10);
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0].process_id, "proc-2");
        assert!(responses[0].response["continue"].as_bool().unwrap_or(false));
    }
}
