//! PowerShell tool -- execute PowerShell commands.
//!
//! Similar to BashTool but invokes PowerShell instead of sh/bash.
//! On Windows, uses `powershell.exe -NoProfile -NonInteractive -Command`.
//! On non-Windows, uses `pwsh -NoProfile -NonInteractive -Command`.

use anyhow::Result;
use async_trait::async_trait;
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};

use allthecodes_engine::types::tool::{
    InterruptBehavior, PermissionResult, Tool, ToolProgress, ToolResult, ToolUseContext,
    ValidationResult,
};
use allthecodes_permissions::read_only_shell::is_read_only_powershell_command;
use allthecodes_sandbox::{make_runner, policy_from_app_state, preflight_shell_command};
use allthecodes_shell_command::ReadOnlyResult;
use allthecodes_tools::exec::powershell as powershell_spec;
use allthecodes_types::{message::AssistantMessage, ShellExecutionOutput};
use allthecodes_utils::bash::resolve_timeout;
use allthecodes_utils::git_operation_tracking::track_git_operations_json;

use allthecodes_shell_command::provider::{PowerShellProvider, ShellProvider};

use super::bash::truncate_output;
use super::powershell_parser;
use super::process_control::{
    configure_process_group, wait_for_exit_or_termination, ControlledExit,
};

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

/// PowerShellTool -- execute PowerShell commands.
pub struct PowerShellTool;

impl PowerShellTool {
    fn parse_input(input: &Value) -> (String, u64) {
        let command = input
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let timeout_ms = input
            .get("timeout")
            .and_then(|v| v.as_u64())
            .unwrap_or(120_000);
        (command, timeout_ms)
    }
}

#[async_trait]
impl Tool for PowerShellTool {
    fn name(&self) -> &str {
        powershell_spec::NAME
    }

    async fn description(&self, _input: &Value) -> String {
        powershell_spec::description().to_string()
    }

    fn input_json_schema(&self) -> Value {
        powershell_spec::input_schema()
    }

    fn is_enabled(&self) -> bool {
        PowerShellProvider::is_available()
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        false
    }

    fn is_read_only(&self, input: &Value) -> bool {
        let command = input.get("command").and_then(|v| v.as_str()).unwrap_or("");
        matches!(
            is_read_only_powershell_command(command),
            ReadOnlyResult::ReadOnly
        )
    }

    fn is_destructive(&self, _input: &Value) -> bool {
        true
    }

    fn interrupt_behavior(&self) -> InterruptBehavior {
        InterruptBehavior::Cancel
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        let command = input.get("command").and_then(|v| v.as_str()).unwrap_or("");
        if command.is_empty() {
            return ValidationResult::Error {
                message: "Command must not be empty".to_string(),
                error_code: 1,
            };
        }
        match powershell_parser::analyze(command).await {
            Ok(analysis) if !analysis.parser_errors.is_empty() => {
                return ValidationResult::Error {
                    message: format!(
                        "PowerShell parser rejected command: {}",
                        analysis.parser_errors.join("; ")
                    ),
                    error_code: 1,
                };
            }
            Ok(analysis) if !analysis.security_diagnostics.is_empty() => {
                return ValidationResult::Error {
                    message: format!(
                        "PowerShell security metadata rejected command: {}",
                        analysis.security_diagnostics.join("; ")
                    ),
                    error_code: 1,
                };
            }
            Ok(_) => {}
            Err(err) => {
                return ValidationResult::Error {
                    message: format!("PowerShell parser unavailable: {}", err),
                    error_code: 1,
                };
            }
        }
        ValidationResult::Ok
    }

    async fn check_permissions(&self, input: &Value, ctx: &ToolUseContext) -> PermissionResult {
        // Sandbox escape-hatch gate: same rule as BashTool.
        let wants_escape = input
            .get("dangerouslyDisableSandbox")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if wants_escape {
            let app_state = (ctx.get_app_state)();
            if !app_state
                .settings
                .sandbox
                .allow_unsandboxed_commands
                .unwrap_or(true)
            {
                return PermissionResult::Deny {
                    message: "sandbox.allowUnsandboxedCommands=false rejects \
                              dangerouslyDisableSandbox"
                        .to_string(),
                };
            }
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
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let (raw_command, timeout_ms) = Self::parse_input(&input);

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

        let provider = PowerShellProvider::new();
        let command = provider.normalize_command(&raw_command);
        let mut cmd = tokio::process::Command::new(provider.shell_path());
        for arg in provider.exec_args() {
            cmd.arg(arg);
        }
        cmd.arg(&command);

        // Inject shell environment (TERM, LANG, GIT_PAGER=cat, CLAUDE_CODE=1, etc.)
        for (k, v) in provider.build_env() {
            cmd.env(&k, &v);
        }

        // Sandbox integration — same flow as BashTool.
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

        if let Err(err) = preflight_shell_command(&policy, &raw_command) {
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

        let is_excluded = policy.is_excluded_command(&raw_command);
        if is_excluded && !policy.allow_unsandboxed_commands {
            return Ok(ToolResult {
                data: json!({
                    "error": allthecodes_sandbox::SandboxError::EscapeHatchDisabled {
                        command: raw_command.clone()
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
                            command: raw_command.clone(),
                        }
                        .to_string(),
                    ),
                )),
                ..Default::default()
            });
        }

        if !escape && !is_excluded {
            if let Some(runner) = make_runner(&policy) {
                match runner.prepare(cmd, &policy, &cwd) {
                    Ok(prepared) => {
                        cmd = prepared.cmd;
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
            } else {
                cmd.stdout(std::process::Stdio::piped());
                cmd.stderr(std::process::Stdio::piped());
            }
        } else {
            cmd.stdout(std::process::Stdio::piped());
            cmd.stderr(std::process::Stdio::piped());
        }

        configure_process_group(&mut cmd);
        cmd.kill_on_drop(true);

        let timeout_duration = resolve_timeout(Some(timeout_ms));
        let mut child = match cmd.spawn() {
            Ok(child) => child,
            Err(e) => {
                return Ok(ToolResult {
                    data: json!({ "error": format!("Failed to execute PowerShell command: {}", e) }),
                    new_messages: vec![],
                    shell: Some(shell_execution_output(
                        &raw_command,
                        &cwd,
                        String::new(),
                        String::new(),
                        None,
                        false,
                        None,
                        Some(format!("Failed to execute PowerShell command: {}", e)),
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

        let exit_status =
            wait_for_exit_or_termination(&mut child, timeout_duration, ctx.abort_signal.clone())
                .await;

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
                let max_chars = self.max_result_size_chars();
                combined = truncate_output(&combined, max_chars);

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
                let error = format!(
                    "PowerShell command timed out after {}ms",
                    timeout_duration.as_millis()
                );
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
                            "PowerShell command timed out after {}ms",
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
                        "error": "PowerShell command interrupted",
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
                        Some("PowerShell command interrupted".to_string()),
                    )),
                    ..Default::default()
                })
            }
            ControlledExit::Exited(Err(e)) => Ok(ToolResult {
                data: json!({ "error": format!("Failed to execute PowerShell command: {}", e) }),
                new_messages: vec![],
                shell: Some(shell_execution_output(
                    &raw_command,
                    &cwd,
                    stdout,
                    stderr,
                    None,
                    false,
                    None,
                    Some(format!("Failed to execute PowerShell command: {}", e)),
                )),
                ..Default::default()
            }),
        }
    }

    async fn prompt(&self) -> String {
        powershell_spec::prompt()
    }

    fn user_facing_name(&self, _input: Option<&Value>) -> String {
        powershell_spec::NAME.to_string()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_engine::types::app_state::AppState;
    use allthecodes_engine::types::tool::{FileStateCache, ToolUseOptions};
    #[cfg(windows)]
    use allthecodes_types::message::ContentBlock;
    #[cfg(windows)]
    use uuid::Uuid;

    fn test_context() -> (ToolUseContext, tokio::sync::watch::Sender<bool>) {
        let app_state = AppState::default();
        let (tx, rx) = tokio::sync::watch::channel(false);

        (
            ToolUseContext {
                options: ToolUseOptions {
                    debug: false,
                    main_loop_model: "test".to_string(),
                    verbose: false,
                    is_non_interactive_session: false,
                    custom_system_prompt: None,
                    append_system_prompt: None,
                    max_budget_usd: None,
                },
                abort_signal: rx,
                read_file_state: FileStateCache::default(),
                get_app_state: Arc::new(move || app_state.to_tool_app_state()),
                set_app_state: Arc::new(|_| {}),
                session_id: "powershell-test-session".to_string(),
                langfuse_session_id: "powershell-test-session".to_string(),
                messages: vec![],
                agent_id: None,
                agent_type: None,
                query_tracking: None,
                permission_callback: None,
                ask_user_callback: None,
                permission_event_callback: None,
                bg_agent_tx: None,
                hook_runner: Arc::new(allthecodes_types::hooks::NoopHookRunner::new()),
                command_dispatcher: Arc::new(
                    allthecodes_types::commands::NoopCommandDispatcher::new(),
                ),
                available_tools: vec![],
                execute_deferred_tool: None,
            },
            tx,
        )
    }

    #[cfg(windows)]
    fn parent_message() -> AssistantMessage {
        AssistantMessage {
            uuid: Uuid::new_v4(),
            timestamp: 0,
            role: "assistant".to_string(),
            content: Vec::<ContentBlock>::new(),
            usage: None,
            stop_reason: None,
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        }
    }

    #[test]
    fn test_powershell_tool_name() {
        let tool = PowerShellTool;
        assert_eq!(tool.name(), "PowerShell");
    }

    #[test]
    fn test_powershell_schema() {
        let tool = PowerShellTool;
        let schema = tool.input_json_schema();
        let props = schema["properties"].as_object().unwrap();
        assert!(props.contains_key("command"));
        assert!(props.contains_key("timeout"));

        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("command")));
    }

    #[test]
    fn test_powershell_parse_input_defaults() {
        let input = json!({ "command": "Get-Date" });
        let (cmd, timeout) = PowerShellTool::parse_input(&input);
        assert_eq!(cmd, "Get-Date");
        assert_eq!(timeout, 120_000);
    }

    #[test]
    fn test_powershell_parse_input_custom_timeout() {
        let input = json!({ "command": "dir", "timeout": 5000 });
        let (cmd, timeout) = PowerShellTool::parse_input(&input);
        assert_eq!(cmd, "dir");
        assert_eq!(timeout, 5000);
    }

    #[test]
    fn test_powershell_is_destructive() {
        let tool = PowerShellTool;
        assert!(tool.is_destructive(&json!({})));
    }

    #[test]
    fn test_powershell_read_only_safe_external_command() {
        let tool = PowerShellTool;
        assert!(tool.is_read_only(&json!({ "command": "git status" })));
    }

    #[test]
    fn test_powershell_read_only_blocks_pipeline() {
        let tool = PowerShellTool;
        assert!(!tool.is_read_only(&json!({ "command": "git status | Select-Object -First 1" })));
    }

    #[test]
    fn test_powershell_executable_name() {
        let provider = PowerShellProvider::new();
        let exe = provider.shell_path();
        if cfg!(target_os = "windows") {
            assert_eq!(exe, "powershell.exe");
        } else {
            assert_eq!(exe, "pwsh");
        }
    }

    #[tokio::test]
    async fn test_powershell_native_parser_accepts_valid_command() {
        let tool = PowerShellTool;
        if !tool.is_enabled() {
            return;
        }
        let (ctx, _tx) = test_context();
        let result = tool
            .validate_input(&json!({ "command": "Write-Output 'ok'" }), &ctx)
            .await;
        assert!(matches!(result, ValidationResult::Ok));
    }

    #[tokio::test]
    async fn test_powershell_native_parser_rejects_invalid_command() {
        let tool = PowerShellTool;
        if !tool.is_enabled() {
            return;
        }
        let (ctx, _tx) = test_context();
        let result = tool
            .validate_input(&json!({ "command": "if ($true) { Write-Output ok" }), &ctx)
            .await;
        match result {
            ValidationResult::Error { message, .. } => {
                assert!(message.contains("PowerShell parser rejected command"));
            }
            ValidationResult::Ok => panic!("invalid PowerShell should fail parser validation"),
        }
    }

    #[tokio::test]
    async fn test_powershell_native_parser_accepts_safe_filter_script_block() {
        let tool = PowerShellTool;
        if !tool.is_enabled() {
            return;
        }
        let (ctx, _tx) = test_context();
        let result = tool
            .validate_input(
                &json!({ "command": "Get-Process | Where-Object { $_.ProcessName -like 'pwsh' }" }),
                &ctx,
            )
            .await;
        assert!(matches!(result, ValidationResult::Ok));
    }

    #[tokio::test]
    async fn test_powershell_native_parser_rejects_dynamic_command_name() {
        let tool = PowerShellTool;
        if !tool.is_enabled() {
            return;
        }
        let (ctx, _tx) = test_context();
        let result = tool
            .validate_input(&json!({ "command": "& $env:COMSPEC" }), &ctx)
            .await;
        match result {
            ValidationResult::Error { message, .. } => {
                assert!(message.contains("PowerShell security metadata rejected command"));
                assert!(message.contains("dynamic expression"));
            }
            ValidationResult::Ok => panic!("dynamic PowerShell command name should fail"),
        }
    }

    #[tokio::test]
    async fn test_powershell_native_parser_rejects_colon_bound_expression() {
        let tool = PowerShellTool;
        if !tool.is_enabled() {
            return;
        }
        let (ctx, _tx) = test_context();
        let result = tool
            .validate_input(
                &json!({ "command": "Get-Process -Name:($env:PROCESSOR_ARCHITECTURE)" }),
                &ctx,
            )
            .await;
        match result {
            ValidationResult::Error { message, .. } => {
                assert!(message.contains("PowerShell security metadata rejected command"));
                assert!(message.contains("colon-bound parameter"));
            }
            ValidationResult::Ok => panic!("colon-bound expression should fail"),
        }
    }

    #[tokio::test]
    async fn test_powershell_native_parser_rejects_path_like_command_name() {
        let tool = PowerShellTool;
        if !tool.is_enabled() {
            return;
        }
        let (ctx, _tx) = test_context();
        let result = tool
            .validate_input(&json!({ "command": r"scripts\Get-Process" }), &ctx)
            .await;
        match result {
            ValidationResult::Error { message, .. } => {
                assert!(message.contains("PowerShell security metadata rejected command"));
                assert!(message.contains("path-like command names"));
            }
            ValidationResult::Ok => panic!("path-like PowerShell command name should fail"),
        }
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn test_powershell_timeout_terminates_process() {
        let tool = PowerShellTool;
        let (ctx, _tx) = test_context();
        let result = tool
            .call(
                json!({
                    "command": "Start-Sleep -Seconds 30",
                    "timeout": 100
                }),
                &ctx,
                &parent_message(),
                None,
            )
            .await
            .expect("tool call should return");

        assert_eq!(result.data["termination"], json!("timeout"));
        assert_eq!(result.data["interrupted"], json!(true));
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn test_powershell_abort_signal_terminates_process() {
        let tool = PowerShellTool;
        let (ctx, tx) = test_context();
        let parent = parent_message();
        let task = tokio::spawn(async move {
            tool.call(
                json!({
                    "command": "Start-Sleep -Seconds 30",
                    "timeout": 30_000
                }),
                &ctx,
                &parent,
                None,
            )
            .await
        });

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        tx.send(true).expect("send abort");

        let result = tokio::time::timeout(std::time::Duration::from_secs(5), task)
            .await
            .expect("cancelled command should finish")
            .expect("task join")
            .expect("tool call");

        assert_eq!(result.data["termination"], json!("cancelled"));
        assert_eq!(result.data["interrupted"], json!(true));
    }
}
