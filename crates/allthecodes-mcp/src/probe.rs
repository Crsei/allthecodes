//! Reusable MCP proof-of-life probe.

use std::collections::HashMap;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::client::McpClient;
use crate::McpServerConfig;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpProbeResult {
    pub server: String,
    pub status: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<usize>,
    pub checks: Vec<McpProbeCheck>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpProbeCheck {
    pub name: String,
    pub status: String,
    pub message: String,
}

impl McpProbeCheck {
    pub fn new(
        name: impl Into<String>,
        status: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            status: status.into(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpProbeRequiredEnv {
    pub env_var: String,
    pub check_name: String,
    pub present_message: String,
    pub missing_message: String,
}

impl McpProbeRequiredEnv {
    pub fn new(
        env_var: impl Into<String>,
        check_name: impl Into<String>,
        present_message: impl Into<String>,
        missing_message: impl Into<String>,
    ) -> Self {
        Self {
            env_var: env_var.into(),
            check_name: check_name.into(),
            present_message: present_message.into(),
            missing_message: missing_message.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpProbeOptions {
    pub declaration_check: Option<McpProbeCheck>,
    pub required_env: Vec<McpProbeRequiredEnv>,
}

impl Default for McpProbeOptions {
    fn default() -> Self {
        Self {
            declaration_check: Some(McpProbeCheck::new(
                "configuration",
                "ready",
                "MCP server is configured",
            )),
            required_env: Vec::new(),
        }
    }
}

pub async fn probe_mcp_server(config: McpServerConfig) -> McpProbeResult {
    probe_mcp_server_with_options(config, McpProbeOptions::default()).await
}

pub async fn probe_mcp_server_with_options(
    config: McpServerConfig,
    options: McpProbeOptions,
) -> McpProbeResult {
    let server = config.name.clone();
    let command = config.command.clone();
    let mut checks = Vec::new();
    if let Some(check) = options.declaration_check {
        checks.push(check);
    }

    let mut hard_failure = None;
    if config.disabled.unwrap_or(false) {
        hard_failure = Some("MCP server is disabled".to_string());
        checks.push(check("enabled", "failed", "MCP server is disabled"));
    }

    match config.transport.as_str() {
        "stdio" => validate_stdio_command(&command, &mut checks, &mut hard_failure),
        "sse" | "streamable-http" => {
            validate_remote_url(
                &config.transport,
                &config.url,
                &mut checks,
                &mut hard_failure,
            );
        }
        transport => {
            hard_failure = Some(format!("Unsupported MCP transport: {transport}"));
            checks.push(check(
                "transport",
                "failed",
                format!("Unsupported MCP transport: {transport}"),
            ));
        }
    }

    validate_required_env(
        config.env.as_ref(),
        options.required_env,
        &mut checks,
        &mut hard_failure,
    );

    if let Some(message) = hard_failure {
        return result(server, "failed", message, command, None, None, checks);
    }

    let mut client = McpClient::new(config);
    if let Err(error) = client.connect().await {
        checks.push(check("connect", "failed", error.to_string()));
        client.disconnect().await;
        return result(
            server,
            "failed",
            format!("MCP connect failed: {error}"),
            command,
            None,
            None,
            checks,
        );
    }
    checks.push(check("connect", "ready", "MCP process connected"));

    if let Err(error) = client.initialize().await {
        checks.push(check("initialize", "failed", error.to_string()));
        client.disconnect().await;
        return result(
            server,
            "failed",
            format!("MCP initialize failed: {error}"),
            command,
            None,
            None,
            checks,
        );
    }
    checks.push(check("initialize", "ready", "MCP initialize completed"));

    let mut status = "ready".to_string();
    let mut message = "MCP server is ready".to_string();
    let mut tools = None;
    let mut resources = None;

    if client.supports_tools() {
        match client.list_tools().await {
            Ok(list) => {
                tools = Some(list.len());
                checks.push(check(
                    "tools_list",
                    "ready",
                    format!("{} tools", list.len()),
                ));
            }
            Err(error) => {
                status = "warning".to_string();
                message = format!("tools/list failed: {error}");
                checks.push(check("tools_list", "warning", error.to_string()));
            }
        }
    } else {
        tools = Some(0);
        checks.push(check(
            "tools_list",
            "warning",
            "Server does not advertise tools",
        ));
    }

    if client.supports_resources() {
        match client.list_resources().await {
            Ok(list) => {
                resources = Some(list.len());
                checks.push(check(
                    "resources_list",
                    "ready",
                    format!("{} resources", list.len()),
                ));
            }
            Err(error) => {
                status = "warning".to_string();
                if message == "MCP server is ready" {
                    message = format!("resources/list failed: {error}");
                }
                checks.push(check("resources_list", "warning", error.to_string()));
            }
        }
    } else {
        resources = Some(0);
        checks.push(check(
            "resources_list",
            "warning",
            "Server does not advertise resources",
        ));
    }

    if tools == Some(0) && resources == Some(0) && status == "ready" {
        status = "warning".to_string();
        message = "MCP initialized but returned 0 tools and 0 resources".to_string();
    }

    client.disconnect().await;
    result(server, status, message, command, tools, resources, checks)
}

fn validate_stdio_command(
    command: &Option<String>,
    checks: &mut Vec<McpProbeCheck>,
    hard_failure: &mut Option<String>,
) {
    match command.as_deref() {
        Some(command) if !command.trim().is_empty() => {
            checks.push(check("command", "ready", format!("Command: {command}")));
            if command_has_path_components(command) {
                match std::fs::metadata(command) {
                    Ok(metadata) if metadata.is_file() => {
                        checks.push(check("command_file", "ready", "Command file exists"));
                        if command_is_executable(&metadata) {
                            checks.push(check("executable", "ready", "Command file is executable"));
                        } else {
                            *hard_failure = Some("Command file is not executable".to_string());
                            checks.push(check(
                                "executable",
                                "failed",
                                "Command file is not executable",
                            ));
                        }
                    }
                    Ok(_) => {
                        *hard_failure = Some("Command path is not a file".to_string());
                        checks.push(check(
                            "command_file",
                            "failed",
                            "Command path is not a file",
                        ));
                    }
                    Err(error) => {
                        let message = format!("Command file is missing: {error}");
                        *hard_failure = Some(message.clone());
                        checks.push(check("command_file", "failed", message));
                    }
                }
            } else {
                checks.push(check(
                    "command_file",
                    "warning",
                    "Command will be resolved from PATH",
                ));
            }
        }
        _ => {
            *hard_failure = Some("stdio MCP server is missing a command".to_string());
            checks.push(check(
                "command",
                "failed",
                "stdio MCP server is missing a command",
            ));
        }
    }
}

fn validate_remote_url(
    transport: &str,
    url: &Option<String>,
    checks: &mut Vec<McpProbeCheck>,
    hard_failure: &mut Option<String>,
) {
    match url.as_deref() {
        Some(url) if !url.trim().is_empty() => {
            checks.push(check("url", "ready", format!("URL: {url}")));
        }
        _ => {
            let message = format!("{transport} MCP server is missing a URL");
            *hard_failure = Some(message.clone());
            checks.push(check("url", "failed", message));
        }
    }
}

fn validate_required_env(
    env: Option<&HashMap<String, String>>,
    required: Vec<McpProbeRequiredEnv>,
    checks: &mut Vec<McpProbeCheck>,
    hard_failure: &mut Option<String>,
) {
    for required_env in required {
        let present = env
            .and_then(|env| env.get(&required_env.env_var))
            .is_some_and(|value| !value.trim().is_empty());
        if present {
            checks.push(check(
                required_env.check_name,
                "ready",
                required_env.present_message,
            ));
        } else {
            *hard_failure = Some(required_env.missing_message.clone());
            checks.push(check(
                required_env.check_name,
                "failed",
                required_env.missing_message,
            ));
        }
    }
}

fn result(
    server: String,
    status: impl Into<String>,
    message: impl Into<String>,
    command: Option<String>,
    tools: Option<usize>,
    resources: Option<usize>,
    checks: Vec<McpProbeCheck>,
) -> McpProbeResult {
    McpProbeResult {
        server,
        status: status.into(),
        message: message.into(),
        command,
        tools,
        resources,
        checks,
    }
}

fn check(
    name: impl Into<String>,
    status: impl Into<String>,
    message: impl Into<String>,
) -> McpProbeCheck {
    McpProbeCheck::new(name, status, message)
}

fn command_has_path_components(command: &str) -> bool {
    command.contains('/') || command.contains('\\') || Path::new(command).is_absolute()
}

#[cfg(unix)]
fn command_is_executable(metadata: &std::fs::Metadata) -> bool {
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn command_is_executable(_metadata: &std::fs::Metadata) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stdio_config(command: Option<String>) -> McpServerConfig {
        McpServerConfig {
            name: "probe-test".to_string(),
            transport: "stdio".to_string(),
            command,
            args: None,
            url: None,
            headers: None,
            oauth: None,
            env: None,
            browser_mcp: None,
            disabled: None,
            bearer_token_env_var: None,
            env_http_headers: None,
            auth: None,
        }
    }

    #[tokio::test]
    async fn probe_missing_stdio_command_fails_before_connect() {
        let result = probe_mcp_server(stdio_config(None)).await;

        assert_eq!(result.status, "failed");
        assert_eq!(result.message, "stdio MCP server is missing a command");
        assert!(result.checks.iter().any(|check| {
            check.name == "command"
                && check.status == "failed"
                && check.message == "stdio MCP server is missing a command"
        }));
        assert!(!result.checks.iter().any(|check| check.name == "connect"));
    }

    #[tokio::test]
    async fn probe_options_can_add_manifest_and_required_env_checks() {
        let options = McpProbeOptions {
            declaration_check: Some(McpProbeCheck::new(
                "manifest",
                "ready",
                "MCP server is declared",
            )),
            required_env: vec![McpProbeRequiredEnv::new(
                "ALLTHECODES_COM_ACCESS_TOKEN",
                "account_token",
                "Account access token is present",
                "Official plugin MCP server is missing account token",
            )],
        };

        let result =
            probe_mcp_server_with_options(stdio_config(Some("echo".to_string())), options).await;

        assert_eq!(result.status, "failed");
        assert_eq!(
            result.message,
            "Official plugin MCP server is missing account token"
        );
        assert_eq!(result.checks[0].name, "manifest");
        assert!(result
            .checks
            .iter()
            .any(|check| check.name == "command" && check.status == "ready"));
        assert!(result
            .checks
            .iter()
            .any(|check| check.name == "account_token" && check.status == "failed"));
        assert!(!result.checks.iter().any(|check| check.name == "connect"));
    }
}
