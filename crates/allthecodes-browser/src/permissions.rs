//! Browser MCP permission categorization.
//!
//! Splits browser actions into categories so the permission prompt can say
//! "Allow navigating the browser?" instead of
//! "Allow tool 'mcp__chrome__navigate'?", and so that future session-level
//! "always allow" grants can be scoped to a category rather than a single
//! tool name.
//!
//! Category mapping mirrors Issue #3's scope:
//! > navigation, page reading, form writing, file upload, JS execution,
//! > console/network reading.

use crate::detection::extract_browser_action;

/// Coarse-grained browser action category used for permission UX and audit logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserCategory {
    /// Changing which page is visible (navigate, open/close tabs).
    Navigation,
    /// Reading page content without mutating DOM state (read_page, snapshot).
    Read,
    /// Writing into page inputs (fill, click, type, hover, drag).
    Write,
    /// Uploading a file into a page form element.
    Upload,
    /// Executing arbitrary JavaScript in the page.
    JavaScript,
    /// Reading browser console messages or network requests.
    Observability,
    /// Any other browser tool (unknown basename on a flagged server).
    Other,
}

impl BrowserCategory {
    /// Short, lowercased label for the category (used in logs and UI).
    pub fn label(self) -> &'static str {
        match self {
            BrowserCategory::Navigation => "navigation",
            BrowserCategory::Read => "read",
            BrowserCategory::Write => "write",
            BrowserCategory::Upload => "upload",
            BrowserCategory::JavaScript => "javascript",
            BrowserCategory::Observability => "observability",
            BrowserCategory::Other => "other",
        }
    }

    /// Human-readable risk tag to attach to permission prompts.
    pub fn risk_tag(self) -> &'static str {
        match self {
            BrowserCategory::Read | BrowserCategory::Observability => "[low risk]",
            BrowserCategory::Navigation => "[medium risk]",
            BrowserCategory::Write => "[medium risk]",
            BrowserCategory::Upload | BrowserCategory::JavaScript => "[HIGH RISK]",
            BrowserCategory::Other => "[medium risk]",
        }
    }
}

/// Classify a browser action basename into a category.
///
/// Accepts just the action part (`navigate`, not `mcp__chrome__navigate`).
/// Callers holding a full MCP tool name should strip the `mcp__{server}__`
/// prefix via [`super::detection::extract_browser_action`] first.
pub fn classify_browser_action(action: &str) -> BrowserCategory {
    match action {
        // Navigation / tabs / pages
        "navigate" | "navigate_page" | "goto" | "tabs_create" | "tabs_create_mcp"
        | "tabs_close" | "tabs_close_mcp" | "tabs_context" | "tabs_context_mcp" | "new_page"
        | "close_page" | "switch_browser" | "select_page" | "list_pages" => {
            BrowserCategory::Navigation
        }

        // Page reading / observation
        "read_page" | "get_page_text" | "take_snapshot" | "snapshot" | "get_page"
        | "take_screenshot" | "screenshot" | "find" | "wait_for" => BrowserCategory::Read,

        // DOM / form writing
        "click" | "browser_click" | "double_click" | "hover" | "drag" | "press_key"
        | "type_text" | "fill" | "fill_form" | "form_input" | "select" | "resize_page"
        | "resize_window" | "emulate" | "handle_dialog" => BrowserCategory::Write,

        // File upload
        "upload_file" | "file_upload" => BrowserCategory::Upload,

        // JavaScript execution
        "evaluate_script" | "javascript_tool" | "evaluate" => BrowserCategory::JavaScript,

        // Console / network
        "get_console_message"
        | "list_console_messages"
        | "read_console_messages"
        | "get_network_request"
        | "list_network_requests"
        | "read_network_requests" => BrowserCategory::Observability,

        _ => BrowserCategory::Other,
    }
}

/// Generate a human-readable permission prompt for a browser MCP tool.
///
/// Returns `None` when the tool isn't a recognized browser tool — the caller
/// should fall back to the generic "Allow tool 'X'?" prompt.
pub fn browser_permission_message(tool_name: &str) -> Option<String> {
    // Only produce a browser-styled prompt for names that the detection
    // module already recognizes — otherwise we'd re-skin unrelated MCP tools.
    let (_server, action) = extract_browser_action(tool_name)?;
    let category = classify_browser_action(action);
    let verb = action_verb(action);
    Some(format!("Allow {} {}?", verb, category.risk_tag()))
}

/// Describe an action as a present-participle verb phrase suitable for prompts.
fn action_verb(action: &str) -> String {
    match action {
        "navigate" | "navigate_page" | "goto" => "navigating the browser".to_string(),
        "tabs_create" | "tabs_create_mcp" | "new_page" => "opening a new browser tab".to_string(),
        "tabs_close" | "tabs_close_mcp" | "close_page" => "closing a browser tab".to_string(),
        "tabs_context" | "tabs_context_mcp" | "list_pages" | "select_page" | "switch_browser" => {
            "reading tab context".to_string()
        }
        "read_page" | "get_page_text" | "get_page" => "reading the current page".to_string(),
        "take_snapshot" | "snapshot" => "taking a DOM snapshot".to_string(),
        "take_screenshot" | "screenshot" => "taking a screenshot".to_string(),
        "find" => "searching the page".to_string(),
        "wait_for" => "waiting for a page condition".to_string(),
        "click" | "browser_click" => "clicking on the page".to_string(),
        "double_click" => "double-clicking on the page".to_string(),
        "hover" => "hovering over a page element".to_string(),
        "drag" => "dragging a page element".to_string(),
        "press_key" => "pressing a key in the browser".to_string(),
        "type_text" => "typing text into the page".to_string(),
        "fill" | "fill_form" | "form_input" => "filling a form".to_string(),
        "select" => "selecting a dropdown option".to_string(),
        "resize_page" | "resize_window" | "emulate" => {
            "changing browser viewport/emulation".to_string()
        }
        "handle_dialog" => "responding to a browser dialog".to_string(),
        "upload_file" | "file_upload" => "uploading a file to the page".to_string(),
        "evaluate_script" | "javascript_tool" | "evaluate" => {
            "running arbitrary JavaScript in the page".to_string()
        }
        "get_console_message" | "list_console_messages" | "read_console_messages" => {
            "reading browser console messages".to_string()
        }
        "get_network_request" | "list_network_requests" | "read_network_requests" => {
            "reading browser network requests".to_string()
        }
        other => format!("running browser action '{}'", other),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categorizes_by_action_basename() {
        assert_eq!(
            classify_browser_action("navigate"),
            BrowserCategory::Navigation
        );
        assert_eq!(classify_browser_action("read_page"), BrowserCategory::Read);
        assert_eq!(classify_browser_action("click"), BrowserCategory::Write);
        assert_eq!(
            classify_browser_action("upload_file"),
            BrowserCategory::Upload
        );
        assert_eq!(
            classify_browser_action("evaluate_script"),
            BrowserCategory::JavaScript
        );
        assert_eq!(
            classify_browser_action("list_console_messages"),
            BrowserCategory::Observability
        );
        assert_eq!(
            classify_browser_action("something_else"),
            BrowserCategory::Other
        );
    }

    #[test]
    fn high_risk_tag_for_js_and_upload() {
        assert_eq!(BrowserCategory::JavaScript.risk_tag(), "[HIGH RISK]");
        assert_eq!(BrowserCategory::Upload.risk_tag(), "[HIGH RISK]");
        assert_eq!(BrowserCategory::Read.risk_tag(), "[low risk]");
    }

    #[test]
    fn permission_message_for_navigation() {
        let msg = browser_permission_message("mcp__chrome__navigate").unwrap();
        assert!(msg.contains("navigating the browser"));
        assert!(msg.contains("[medium risk]"));
    }

    #[test]
    fn permission_message_for_js_exec() {
        let msg = browser_permission_message("mcp__chrome__evaluate_script").unwrap();
        assert!(msg.contains("JavaScript"));
        assert!(msg.contains("[HIGH RISK]"));
    }

    #[test]
    fn permission_message_none_for_non_browser() {
        assert!(browser_permission_message("Bash").is_none());
        assert!(browser_permission_message("mcp__filesystem__read_file").is_none());
    }
}
