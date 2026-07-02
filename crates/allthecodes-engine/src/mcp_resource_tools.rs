use std::sync::Arc;

use anyhow::{bail, Result};
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::types::message::{AssistantMessage, ToolResultContent};
use crate::types::tool::*;

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

    contents
        .iter()
        .map(|content| {
            if let Some(text) = &content.text {
                format!("[Resource from {}: {}]\n{}", server, content.uri, text)
            } else if content.blob.is_some() {
                let mime = content
                    .mime_type
                    .as_deref()
                    .unwrap_or("application/octet-stream");
                format!(
                    "[Resource from {}: {} ({}) binary content omitted]",
                    server, content.uri, mime
                )
            } else {
                format!("[Resource from {}: {}]", server, content.uri)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_mcp::client::McpClient;
    use allthecodes_mcp::{McpConnectionState, McpResource, McpServerConfig};
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
}
