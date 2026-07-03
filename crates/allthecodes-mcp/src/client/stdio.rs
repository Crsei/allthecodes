use std::collections::VecDeque;
use std::process::Stdio;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::Mutex;
use tracing::{debug, info};

use super::super::transport::reader_loop;
use super::super::McpConnectionState;

use super::McpClient;

pub(super) const STDERR_TAIL_LINES: usize = 50;

impl McpClient {
    /// Connect via stdio transport -- spawn subprocess.
    pub(super) async fn connect_stdio(&mut self) -> Result<()> {
        let command = self
            .config
            .command
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("stdio transport requires 'command' field"))?;

        let args = self.config.args.clone().unwrap_or_default();

        info!(
            server = %self.config.name,
            command = command,
            args = ?args,
            "MCP: spawning stdio server"
        );

        let mut cmd = tokio::process::Command::new(command);
        cmd.args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if let Some(ref env_map) = self.config.env {
            for (k, v) in env_map {
                cmd.env(k, v);
            }
        }

        let mut child = cmd
            .spawn()
            .with_context(|| format!("failed to spawn MCP server: {}", command))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("failed to capture MCP server stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("failed to capture MCP server stdout"))?;

        // Capture stderr for logging
        let stderr = child.stderr.take();
        if let Some(stderr) = stderr {
            let server_name = self.config.name.clone();
            let redactions = stderr_redactions(self.config.env.as_ref());
            let stderr_tail = self.stderr_tail.clone();
            let stderr_tail_dropped_line_count = self.stderr_tail_dropped_line_count.clone();
            if let Ok(mut tail) = stderr_tail.lock() {
                tail.clear();
            }
            stderr_tail_dropped_line_count.store(0, Ordering::SeqCst);
            tokio::spawn(async move {
                let reader = BufReader::new(stderr);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let line = redact_line(&line, &redactions);
                    let dropped = record_stderr_tail(&stderr_tail, line.clone());
                    if dropped > 0 {
                        stderr_tail_dropped_line_count.fetch_add(dropped, Ordering::SeqCst);
                    }
                    debug!(
                        event = "McpServerStderrCaptured",
                        server = %server_name,
                        stderr = %line,
                        dropped_lines = dropped,
                        "MCP server stderr"
                    );
                }
            });
        }

        let stdin_writer = Arc::new(Mutex::new(stdin));
        self.stdin_writer = Some(stdin_writer.clone());

        // Start background reader task
        let pending = self.pending.clone();
        let server_name = self.config.name.clone();
        let runtime = self.runtime.clone();
        let reader_handle = tokio::spawn(async move {
            reader_loop(stdout, pending, server_name, runtime).await;
        });
        self.replace_reader_handle(Some(reader_handle));

        self.child = Some(child);
        self.state = McpConnectionState::Connected;

        self.runtime
            .emit_event(super::super::McpSubsystemEvent::ServerStateChanged {
                server_name: self.config.name.clone(),
                state: "connected".to_string(),
                error: None,
            });

        debug!(server = %self.config.name, "MCP: stdio server connected");
        Ok(())
    }
}

fn stderr_redactions(env: Option<&std::collections::HashMap<String, String>>) -> Vec<String> {
    env.into_iter()
        .flat_map(|env| env.iter())
        .filter(|(key, value)| {
            !value.is_empty()
                && (key.contains("TOKEN")
                    || key.contains("SECRET")
                    || key.contains("PASSWORD")
                    || key.contains("ACCESS_KEY"))
        })
        .map(|(_, value)| value.clone())
        .collect()
}

fn redact_line(line: &str, redactions: &[String]) -> String {
    let mut redacted = line.to_string();
    for value in redactions {
        redacted = redacted.replace(value, "[REDACTED]");
    }
    redacted
}

pub(super) fn record_stderr_tail(
    tail: &Arc<std::sync::Mutex<VecDeque<String>>>,
    line: String,
) -> u64 {
    if let Ok(mut tail) = tail.lock() {
        tail.push_back(line);
        let mut dropped = 0;
        while tail.len() > STDERR_TAIL_LINES {
            tail.pop_front();
            dropped += 1;
        }
        return dropped;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::collections::VecDeque;

    #[test]
    fn redacts_token_env_values_from_stderr() {
        let mut env = HashMap::new();
        env.insert(
            "ALLTHECODES_COM_ACCESS_TOKEN".to_string(),
            "secret-token".to_string(),
        );
        env.insert("NORMAL".to_string(), "visible".to_string());
        let redactions = stderr_redactions(Some(&env));

        let line = redact_line("token=secret-token normal=visible", &redactions);
        assert_eq!(line, "token=[REDACTED] normal=visible");
    }

    #[test]
    fn stderr_tail_keeps_redacted_bounded_lines() {
        let mut env = HashMap::new();
        env.insert("API_TOKEN".to_string(), "secret-token".to_string());
        let redactions = stderr_redactions(Some(&env));
        let tail = Arc::new(std::sync::Mutex::new(VecDeque::new()));

        for index in 0..55 {
            let line = redact_line(&format!("line {index} secret-token"), &redactions);
            let _ = record_stderr_tail(&tail, line);
        }

        let lines = tail
            .lock()
            .expect("stderr tail")
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(lines.len(), STDERR_TAIL_LINES);
        assert_eq!(lines.first().map(String::as_str), Some("line 5 [REDACTED]"));
        assert_eq!(lines.last().map(String::as_str), Some("line 54 [REDACTED]"));
    }

    #[test]
    fn stderr_tail_reports_dropped_lines() {
        let tail = Arc::new(std::sync::Mutex::new(VecDeque::new()));
        let mut dropped = 0;

        for index in 0..55 {
            dropped += record_stderr_tail(&tail, format!("line {index}"));
        }

        assert_eq!(dropped, 5);
    }
}
