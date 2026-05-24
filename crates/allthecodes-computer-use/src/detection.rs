//! Computer Use tool detection and classification.
//!
//! Identifies `mcp__computer-use__*` tools by name prefix and classifies
//! them into risk categories for permissions and UI.

/// Prefix for all Computer Use tool names.
pub const COMPUTER_USE_PREFIX: &str = "mcp__computer-use__";

/// Risk level for Computer Use tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CuRiskLevel {
    /// Read-only observation (screenshot, cursor_position)
    Medium,
    /// Active input (click, type, key, scroll)
    High,
}

/// Extract the action name from a Computer Use tool name.
///
/// `"mcp__computer-use__screenshot"` → `Some("screenshot")`
/// `"Bash"` → `None`
pub fn extract_cu_action(tool_name: &str) -> Option<&str> {
    tool_name.strip_prefix(COMPUTER_USE_PREFIX)
}

/// Classify the risk level of a Computer Use action.
pub fn classify_risk(action: &str) -> CuRiskLevel {
    match action {
        "screenshot" | "cursor_position" => CuRiskLevel::Medium,
        _ => CuRiskLevel::High, // click, type, key, scroll, etc.
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_cu_action() {
        assert_eq!(
            extract_cu_action("mcp__computer-use__screenshot"),
            Some("screenshot")
        );
        assert_eq!(
            extract_cu_action("mcp__computer-use__left_click"),
            Some("left_click")
        );
        assert_eq!(extract_cu_action("Bash"), None);
    }

    #[test]
    fn test_classify_risk() {
        assert_eq!(classify_risk("screenshot"), CuRiskLevel::Medium);
        assert_eq!(classify_risk("cursor_position"), CuRiskLevel::Medium);
        assert_eq!(classify_risk("left_click"), CuRiskLevel::High);
        assert_eq!(classify_risk("type_text"), CuRiskLevel::High);
        assert_eq!(classify_risk("key"), CuRiskLevel::High);
        assert_eq!(classify_risk("scroll"), CuRiskLevel::High);
    }
}
