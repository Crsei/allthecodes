use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::types::message::{AssistantMessage, ToolResultContent};
use crate::types::tool::*;

pub fn tools() -> Tools {
    vec![Arc::new(WebBrowserTool)]
}

pub struct WebBrowserTool;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExtractMode {
    Text,
    Screenshot,
    Both,
}

impl ExtractMode {
    fn parse(raw: Option<&str>) -> Result<Self> {
        match raw.unwrap_or("text") {
            "text" => Ok(Self::Text),
            "screenshot" => Ok(Self::Screenshot),
            "both" => Ok(Self::Both),
            other => bail!("extract must be one of text, screenshot, both; got '{other}'"),
        }
    }
}

fn validate_url(raw: &str) -> Result<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        bail!("url is required");
    }
    let parsed = url::Url::parse(raw).context("invalid URL")?;
    if !matches!(parsed.scheme(), "http" | "https") {
        bail!("url must use http or https");
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        bail!("url must not contain embedded credentials");
    }
    Ok(parsed.to_string())
}

fn wait_ms(input: &Value) -> Result<u64> {
    let value = input.get("wait_ms").and_then(Value::as_u64).unwrap_or(1500);
    if value > 10_000 {
        bail!("wait_ms must be <= 10000");
    }
    Ok(value)
}

fn content_text(contents: &[allthecodes_mcp::ToolCallContent]) -> String {
    contents
        .iter()
        .filter_map(|content| match content {
            allthecodes_mcp::ToolCallContent::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn has_tool(client: &allthecodes_mcp::client::McpClient, name: &str) -> bool {
    client.tools.iter().any(|tool| tool.name == name)
}

fn browser_server_name(manager: &allthecodes_mcp::manager::McpManager) -> Option<String> {
    let configured = allthecodes_browser::detection::browser_servers_snapshot();
    let mut candidates = manager.clients.keys().cloned().collect::<Vec<_>>();
    candidates.sort();
    candidates.into_iter().find(|server| {
        configured.contains(server)
            || manager.clients.get(server).is_some_and(|client| {
                has_tool(client, "navigate") && has_tool(client, "get_page_text")
            })
    })
}

#[async_trait]
impl Tool for WebBrowserTool {
    fn name(&self) -> &str {
        "WebBrowser"
    }

    async fn description(&self, _input: &Value) -> String {
        "Open a URL in the connected browser and return JS-rendered page content.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {"type": "string", "description": "HTTP/HTTPS URL to open in the connected browser."},
                "wait_ms": {"type": "integer", "minimum": 0, "maximum": 10000, "default": 1500},
                "extract": {
                    "type": "string",
                    "enum": ["text", "screenshot", "both"],
                    "default": "text"
                }
            },
            "required": ["url"]
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        false
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        let result = (|| -> Result<()> {
            validate_url(input.get("url").and_then(Value::as_str).unwrap_or(""))?;
            wait_ms(input)?;
            ExtractMode::parse(input.get("extract").and_then(Value::as_str))?;
            Ok(())
        })();
        match result {
            Ok(()) => ValidationResult::Ok,
            Err(err) => ValidationResult::Error {
                message: err.to_string(),
                error_code: 400,
            },
        }
    }

    async fn check_permissions(&self, input: &Value, _ctx: &ToolUseContext) -> PermissionResult {
        let url = input.get("url").and_then(Value::as_str).unwrap_or("");
        PermissionResult::Ask {
            message: format!("Allow opening and reading this page in the browser? {url}"),
        }
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
        _parent_message: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let url = validate_url(input.get("url").and_then(Value::as_str).unwrap_or(""))?;
        let wait = wait_ms(&input)?;
        let extract = ExtractMode::parse(input.get("extract").and_then(Value::as_str))?;
        let Some(manager) = allthecodes_mcp::runtime::current_manager() else {
            bail!("No MCP runtime manager is installed for this session");
        };

        let mut warnings = Vec::new();
        let (server_name, text, screenshot_supported) = {
            let manager = manager.lock().await;
            let server_name = browser_server_name(&manager).ok_or_else(|| {
                anyhow::anyhow!(
                    "No connected browser MCP server found. Enable the browser integration and connect Chrome before using WebBrowser."
                )
            })?;
            let client = manager.clients.get(&server_name).ok_or_else(|| {
                anyhow::anyhow!("browser MCP server '{}' disappeared", server_name)
            })?;
            if !has_tool(client, "navigate") {
                bail!(
                    "browser MCP server '{}' does not expose navigate",
                    server_name
                );
            }
            if !has_tool(client, "get_page_text") {
                bail!(
                    "browser MCP server '{}' does not expose get_page_text",
                    server_name
                );
            }

            let nav = client.call_tool("navigate", json!({ "url": url })).await?;
            if nav.is_error {
                bail!("browser navigation failed: {}", content_text(&nav.content));
            }

            if wait > 0 {
                tokio::time::sleep(Duration::from_millis(wait)).await;
            }

            let text = if matches!(extract, ExtractMode::Text | ExtractMode::Both) {
                let result = client.call_tool("get_page_text", json!({})).await?;
                if result.is_error {
                    bail!(
                        "browser text extraction failed: {}",
                        content_text(&result.content)
                    );
                }
                content_text(&result.content)
            } else {
                String::new()
            };
            let screenshot_supported = has_tool(client, "screenshot")
                || has_tool(client, "take_screenshot")
                || has_tool(client, "take_snapshot");
            (server_name, text, screenshot_supported)
        };

        if matches!(extract, ExtractMode::Screenshot | ExtractMode::Both) && !screenshot_supported {
            warnings
                .push("Connected browser server does not expose screenshot capture.".to_string());
        }
        if matches!(extract, ExtractMode::Screenshot) && !screenshot_supported {
            bail!("screenshot extraction is unsupported by the connected browser server");
        }

        let preview = if text.is_empty() {
            format!("Opened {url} in browser")
        } else {
            text.chars().take(2_000).collect::<String>()
        };
        Ok(ToolResult {
            data: json!({
                "url": url,
                "server": server_name,
                "text": text,
                "screenshot_supported": screenshot_supported,
                "warnings": warnings,
            }),
            model_content: Some(ToolResultContent::Text(preview.clone())),
            display_preview: Some(preview),
            new_messages: vec![],
        })
    }

    async fn prompt(&self) -> String {
        "Use WebBrowser for pages that require JavaScript rendering. Use WebFetch for ordinary static HTTP content.".to_string()
    }

    fn user_facing_name(&self, _input: Option<&Value>) -> String {
        "WebBrowser".to_string()
    }

    fn to_auto_classifier_input(&self, input: &Value) -> Value {
        Value::String(
            input
                .get("url")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_extract_modes() {
        assert_eq!(ExtractMode::parse(None).unwrap(), ExtractMode::Text);
        assert_eq!(
            ExtractMode::parse(Some("screenshot")).unwrap(),
            ExtractMode::Screenshot
        );
        assert!(ExtractMode::parse(Some("bad")).is_err());
    }

    #[test]
    fn rejects_credentialed_url() {
        assert!(validate_url("https://user:pass@example.com").is_err());
    }
}
