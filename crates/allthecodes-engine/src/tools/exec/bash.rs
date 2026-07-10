use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use async_trait::async_trait;
use parking_lot::Mutex;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use allthecodes_engine::types::tool::{
    InterruptBehavior, PermissionResult, Tool, ToolProgress, ToolResult, ToolUseContext,
    ValidationResult,
};
use allthecodes_permissions::dangerous::is_dangerous_command;
use allthecodes_permissions::shell_policy::{
    decide_shell_policy, ShellPolicyKind, ShellPolicyShell,
};
use allthecodes_sandbox::{make_runner, policy_from_app_state, preflight_shell_command};
use allthecodes_tools::exec::bash as bash_spec;
pub(crate) use allthecodes_tools::exec::truncate_output;
use allthecodes_types::{message::AssistantMessage, ShellExecutionOutput};
use allthecodes_utils::bash::{
    extract_command_name, extract_command_prefixes, has_malformed_tokens, has_unterminated_quotes,
    is_command_parseable, parse_command, resolve_timeout, split_compound_command,
    validate_heredocs,
};
use allthecodes_utils::git_operation_tracking::track_git_operations_json;
use allthecodes_utils::shell::detect_default_shell;

use allthecodes_shell_command::provider::{BashProvider, ShellProvider};

use super::process_control::{
    configure_process_group, wait_for_exit_or_termination, ControlledExit,
};

#[allow(clippy::too_many_arguments)]
fn shell_execution_output(
    command: &str,
    cwd: &std::path::Path,
    stdout: String,
    stderr: String,
    exit_code: Option<i32>,
    interrupted: bool,
    termination: Option<&str>,
    error: Option<String>,
) -> ShellExecutionOutput {
    ShellExecutionOutput {
        command: Some(command.to_string()),
        cwd: Some(cwd.to_path_buf()),
        stdout,
        stderr,
        exit_code,
        interrupted,
        termination: termination.map(str::to_string),
        error,
    }
}

/// Snapshot for `ToolProgress::output` — carries the tail-capped
/// rendering of the current stdout/stderr buffers plus whole-stream
/// counters (lines/bytes) so the frontend can show `+N lines` / total
/// bytes even when the output itself is truncated.
struct ProgressSnapshot {
    output: String,
    total_lines: u64,
    total_bytes: u64,
}

/// Build a `ProgressSnapshot` from the live stdout/stderr buffers.
///
/// The merge rule matches `call`'s final `combined` output: `stdout`
/// first, then a separator newline, then `stderr`. Counters always
/// reflect the full (pre-truncation) streams so the UI can show an
/// accurate `~N lines`.
fn build_progress_snapshot(
    stdout_buf: &Arc<Mutex<String>>,
    stderr_buf: &Arc<Mutex<String>>,
    max_chars: usize,
) -> ProgressSnapshot {
    let stdout = stdout_buf.lock().clone();
    let stderr = stderr_buf.lock().clone();

    let mut combined = String::new();
    if !stdout.is_empty() {
        combined.push_str(&stdout);
    }
    if !stderr.is_empty() {
        if !combined.is_empty() {
            combined.push('\n');
        }
        combined.push_str(&stderr);
    }

    let total_bytes = combined.len() as u64;
    let total_lines = combined.lines().count() as u64;

    let truncated = truncate_output(&combined, max_chars);

    ProgressSnapshot {
        output: truncated,
        total_lines,
        total_bytes,
    }
}

/// BashTool — Execute shell commands
///
/// Corresponds to TypeScript: tools/BashTool
pub struct BashTool;

impl BashTool {
    pub fn new() -> Self {
        BashTool
    }

    fn parse_input(input: &Value) -> (String, Option<u64>, Option<String>) {
        let command = input
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let timeout_ms = input.get("timeout").and_then(|v| v.as_u64());
        let description = input
            .get("description")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        (command, timeout_ms, description)
    }
}

impl Default for BashTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str {
        bash_spec::NAME
    }

    async fn description(&self, _input: &Value) -> String {
        bash_spec::description().to_string()
    }

    fn input_json_schema(&self) -> Value {
        bash_spec::input_schema()
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        false
    }

    fn is_read_only(&self, input: &Value) -> bool {
        let command = input.get("command").and_then(|v| v.as_str()).unwrap_or("");
        matches!(
            decide_shell_policy(command, ShellPolicyShell::Bash).kind,
            ShellPolicyKind::ReadOnly
        )
    }

    fn interrupt_behavior(&self) -> InterruptBehavior {
        InterruptBehavior::Cancel
    }

    fn get_path(&self, _input: &Value) -> Option<String> {
        None
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        let command = input.get("command").and_then(|v| v.as_str()).unwrap_or("");
        if !is_command_parseable(command) {
            return ValidationResult::Error {
                message: if command.is_empty() {
                    "Command must not be empty".to_string()
                } else {
                    "Command exceeds maximum parseable length".to_string()
                },
                error_code: 1,
            };
        }
        if has_unterminated_quotes(command) {
            return ValidationResult::Error {
                message: "Command has unterminated quotes".to_string(),
                error_code: 1,
            };
        }
        if let Err(message) = validate_heredocs(command) {
            return ValidationResult::Error {
                message,
                error_code: 1,
            };
        }
        // Check each parsed token for unbalanced brackets/braces
        if let Ok(tokens) = parse_command(command) {
            for token in &tokens {
                if has_malformed_tokens(token) {
                    return ValidationResult::Error {
                        message: format!(
                            "Command contains malformed token with unbalanced brackets: {}",
                            token
                        ),
                        error_code: 1,
                    };
                }
            }
        }
        ValidationResult::Ok
    }

    async fn check_permissions(&self, input: &Value, ctx: &ToolUseContext) -> PermissionResult {
        let command = input.get("command").and_then(|v| v.as_str()).unwrap_or("");

        // Check each sub-command in a compound command for dangerous patterns
        for subcmd in split_compound_command(command) {
            if let Some(reason) = is_dangerous_command(&subcmd) {
                return PermissionResult::Ask {
                    message: format!("Dangerous command detected: {}", reason),
                };
            }
        }

        // Check per-command prefix deny rules from permission context.
        // Rules like "Bash(prefix:rm)" block commands starting with "rm".
        let app_state = (ctx.get_app_state)();
        let prefixes = extract_command_prefixes(command);
        for deny_rules in app_state.tool_permission_context.always_deny_rules.values() {
            for rule in deny_rules {
                if let Some(prefix_pat) = rule
                    .strip_prefix("Bash(prefix:")
                    .and_then(|s| s.strip_suffix(')'))
                {
                    if prefixes.iter().any(|p| p.starts_with(prefix_pat)) {
                        return PermissionResult::Deny {
                            message: format!("Denied by rule: {}", rule),
                        };
                    }
                }
            }
        }

        // Sandbox escape-hatch gate: when the caller passed
        // `dangerouslyDisableSandbox: true` but settings forbid it, deny.
        let wants_escape = input
            .get("dangerouslyDisableSandbox")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if wants_escape
            && !app_state
                .settings
                .sandbox
                .allow_unsandboxed_commands
                .unwrap_or(!app_state.settings.sandbox.enabled.unwrap_or(false))
        {
            return PermissionResult::Deny {
                message: "sandbox.allowUnsandboxedCommands=false rejects \
                          dangerouslyDisableSandbox"
                    .to_string(),
            };
        }

        PermissionResult::Allow {
            updated_input: input.clone(),
        }
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolUseContext,
        _parent_message: &AssistantMessage,
        on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let (raw_command, timeout_ms, _description) = Self::parse_input(&input);

        if raw_command.is_empty() {
            let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
            return Ok(ToolResult {
                data: json!({ "error": "Command must not be empty" }),
                new_messages: vec![],
                shell: Some(shell_execution_output(
                    &raw_command,
                    &cwd,
                    String::new(),
                    String::new(),
                    None,
                    false,
                    None,
                    Some("Command must not be empty".to_string()),
                )),
                ..Default::default()
            });
        }

        // Detect the best available shell and create a BashProvider
        let shell = detect_default_shell();
        let provider = BashProvider::new(shell.path, shell.exec_args, shell.needs_stdin_redirect);
        let command = provider.normalize_command(&raw_command);
        let mut cmd = Command::new(provider.shell_path());
        for arg in provider.exec_args() {
            cmd.arg(arg);
        }
        cmd.arg(&command);

        // Inject shell environment (TERM, LANG, GIT_PAGER=cat, CLAUDE_CODE=1, etc.)
        for (k, v) in provider.build_env() {
            cmd.env(&k, &v);
        }

        // Sandbox integration. Build the effective policy from the current
        // AppState + cwd and wrap the command when the sandbox is active.
        // The `dangerouslyDisableSandbox` escape hatch (already gated by
        // check_permissions) bypasses wrapping but still records a warning.
        let escape = input
            .get("dangerouslyDisableSandbox")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let app_state_arc = (ctx.get_app_state)();
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let policy = policy_from_app_state(
            &app_state_arc.tool_permission_context,
            &app_state_arc.settings.sandbox,
            cwd.clone(),
            false,
        );

        if let Err(err) = preflight_shell_command(&policy, &command) {
            return Ok(ToolResult {
                data: json!({
                    "error": err.to_string(),
                    "sandbox_blocked": true,
                }),
                new_messages: vec![],
                shell: Some(shell_execution_output(
                    &raw_command,
                    &cwd,
                    String::new(),
                    String::new(),
                    None,
                    false,
                    None,
                    Some(err.to_string()),
                )),
                ..Default::default()
            });
        }

        // Excluded-commands check: if the command matches, short-circuit the
        // sandbox wrapping (runs in the host) unless unsandboxed is forbidden.
        let is_excluded = policy.is_excluded_command(&command);
        if is_excluded && !policy.allow_unsandboxed_commands {
            return Ok(ToolResult {
                data: json!({
                    "error": allthecodes_sandbox::SandboxError::EscapeHatchDisabled {
                        command: command.clone()
                    }
                    .to_string(),
                    "sandbox_blocked": true,
                }),
                new_messages: vec![],
                shell: Some(shell_execution_output(
                    &raw_command,
                    &cwd,
                    String::new(),
                    String::new(),
                    None,
                    false,
                    None,
                    Some(
                        allthecodes_sandbox::SandboxError::EscapeHatchDisabled {
                            command: command.clone(),
                        }
                        .to_string(),
                    ),
                )),
                ..Default::default()
            });
        }

        let mut sandbox_description = String::from("unsandboxed");
        if !escape && !is_excluded {
            if let Some(runner) = make_runner(&policy) {
                match runner.prepare(cmd, &policy, &cwd) {
                    Ok(prepared) => {
                        sandbox_description = prepared.description;
                        cmd = prepared.cmd;
                        // Piped stdio must be re-applied because the wrapper command
                        // is freshly constructed.
                        cmd.stdout(std::process::Stdio::piped());
                        cmd.stderr(std::process::Stdio::piped());
                    }
                    Err(e) => {
                        return Ok(ToolResult {
                            data: json!({
                                "error": e.to_string(),
                                "sandbox_blocked": true,
                            }),
                            new_messages: vec![],
                            shell: Some(shell_execution_output(
                                &raw_command,
                                &cwd,
                                String::new(),
                                String::new(),
                                None,
                                false,
                                None,
                                Some(e.to_string()),
                            )),
                            ..Default::default()
                        });
                    }
                }
            }
        } else {
            // Capture stdout and stderr on the unwrapped command.
            cmd.stdout(std::process::Stdio::piped());
            cmd.stderr(std::process::Stdio::piped());
        }
        tracing::debug!(sandbox = %sandbox_description, "bash tool exec");

        let timeout_duration = resolve_timeout(timeout_ms);
        let timeout_millis = timeout_duration.as_millis() as u64;

        // Ensure stdio is piped even on the unwrapped path so we can stream.
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        configure_process_group(&mut cmd);
        cmd.kill_on_drop(true);

        // Spawn the child so we can stream stdout/stderr back as the
        // command runs. The upstream Ink UI renders a "Running… (Xs)"
        // ticker plus a tail of recent output driven by `ToolProgress`
        // events — we emit those here via `on_progress`.
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                return Ok(ToolResult {
                    data: json!({
                        "error": format!("Failed to spawn command: {}", e)
                    }),
                    new_messages: vec![],
                    shell: Some(shell_execution_output(
                        &raw_command,
                        &cwd,
                        String::new(),
                        String::new(),
                        None,
                        false,
                        None,
                        Some(format!("Failed to spawn command: {}", e)),
                    )),
                    ..Default::default()
                });
            }
        };

        let stdout_pipe = child.stdout.take();
        let stderr_pipe = child.stderr.take();
        let stdout_buf: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        let stderr_buf: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));

        let stdout_task = stdout_pipe.map(|pipe| {
            let buf = stdout_buf.clone();
            tokio::spawn(async move {
                let mut reader = BufReader::new(pipe).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    let mut b = buf.lock();
                    b.push_str(&line);
                    b.push('\n');
                }
            })
        });

        let stderr_task = stderr_pipe.map(|pipe| {
            let buf = stderr_buf.clone();
            tokio::spawn(async move {
                let mut reader = BufReader::new(pipe).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    let mut b = buf.lock();
                    b.push_str(&line);
                    b.push('\n');
                }
            })
        });

        // The outer `QueryEngineDeps::execute_tool` wrapper stamps the
        // real `tool_use_id` onto every `ToolProgress` we emit, so the
        // tool itself only needs to fill in `data`.
        let start = Instant::now();
        let progress_interval = Duration::from_millis(1000);
        let max_chars = self.max_result_size_chars();

        // Move the box into a shared `Arc` so both the tick task (below)
        // and the post-exit emit reuse the same callback without raw
        // pointer gymnastics.
        let on_progress_arc: Option<Arc<dyn Fn(ToolProgress) + Send + Sync>> =
            on_progress.map(Arc::from);

        // Fire a ticker on a background task so the frontend sees
        // `ToolProgress` updates roughly once a second while the command
        // runs. The task is aborted as soon as the child exits.
        let progress_handle = on_progress_arc.as_ref().map(|cb| {
            let cb = cb.clone();
            let stdout_buf = stdout_buf.clone();
            let stderr_buf = stderr_buf.clone();
            let timeout_ms = timeout_millis;
            tokio::spawn(async move {
                let mut ticker = tokio::time::interval(progress_interval);
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                // Skip the immediate tick so we don't fire at t=0.
                ticker.tick().await;
                loop {
                    ticker.tick().await;
                    let elapsed_seconds = start.elapsed().as_secs();
                    let snapshot = build_progress_snapshot(&stdout_buf, &stderr_buf, max_chars);
                    cb(ToolProgress {
                        tool_use_id: String::new(),
                        data: json!({
                            "tool": "Bash",
                            "output": snapshot.output,
                            "elapsed_seconds": elapsed_seconds,
                            "total_lines": snapshot.total_lines,
                            "total_bytes": snapshot.total_bytes,
                            "timeout_ms": timeout_ms,
                        }),
                    });
                }
            })
        });

        let exit_status =
            wait_for_exit_or_termination(&mut child, timeout_duration, ctx.abort_signal.clone())
                .await;

        if let Some(handle) = progress_handle {
            handle.abort();
        }

        // Ensure the reader tasks drain whatever was left after the child
        // exited before we snapshot the buffers for the final result.
        if let Some(handle) = stdout_task {
            let _ = handle.await;
        }
        if let Some(handle) = stderr_task {
            let _ = handle.await;
        }

        let stdout = stdout_buf.lock().clone();
        let stderr = stderr_buf.lock().clone();

        match exit_status {
            ControlledExit::Exited(Ok(status)) => {
                let exit_code = status.code().unwrap_or(-1);

                let mut combined = String::new();
                if !stdout.is_empty() {
                    combined.push_str(&stdout);
                }
                if !stderr.is_empty() {
                    if !combined.is_empty() {
                        combined.push('\n');
                    }
                    combined.push_str(&stderr);
                }

                let git_operations =
                    track_git_operations_json(&raw_command, exit_code, Some(&combined));
                combined = truncate_output(&combined, max_chars);

                // Final progress tick — lets the UI flip from
                // "Running…" to "Done" with the complete tail.
                if let Some(ref cb) = on_progress_arc {
                    let elapsed_seconds = start.elapsed().as_secs();
                    let snapshot = build_progress_snapshot(&stdout_buf, &stderr_buf, max_chars);
                    cb(ToolProgress {
                        tool_use_id: String::new(),
                        data: json!({
                            "tool": "Bash",
                            "output": snapshot.output,
                            "elapsed_seconds": elapsed_seconds,
                            "total_lines": snapshot.total_lines,
                            "total_bytes": snapshot.total_bytes,
                            "timeout_ms": timeout_millis,
                        }),
                    });
                }

                let mut data = json!({
                    "stdout": stdout.clone(),
                    "stderr": stderr.clone(),
                    "exit_code": exit_code,
                    "output": combined,
                });
                if let Some(git_operations) = git_operations {
                    if let Some(object) = data.as_object_mut() {
                        object.insert("git_operations".to_string(), git_operations);
                    }
                }

                Ok(ToolResult {
                    data,
                    new_messages: vec![],
                    shell: Some(shell_execution_output(
                        &raw_command,
                        &cwd,
                        stdout,
                        stderr,
                        Some(exit_code),
                        false,
                        None,
                        None,
                    )),
                    ..Default::default()
                })
            }
            ControlledExit::TimedOut(wait_result) => {
                let exit_code = wait_result
                    .ok()
                    .and_then(|status| status.code())
                    .unwrap_or(143);
                let error = format!("Command timed out after {}ms", timeout_duration.as_millis());
                Ok(ToolResult {
                    data: json!({
                        "error": error,
                        "stdout": stdout.clone(),
                        "stderr": stderr.clone(),
                        "exit_code": exit_code,
                        "interrupted": true,
                        "termination": "timeout",
                    }),
                    new_messages: vec![],
                    shell: Some(shell_execution_output(
                        &raw_command,
                        &cwd,
                        stdout,
                        stderr,
                        Some(exit_code),
                        true,
                        Some("timeout"),
                        Some(format!(
                            "Command timed out after {}ms",
                            timeout_duration.as_millis()
                        )),
                    )),
                    ..Default::default()
                })
            }
            ControlledExit::Cancelled(wait_result) => {
                let exit_code = wait_result
                    .ok()
                    .and_then(|status| status.code())
                    .unwrap_or(137);
                Ok(ToolResult {
                    data: json!({
                        "error": "Command interrupted",
                        "stdout": stdout.clone(),
                        "stderr": stderr.clone(),
                        "exit_code": exit_code,
                        "interrupted": true,
                        "termination": "cancelled",
                    }),
                    new_messages: vec![],
                    shell: Some(shell_execution_output(
                        &raw_command,
                        &cwd,
                        stdout,
                        stderr,
                        Some(exit_code),
                        true,
                        Some("cancelled"),
                        Some("Command interrupted".to_string()),
                    )),
                    ..Default::default()
                })
            }
            ControlledExit::Exited(Err(e)) => Ok(ToolResult {
                data: json!({ "error": format!("Failed to execute command: {}", e) }),
                new_messages: vec![],
                shell: Some(shell_execution_output(
                    &raw_command,
                    &cwd,
                    stdout,
                    stderr,
                    None,
                    false,
                    None,
                    Some(format!("Failed to execute command: {}", e)),
                )),
                ..Default::default()
            }),
        }
    }

    async fn prompt(&self) -> String {
        bash_spec::prompt()
    }

    fn user_facing_name(&self, input: Option<&Value>) -> String {
        if let Some(name) = input
            .and_then(|v| v.get("command"))
            .and_then(|v| v.as_str())
            .and_then(extract_command_name)
        {
            bash_spec::user_facing_name(Some(&name))
        } else {
            bash_spec::user_facing_name(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_short_output() {
        let output = "Hello, world!\nLine 2\nLine 3\n";
        let result = truncate_output(output, 1000);
        assert_eq!(result, output);
    }

    #[test]
    fn test_truncate_head_tail() {
        // Generate 500 lines of output
        let lines: Vec<String> = (1..=500).map(|i| format!("Line {}", i)).collect();
        let output = lines.join("\n");
        // Use a generous limit so head+tail fits but full output doesn't
        let max_chars = output.len() / 2;
        let result = truncate_output(&output, max_chars);

        // Should contain the separator
        assert!(
            result.contains("lines omitted"),
            "Expected separator with 'lines omitted' in truncated output"
        );

        // Should start with the first line
        assert!(
            result.starts_with("Line 1\n"),
            "Expected output to start with 'Line 1'"
        );

        // Should end with the last line
        assert!(
            result.ends_with("Line 500"),
            "Expected output to end with 'Line 500'"
        );

        // Should be within the limit
        assert!(
            result.len() <= max_chars,
            "Truncated output ({}) exceeds max_chars ({})",
            result.len(),
            max_chars
        );
    }

    #[test]
    fn test_truncate_preserves_lines() {
        // Generate 400 lines
        let lines: Vec<String> = (1..=400).map(|i| format!("Line {:04}", i)).collect();
        let output = lines.join("\n");
        let max_chars = output.len() / 2;
        let result = truncate_output(&output, max_chars);

        // Every line in the result should be complete (not cut mid-line)
        for line in result.lines() {
            if line.contains("omitted") {
                // This is the separator line, skip it
                continue;
            }
            if line.is_empty() {
                // Empty lines around the separator
                continue;
            }
            assert!(
                line.starts_with("Line "),
                "Found a line that doesn't start with 'Line ': '{}'",
                line
            );
        }
    }

    #[test]
    fn test_bash_read_only_safe_command() {
        let tool = BashTool;
        assert!(tool.is_read_only(&json!({ "command": "git status --short" })));
        assert!(tool.is_read_only(&json!({ "command": "rg pattern src" })));
    }

    #[test]
    fn test_bash_read_only_blocks_shell_syntax() {
        let tool = BashTool;
        assert!(!tool.is_read_only(&json!({ "command": "git status > out.txt" })));
        assert!(!tool.is_read_only(&json!({ "command": "git status && git diff" })));
        assert!(!tool.is_read_only(&json!({ "command": "echo $(git status)" })));
    }
}
