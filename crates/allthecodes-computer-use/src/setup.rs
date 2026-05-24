//! Computer Use setup — dynamic tool registration.
//!
//! When `--computer-use` is passed on the CLI, this module registers native
//! Computer Use tools into the tool vector. The tools use the
//! `mcp__computer-use__` naming prefix for seamless compatibility with
//! the existing detection, permission, and system prompt code.

use std::sync::Arc;

use allthecodes_engine::types::tool::Tool;

use super::tools;

/// Register all native Computer Use tools.
///
/// Called from `main.rs` when `--computer-use` is set.
/// Returns the tools to be appended to the main tools vector.
pub fn register_cu_tools() -> Vec<Arc<dyn Tool>> {
    tracing::info!("Registering native Computer Use tools");

    vec![
        Arc::new(tools::ScreenshotTool) as Arc<dyn Tool>,
        Arc::new(tools::ClickTool::left()),
        Arc::new(tools::ClickTool::right()),
        Arc::new(tools::ClickTool::middle()),
        Arc::new(tools::DoubleClickTool),
        Arc::new(tools::TypeTextTool),
        Arc::new(tools::KeyTool),
        Arc::new(tools::ScrollTool),
        Arc::new(tools::MouseMoveTool),
        Arc::new(tools::CursorPositionTool),
    ]
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_cu_tools_count() {
        let tools = register_cu_tools();
        assert_eq!(tools.len(), 10, "should register 10 CU tools");
    }

    #[test]
    fn test_all_tools_have_cu_prefix() {
        let tools = register_cu_tools();
        for tool in &tools {
            let name = tool.user_facing_name(None);
            assert!(
                tool.name()
                    .starts_with(crate::detection::COMPUTER_USE_PREFIX),
                "runtime tool name '{}' should have mcp__computer-use__ prefix",
                tool.name()
            );
            assert!(
                name.starts_with(crate::detection::COMPUTER_USE_PREFIX),
                "tool '{}' should have mcp__computer-use__ prefix, got '{}'",
                tool.name(),
                name
            );
        }
    }

    #[test]
    fn test_tool_names_unique() {
        let tools = register_cu_tools();
        let mut names: Vec<String> = tools.iter().map(|t| t.user_facing_name(None)).collect();
        let len_before = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), len_before, "all tool names should be unique");
    }

    #[test]
    fn test_screenshot_is_read_only() {
        let tools = register_cu_tools();
        let screenshot = tools
            .iter()
            .find(|t| t.name() == "mcp__computer-use__screenshot")
            .expect("screenshot tool");
        assert!(screenshot.is_read_only(&serde_json::Value::Null));
    }

    #[test]
    fn test_click_tools_have_schemas() {
        let tools = register_cu_tools();
        for tool in &tools {
            let schema = tool.input_json_schema();
            assert!(
                schema.get("type").is_some(),
                "tool '{}' should have a valid JSON schema",
                tool.name()
            );
        }
    }
}
