use std::sync::Arc;

use anyhow::{bail, Result};
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::types::message::{AssistantMessage, ToolResultContent};
use crate::types::tool::*;

const MAX_MCP_RESOURCE_MODEL_BYTES: usize = 256 * 1024;
const MCP_RESOURCE_TRUNCATION_MARKER: &str = "\n[MCP resource content truncated]";

pub fn tools() -> Tools {
    vec![
        Arc::new(ListMcpResourcesTool) as Arc<dyn Tool>,
        Arc::new(ReadMcpResourceTool) as Arc<dyn Tool>,
    ]
}

pub struct ListMcpResourcesTool;

#[async_trait]
impl Tool for ListMcpResourcesTool {
    fn name(&self) -> &str {
        "ListMcpResources"
    }

    async fn description(&self, _input: &Value) -> String {
        "Lists resources exposed by connected MCP servers.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "server": {
                    "type": "string",
                    "description": "Optional MCP server name to filter resources by"
                }
            }
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolUseContext,
        _parent_message: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let server = input.get("server").and_then(Value::as_str);
        let Some(manager) = allthecodes_mcp::runtime::current_manager() else {
            bail!("No MCP runtime manager is installed for this session");
        };

        let binding_context = crate::mcp_tool_adapter::mcp_binding_context_for_tool_use(ctx);
        let resources = {
            let manager = manager.lock().await;
            manager.list_resources_for_context(&binding_context, server)?
        };
        let content = if resources.is_empty() {
            "No resources found. MCP servers may still provide tools even if they have no resources."
                .to_string()
        } else {
            serde_json::to_string_pretty(&resources)?
        };

        Ok(ToolResult {
            data: json!(resources),
            model_content: Some(ToolResultContent::Text(content.clone())),
            display_preview: Some(content),
            new_messages: vec![],
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "Lists MCP resources from connected MCP servers. Use this before ReadMcpResource when you need MCP-provided data and do not know the exact URI."
            .to_string()
    }

    fn user_facing_name(&self, _input: Option<&Value>) -> String {
        "ListMcpResources".to_string()
    }

    fn to_auto_classifier_input(&self, input: &Value) -> Value {
        Value::String(
            input
                .get("server")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        )
    }
}

pub struct ReadMcpResourceTool;

// Resource reads keep the manager guard while delegating to the selected MCP
// client, preserving the current serialized manager access model.
#[allow(clippy::await_holding_invalid_type)]
#[async_trait]
impl Tool for ReadMcpResourceTool {
    fn name(&self) -> &str {
        "ReadMcpResource"
    }

    async fn description(&self, _input: &Value) -> String {
        "Reads a resource exposed by a connected MCP server.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "server": {
                    "type": "string",
                    "description": "The MCP server name"
                },
                "uri": {
                    "type": "string",
                    "description": "The resource URI to read"
                }
            },
            "required": ["server", "uri"]
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        if input
            .get("server")
            .and_then(Value::as_str)
            .unwrap_or("")
            .is_empty()
        {
            return ValidationResult::Error {
                message: "server is required".to_string(),
                error_code: 1,
            };
        }
        if input
            .get("uri")
            .and_then(Value::as_str)
            .unwrap_or("")
            .is_empty()
        {
            return ValidationResult::Error {
                message: "uri is required".to_string(),
                error_code: 1,
            };
        }
        ValidationResult::Ok
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolUseContext,
        _parent_message: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let server = input.get("server").and_then(Value::as_str).unwrap_or("");
        let uri = input.get("uri").and_then(Value::as_str).unwrap_or("");
        if server.is_empty() || uri.is_empty() {
            bail!("server and uri are required");
        }

        let Some(manager) = allthecodes_mcp::runtime::current_manager() else {
            bail!("No MCP runtime manager is installed for this session");
        };
        let binding_context = crate::mcp_tool_adapter::mcp_binding_context_for_tool_use(ctx);
        let result = {
            let manager = manager.lock().await;
            manager
                .read_resource_for_context(&binding_context, server, uri)
                .await?
        };
        let model_text = format_resource_contents(server, &result.contents);

        Ok(ToolResult {
            data: json!({ "contents": result.contents }),
            model_content: Some(ToolResultContent::Text(model_text.clone())),
            display_preview: Some(model_text),
            new_messages: vec![],
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "Reads a specific MCP resource by server name and URI. Use ListMcpResources first when the URI is unknown. Binary resources are summarized and are not inserted as raw base64."
            .to_string()
    }

    fn user_facing_name(&self, _input: Option<&Value>) -> String {
        "ReadMcpResource".to_string()
    }

    fn to_auto_classifier_input(&self, input: &Value) -> Value {
        Value::String(format!(
            "{} {}",
            input.get("server").and_then(Value::as_str).unwrap_or(""),
            input.get("uri").and_then(Value::as_str).unwrap_or("")
        ))
    }
}

fn format_resource_contents(
    server: &str,
    contents: &[allthecodes_mcp::McpResourceContent],
) -> String {
    if contents.is_empty() {
        return format!("MCP resource from '{}' returned no content.", server);
    }

    let mut output = String::new();
    for (index, content) in contents.iter().enumerate() {
        let mime = content
            .mime_type
            .as_deref()
            .unwrap_or("application/octet-stream");
        let pieces = if let Some(text) = content.text.as_deref() {
            vec![
                (index > 0).then_some("\n"),
                Some("[Resource from "),
                Some(server),
                Some(": "),
                Some(content.uri.as_str()),
                Some("]\n"),
                Some(text),
            ]
        } else if content.blob.is_some() {
            vec![
                (index > 0).then_some("\n"),
                Some("[Resource from "),
                Some(server),
                Some(": "),
                Some(content.uri.as_str()),
                Some(" ("),
                Some(mime),
                Some(") binary content omitted]"),
            ]
        } else {
            vec![
                (index > 0).then_some("\n"),
                Some("[Resource from "),
                Some(server),
                Some(": "),
                Some(content.uri.as_str()),
                Some("]"),
            ]
        };

        for piece in pieces.into_iter().flatten() {
            if !append_resource_piece(&mut output, piece) {
                return output;
            }
        }
    }
    output
}

fn append_resource_piece(output: &mut String, piece: &str) -> bool {
    if output.len().saturating_add(piece.len()) <= MAX_MCP_RESOURCE_MODEL_BYTES {
        output.push_str(piece);
        return true;
    }

    let content_budget = MAX_MCP_RESOURCE_MODEL_BYTES - MCP_RESOURCE_TRUNCATION_MARKER.len();
    if output.len() > content_budget {
        let mut boundary = content_budget;
        while !output.is_char_boundary(boundary) {
            boundary -= 1;
        }
        output.truncate(boundary);
    } else {
        let mut remaining = (content_budget - output.len()).min(piece.len());
        while !piece.is_char_boundary(remaining) {
            remaining -= 1;
        }
        output.push_str(&piece[..remaining]);
    }
    output.push_str(MCP_RESOURCE_TRUNCATION_MARKER);
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_mcp::client::McpClient;
    use allthecodes_mcp::{McpConnectionState, McpResource, McpServerConfig, ServerCapabilities};
    use serial_test::serial;
    use tokio::sync::Mutex;

    fn test_context() -> ToolUseContext {
        let app_state = ToolAppState::default();
        let (_tx, rx) = tokio::sync::watch::channel(false);
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
            get_app_state: Arc::new(move || app_state.clone()),
            set_app_state: Arc::new(|_| {}),
            session_id: "mcp-resource-test-session".to_string(),
            langfuse_session_id: "mcp-resource-test-session".to_string(),
            messages: vec![],
            agent_id: None,
            agent_type: None,
            query_tracking: None,
            permission_callback: None,
            ask_user_callback: None,
            permission_event_callback: None,
            bg_agent_tx: None,
            hook_runner: Arc::new(allthecodes_types::hooks::NoopHookRunner::new()),
            command_dispatcher: Arc::new(allthecodes_types::commands::NoopCommandDispatcher::new()),
            available_tools: vec![],
            execute_deferred_tool: None,
        }
    }

    fn parent_message() -> AssistantMessage {
        AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 0,
            role: "assistant".to_string(),
            content: Vec::new(),
            usage: None,
            stop_reason: None,
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        }
    }

    fn connected_client(name: &str, resources: Vec<McpResource>) -> McpClient {
        let mut client = McpClient::new(McpServerConfig {
            name: name.to_string(),
            transport: "stdio".to_string(),
            command: Some("dummy".to_string()),
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
        });
        client.state = McpConnectionState::Connected;
        client.resources = resources;
        client.server_capabilities = ServerCapabilities {
            resources: Some(json!({})),
            ..Default::default()
        };
        client
    }

    #[test]
    fn resource_tools_have_expected_static_contracts() {
        let tools = tools();
        let names = tools.iter().map(|tool| tool.name()).collect::<Vec<_>>();
        assert_eq!(names, vec!["ListMcpResources", "ReadMcpResource"]);
        assert!(tools[0].is_read_only(&json!({})));
        assert!(tools[0].is_concurrency_safe(&json!({})));
        assert!(tools[1].is_read_only(&json!({})));
        assert!(tools[1].is_concurrency_safe(&json!({})));
    }

    #[tokio::test]
    #[serial]
    async fn list_mcp_resources_uses_installed_runtime_manager() {
        allthecodes_mcp::runtime::clear_for_tests();
        let mut manager = allthecodes_mcp::manager::McpManager::new();
        manager.clients.insert(
            "demo".to_string(),
            connected_client(
                "demo",
                vec![McpResource {
                    uri: "file:///demo".to_string(),
                    name: "Demo".to_string(),
                    description: None,
                    mime_type: Some("text/plain".to_string()),
                }],
            ),
        );
        allthecodes_mcp::runtime::install_manager(Arc::new(Mutex::new(manager)));

        let result = ListMcpResourcesTool
            .call(json!({}), &test_context(), &parent_message(), None)
            .await
            .unwrap();
        assert_eq!(result.data[0]["server"], "demo");
        assert_eq!(result.data[0]["uri"], "file:///demo");
        allthecodes_mcp::runtime::clear_for_tests();
    }

    #[tokio::test]
    #[serial]
    async fn list_mcp_resources_errors_without_runtime_manager() {
        allthecodes_mcp::runtime::clear_for_tests();
        let err = ListMcpResourcesTool
            .call(json!({}), &test_context(), &parent_message(), None)
            .await
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("No MCP runtime manager is installed"));
    }

    #[test]
    fn format_resource_contents_omits_binary_base64() {
        let text = format_resource_contents(
            "demo",
            &[allthecodes_mcp::McpResourceContent {
                uri: "file:///image".to_string(),
                mime_type: Some("image/png".to_string()),
                text: None,
                blob: Some("AAAA".to_string()),
            }],
        );
        assert!(text.contains("binary content omitted"));
        assert!(!text.contains("AAAA"));
    }

    #[test]
    fn format_resource_contents_enforces_utf8_safe_total_budget() {
        let oversized = "界".repeat(MAX_MCP_RESOURCE_MODEL_BYTES);
        let contents = vec![
            allthecodes_mcp::McpResourceContent {
                uri: "file:///first".to_string(),
                mime_type: Some("text/plain".to_string()),
                text: Some(oversized),
                blob: None,
            },
            allthecodes_mcp::McpResourceContent {
                uri: "file:///second".to_string(),
                mime_type: Some("text/plain".to_string()),
                text: Some("must not bypass the total budget".to_string()),
                blob: None,
            },
        ];

        let formatted = format_resource_contents("demo", &contents);

        assert!(formatted.len() <= MAX_MCP_RESOURCE_MODEL_BYTES);
        assert!(formatted.contains("[MCP resource content truncated]"));
        assert!(std::str::from_utf8(formatted.as_bytes()).is_ok());
    }
}
