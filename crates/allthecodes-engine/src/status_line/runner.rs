//! Subprocess runner for the scriptable status line.
//!
//! Responsibilities:
//!
//! 1. Accept a [`StatusLinePayload`] plus a [`StatusLineSettings`], spawn
//!    the configured command, write the payload to its stdin as JSON, and
//!    capture stdout within a timeout.
//! 2. Throttle refreshes to `refreshIntervalMs`: if a request arrives
//!    inside the window, remember the payload but don't spawn yet.
//! 3. Cancel an in-flight subprocess when a newer refresh starts — we
//!    never want to display stale output on top of stale output.
//! 4. On failure (spawn error, non-zero exit, timeout) surface the error
//!    through [`StatusLineOutput::error`] so the TUI can fall back to the
//!    default footer.
//!
//! The runner is driven by the UI layer's event loop (not a background
//! task) via [`StatusLineRunner::tick`] — this keeps things deterministic
//! and avoids spurious redraws from a separately-timed watchdog.

use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::tools::exec::process_control::{configure_process_group, terminate_process_tree};
use parking_lot::Mutex;
use serde_json::Value;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::task::{AbortHandle, JoinHandle};

use allthecodes_config::settings::StatusLineSettings;

use super::payload::StatusLinePayload;

/// Most recent result of running the status-line command.
#[derive(Debug, Clone, Default)]
pub struct StatusLineOutput {
    /// Captured stdout, trimmed of a single trailing newline. May contain
    /// multiple lines (separated by `\n`).
    pub stdout: String,
    /// Non-empty when the last run failed (spawn error, timeout, non-zero
    /// exit). Shown via `/statusline status`; the TUI silently falls back
    /// to the default footer.
    pub error: Option<String>,
    /// When the result was produced.
    pub updated_at: Option<Instant>,
}

impl StatusLineOutput {
    /// Is there anything we should display (non-empty stdout, no error)?
    pub fn is_usable(&self) -> bool {
        self.error.is_none() && !self.stdout.trim().is_empty()
    }

    /// Split into lines (up to `max_lines`, trimmed trailing blanks).
    pub fn lines(&self, max_lines: usize) -> Vec<String> {
        self.stdout
            .lines()
            .take(max_lines.max(1))
            .map(|l| l.to_string())
            .collect()
    }
}

/// Shared runner state. Cheap to clone; all fields are behind a mutex so
/// the command handler and the TUI can read/write without racing.
#[derive(Clone, Default)]
pub struct StatusLineRunner {
    inner: Arc<Mutex<RunnerState>>,
}

impl std::fmt::Debug for StatusLineRunner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let guard = self.inner.lock();
        f.debug_struct("StatusLineRunner")
            .field("runs", &guard.runs)
            .field("errors", &guard.errors)
            .field("in_flight", &guard.in_flight.is_some())
            .finish()
    }
}

#[derive(Default)]
struct RunnerState {
    last_output: StatusLineOutput,
    /// Hash of the last payload sent to the script — used to skip redundant
    /// refreshes when nothing meaningful changed.
    last_payload_fingerprint: Option<u64>,
    /// When we last started a run. Throttles `refreshIntervalMs`.
    last_started: Option<Instant>,
    /// In-flight task — aborted when a newer run starts.
    in_flight: Option<AbortHandle>,
    /// Total spawns so far (for `/statusline status`).
    runs: u64,
    /// Total errors so far.
    errors: u64,
}

impl StatusLineRunner {
    /// Construct a fresh runner with no cached output.
    pub fn new() -> Self {
        Self::default()
    }

    /// Snapshot of the most recent output (clone).
    pub fn latest(&self) -> StatusLineOutput {
        self.inner.lock().last_output.clone()
    }

    /// Spawn counters for introspection (`/statusline status`).
    pub fn stats(&self) -> (u64, u64) {
        let guard = self.inner.lock();
        (guard.runs, guard.errors)
    }

    /// Abort any in-flight command and forget the cached output. Called
    /// when the user disables the status-line via `/statusline clear`.
    pub fn reset(&self) {
        let mut guard = self.inner.lock();
        if let Some(handle) = guard.in_flight.take() {
            handle.abort();
        }
        guard.last_output = StatusLineOutput::default();
        guard.last_payload_fingerprint = None;
        guard.last_started = None;
    }

    /// Request a refresh.
    ///
    /// - If the runner is disabled by config, returns immediately.
    /// - If a refresh is already queued/running and `refreshIntervalMs`
    ///   hasn't elapsed, returns without touching anything.
    /// - Otherwise spawns a new run; any in-flight run is aborted first.
    ///
    /// The returned [`JoinHandle`] is mostly useful in tests (production
    /// callers poll [`latest`](Self::latest) on the next frame).
    pub fn refresh(
        &self,
        settings: &StatusLineSettings,
        payload: &StatusLinePayload,
    ) -> Option<JoinHandle<()>> {
        if !settings.is_command_mode() {
            return None;
        }
        let command = settings.runnable_command()?;
        let timeout_ms = settings.effective_timeout_ms();
        let refresh_ms = settings.effective_refresh_ms();

        let payload_json = match serde_json::to_string(payload) {
            Ok(s) => s,
            Err(e) => {
                self.record_failure(format!("payload encode failed: {}", e));
                return None;
            }
        };
        let fingerprint = fingerprint_str(&payload_json);

        {
            let mut guard = self.inner.lock();
            // Throttle — but only when the payload hasn't changed.
            if let Some(started) = guard.last_started {
                let elapsed = started.elapsed();
                if elapsed < Duration::from_millis(refresh_ms)
                    && guard.last_payload_fingerprint == Some(fingerprint)
                {
                    return None;
                }
            }
            // Cancel any previous run.
            if let Some(handle) = guard.in_flight.take() {
                handle.abort();
            }
            guard.last_started = Some(Instant::now());
            guard.last_payload_fingerprint = Some(fingerprint);
            guard.runs = guard.runs.saturating_add(1);
        }

        let inner = Arc::clone(&self.inner);
        let cmd_str = command.to_string();
        let handle = tokio::spawn(async move {
            let result = spawn_and_capture(&cmd_str, payload_json, timeout_ms).await;
            let mut guard = inner.lock();
            guard.in_flight = None;
            match result {
                Ok(stdout) => {
                    guard.last_output = StatusLineOutput {
                        stdout,
                        error: None,
                        updated_at: Some(Instant::now()),
                    };
                }
                Err(e) => {
                    guard.errors = guard.errors.saturating_add(1);
                    guard.last_output.error = Some(e);
                    guard.last_output.updated_at = Some(Instant::now());
                }
            }
        });

        self.inner.lock().in_flight = Some(handle.abort_handle());
        Some(handle)
    }

    /// Record a synchronous failure (e.g. payload serialization error).
    /// Used when a run can't even begin.
    fn record_failure(&self, err: impl Into<String>) {
        let mut guard = self.inner.lock();
        guard.errors = guard.errors.saturating_add(1);
        guard.last_output.error = Some(err.into());
        guard.last_output.updated_at = Some(Instant::now());
    }
}

/// Actually spawn the command and capture stdout with a timeout.
///
/// The command runs through the platform shell (`sh -c` or `cmd /C`) so
/// users can paste pipelines straight from the docs.
async fn spawn_and_capture(
    command: &str,
    payload_json: String,
    timeout_ms: u64,
) -> Result<String, String> {
    let mut cmd = shell_command(command);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    configure_process_group(&mut cmd);
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to spawn '{}': {}", command, e))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(payload_json.as_bytes())
            .await
            .map_err(|e| format!("write stdin failed: {}", e))?;
        stdin
            .shutdown()
            .await
            .map_err(|e| format!("close stdin failed: {}", e))?;
    }
    let pid = child.id();
    let output_future = child.wait_with_output();
    tokio::pin!(output_future);
    let output = tokio::select! {
        result = &mut output_future => result.map_err(|e| format!("wait failed: {}", e))?,
        _ = tokio::time::sleep(Duration::from_millis(timeout_ms)) => {
            terminate_process_tree(pid).await.map_err(|e| format!("terminate failed: {}", e))?;
            let _ = tokio::time::timeout(Duration::from_secs(5), &mut output_future).await;
            return Err(format!("timed out after {} ms", timeout_ms));
        }
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "command exited with {} — {}",
            output
                .status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "signal".into()),
            stderr.trim()
        ));
    }
    let mut stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    if stdout.ends_with('\n') {
        stdout.pop();
        if stdout.ends_with('\r') {
            stdout.pop();
        }
    }
    Ok(stdout)
}

#[cfg(unix)]
fn shell_command(command: &str) -> Command {
    let mut c = Command::new("sh");
    c.arg("-c").arg(command);
    c
}

#[cfg(windows)]
fn shell_command(command: &str) -> Command {
    let mut c = Command::new("cmd");
    c.arg("/C").arg(command);
    c
}

/// Stable-ish non-cryptographic fingerprint for payload change detection.
/// Using `serde_json::Value`'s Display wouldn't help here; we just hash the
/// serialized JSON string. Stability across runs isn't required — we only
/// compare within a single process.
fn fingerprint_str(s: &str) -> u64 {
    allthecodes_utils::hash::hash_content(s.as_bytes())
}

/// Build a payload from a plain `serde_json::Value` (for tests and
/// `/statusline test`). Most production callers use
/// [`crate::ui::status_line::payload::StatusLinePayload`] directly.
pub fn payload_from_value(v: Value) -> Result<StatusLinePayload, serde_json::Error> {
    serde_json::from_value(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_config::settings::StatusLineSettings;

    fn make_settings(command: &str) -> StatusLineSettings {
        StatusLineSettings {
            r#type: Some("command".into()),
            command: Some(command.into()),
            refresh_interval_ms: Some(100),
            timeout_ms: Some(2_000),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn runner_captures_stdout_from_simple_command() {
        #[cfg(unix)]
        let cmd = "cat";
        #[cfg(windows)]
        let cmd = "findstr x*"; // identity filter on Windows
        let runner = StatusLineRunner::new();
        let payload = StatusLinePayload {
            hook_event_name: "StatusLine".to_string(),
            version: 1,
            session_id: Some("sess-1".into()),
            ..Default::default()
        };
        let handle = runner
            .refresh(&make_settings(cmd), &payload)
            .expect("refresh to start");
        handle.await.expect("join");
        let output = runner.latest();
        assert!(
            output.error.is_none(),
            "unexpected error: {:?}",
            output.error
        );
        assert!(
            output.stdout.contains("sess-1"),
            "expected payload echo, got: {:?}",
            output.stdout
        );
    }

    #[tokio::test]
    async fn runner_reports_timeout() {
        #[cfg(unix)]
        let cmd = "sleep 5";
        #[cfg(windows)]
        let cmd = "powershell -NoProfile -Command Start-Sleep -Seconds 5";
        let mut s = make_settings(cmd);
        s.timeout_ms = Some(150);
        let runner = StatusLineRunner::new();
        let payload = StatusLinePayload::new();
        let handle = runner.refresh(&s, &payload).expect("refresh to start");
        handle.await.expect("join");
        let output = runner.latest();
        assert!(
            output.error.as_deref().unwrap_or("").contains("timed out"),
            "expected timeout error, got: {:?}",
            output.error
        );
    }

    #[tokio::test]
    async fn runner_reports_spawn_or_exit_error() {
        let cmd = "this-command-definitely-does-not-exist-12345";
        let runner = StatusLineRunner::new();
        let payload = StatusLinePayload::new();
        let handle = runner
            .refresh(&make_settings(cmd), &payload)
            .expect("refresh to start");
        handle.await.expect("join");
        let output = runner.latest();
        assert!(output.error.is_some(), "expected error for missing command");
    }

    #[tokio::test]
    async fn runner_skips_when_disabled() {
        let settings = StatusLineSettings {
            r#type: Some("command".into()),
            command: Some("true".into()),
            enabled: Some(false),
            ..Default::default()
        };
        let runner = StatusLineRunner::new();
        let handle = runner.refresh(&settings, &StatusLinePayload::new());
        assert!(handle.is_none());
    }

    #[tokio::test]
    async fn runner_throttles_within_window() {
        // Use a command that writes a known marker so we can verify the
        // second call did NOT replace the output.
        #[cfg(unix)]
        let cmd = "printf 'first'";
        #[cfg(windows)]
        let cmd = "cmd /C echo|set /p=first";
        let settings = StatusLineSettings {
            r#type: Some("command".into()),
            command: Some(cmd.into()),
            refresh_interval_ms: Some(5_000),
            timeout_ms: Some(2_000),
            ..Default::default()
        };
        let runner = StatusLineRunner::new();
        let payload = StatusLinePayload::new();
        let h1 = runner.refresh(&settings, &payload).expect("first run");
        h1.await.expect("join1");
        // Second refresh inside window with identical payload → skipped.
        let h2 = runner.refresh(&settings, &payload);
        assert!(h2.is_none(), "expected throttle, got spawn");
    }

    #[test]
    fn output_is_usable_only_with_content_and_no_error() {
        let empty = StatusLineOutput::default();
        assert!(!empty.is_usable());
        let ok = StatusLineOutput {
            stdout: "hello".into(),
            error: None,
            updated_at: Some(Instant::now()),
        };
        assert!(ok.is_usable());
        let errored = StatusLineOutput {
            stdout: "hello".into(),
            error: Some("oh no".into()),
            updated_at: Some(Instant::now()),
        };
        assert!(!errored.is_usable());
    }

    #[test]
    fn payload_from_value_parses_status_line_payload() {
        let payload = payload_from_value(serde_json::json!({
            "hookEventName": "StatusLine",
            "version": 1,
            "sessionId": "sess-1",
            "model": {"id": "claude-sonnet-4-20250514"},
            "streaming": false,
            "messageCount": 3
        }))
        .expect("payload should parse");

        assert_eq!(payload.session_id.as_deref(), Some("sess-1"));
        assert_eq!(
            payload.model.as_ref().unwrap().id,
            "claude-sonnet-4-20250514"
        );
        assert_eq!(payload.message_count, 3);
    }

    #[test]
    fn output_lines_splits_multi_line_stdout() {
        let out = StatusLineOutput {
            stdout: "line-a\nline-b\nline-c".into(),
            error: None,
            updated_at: None,
        };
        assert_eq!(out.lines(2), vec!["line-a", "line-b"]);
        assert_eq!(out.lines(10).len(), 3);
    }
}
