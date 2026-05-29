use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::types::message::{AssistantMessage, ContentBlock, ImageSource, ToolResultContent};
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

fn first_supported_tool(
    client: &allthecodes_mcp::client::McpClient,
    candidates: &[&str],
) -> Option<String> {
    candidates
        .iter()
        .find(|name| has_tool(client, name))
        .map(|name| (*name).to_string())
}

async fn call_browser_tool(
    manager: &tokio::sync::Mutex<allthecodes_mcp::manager::McpManager>,
    server_name: &str,
    tool_name: &str,
    arguments: Value,
) -> Result<allthecodes_mcp::CallToolResult> {
    let manager = manager.lock().await;
    let client = manager
        .clients
        .get(server_name)
        .ok_or_else(|| anyhow::anyhow!("browser MCP server '{}' disappeared", server_name))?;
    client.call_tool(tool_name, arguments).await
}

fn mcp_content_to_blocks(content: &[allthecodes_mcp::ToolCallContent]) -> Vec<ContentBlock> {
    content
        .iter()
        .filter_map(|item| match item {
            allthecodes_mcp::ToolCallContent::Text { text } => {
                Some(ContentBlock::Text { text: text.clone() })
            }
            allthecodes_mcp::ToolCallContent::Image { data, mime_type } => {
                Some(ContentBlock::Image {
                    source: ImageSource {
                        source_type: "base64".to_string(),
                        media_type: mime_type.clone(),
                        data: data.clone(),
                    },
                })
            }
            allthecodes_mcp::ToolCallContent::Resource { resource } => {
                if let Some(text) = &resource.text {
                    Some(ContentBlock::Text { text: text.clone() })
                } else {
                    resource
                        .blob
                        .as_ref()
                        .zip(resource.mime_type.as_deref())
                        .filter(|(_, mime)| mime.starts_with("image/"))
                        .map(|(data, mime)| ContentBlock::Image {
                            source: ImageSource {
                                source_type: "base64".to_string(),
                                media_type: mime.to_string(),
                                data: data.clone(),
                            },
                        })
                }
            }
        })
        .collect()
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

        let (server_name, screenshot_tool) = {
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
            let screenshot_tool =
                first_supported_tool(client, &["screenshot", "take_screenshot", "take_snapshot"]);
            (server_name, screenshot_tool)
        };

        let nav =
            call_browser_tool(&manager, &server_name, "navigate", json!({ "url": url })).await?;
        if nav.is_error {
            bail!("browser navigation failed: {}", content_text(&nav.content));
        }

        if wait > 0 {
            tokio::time::sleep(Duration::from_millis(wait)).await;
        }

        let text = if matches!(extract, ExtractMode::Text | ExtractMode::Both) {
            let result =
                call_browser_tool(&manager, &server_name, "get_page_text", json!({})).await?;
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

        let mut warnings = Vec::new();
        if matches!(extract, ExtractMode::Screenshot | ExtractMode::Both)
            && screenshot_tool.is_none()
        {
            warnings
                .push("Connected browser server does not expose screenshot capture.".to_string());
        }
        if matches!(extract, ExtractMode::Screenshot) && screenshot_tool.is_none() {
            bail!("screenshot extraction is unsupported by the connected browser server");
        }
        let screenshot_result = if matches!(extract, ExtractMode::Screenshot | ExtractMode::Both) {
            if let Some(tool_name) = screenshot_tool.as_deref() {
                let result =
                    call_browser_tool(&manager, &server_name, tool_name, json!({})).await?;
                if result.is_error {
                    bail!(
                        "browser screenshot failed: {}",
                        content_text(&result.content)
                    );
                }
                Some(result)
            } else {
                None
            }
        } else {
            None
        };

        let mut blocks = Vec::new();
        if !text.is_empty() {
            blocks.push(ContentBlock::Text { text: text.clone() });
        }
        if let Some(result) = &screenshot_result {
            blocks.extend(mcp_content_to_blocks(&result.content));
        }

        let preview = if text.is_empty() {
            format!("Opened {url} in browser")
        } else {
            text.chars().take(2_000).collect::<String>()
        };
        let model_content = if blocks.is_empty() {
            ToolResultContent::Text(preview.clone())
        } else {
            ToolResultContent::Blocks(blocks)
        };
        Ok(ToolResult {
            data: json!({
                "url": url,
                "server": server_name,
                "text": text,
                "screenshot_tool": screenshot_tool,
                "screenshot_available": screenshot_result.is_some(),
                "warnings": warnings,
            }),
            model_content: Some(model_content),
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

    #[test]
    fn mcp_content_to_blocks_preserves_screenshot_images() {
        let blocks = mcp_content_to_blocks(&[allthecodes_mcp::ToolCallContent::Image {
            data: "base64png".to_string(),
            mime_type: "image/png".to_string(),
        }]);

        match &blocks[0] {
            ContentBlock::Image { source } => {
                assert_eq!(source.source_type, "base64");
                assert_eq!(source.media_type, "image/png");
                assert_eq!(source.data, "base64png");
            }
            other => panic!("expected image block, got {other:?}"),
        }
    }
}
