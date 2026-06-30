use std::process::Stdio;
use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::Mutex;
use tracing::{debug, info};

use super::super::transport::reader_loop;
use super::super::McpConnectionState;

use super::McpClient;

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
            tokio::spawn(async move {
                let reader = BufReader::new(stderr);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let line = redact_line(&line, &redactions);
                    debug!(server = %server_name, stderr = %line, "MCP server stderr");
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

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
}
