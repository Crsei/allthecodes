//! Native Computer Use tool implementations.
//!
//! Each tool uses the `mcp__computer-use__` naming prefix for compatibility
//! with the existing CU detection, permission, and system prompt code.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};

use allthecodes_engine::types::tool::{
    PermissionResult, Tool, ToolProgress, ToolResult, ToolUseContext, ValidationResult,
};
use allthecodes_types::message::{AssistantMessage, ContentBlock, ImageSource, ToolResultContent};

use crate::executor::Executor;
use crate::input::MouseButton;

use super::detection::{classify_risk, extract_cu_action, CuRiskLevel};

// ---------------------------------------------------------------------------
// Shared permission check for all CU tools
// ---------------------------------------------------------------------------

/// Check permissions for a Computer Use tool.
///
/// Order:
///   1. Session-level grants → Allow (no re-prompt within a session)
///   2. Persistent allow rules → Allow
///   3. Otherwise → Ask with a descriptive CU message
fn cu_check_permissions(
    cu_tool_name: &str,
    input: &Value,
    ctx: &ToolUseContext,
) -> PermissionResult {
    let app_state = (ctx.get_app_state)();
    let perm_ctx = &app_state.tool_permission_context;

    // 1. Session-level grant
    if perm_ctx.has_session_grant(cu_tool_name) {
        return PermissionResult::Allow {
            updated_input: input.clone(),
        };
    }

    // 2. Persistent allow rules (any source)
    for rules in perm_ctx.always_allow_rules.values() {
        if rules.iter().any(|r| r == cu_tool_name) {
            return PermissionResult::Allow {
                updated_input: input.clone(),
            };
        }
    }

    // 3. Ask — build a descriptive message
    let action = extract_cu_action(cu_tool_name).unwrap_or(cu_tool_name);
    let risk = classify_risk(action);
    let risk_tag = match risk {
        CuRiskLevel::Medium => "[medium risk]",
        CuRiskLevel::High => "[HIGH RISK]",
    };

    let description = match action {
        "screenshot" => "read the screen (take a screenshot)",
        "cursor_position" => "read the current cursor position",
        "left_click" => "click the left mouse button on your screen",
        "right_click" => "click the right mouse button on your screen",
        "middle_click" => "click the middle mouse button on your screen",
        "double_click" => "double-click the mouse on your screen",
        "type_text" | "type" => "type text using the keyboard",
        "key" => "press a keyboard shortcut",
        "scroll" => "scroll the mouse wheel",
        "mouse_move" => "move the mouse cursor",
        _ => "perform a desktop control action",
    };

    PermissionResult::Ask {
        message: format!("Allow {} {}?", description, risk_tag),
    }
}

// ---------------------------------------------------------------------------
// Screenshot tool
// ---------------------------------------------------------------------------

pub struct ScreenshotTool;

#[async_trait]
impl Tool for ScreenshotTool {
    fn name(&self) -> &str {
        "mcp__computer-use__screenshot"
    }

    async fn description(&self, _input: &Value) -> String {
        "Take a screenshot of the entire screen.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    async fn check_permissions(&self, input: &Value, ctx: &ToolUseContext) -> PermissionResult {
        cu_check_permissions("mcp__computer-use__screenshot", input, ctx)
    }

    async fn call(
        &self,
        _input: Value,
        _ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let result = Executor::new().screenshot().await?;
        let preview = format!(
            "Screenshot captured ({}x{} pixels)",
            result.width, result.height
        );
        let image_block = ContentBlock::Image {
            source: ImageSource {
                source_type: "base64".to_string(),
                media_type: "image/png".to_string(),
                data: result.base64_png,
            },
        };

        Ok(ToolResult::with_content(
            json!({ "width": result.width, "height": result.height }),
            ToolResultContent::Blocks(vec![image_block]),
            preview,
        ))
    }

    async fn prompt(&self) -> String {
        "Take a screenshot to see what is currently displayed on the screen.".to_string()
    }

    fn user_facing_name(&self, _input: Option<&Value>) -> String {
        "mcp__computer-use__screenshot".to_string()
    }
}

// ---------------------------------------------------------------------------
// Click tools
// ---------------------------------------------------------------------------

pub struct ClickTool {
    button: MouseButton,
    tool_name: &'static str,
    display_name: &'static str,
}

impl ClickTool {
    pub fn left() -> Self {
        Self {
            button: MouseButton::Left,
            tool_name: "mcp__computer-use__left_click",
            display_name: "left click",
        }
    }
    pub fn right() -> Self {
        Self {
            button: MouseButton::Right,
            tool_name: "mcp__computer-use__right_click",
            display_name: "right click",
        }
    }
    pub fn middle() -> Self {
        Self {
            button: MouseButton::Middle,
            tool_name: "mcp__computer-use__middle_click",
            display_name: "middle click",
        }
    }
}

#[async_trait]
impl Tool for ClickTool {
    fn name(&self) -> &str {
        self.tool_name
    }

    async fn description(&self, _input: &Value) -> String {
        format!(
            "Perform a {} at the specified screen coordinates.",
            self.display_name
        )
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "x": { "type": "integer", "description": "X coordinate on screen" },
                "y": { "type": "integer", "description": "Y coordinate on screen" }
            },
            "required": ["x", "y"]
        })
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        if input.get("x").and_then(|v| v.as_i64()).is_none()
            || input.get("y").and_then(|v| v.as_i64()).is_none()
        {
            return ValidationResult::Error {
                message: "x and y coordinates are required".to_string(),
                error_code: 400,
            };
        }
        ValidationResult::Ok
    }

    async fn check_permissions(&self, input: &Value, ctx: &ToolUseContext) -> PermissionResult {
        cu_check_permissions(&self.user_facing_name(None), input, ctx)
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let x = input["x"].as_i64().unwrap_or(0) as i32;
        let y = input["y"].as_i64().unwrap_or(0) as i32;

        let result = Executor::new().click(x, y, self.button).await?;

        Ok(ToolResult {
            data: json!(result),
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        format!("Perform a {} at screen coordinates.", self.display_name)
    }

    fn user_facing_name(&self, _input: Option<&Value>) -> String {
        self.tool_name.to_string()
    }
}

// ---------------------------------------------------------------------------
// Double-click tool
// ---------------------------------------------------------------------------

pub struct DoubleClickTool;

#[async_trait]
impl Tool for DoubleClickTool {
    fn name(&self) -> &str {
        "mcp__computer-use__double_click"
    }

    async fn description(&self, _input: &Value) -> String {
        "Perform a double-click at the specified screen coordinates.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "x": { "type": "integer", "description": "X coordinate on screen" },
                "y": { "type": "integer", "description": "Y coordinate on screen" }
            },
            "required": ["x", "y"]
        })
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        if input.get("x").and_then(|v| v.as_i64()).is_none()
            || input.get("y").and_then(|v| v.as_i64()).is_none()
        {
            return ValidationResult::Error {
                message: "x and y coordinates are required".to_string(),
                error_code: 400,
            };
        }
        ValidationResult::Ok
    }

    async fn check_permissions(&self, input: &Value, ctx: &ToolUseContext) -> PermissionResult {
        cu_check_permissions("mcp__computer-use__double_click", input, ctx)
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let x = input["x"].as_i64().unwrap_or(0) as i32;
        let y = input["y"].as_i64().unwrap_or(0) as i32;

        let result = Executor::new().double_click(x, y).await?;

        Ok(ToolResult {
            data: json!(result),
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "Perform a double-click at screen coordinates.".to_string()
    }

    fn user_facing_name(&self, _input: Option<&Value>) -> String {
        "mcp__computer-use__double_click".to_string()
    }
}

// ---------------------------------------------------------------------------
// Type text tool
// ---------------------------------------------------------------------------

pub struct TypeTextTool;

#[async_trait]
impl Tool for TypeTextTool {
    fn name(&self) -> &str {
        "mcp__computer-use__type_text"
    }

    async fn description(&self, _input: &Value) -> String {
        "Type text using the keyboard.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "text": { "type": "string", "description": "The text to type" }
            },
            "required": ["text"]
        })
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        if input.get("text").and_then(|v| v.as_str()).is_none() {
            return ValidationResult::Error {
                message: "text is required".to_string(),
                error_code: 400,
            };
        }
        ValidationResult::Ok
    }

    async fn check_permissions(&self, input: &Value, ctx: &ToolUseContext) -> PermissionResult {
        cu_check_permissions("mcp__computer-use__type_text", input, ctx)
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let text = input["text"].as_str().unwrap_or("");
        let result = Executor::new().type_text(text).await?;
        Ok(ToolResult {
            data: json!(result),
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "Type text using the keyboard. The text will be typed character by character.".to_string()
    }

    fn user_facing_name(&self, _input: Option<&Value>) -> String {
        "mcp__computer-use__type_text".to_string()
    }
}

// ---------------------------------------------------------------------------
// Key press tool
// ---------------------------------------------------------------------------

pub struct KeyTool;

#[async_trait]
impl Tool for KeyTool {
    fn name(&self) -> &str {
        "mcp__computer-use__key"
    }

    async fn description(&self, _input: &Value) -> String {
        "Press a key or key combination (e.g. 'Return', 'ctrl+c', 'alt+tab').".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "key": {
                    "type": "string",
                    "description": "Key or key combination to press (e.g. 'Return', 'ctrl+c', 'alt+tab')"
                }
            },
            "required": ["key"]
        })
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        if input.get("key").and_then(|v| v.as_str()).is_none() {
            return ValidationResult::Error {
                message: "key is required".to_string(),
                error_code: 400,
            };
        }
        ValidationResult::Ok
    }

    async fn check_permissions(&self, input: &Value, ctx: &ToolUseContext) -> PermissionResult {
        cu_check_permissions("mcp__computer-use__key", input, ctx)
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let key = input["key"].as_str().unwrap_or("");
        let result = Executor::new().key_press(key).await?;
        Ok(ToolResult {
            data: json!(result),
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "Press a keyboard shortcut or key. Use '+' for combinations (e.g. 'ctrl+c').".to_string()
    }

    fn user_facing_name(&self, _input: Option<&Value>) -> String {
        "mcp__computer-use__key".to_string()
    }
}

// ---------------------------------------------------------------------------
// Scroll tool
// ---------------------------------------------------------------------------

pub struct ScrollTool;

#[async_trait]
impl Tool for ScrollTool {
    fn name(&self) -> &str {
        "mcp__computer-use__scroll"
    }

    async fn description(&self, _input: &Value) -> String {
        "Scroll the mouse wheel at specified coordinates.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "x": { "type": "integer", "description": "X coordinate on screen" },
                "y": { "type": "integer", "description": "Y coordinate on screen" },
                "amount": {
                    "type": "integer",
                    "description": "Scroll amount: positive = down, negative = up"
                }
            },
            "required": ["x", "y", "amount"]
        })
    }

    async fn check_permissions(&self, input: &Value, ctx: &ToolUseContext) -> PermissionResult {
        cu_check_permissions("mcp__computer-use__scroll", input, ctx)
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let x = input["x"].as_i64().unwrap_or(0) as i32;
        let y = input["y"].as_i64().unwrap_or(0) as i32;
        let amount = input["amount"].as_i64().unwrap_or(3) as i32;

        let result = Executor::new().scroll(x, y, amount).await?;
        Ok(ToolResult {
            data: json!(result),
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "Scroll the mouse wheel. Positive amount scrolls down, negative scrolls up.".to_string()
    }

    fn user_facing_name(&self, _input: Option<&Value>) -> String {
        "mcp__computer-use__scroll".to_string()
    }
}

// ---------------------------------------------------------------------------
// Mouse move tool
// ---------------------------------------------------------------------------

pub struct MouseMoveTool;

#[async_trait]
impl Tool for MouseMoveTool {
    fn name(&self) -> &str {
        "mcp__computer-use__mouse_move"
    }

    async fn description(&self, _input: &Value) -> String {
        "Move the mouse cursor to specified screen coordinates.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "x": { "type": "integer", "description": "X coordinate on screen" },
                "y": { "type": "integer", "description": "Y coordinate on screen" }
            },
            "required": ["x", "y"]
        })
    }

    async fn check_permissions(&self, input: &Value, ctx: &ToolUseContext) -> PermissionResult {
        cu_check_permissions("mcp__computer-use__mouse_move", input, ctx)
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let x = input["x"].as_i64().unwrap_or(0) as i32;
        let y = input["y"].as_i64().unwrap_or(0) as i32;

        let result = Executor::new().mouse_move(x, y).await?;
        Ok(ToolResult {
            data: json!(result),
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "Move the mouse cursor to screen coordinates.".to_string()
    }

    fn user_facing_name(&self, _input: Option<&Value>) -> String {
        "mcp__computer-use__mouse_move".to_string()
    }
}

// ---------------------------------------------------------------------------
// Cursor position tool
// ---------------------------------------------------------------------------

pub struct CursorPositionTool;

#[async_trait]
impl Tool for CursorPositionTool {
    fn name(&self) -> &str {
        "mcp__computer-use__cursor_position"
    }

    async fn description(&self, _input: &Value) -> String {
        "Get the current position of the mouse cursor.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    async fn check_permissions(&self, input: &Value, ctx: &ToolUseContext) -> PermissionResult {
        cu_check_permissions("mcp__computer-use__cursor_position", input, ctx)
    }

    async fn call(
        &self,
        _input: Value,
        _ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let pos = Executor::new().cursor_position().await?;
        Ok(ToolResult {
            data: json!({ "x": pos.x, "y": pos.y }),
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "Get the current mouse cursor position on screen.".to_string()
    }

    fn user_facing_name(&self, _input: Option<&Value>) -> String {
        "mcp__computer-use__cursor_position".to_string()
    }
}
