//! MCP Channel notification protocol extension.
//!
//! MCP servers that support channels declare `capabilities.experimental["claude/channel"]`.
//! When they send `notifications/claude/channel`, the content is parsed and routed
//! through the ChannelManager.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Check if an MCP server supports channel notifications.
pub fn supports_channel(capabilities: &Value) -> bool {
    capabilities
        .get("experimental")
        .and_then(|v| v.get("claude/channel"))
        .is_some()
}

/// Parsed channel notification from an MCP server.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct McpChannelNotification {
    pub content: String,
    pub meta: Value,
}

/// Parse a raw MCP notification into a channel notification.
pub fn parse_channel_notification(params: &Value) -> Option<McpChannelNotification> {
    let content = params.get("content").and_then(|v| v.as_str())?;
    let meta = params.get("meta").cloned().unwrap_or(Value::Null);
    Some(McpChannelNotification {
        content: content.to_string(),
        meta,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_supports_channel() {
        // Server that declares claude/channel support
        let caps_with = json!({
            "experimental": {
                "claude/channel": {}
            }
        });
        assert!(supports_channel(&caps_with));

        // Server without experimental section
        let caps_no_experimental = json!({
            "tools": {}
        });
        assert!(!supports_channel(&caps_no_experimental));

        // Server with experimental but no claude/channel
        let caps_no_channel = json!({
            "experimental": {
                "other_feature": true
            }
        });
        assert!(!supports_channel(&caps_no_channel));

        // Empty object
        assert!(!supports_channel(&json!({})));

        // Null value
        assert!(!supports_channel(&Value::Null));
    }

    #[test]
    fn test_parse_channel_notification() {
        // Valid notification with content and meta
        let params = json!({
            "content": "Hello from MCP channel",
            "meta": {"source": "test-server", "priority": 1}
        });
        let notif = parse_channel_notification(&params).unwrap();
        assert_eq!(notif.content, "Hello from MCP channel");
        assert_eq!(notif.meta["source"], "test-server");
        assert_eq!(notif.meta["priority"], 1);

        // Valid notification with content only (no meta)
        let params_no_meta = json!({
            "content": "Just content"
        });
        let notif = parse_channel_notification(&params_no_meta).unwrap();
        assert_eq!(notif.content, "Just content");
        assert_eq!(notif.meta, Value::Null);

        // Missing content field -> None
        let params_no_content = json!({
            "meta": {"source": "test"}
        });
        assert!(parse_channel_notification(&params_no_content).is_none());

        // Content is not a string -> None
        let params_bad_content = json!({
            "content": 42
        });
        assert!(parse_channel_notification(&params_bad_content).is_none());

        // Empty object -> None
        assert!(parse_channel_notification(&json!({})).is_none());
    }
}
