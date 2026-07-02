//! MCP tool integration — wraps MCP tools as local Tool trait objects.
//!
//! Each MCP tool is wrapped in `McpToolWrapper` which delegates `call()` to
//! the real `McpClient` for JSON-RPC `tools/call` execution. The wrapper
//! holds a reference to the manager so it can find the right client at
//! call time.

use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::Mutex;
use tracing::debug;

use crate::types::config::AgentContext;
use crate::types::message::{AssistantMessage, ContentBlock, ImageSource, ToolResultContent};
use crate::types::tool::*;

use allthecodes_mcp::bindings::canonical_workspace_root;
use allthecodes_mcp::manager::McpManager;
use allthecodes_mcp::{McpBindingContext, McpPermission, McpToolDef, ToolCallContent};

const MCP_SKILL_URI_PREFIX: &str = "skill://";

// ---------------------------------------------------------------------------
// McpToolWrapper
// ---------------------------------------------------------------------------

/// Wraps an MCP tool definition as a local `Tool` trait object.
///
/// When `call()` is invoked, the wrapper looks up the owning `McpClient`
/// via the shared `McpManager` and sends a `tools/call` JSON-RPC request.
pub struct McpToolWrapper {
    /// The tool definition from the MCP server.
    pub def: McpToolDef,
    /// Model-facing tool name, namespaced to avoid collisions across servers.
    pub exposed_name: String,
    /// Server name that provides this tool.
    pub server_name: String,
    /// Shared manager for accessing the MCP client at call time.
    pub manager: Arc<Mutex<McpManager>>,
    /// Runtime scope used for visibility and call permission checks.
    pub binding_context: McpBindingContext,
}

#[async_trait]
impl Tool for McpToolWrapper {
    fn name(&self) -> &str {
        &self.exposed_name
    }

    async fn description(&self, _input: &Value) -> String {
        self.def.description.clone()
    }

    fn input_json_schema(&self) -> Value {
        self.def.input_schema.clone()
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        // MCP tools share a single stdin/stdout channel per server,
        // but the client handles multiplexing via request IDs.
        true
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        false // We don't know — assume MCP tools may have side effects
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        debug!(
            server = %self.server_name,
            tool = %self.exposed_name,
            mcp_tool = %self.def.name,
            "MCP tool call"
        );

        let manager = self.manager.lock().await;
        if !manager.can_call_tool(&self.binding_context, &self.server_name, &self.def.name) {
            anyhow::bail!(
                "{}",
                manager.permission_denied_message(
                    &self.binding_context,
                    &self.server_name,
                    McpPermission::CallTools,
                )
            );
        }

        let client = manager.clients.get(&self.server_name).ok_or_else(|| {
            anyhow::anyhow!(
                "MCP server '{}' not connected (tool '{}')",
                self.server_name,
                self.def.name
            )
        })?;

        let result = client.call_tool(&self.def.name, input).await?;

        // Build display-only text (for UI/logs — images become "[Image: mime]")
        let display_text = format_tool_call_result(&result.content, result.is_error);

        if result.is_error {
            Ok(ToolResult {
                data: json!(format!("MCP tool error: {}", display_text)),
                new_messages: vec![],
                ..Default::default()
            })
        } else {
            let has_multimodal = result
                .content
                .iter()
                .any(|b| matches!(b, ToolCallContent::Image { .. }));

            // Browser MCP tools get an enriched display preview
            // ("[navigation] navigated → https://…", etc.) without touching the
            // full content that the model sees.
            let browser_preview = browser_display_preview(
                &self.server_name,
                &self.def.name,
                &result.content,
                has_multimodal,
            );

            if has_multimodal {
                let model_blocks = convert_mcp_to_content_blocks(&result.content);
                let preview = browser_preview
                    .clone()
                    .unwrap_or_else(|| display_text.clone());
                Ok(ToolResult::with_content(
                    json!(display_text),
                    ToolResultContent::Blocks(model_blocks),
                    preview,
                ))
            } else {
                Ok(ToolResult {
                    data: json!(display_text.clone()),
                    display_preview: browser_preview,
                    new_messages: vec![],
                    ..Default::default()
                })
            }
        }
    }

    async fn prompt(&self) -> String {
        format!(
            "MCP tool '{}' from server '{}': {}",
            self.def.name, self.server_name, self.def.description
        )
    }

    fn user_facing_name(&self, _input: Option<&Value>) -> String {
        self.exposed_name.clone()
    }

    fn mcp_server_name(&self) -> Option<&str> {
        Some(&self.server_name)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a compact display preview for a browser MCP tool result.
///
/// Returns `None` when the tool isn't classified as browser-shaped, letting
/// the caller fall back to the generic tool display. For browser tools, emits
/// a prefix-tagged preview such as `[navigation] navigated → https://…` or
/// `[screenshot] screenshot (12KB text + image)` that frontends can use as the
/// collapsed-view label without re-parsing the full payload.
fn browser_display_preview(
    server_name: &str,
    tool_basename: &str,
    content: &[ToolCallContent],
    has_image: bool,
) -> Option<String> {
    use allthecodes_browser::detection::{is_browser_server, BROWSER_TOOL_BASENAMES};
    use allthecodes_browser::tool_rendering::{infer_kind, short_summary};

    let is_known_action = BROWSER_TOOL_BASENAMES.contains(&tool_basename);
    if !is_known_action && !is_browser_server(server_name) {
        return None;
    }

    // Flatten the text parts of the result so short_summary can inspect it.
    let text: String = content
        .iter()
        .filter_map(|b| match b {
            ToolCallContent::Text { text } => Some(text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");

    let kind = infer_kind(tool_basename, has_image);
    let summary = short_summary(tool_basename, &text, has_image);
    Some(format!("[{}] {}", kind.label(), summary))
}

/// Convert MCP tool call content blocks to Anthropic API ContentBlocks.
///
/// Text blocks become `ContentBlock::Text`, image blocks become `ContentBlock::Image`
/// with base64 source. Resource blocks with text become `ContentBlock::Text`.
fn convert_mcp_to_content_blocks(content: &[ToolCallContent]) -> Vec<ContentBlock> {
    let mut blocks = Vec::new();

    for item in content {
        match item {
            ToolCallContent::Text { text } => {
                blocks.push(ContentBlock::Text { text: text.clone() });
            }
            ToolCallContent::Image { data, mime_type } => {
                blocks.push(ContentBlock::Image {
                    source: ImageSource {
                        source_type: "base64".to_string(),
                        media_type: mime_type.clone(),
                        data: data.clone(),
                    },
                });
            }
            ToolCallContent::Resource { resource } => {
                // Resources with text content become text blocks
                if let Some(ref text) = resource.text {
                    blocks.push(ContentBlock::Text {
                        text: format!("[Resource: {}]\n{}", resource.uri, text),
                    });
                } else if let Some(ref blob) = resource.blob {
                    // Binary resources with a recognized image MIME type become image blocks
                    let mime = resource
                        .mime_type
                        .as_deref()
                        .unwrap_or("application/octet-stream");
                    if mime.starts_with("image/") {
                        blocks.push(ContentBlock::Image {
                            source: ImageSource {
                                source_type: "base64".to_string(),
                                media_type: mime.to_string(),
                                data: blob.clone(),
                            },
                        });
                    } else {
                        blocks.push(ContentBlock::Text {
                            text: format!("[Resource: {} (binary)]", resource.uri),
                        });
                    }
                } else {
                    blocks.push(ContentBlock::Text {
                        text: format!("[Resource: {}]", resource.uri),
                    });
                }
            }
        }
    }

    blocks
}

/// Format MCP tool call content blocks into a single string.
fn format_tool_call_result(content: &[ToolCallContent], is_error: bool) -> String {
    let mut parts: Vec<String> = Vec::new();

    for block in content {
        match block {
            ToolCallContent::Text { text } => {
                parts.push(text.clone());
            }
            ToolCallContent::Image { mime_type, .. } => {
                parts.push(format!("[Image: {}]", mime_type));
            }
            ToolCallContent::Resource { resource } => {
                if let Some(ref text) = resource.text {
                    parts.push(format!("[Resource: {}]\n{}", resource.uri, text));
                } else if resource.blob.is_some() {
                    parts.push(format!("[Resource: {} (binary)]", resource.uri));
                } else {
                    parts.push(format!("[Resource: {}]", resource.uri));
                }
            }
        }
    }

    if parts.is_empty() {
        if is_error {
            "(MCP tool returned error with no content)".to_string()
        } else {
            "(MCP tool returned no content)".to_string()
        }
    } else {
        parts.join("\n")
    }
}

/// Convert all MCP tool definitions into `Tool` trait objects.
///
/// Each tool gets a shared reference to the `McpManager` so it can
/// delegate `call()` at runtime.
pub fn mcp_tools_to_tools(
    defs: Vec<McpToolDef>,
    manager: Arc<Mutex<McpManager>>,
) -> Vec<Arc<dyn Tool>> {
    mcp_tools_to_tools_for_context(defs, manager, McpBindingContext::startup(None))
}

pub fn mcp_tools_to_tools_for_context(
    defs: Vec<McpToolDef>,
    manager: Arc<Mutex<McpManager>>,
    binding_context: McpBindingContext,
) -> Vec<Arc<dyn Tool>> {
    defs.into_iter()
        .map(|def| {
            let server_name = def.server_name.clone();
            let exposed_name = mcp_exposed_tool_name(&server_name, &def.name);
            Arc::new(McpToolWrapper {
                def,
                exposed_name,
                server_name,
                manager: manager.clone(),
                binding_context: binding_context.clone(),
            }) as Arc<dyn Tool>
        })
        .collect()
}

fn mcp_exposed_tool_name(server_name: &str, tool_name: &str) -> String {
    format!("mcp__{server_name}__{tool_name}")
}

/// Discover skills exposed by connected MCP servers as `skill://` resources.
///
/// This mirrors the upstream MCP skill-builder bridge at a Rust boundary:
/// MCP remains responsible for listing and reading resources, while the
/// `cc-skills` loader owns frontmatter parsing and package validation.
pub async fn discover_mcp_skill_resources(
    manager: &McpManager,
) -> (
    Vec<crate::skills::SkillDefinition>,
    Vec<crate::skills::SkillDiagnostic>,
) {
    discover_mcp_skill_resources_for_context(manager, &McpBindingContext::startup(None)).await
}

pub async fn discover_mcp_skill_resources_for_context(
    manager: &McpManager,
    binding_context: &McpBindingContext,
) -> (
    Vec<crate::skills::SkillDefinition>,
    Vec<crate::skills::SkillDiagnostic>,
) {
    let mut skills = Vec::new();
    let mut diagnostics = Vec::new();

    let resources = match manager.list_resources_for_context(binding_context, None) {
        Ok(resources) => resources,
        Err(err) => {
            diagnostics.push(crate::skills::SkillDiagnostic::warning(
                "mcp-skill-list-failed",
                format!("Failed to list MCP skill resources: {err}"),
            ));
            return (skills, diagnostics);
        }
    };

    for resource in resources
        .into_iter()
        .filter(|resource| resource.uri.starts_with(MCP_SKILL_URI_PREFIX))
    {
        let server_name = resource.server.clone();
        let raw_name = resource.uri.trim_start_matches(MCP_SKILL_URI_PREFIX);
        let skill_name = format!(
            "mcp__{}__{}",
            normalize_mcp_skill_component(&server_name),
            normalize_mcp_skill_component(raw_name)
        );

        let read = match manager
            .read_resource_for_context(binding_context, &server_name, &resource.uri)
            .await
        {
            Ok(read) => read,
            Err(err) => {
                diagnostics.push(
                    crate::skills::SkillDiagnostic::warning(
                        "mcp-skill-read-failed",
                        format!(
                            "Failed to read MCP skill resource '{}' from '{}': {}",
                            resource.uri, server_name, err
                        ),
                    )
                    .with_skill(skill_name)
                    .with_source(crate::skills::SkillSource::Mcp(server_name)),
                );
                continue;
            }
        };

        let text = read
            .contents
            .iter()
            .filter_map(|content| content.text.as_deref())
            .collect::<Vec<_>>()
            .join("\n");

        if text.trim().is_empty() {
            diagnostics.push(
                crate::skills::SkillDiagnostic::warning(
                    "mcp-skill-empty",
                    format!(
                        "MCP skill resource '{}' from '{}' returned no text content.",
                        resource.uri, server_name
                    ),
                )
                .with_skill(skill_name)
                .with_source(crate::skills::SkillSource::Mcp(server_name)),
            );
            continue;
        }

        let (skill, parse_diagnostics) = crate::skills::loader::load_skill_from_content(
            &text,
            &skill_name,
            crate::skills::SkillSource::Mcp(server_name),
        );
        skills.push(skill);
        diagnostics.extend(parse_diagnostics);
    }

    (skills, diagnostics)
}

pub fn mcp_binding_context_for_engine(
    cwd: &str,
    session_id: &str,
    agent_context: Option<&AgentContext>,
) -> McpBindingContext {
    let project_path = Some(canonical_workspace_root(Path::new(cwd)));
    if let Some(agent_context) = agent_context {
        McpBindingContext::agent(
            project_path,
            session_id.to_string(),
            agent_context.agent_id.clone(),
        )
    } else {
        McpBindingContext::main(project_path, session_id.to_string())
    }
}

pub fn mcp_binding_context_for_tool_use(ctx: &ToolUseContext) -> McpBindingContext {
    let project_path = std::env::current_dir()
        .ok()
        .map(|cwd| canonical_workspace_root(&cwd));
    if let Some(agent_id) = ctx.agent_id.as_ref() {
        McpBindingContext::agent(project_path, ctx.session_id.clone(), agent_id.clone())
    } else {
        McpBindingContext::main(project_path, ctx.session_id.clone())
    }
}

fn normalize_mcp_skill_component(value: &str) -> String {
    let mut out = String::new();
    let trimmed = value.trim_matches(['/', '\\', ' ', '\t', '\r', '\n']);

    for ch in trimmed.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | ':' | '.') {
            out.push(ch);
        } else {
            out.push('_');
        }
    }

    while out.contains("__") {
        out = out.replace("__", "_");
    }

    let out = out.trim_matches('_').to_string();
    if out.is_empty() {
        "skill".to_string()
    } else {
        out
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_tool_call_result_text() {
        let content = vec![ToolCallContent::Text {
            text: "hello world".to_string(),
        }];
        let result = format_tool_call_result(&content, false);
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_format_tool_call_result_multiple_blocks() {
        let content = vec![
            ToolCallContent::Text {
                text: "line 1".to_string(),
            },
            ToolCallContent::Text {
                text: "line 2".to_string(),
            },
        ];
        let result = format_tool_call_result(&content, false);
        assert_eq!(result, "line 1\nline 2");
    }

    #[test]
    fn test_format_tool_call_result_image() {
        let content = vec![ToolCallContent::Image {
            data: "base64data".to_string(),
            mime_type: "image/png".to_string(),
        }];
        let result = format_tool_call_result(&content, false);
        assert_eq!(result, "[Image: image/png]");
    }

    #[test]
    fn test_format_tool_call_result_resource_text() {
        let content = vec![ToolCallContent::Resource {
            resource: allthecodes_mcp::McpResourceContent {
                uri: "file:///tmp/test.txt".to_string(),
                mime_type: Some("text/plain".to_string()),
                text: Some("file contents".to_string()),
                blob: None,
            },
        }];
        let result = format_tool_call_result(&content, false);
        assert!(result.contains("file:///tmp/test.txt"));
        assert!(result.contains("file contents"));
    }

    #[test]
    fn test_format_tool_call_result_empty() {
        let result = format_tool_call_result(&[], false);
        assert_eq!(result, "(MCP tool returned no content)");

        let result_err = format_tool_call_result(&[], true);
        assert!(result_err.contains("error"));
    }

    #[test]
    fn test_mcp_tool_wrapper_user_facing_name() {
        let manager = Arc::new(Mutex::new(McpManager::new()));
        let wrapper = McpToolWrapper {
            def: McpToolDef {
                name: "read_file".to_string(),
                description: "Read a file".to_string(),
                input_schema: json!({}),
                server_name: "filesystem".to_string(),
            },
            exposed_name: "mcp__filesystem__read_file".to_string(),
            server_name: "filesystem".to_string(),
            manager,
            binding_context: McpBindingContext::startup(None),
        };

        assert_eq!(wrapper.name(), "mcp__filesystem__read_file");
        assert_eq!(wrapper.user_facing_name(None), "mcp__filesystem__read_file");
        assert_eq!(wrapper.def.name, "read_file");
    }

    #[test]
    fn test_mcp_tools_to_tools() {
        let manager = Arc::new(Mutex::new(McpManager::new()));
        let defs = vec![
            McpToolDef {
                name: "tool_a".to_string(),
                description: "Tool A".to_string(),
                input_schema: json!({"type": "object"}),
                server_name: "server1".to_string(),
            },
            McpToolDef {
                name: "tool_b".to_string(),
                description: "Tool B".to_string(),
                input_schema: json!({"type": "object"}),
                server_name: "server1".to_string(),
            },
        ];

        let tools = mcp_tools_to_tools(defs, manager);
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name(), "mcp__server1__tool_a");
        assert_eq!(tools[1].name(), "mcp__server1__tool_b");
    }

    #[test]
    fn test_normalize_mcp_skill_component() {
        assert_eq!(normalize_mcp_skill_component("filesystem"), "filesystem");
        assert_eq!(normalize_mcp_skill_component("/review/code"), "review_code");
        assert_eq!(normalize_mcp_skill_component(""), "skill");
    }

    #[test]
    fn test_convert_mcp_image_to_content_block() {
        let content = vec![ToolCallContent::Image {
            data: "iVBORw0KGgo=".to_string(),
            mime_type: "image/png".to_string(),
        }];
        let blocks = convert_mcp_to_content_blocks(&content);
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            ContentBlock::Image { source } => {
                assert_eq!(source.source_type, "base64");
                assert_eq!(source.media_type, "image/png");
                assert_eq!(source.data, "iVBORw0KGgo=");
            }
            other => panic!("expected Image block, got {:?}", other),
        }
    }

    #[test]
    fn test_convert_mcp_mixed_text_and_image() {
        let content = vec![
            ToolCallContent::Text {
                text: "Screenshot taken".to_string(),
            },
            ToolCallContent::Image {
                data: "base64png".to_string(),
                mime_type: "image/png".to_string(),
            },
        ];
        let blocks = convert_mcp_to_content_blocks(&content);
        assert_eq!(blocks.len(), 2);
        assert!(matches!(&blocks[0], ContentBlock::Text { text } if text == "Screenshot taken"));
        assert!(matches!(&blocks[1], ContentBlock::Image { .. }));
    }

    #[test]
    fn test_convert_mcp_resource_blob_image() {
        let content = vec![ToolCallContent::Resource {
            resource: allthecodes_mcp::McpResourceContent {
                uri: "screenshot://latest".to_string(),
                mime_type: Some("image/jpeg".to_string()),
                text: None,
                blob: Some("base64jpeg".to_string()),
            },
        }];
        let blocks = convert_mcp_to_content_blocks(&content);
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            ContentBlock::Image { source } => {
                assert_eq!(source.media_type, "image/jpeg");
                assert_eq!(source.data, "base64jpeg");
            }
            other => panic!("expected Image block, got {:?}", other),
        }
    }
}
