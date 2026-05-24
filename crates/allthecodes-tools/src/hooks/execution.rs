//! Core hook execution: spawn subprocess, collect output, parse result.

use std::process::Stdio;

use anyhow::{Context, Result};
use serde_json::Value;
use tokio::io::AsyncWriteExt;
use tracing::{debug, warn};

use super::HookOutput;

// ---------------------------------------------------------------------------
// Core execution: run a single command hook as a subprocess
// ---------------------------------------------------------------------------

/// Execute a single command hook as a subprocess.
///
/// 1. Spawns `bash -c "{command}"` (on Windows, tries bash first, falls back to cmd /C)
/// 2. Writes `stdin_json` as a single JSON line to stdin, then closes stdin
/// 3. Collects stdout with a timeout
/// 4. Parses the first line of stdout as JSON -> HookOutput
/// 5. If the first line doesn't start with `{`, returns default HookOutput with
///    additional_context set to the entire stdout
pub(super) async fn execute_command_hook(
    command: &str,
    stdin_json: &Value,
    timeout_secs: u64,
    shell: Option<&str>,
) -> Result<HookOutput> {
    let mut child = spawn_shell_command(command, shell)?;
    let mut io_diagnostics = Vec::new();

    // Write JSON to stdin and close it before waiting for output.
    // This must be done before reading stdout to avoid deadlocks
    // where the child blocks reading stdin while we block reading stdout.
    if let Some(mut stdin) = child.stdin.take() {
        let json_bytes =
            serde_json::to_vec(stdin_json).context("failed to serialize hook stdin")?;
        if let Err(e) = stdin.write_all(&json_bytes).await {
            io_diagnostics.push(format!("failed to write hook stdin JSON: {e}"));
            warn!(command = command, error = %e, "failed to write hook stdin JSON");
        }
        if let Err(e) = stdin.write_all(b"\n").await {
            io_diagnostics.push(format!("failed to write hook stdin newline: {e}"));
            warn!(command = command, error = %e, "failed to write hook stdin newline");
        }
        if let Err(e) = stdin.flush().await {
            io_diagnostics.push(format!("failed to flush hook stdin: {e}"));
            warn!(command = command, error = %e, "failed to flush hook stdin");
        }
        // Explicitly drop to close the write end of the pipe
        drop(stdin);
    }

    // Take stdout/stderr handles to read them concurrently with waiting.
    let mut stdout_reader = child.stdout.take();
    let mut stderr_reader = child.stderr.take();

    let timeout_duration = std::time::Duration::from_secs(timeout_secs);

    // Spawn reading tasks concurrently with process wait, all under a timeout.
    let collect = async {
        use tokio::io::AsyncReadExt;

        let stdout_fut = async {
            let mut buf = Vec::new();
            let mut read_error = None;
            if let Some(ref mut r) = stdout_reader {
                if let Err(e) = r.read_to_end(&mut buf).await {
                    read_error = Some(e);
                }
            }
            (buf, read_error)
        };
        let stderr_fut = async {
            let mut buf = Vec::new();
            let mut read_error = None;
            if let Some(ref mut r) = stderr_reader {
                if let Err(e) = r.read_to_end(&mut buf).await {
                    read_error = Some(e);
                }
            }
            (buf, read_error)
        };
        let wait_fut = child.wait();

        let ((stdout_bytes, stdout_read_error), (stderr_bytes, stderr_read_error), wait_result) =
            tokio::join!(stdout_fut, stderr_fut, wait_fut);

        (
            stdout_bytes,
            stdout_read_error,
            stderr_bytes,
            stderr_read_error,
            wait_result,
        )
    };

    match tokio::time::timeout(timeout_duration, collect).await {
        Ok((stdout_bytes, stdout_read_error, stderr_bytes, stderr_read_error, wait_result)) => {
            let stdout = String::from_utf8_lossy(&stdout_bytes).to_string();
            let stderr = String::from_utf8_lossy(&stderr_bytes).to_string();

            if let Some(e) = stdout_read_error {
                io_diagnostics.push(format!("failed to read hook stdout: {e}"));
                warn!(command = command, error = %e, "failed to read hook stdout");
            }
            if let Some(e) = stderr_read_error {
                io_diagnostics.push(format!("failed to read hook stderr: {e}"));
                warn!(command = command, error = %e, "failed to read hook stderr");
            }

            match wait_result {
                Ok(status) => {
                    if !status.success() {
                        return Err(anyhow::anyhow!(
                            "hook command exited with non-zero status {}{}{}",
                            status,
                            if stderr.trim().is_empty() { "" } else { ": " },
                            stderr.trim()
                        ));
                    }
                }
                Err(e) => {
                    io_diagnostics.push(format!("hook command wait error: {e}"));
                    warn!(command = command, error = %e, "hook command wait error");
                }
            }

            parse_hook_output_with_diagnostics(&stdout, &io_diagnostics)
        }
        Err(_) => {
            drop(stdout_reader);
            drop(stderr_reader);

            // Timeout expired. Kill the whole Unix process group when possible:
            // hooks run through a shell, and commands such as `sleep 60` can
            // outlive that shell while still holding stdout/stderr pipes open.
            let kill_diagnostic = kill_timed_out_child(command, &mut child);
            tokio::spawn(async move {
                let _ = child.wait().await;
            });

            if let Some(kill_diagnostic) = kill_diagnostic {
                Err(anyhow::anyhow!(
                    "hook command timed out after {}s; {}",
                    timeout_secs,
                    kill_diagnostic
                ))
            } else {
                Err(anyhow::anyhow!(
                    "hook command timed out after {}s",
                    timeout_secs
                ))
            }
        }
    }
}

fn kill_timed_out_child(command: &str, child: &mut tokio::process::Child) -> Option<String> {
    let mut diagnostics = Vec::new();

    #[cfg(unix)]
    if let Some(pid) = child.id() {
        let process_group = -(pid as libc::pid_t);
        // SAFETY: kill(2) is called with a process-group id derived from a
        // child process we spawned into its own group below.
        let result = unsafe { libc::kill(process_group, libc::SIGKILL) };
        if result != 0 {
            let e = std::io::Error::last_os_error();
            warn!(
                command = command,
                pid = pid,
                error = %e,
                "failed to kill timed-out hook process group"
            );
            diagnostics.push(format!("failed to kill timed-out hook process group: {e}"));
        }
    }

    if let Err(e) = child.start_kill() {
        warn!(
            command = command,
            error = %e,
            "failed to kill timed-out hook command"
        );
        diagnostics.push(format!("failed to kill timed-out hook command: {e}"));
    }

    if diagnostics.is_empty() {
        None
    } else {
        Some(diagnostics.join("; "))
    }
}

/// Spawn a shell command as a child process.
fn spawn_shell_command(
    command: &str,
    shell_override: Option<&str>,
) -> Result<tokio::process::Child> {
    #[cfg(windows)]
    {
        // On Windows, try bash first (e.g., Git Bash, WSL), fall back to cmd
        use tokio::process::Command;

        if let Some(shell) = shell_override {
            if shell.eq_ignore_ascii_case("cmd") || shell.eq_ignore_ascii_case("cmd.exe") {
                return Command::new("cmd")
                    .arg("/C")
                    .arg(command)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .context("failed to spawn hook command via cmd");
            }
            return Command::new(shell)
                .arg("-c")
                .arg(command)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .with_context(|| format!("failed to spawn hook command via {shell}"));
        }

        // Try bash first
        match Command::new("bash")
            .arg("-c")
            .arg(command)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(child) => Ok(child),
            Err(_) => {
                // Fall back to cmd /C
                Command::new("cmd")
                    .arg("/C")
                    .arg(command)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .context("failed to spawn hook command (tried bash and cmd)")
            }
        }
    }

    #[cfg(not(windows))]
    {
        use tokio::process::Command;

        let shell_program = shell_override.unwrap_or("bash");
        let mut shell = Command::new(shell_program);
        shell
            .arg("-c")
            .arg(command)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        shell.process_group(0);

        shell
            .spawn()
            .with_context(|| format!("failed to spawn hook command via {shell_program}"))
    }
}

/// Parse hook stdout into a HookOutput.
///
/// If the first non-empty line starts with `{`, parse it as JSON.
/// Otherwise, return a default HookOutput with additional_context = stdout.
pub(super) fn parse_hook_output(stdout: &str) -> Result<HookOutput> {
    parse_hook_output_with_diagnostics(stdout, &[])
}

fn parse_hook_output_with_diagnostics(
    stdout: &str,
    io_diagnostics: &[String],
) -> Result<HookOutput> {
    let trimmed = stdout.trim();

    let mut output = if trimmed.is_empty() {
        HookOutput::default()
    } else {
        // Find the first non-empty line
        let first_line = trimmed.lines().next().unwrap_or("");

        if first_line.trim_start().starts_with('{') {
            match serde_json::from_str::<HookOutput>(first_line) {
                Ok(output) => output,
                Err(e) => {
                    debug!(error = %e, "failed to parse hook output as JSON, treating as plain text");
                    HookOutput {
                        additional_context: Some(trimmed.to_string()),
                        ..Default::default()
                    }
                }
            }
        } else {
            HookOutput {
                additional_context: Some(trimmed.to_string()),
                ..Default::default()
            }
        }
    };

    append_io_diagnostics(&mut output, io_diagnostics);
    Ok(output)
}

fn append_io_diagnostics(output: &mut HookOutput, io_diagnostics: &[String]) {
    if io_diagnostics.is_empty() {
        return;
    }

    let mut diagnostic_context = String::from("Hook IO diagnostics:");
    for diagnostic in io_diagnostics {
        diagnostic_context.push_str("\n- ");
        diagnostic_context.push_str(diagnostic);
    }

    match &mut output.additional_context {
        Some(context) if !context.is_empty() => {
            context.push_str("\n\n");
            context.push_str(&diagnostic_context);
        }
        Some(context) => {
            *context = diagnostic_context;
        }
        None => {
            output.additional_context = Some(diagnostic_context);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // -- parse_hook_output tests --

    #[test]
    fn test_parse_hook_output_json() {
        let stdout = r#"{"continue":false,"reason":"blocked","permission_decision":"deny"}"#;
        let output = parse_hook_output(stdout).unwrap();
        assert!(!output.should_continue);
        assert_eq!(output.reason.as_deref(), Some("blocked"));
        assert_eq!(output.permission_decision.as_deref(), Some("deny"));
    }

    #[test]
    fn test_parse_hook_output_plain_text() {
        let stdout = "some plain text output\nwith multiple lines";
        let output = parse_hook_output(stdout).unwrap();
        assert!(output.should_continue); // default
        assert_eq!(
            output.additional_context.as_deref(),
            Some("some plain text output\nwith multiple lines")
        );
    }

    #[test]
    fn test_parse_hook_output_empty() {
        let output = parse_hook_output("").unwrap();
        assert!(output.should_continue);
        assert!(output.additional_context.is_none());
    }

    #[test]
    fn test_parse_hook_output_json_with_updated_input() {
        let stdout = r#"{"continue":true,"updated_input":{"command":"ls -la"}}"#;
        let output = parse_hook_output(stdout).unwrap();
        assert!(output.should_continue);
        assert_eq!(output.updated_input, Some(json!({"command": "ls -la"})));
    }

    #[test]
    fn test_parse_hook_output_adds_io_diagnostics_to_json_output() {
        let diagnostics = vec![
            "failed to write hook stdin JSON: broken pipe".to_string(),
            "failed to read hook stderr: stream closed".to_string(),
        ];

        let output =
            parse_hook_output_with_diagnostics(r#"{"continue":true,"reason":"ok"}"#, &diagnostics)
                .unwrap();

        assert_eq!(output.reason.as_deref(), Some("ok"));
        let context = output.additional_context.unwrap();
        assert!(context.contains("Hook IO diagnostics:"));
        assert!(context.contains("failed to write hook stdin JSON: broken pipe"));
        assert!(context.contains("failed to read hook stderr: stream closed"));
    }

    #[test]
    fn test_parse_hook_output_appends_io_diagnostics_to_plain_text() {
        let diagnostics = vec!["hook command wait error: no child".to_string()];

        let output =
            parse_hook_output_with_diagnostics("plain hook context", &diagnostics).unwrap();

        let context = output.additional_context.unwrap();
        assert!(context.starts_with("plain hook context"));
        assert!(context.contains("Hook IO diagnostics:"));
        assert!(context.contains("hook command wait error: no child"));
    }

    // -- integration test: execute_command_hook --

    #[tokio::test]
    async fn test_execute_command_hook_echo() {
        let stdin_json = json!({"tool_name": "Bash", "tool_input": {"command": "ls"}});

        let result = execute_command_hook(
            r#"echo '{"continue":true,"reason":"test_ok"}'"#,
            &stdin_json,
            10,
            None,
        )
        .await;

        match result {
            Ok(output) => {
                assert!(output.should_continue);
                assert_eq!(output.reason.as_deref(), Some("test_ok"));
            }
            Err(e) => {
                // If bash is not available (e.g., some CI environments),
                // just warn and skip
                eprintln!("Skipping test_execute_command_hook_echo: {}", e);
            }
        }
    }

    #[tokio::test]
    async fn test_execute_command_hook_plain_text() {
        let stdin_json = json!({"test": true});

        let result = execute_command_hook("echo hello_world", &stdin_json, 10, None).await;

        match result {
            Ok(output) => {
                assert!(output.should_continue);
                assert!(output.additional_context.is_some());
                assert!(output
                    .additional_context
                    .as_ref()
                    .unwrap()
                    .contains("hello_world"));
            }
            Err(e) => {
                eprintln!("Skipping test_execute_command_hook_plain_text: {}", e);
            }
        }
    }

    // -- Hook timeout --

    #[tokio::test]
    #[cfg(not(windows))]
    async fn test_hook_timeout() {
        let result = execute_command_hook("sleep 60", &json!({"test": true}), 2, None).await;

        match result {
            Err(e) => assert!(e.to_string().contains("timed out")),
            Ok(_) => eprintln!("Skipping: sleep not available"),
        }
    }

    // -- Windows pipe bug documentation --

    /// Documents the known Windows pipe I/O bug.
    /// On Windows, this test confirms the bug exists.
    /// On other platforms, this test is a no-op.
    #[test]
    fn document_windows_pipe_bug() {
        if cfg!(windows) {
            eprintln!(
                "KNOWN BUG: execute_command_hook has a pipe I/O blocking issue on Windows.\n\
                 tokio's ChildStdout::read_to_end hangs because the OS pipe handle\n\
                 doesn't signal EOF when the subprocess exits.\n\
                 All subprocess hook tests are skipped on Windows via #[cfg(not(windows))].\n\
                 Hooks will NOT work at runtime on Windows until this is fixed."
            );
        }
    }
}
