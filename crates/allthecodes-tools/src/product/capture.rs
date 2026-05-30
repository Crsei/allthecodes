//! Process monitoring and terminal capture product tools.

use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use allthecodes_types::message::{AssistantMessage, ToolResultContent};
use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::tool::{
    PermissionResult, Tool, ToolProgress, ToolResult, ToolUseContext, ValidationResult,
};

use super::common::{find_tool_result_text, shell_tool_use_ids, tail_lines};

pub(super) struct MonitorTool;
pub(super) struct TerminalCaptureTool;

#[async_trait]
impl Tool for MonitorTool {
    fn name(&self) -> &str {
        "Monitor"
    }

    async fn description(&self, _input: &Value) -> String {
        "Run a command under periodic monitoring and return status updates.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": { "type": "string" },
                "duration_seconds": { "type": "integer", "minimum": 1, "maximum": 3600, "default": 60 },
                "interval_ms": { "type": "integer", "minimum": 250, "maximum": 60000, "default": 1000 }
            },
            "required": ["command"]
        })
    }

    fn is_destructive(&self, _input: &Value) -> bool {
        true
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        match parse_process_capture_input(input, 60, 1000) {
            Ok(_) => ValidationResult::Ok,
            Err(err) => ValidationResult::Error {
                message: err.to_string(),
                error_code: 400,
            },
        }
    }

    async fn check_permissions(&self, input: &Value, _ctx: &ToolUseContext) -> PermissionResult {
        PermissionResult::Ask {
            message: format!(
                "Run monitored command '{}'?",
                input
                    .get("command")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .chars()
                    .take(160)
                    .collect::<String>()
            ),
        }
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let spec = parse_process_capture_input(&input, 60, 1000)?;
        let captured = run_captured_process(&spec, false, ctx, on_progress).await?;
        let preview = format!(
            "Monitor {} after {}s ({} line(s))",
            captured.status,
            captured.elapsed_seconds,
            captured.output.lines().count()
        );
        let output = captured.output;
        Ok(ToolResult {
            data: json!({
                "command": spec.command,
                "status": captured.status,
                "exit_code": captured.exit_code,
                "elapsed_seconds": captured.elapsed_seconds,
                "output": output.clone(),
            }),
            model_content: Some(ToolResultContent::Text(output)),
            display_preview: Some(preview),
            new_messages: vec![],
        })
    }

    async fn prompt(&self) -> String {
        "Run a command with periodic progress updates when the user asks to monitor a process or log-producing command.".to_string()
    }
}

#[async_trait]
impl Tool for TerminalCaptureTool {
    fn name(&self) -> &str {
        "TerminalCapture"
    }

    async fn description(&self, _input: &Value) -> String {
        "Capture recent shell tool output from the conversation.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "lines": { "type": "integer", "minimum": 1, "maximum": 1000 },
                "tool_use_id": { "type": "string" },
                "panel_id": { "type": "string" },
                "command": { "type": "string", "description": "Optional command to run through a PTY-compatible capture path." },
                "duration_seconds": { "type": "integer", "minimum": 1, "maximum": 3600, "default": 30 },
                "interval_ms": { "type": "integer", "minimum": 250, "maximum": 60000, "default": 1000 }
            }
        })
    }

    fn is_concurrency_safe(&self, input: &Value) -> bool {
        input.get("command").is_none()
    }

    fn is_read_only(&self, input: &Value) -> bool {
        input.get("command").is_none()
    }

    fn is_destructive(&self, input: &Value) -> bool {
        input.get("command").is_some()
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        let lines = input.get("lines").and_then(Value::as_u64).unwrap_or(50);
        if lines == 0 || lines > 1000 {
            return ValidationResult::Error {
                message: "'lines' must be between 1 and 1000".to_string(),
                error_code: 400,
            };
        }
        if input.get("command").is_some() {
            return match parse_process_capture_input(input, 30, 1000) {
                Ok(_) => ValidationResult::Ok,
                Err(err) => ValidationResult::Error {
                    message: err.to_string(),
                    error_code: 400,
                },
            };
        }
        ValidationResult::Ok
    }

    async fn check_permissions(&self, input: &Value, _ctx: &ToolUseContext) -> PermissionResult {
        if let Some(command) = input.get("command").and_then(Value::as_str) {
            return PermissionResult::Ask {
                message: format!(
                    "Run PTY capture command '{}'?",
                    command.chars().take(160).collect::<String>()
                ),
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
        _parent: &AssistantMessage,
        on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        if input.get("command").is_some() {
            let spec = parse_process_capture_input(&input, 30, 1000)?;
            let captured = run_captured_process(&spec, true, ctx, on_progress).await?;
            let content = tail_lines(
                &captured.output,
                input.get("lines").and_then(Value::as_u64).unwrap_or(50) as usize,
            );
            return Ok(ToolResult {
                data: json!({
                    "content": content,
                    "line_count": content.lines().count(),
                    "source": "pty-command",
                    "command": spec.command,
                    "status": captured.status,
                    "exit_code": captured.exit_code,
                }),
                model_content: Some(ToolResultContent::Text(content.clone())),
                display_preview: Some(format!("Captured {} PTY line(s)", content.lines().count())),
                new_messages: vec![],
            });
        }

        let lines = input.get("lines").and_then(Value::as_u64).unwrap_or(50) as usize;
        let requested_id = input.get("tool_use_id").and_then(Value::as_str);
        let shell_ids = shell_tool_use_ids(&ctx.messages);
        let captured = find_tool_result_text(&ctx.messages, requested_id, &shell_ids);
        let content = captured
            .as_ref()
            .map(|captured| tail_lines(&captured.content, lines))
            .unwrap_or_default();
        let line_count = content.lines().count();
        Ok(ToolResult {
            data: json!({
                "content": content,
                "line_count": line_count,
                "tool_use_id": captured.as_ref().map(|captured| captured.tool_use_id.as_str()),
                "source": captured
                    .as_ref()
                    .map(|captured| captured.source_tool.as_str())
                    .unwrap_or("terminal-panel-runtime"),
                "panel_id": input.get("panel_id").and_then(Value::as_str),
            }),
            display_preview: Some(format!("Captured {line_count} terminal line(s)")),
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "Capture recent shell output already present in this session. Use this to re-read command output without rerunning the command.".to_string()
    }
}

#[derive(Debug, Clone)]
pub(super) struct ProcessCaptureSpec {
    command: String,
    duration: Duration,
    interval: Duration,
}

#[derive(Debug)]
pub(super) struct ProcessCaptureResult {
    status: String,
    exit_code: Option<i32>,
    elapsed_seconds: u64,
    output: String,
}

fn parse_process_capture_input(
    input: &Value,
    default_duration_seconds: u64,
    default_interval_ms: u64,
) -> Result<ProcessCaptureSpec> {
    let command = input
        .get("command")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("command is required"))?
        .to_string();
    let duration_seconds = input
        .get("duration_seconds")
        .and_then(Value::as_u64)
        .unwrap_or(default_duration_seconds);
    if !(1..=3600).contains(&duration_seconds) {
        bail!("duration_seconds must be between 1 and 3600");
    }
    let interval_ms = input
        .get("interval_ms")
        .and_then(Value::as_u64)
        .unwrap_or(default_interval_ms);
    if !(250..=60_000).contains(&interval_ms) {
        bail!("interval_ms must be between 250 and 60000");
    }
    Ok(ProcessCaptureSpec {
        command,
        duration: Duration::from_secs(duration_seconds),
        interval: Duration::from_millis(interval_ms),
    })
}

async fn run_captured_process(
    spec: &ProcessCaptureSpec,
    pty_mode: bool,
    ctx: &ToolUseContext,
    on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
) -> Result<ProcessCaptureResult> {
    let command_line = if pty_mode {
        pty_capture_command(&spec.command)
    } else {
        spec.command.clone()
    };
    let mut command = shell_command(&command_line);
    command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    let mut child = command
        .spawn()
        .with_context(|| format!("failed to spawn '{}'", spec.command))?;
    let output = Arc::new(tokio::sync::Mutex::new(String::new()));
    let stdout_task = child
        .stdout
        .take()
        .map(|stdout| spawn_output_reader(stdout, output.clone()));
    let stderr_task = child
        .stderr
        .take()
        .map(|stderr| spawn_output_reader(stderr, output.clone()));
    let started = std::time::Instant::now();
    let progress: Option<Arc<dyn Fn(ToolProgress) + Send + Sync>> = on_progress.map(Arc::from);
    let progress_task = progress
        .as_ref()
        .map(|cb: &Arc<dyn Fn(ToolProgress) + Send + Sync>| {
            let cb = cb.clone();
            let output = output.clone();
            let interval = spec.interval;
            let command = spec.command.clone();
            tokio::spawn(async move {
                let mut ticker = tokio::time::interval(interval);
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                ticker.tick().await;
                loop {
                    ticker.tick().await;
                    let snapshot = output.lock().await.clone();
                    cb(ToolProgress {
                        tool_use_id: String::new(),
                        data: json!({
                            "tool": if pty_mode { "TerminalCapture" } else { "Monitor" },
                            "command": command,
                            "output": snapshot,
                            "elapsed_seconds": started.elapsed().as_secs(),
                        }),
                    });
                }
            })
        });

    let mut abort_signal = ctx.abort_signal.clone();
    let sleep = tokio::time::sleep(spec.duration);
    tokio::pin!(sleep);
    let (status, exit_code) = tokio::select! {
        result = child.wait() => {
            let status = result?;
            ("exited".to_string(), status.code())
        }
        _ = &mut sleep => {
            let _ = child.kill().await;
            ("timed_out".to_string(), None)
        }
        changed = abort_signal.changed() => {
            let _ = child.kill().await;
            match changed {
                Ok(()) if *abort_signal.borrow() => ("cancelled".to_string(), None),
                _ => ("interrupted".to_string(), None),
            }
        }
    };

    if let Some(handle) = progress_task {
        handle.abort();
    }
    if let Some(handle) = stdout_task {
        let _ = handle.await;
    }
    if let Some(handle) = stderr_task {
        let _ = handle.await;
    }

    let output = output.lock().await.clone();
    if let Some(cb) = progress {
        cb(ToolProgress {
            tool_use_id: String::new(),
            data: json!({
                "tool": if pty_mode { "TerminalCapture" } else { "Monitor" },
                "command": spec.command.clone(),
                "output": output.clone(),
                "elapsed_seconds": started.elapsed().as_secs(),
                "status": status,
                "exit_code": exit_code,
            }),
        });
    }
    Ok(ProcessCaptureResult {
        status,
        exit_code,
        elapsed_seconds: started.elapsed().as_secs(),
        output,
    })
}

fn spawn_output_reader<R>(
    reader: R,
    output: Arc<tokio::sync::Mutex<String>>,
) -> tokio::task::JoinHandle<()>
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        loop {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    let mut output = output.lock().await;
                    append_limited(&mut output, &line, 64 * 1024);
                    append_limited(&mut output, "\n", 64 * 1024);
                }
                Ok(None) | Err(_) => break,
            }
        }
    })
}

fn append_limited(output: &mut String, chunk: &str, max_chars: usize) {
    output.push_str(chunk);
    let count = output.chars().count();
    if count > max_chars {
        *output = output.chars().skip(count - max_chars).collect();
    }
}

fn shell_command(command: &str) -> Command {
    #[cfg(windows)]
    {
        let mut cmd = Command::new("cmd");
        cmd.arg("/C").arg(command);
        cmd
    }
    #[cfg(not(windows))]
    {
        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-c").arg(command);
        cmd
    }
}

fn pty_capture_command(command: &str) -> String {
    #[cfg(not(windows))]
    {
        format!("script -q -e -c {} /dev/null", shell_quote(command))
    }
    #[cfg(windows)]
    {
        command.to_string()
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_process_capture_input_validates_bounds() {
        let spec = parse_process_capture_input(
            &json!({
                "command": "printf ok",
                "duration_seconds": 2,
                "interval_ms": 250
            }),
            60,
            1000,
        )
        .unwrap();

        assert_eq!(spec.command, "printf ok");
        assert_eq!(spec.duration, Duration::from_secs(2));
        assert!(parse_process_capture_input(&json!({"command": ""}), 60, 1000).is_err());
        assert!(
            parse_process_capture_input(&json!({"command": "x", "interval_ms": 10}), 60, 1000)
                .is_err()
        );
    }

    #[test]
    fn pty_command_quotes_shell_input() {
        let quoted = shell_quote("printf 'hello'");
        assert!(quoted.starts_with('\''));
        assert!(quoted.contains("\\''"));
    }
}
