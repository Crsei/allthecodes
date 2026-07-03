//! SystemStatus tool - lets the Agent query subsystem status.

use std::sync::{Arc, OnceLock};

use allthecodes_ipc_protocol::subsystem_types::*;
use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::tool::{Tool, ToolProgress, ToolResult, ToolUseContext, ValidationResult};
use allthecodes_types::agent_types::AgentNode;
use allthecodes_types::message::AssistantMessage;

#[derive(Debug, Clone, Default)]
pub struct ChromeStatus {
    pub connection_label: String,
    pub extension_detected: Option<bool>,
    pub last_error: Option<String>,
}

pub trait SystemStatusRuntimeHost: Send + Sync + 'static {
    fn build_lsp_server_info_list(&self) -> Vec<LspServerInfo>;
    fn build_mcp_server_info_list(&self) -> Vec<McpServerStatusInfo>;
    fn build_plugin_info_list(&self) -> Vec<PluginInfo>;
    fn build_skill_info_list(&self) -> Vec<SkillInfo>;
    fn build_ide_info_list(&self) -> Vec<IdeInfo>;
    fn active_agents(&self) -> Vec<AgentNode>;
}

static HOST: OnceLock<Arc<dyn SystemStatusRuntimeHost>> = OnceLock::new();

pub fn set_runtime_host(host: Arc<dyn SystemStatusRuntimeHost>) {
    let _ = HOST.set(host);
}

fn host() -> Option<&'static Arc<dyn SystemStatusRuntimeHost>> {
    HOST.get()
}

pub fn system_status_description() -> String {
    "Query the current status of subsystems (LSP, MCP, plugins, skills, IDE, Chrome).".to_string()
}

pub fn system_status_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "subsystem": {
                "type": "string",
                "enum": ["lsp", "mcp", "plugins", "skills", "agents", "teams", "ide", "chrome", "all"],
                "description": "Which subsystem to query. Defaults to 'all'."
            }
        }
    })
}

pub fn system_status_prompt() -> String {
    "Use SystemStatus to check the current status of LSP servers, MCP servers, plugins, skills, IDE, and Chrome. \
     Query a specific subsystem with the `subsystem` parameter, or use \"all\" for a full overview."
        .to_string()
}

pub fn format_status_output(subsystem: &str, chrome: Option<ChromeStatus>) -> String {
    let mut parts = Vec::new();

    if subsystem == "all" || subsystem == "lsp" {
        let servers = host()
            .map(|host| host.build_lsp_server_info_list())
            .unwrap_or_default();
        parts.push(format_lsp_section(&servers));
    }

    if subsystem == "all" || subsystem == "mcp" {
        let servers = host()
            .map(|host| host.build_mcp_server_info_list())
            .unwrap_or_default();
        parts.push(format_mcp_section(&servers));
    }

    if subsystem == "all" || subsystem == "plugins" {
        let plugins = host()
            .map(|host| host.build_plugin_info_list())
            .unwrap_or_default();
        parts.push(format_plugin_section(&plugins));
    }

    if subsystem == "all" || subsystem == "skills" {
        let skills = host()
            .map(|host| host.build_skill_info_list())
            .unwrap_or_default();
        parts.push(format_skill_section(&skills));
    }

    if subsystem == "all" || subsystem == "agents" {
        let agents = host().map(|host| host.active_agents()).unwrap_or_default();
        parts.push(format_agent_section(&agents));
    }

    if subsystem == "all" || subsystem == "teams" {
        parts.push(format_team_section());
    }

    if subsystem == "all" || subsystem == "ide" {
        let ides = host()
            .map(|host| host.build_ide_info_list())
            .unwrap_or_default();
        parts.push(format_ide_section(&ides));
    }

    if subsystem == "all" || subsystem == "chrome" {
        parts.push(format_chrome_section(chrome));
    }

    parts.join("\n")
}

pub struct SystemStatusTool;

#[async_trait]
impl Tool for SystemStatusTool {
    fn name(&self) -> &str {
        "SystemStatus"
    }

    async fn description(&self, _input: &Value) -> String {
        system_status_description()
    }

    fn input_json_schema(&self) -> Value {
        system_status_schema()
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    async fn validate_input(&self, _input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        ValidationResult::Ok
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
        _parent_message: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let subsystem = input["subsystem"].as_str().unwrap_or("all");
        let output = format_status_output_for_tool(subsystem);

        Ok(ToolResult {
            data: json!({ "status": output }),
            model_content: None,
            display_preview: Some(output.clone()),
            new_messages: vec![],
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        system_status_prompt()
    }

    fn user_facing_name(&self, _input: Option<&Value>) -> String {
        "SystemStatus".to_string()
    }
}

fn format_status_output_for_tool(subsystem: &str) -> String {
    let snap = allthecodes_browser::state::snapshot();
    format_status_output(
        subsystem,
        Some(ChromeStatus {
            connection_label: snap.connection.label().to_string(),
            extension_detected: snap.extension_installed,
            last_error: snap.last_error,
        }),
    )
}

fn format_lsp_section(servers: &[LspServerInfo]) -> String {
    let mut section = String::from("## LSP Servers\n");
    if servers.is_empty() {
        section.push_str("No LSP servers configured.\n");
    } else {
        for s in servers {
            section.push_str(&format!(
                "- {}: {} (open_files={}, extensions: {})\n",
                s.language_id,
                s.state,
                s.open_files_count,
                s.extensions.join(", ")
            ));
        }
    }
    section
}

fn format_mcp_section(servers: &[McpServerStatusInfo]) -> String {
    let mut section = String::from("## MCP Servers\n");
    if servers.is_empty() {
        section.push_str("No MCP servers configured.\n");
    } else {
        for s in servers {
            let detail = format_mcp_health_detail(s);
            let mut line = format!(
                "- {}: {} ({}, {} tools, {} resources){}",
                s.name, s.state, s.transport, s.tools_count, s.resources_count, detail
            );
            if let Some(ref info) = s.server_info {
                line.push_str(&format!(" [{}@{}]", info.name, info.version));
            }
            if let Some(error) = s.error.as_deref() {
                line.push_str(&format!(" error={error}"));
            }
            section.push_str(&format!("{}\n", line));
            if let Some(stderr) = s.stderr_tail.last() {
                section.push_str(&format!("  stderr: {stderr}\n"));
            }
        }
    }
    section
}

fn format_mcp_health_detail(server: &McpServerStatusInfo) -> String {
    let mut fields = Vec::new();
    if let Some(kind) = server.last_error_kind.as_deref() {
        fields.push(format!("error_kind={kind}"));
    }
    if let Some(count) = server.failure_count {
        fields.push(format!("failures={count}"));
    }
    if let Some(count) = server.connect_attempt_count {
        fields.push(format!("attempts={count}"));
    }
    if let Some(count) = server.retry_scheduled_count {
        fields.push(format!("retries_scheduled={count}"));
    }
    if let Some(count) = server.retry_exhausted_count {
        fields.push(format!("retries_exhausted={count}"));
    }
    if let Some(count) = server.recovered_count {
        fields.push(format!("recovered={count}"));
    }
    if let Some(next_retry_at) = server.next_retry_at {
        fields.push(format!("next_retry_at={next_retry_at}"));
    }
    if let Some(count) = server.stderr_tail_dropped_line_count {
        fields.push(format!("stderr_dropped={count}"));
    }
    if fields.is_empty() {
        String::new()
    } else {
        format!(" [{}]", fields.join(", "))
    }
}

fn format_plugin_section(plugins: &[PluginInfo]) -> String {
    let mut section = String::from("## Plugins\n");
    if plugins.is_empty() {
        section.push_str("No plugins installed.\n");
    } else {
        for p in plugins {
            let mut line = format!("- {}: {} (v{})", p.id, p.status, p.version);
            if !p.contributed_skills.is_empty() {
                line.push_str(&format!("\n  Skills: {}", p.contributed_skills.join(", ")));
            }
            if !p.contributed_tools.is_empty() {
                line.push_str(&format!("\n  Tools: {}", p.contributed_tools.join(", ")));
            }
            section.push_str(&format!("{}\n", line));
        }
    }
    section
}

fn format_skill_section(skills: &[SkillInfo]) -> String {
    let mut section = format!("## Skills ({} total)\n", skills.len());
    if skills.is_empty() {
        section.push_str("No skills loaded.\n");
    } else {
        for s in skills {
            section.push_str(&format!(
                "- {} [{}] - {}\n",
                s.name, s.source, s.description
            ));
        }
    }
    section
}

fn format_agent_section(active: &[AgentNode]) -> String {
    let bg_count = active.iter().filter(|a| a.is_background).count();
    let mut section = format!(
        "## Active Agents ({} total, {} background)\n",
        active.len(),
        bg_count
    );
    if active.is_empty() {
        section.push_str("No active agents.\n");
    } else {
        for a in active {
            section.push_str(&format!(
                "- {}: {} [{}{}] - \"{}\" (depth {})\n",
                a.agent_id,
                a.state,
                if a.is_background {
                    "background"
                } else {
                    "sync"
                },
                a.agent_type
                    .as_ref()
                    .map(|t| format!(", {}", t))
                    .unwrap_or_default(),
                a.description,
                a.depth,
            ));
        }
    }
    section
}

fn format_team_section() -> String {
    let mut section = String::from("## Teams\n");
    section.push_str("Team status requires ALLTHECODES_EXPERIMENTAL_AGENT_TEAMS feature.\n");
    section
}

fn format_ide_section(ides: &[IdeInfo]) -> String {
    let mut section = String::from("## IDE Integrations\n");
    if ides.is_empty() {
        section.push_str("No IDE integrations detected.\n");
    } else {
        for ide in ides {
            section.push_str(&format!(
                "- {}: installed={} running={} selected={} connection={}\n",
                ide.id,
                ide.installed,
                ide.running,
                ide.selected,
                ide.connection_state.as_deref().unwrap_or("disconnected")
            ));
        }
    }
    section
}

fn format_chrome_section(chrome: Option<ChromeStatus>) -> String {
    let mut section = String::from("## Chrome Integration\n");
    if let Some(chrome) = chrome {
        section.push_str(&format!("Connection: {}\n", chrome.connection_label));
        section.push_str(&format!(
            "Extension detected: {}\n",
            chrome
                .extension_detected
                .map(|value| value.to_string())
                .unwrap_or_else(|| "not checked".to_string())
        ));
        if let Some(error) = chrome.last_error.as_deref() {
            section.push_str(&format!("Last error: {error}\n"));
        }
    } else {
        section.push_str("Connection: unavailable\n");
        section.push_str("Extension detected: not checked\n");
    }
    section
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_status_tool_name() {
        let tool = SystemStatusTool;
        assert_eq!(tool.name(), "SystemStatus");
    }

    #[test]
    fn system_status_tool_is_read_only() {
        let tool = SystemStatusTool;
        assert!(tool.is_read_only(&json!({})));
    }

    #[test]
    fn system_status_tool_schema_has_subsystem_property() {
        let tool = SystemStatusTool;
        let schema = tool.input_json_schema();
        assert!(schema["properties"]["subsystem"].is_object());
    }

    #[test]
    fn format_status_output_all_returns_all_sections() {
        let output = format_status_output("all", None);
        assert!(output.contains("## LSP Servers"));
        assert!(output.contains("## MCP Servers"));
        assert!(output.contains("## Plugins"));
        assert!(output.contains("## Skills"));
        assert!(output.contains("## IDE Integrations"));
        assert!(output.contains("## Chrome Integration"));
    }

    #[test]
    fn format_status_output_lsp_only() {
        let output = format_status_output("lsp", None);
        assert!(output.contains("## LSP Servers"));
        assert!(!output.contains("## MCP Servers"));
    }

    #[test]
    fn format_mcp_section_includes_health_details() {
        let output = format_mcp_section(&[McpServerStatusInfo {
            name: "broken".to_string(),
            state: "error".to_string(),
            transport: "stdio".to_string(),
            tools_count: 0,
            resources_count: 0,
            error: Some("failed to spawn MCP server".to_string()),
            last_error_kind: Some("spawn_failed".to_string()),
            failure_count: Some(3),
            connect_attempt_count: Some(3),
            retry_scheduled_count: Some(2),
            retry_exhausted_count: Some(1),
            recovered_count: Some(0),
            stderr_tail: vec!["startup failed".to_string()],
            stderr_tail_dropped_line_count: Some(4),
            ..Default::default()
        }]);

        assert!(output.contains("error_kind=spawn_failed"));
        assert!(output.contains("failures=3"));
        assert!(output.contains("attempts=3"));
        assert!(output.contains("retries_scheduled=2"));
        assert!(output.contains("retries_exhausted=1"));
        assert!(output.contains("recovered=0"));
        assert!(output.contains("stderr_dropped=4"));
        assert!(output.contains("failed to spawn MCP server"));
        assert!(output.contains("stderr: startup failed"));
    }
}
