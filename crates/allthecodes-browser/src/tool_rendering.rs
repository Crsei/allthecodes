//! Browser MCP tool-result rendering helpers.
//!
//! When a browser tool returns a result we can often recognize the shape
//! (navigation confirmation, page text dump, console message list, network
//! request log, screenshot) and produce a tighter summary than the raw MCP
//! payload. Frontends use the summary for compact display; the raw content is
//! still forwarded unchanged so the model sees everything it needs.

use super::permissions::{classify_browser_action, BrowserCategory};

/// Result shape hint attached to a browser tool result for frontend use.
///
/// Serialized as part of the tool-result metadata (via `ToolResultContentInfo`
/// or a sibling field) so UIs can pick the right renderer without having to
/// reparse JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserResultKind {
    /// Confirmation of a navigation (e.g. new URL, tab id).
    NavigationAck,
    /// Long page text dump (from `read_page` / `get_page_text`).
    PageText,
    /// DOM snapshot — JSON tree of accessible elements.
    DomSnapshot,
    /// One or more screenshot images.
    Screenshot,
    /// Array of console messages.
    ConsoleLog,
    /// Array of network request records.
    NetworkLog,
    /// Generic acknowledgement of a write (click, fill, etc.).
    WriteAck,
    /// Unknown / fallback.
    Generic,
}

impl BrowserResultKind {
    pub fn label(self) -> &'static str {
        match self {
            BrowserResultKind::NavigationAck => "navigation",
            BrowserResultKind::PageText => "page_text",
            BrowserResultKind::DomSnapshot => "dom_snapshot",
            BrowserResultKind::Screenshot => "screenshot",
            BrowserResultKind::ConsoleLog => "console_log",
            BrowserResultKind::NetworkLog => "network_log",
            BrowserResultKind::WriteAck => "write_ack",
            BrowserResultKind::Generic => "generic",
        }
    }
}

/// Infer a result kind from the browser action name plus whether the result
/// contained any image blocks.
///
/// The caller already knows if an image was produced (it's visible in the
/// MCP content blocks), so we take that as a parameter instead of trying to
/// re-sniff base64.
pub fn infer_kind(action: &str, has_image: bool) -> BrowserResultKind {
    if has_image {
        return BrowserResultKind::Screenshot;
    }
    match classify_browser_action(action) {
        BrowserCategory::Navigation => BrowserResultKind::NavigationAck,
        BrowserCategory::Read => match action {
            "take_snapshot" | "snapshot" => BrowserResultKind::DomSnapshot,
            _ => BrowserResultKind::PageText,
        },
        BrowserCategory::Write => BrowserResultKind::WriteAck,
        BrowserCategory::Observability => match action {
            "get_network_request" | "list_network_requests" | "read_network_requests" => {
                BrowserResultKind::NetworkLog
            }
            _ => BrowserResultKind::ConsoleLog,
        },
        _ => BrowserResultKind::Generic,
    }
}

/// Produce a short one-line summary for a browser tool result.
///
/// Used as the collapsed-view label in chat UIs ("navigated to X",
/// "24 console messages", etc.). Never panics; falls back to a generic
/// label if the content doesn't match any known shape.
pub fn short_summary(action: &str, text: &str, has_image: bool) -> String {
    let kind = infer_kind(action, has_image);
    match kind {
        BrowserResultKind::Screenshot => {
            let est_kb = text.len() / 1024;
            if est_kb > 0 {
                format!("screenshot ({}KB text + image)", est_kb)
            } else {
                "screenshot".to_string()
            }
        }
        BrowserResultKind::NavigationAck => {
            if let Some(url) = find_url_hint(text) {
                format!("navigated → {}", truncate(&url, 80))
            } else {
                "navigation ok".to_string()
            }
        }
        BrowserResultKind::PageText => {
            let chars = text.chars().count();
            format!("page text ({} chars)", chars)
        }
        BrowserResultKind::DomSnapshot => {
            let bytes = text.len();
            format!("DOM snapshot ({} bytes)", bytes)
        }
        BrowserResultKind::ConsoleLog => {
            if let Some(n) = count_array_items(text) {
                format!("{} console messages", n)
            } else {
                "console messages".to_string()
            }
        }
        BrowserResultKind::NetworkLog => {
            if let Some(n) = count_array_items(text) {
                format!("{} network requests", n)
            } else {
                "network requests".to_string()
            }
        }
        BrowserResultKind::WriteAck => {
            let first = text.lines().next().unwrap_or("").trim();
            if first.is_empty() {
                format!("{} ok", action)
            } else {
                format!("{}: {}", action, truncate(first, 80))
            }
        }
        BrowserResultKind::Generic => {
            let first = text.lines().next().unwrap_or("").trim();
            if first.is_empty() {
                action.to_string()
            } else {
                truncate(first, 100).to_string()
            }
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max).collect();
        out.push('…');
        out
    }
}

/// Try to extract a URL from a navigation-ack payload.
///
/// Browser MCPs typically return something like `{"url": "...", "title": "..."}`
/// or plain text like `Navigated to https://example.com`. We look for either.
fn find_url_hint(text: &str) -> Option<String> {
    // JSON shape
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(text) {
        if let Some(url) = val.get("url").and_then(|v| v.as_str()) {
            return Some(url.to_string());
        }
        if let Some(arr) = val.as_array() {
            if let Some(first) = arr.first() {
                if let Some(url) = first.get("url").and_then(|v| v.as_str()) {
                    return Some(url.to_string());
                }
            }
        }
    }
    // Loose text match: look for http(s)://... token
    text.split_whitespace()
        .find(|tok| tok.starts_with("http://") || tok.starts_with("https://"))
        .map(|s| s.trim_matches(|c: char| !c.is_ascii_graphic()).to_string())
}

/// Count items in a JSON array or object that looks like a list.
fn count_array_items(text: &str) -> Option<usize> {
    let val: serde_json::Value = serde_json::from_str(text).ok()?;
    if let Some(arr) = val.as_array() {
        return Some(arr.len());
    }
    if let Some(obj) = val.as_object() {
        for key in ["messages", "requests", "logs", "items", "entries"] {
            if let Some(arr) = obj.get(key).and_then(|v| v.as_array()) {
                return Some(arr.len());
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infer_kind_basics() {
        assert_eq!(
            infer_kind("navigate", false),
            BrowserResultKind::NavigationAck
        );
        assert_eq!(infer_kind("read_page", false), BrowserResultKind::PageText);
        assert_eq!(
            infer_kind("take_snapshot", false),
            BrowserResultKind::DomSnapshot
        );
        // screenshot flag dominates
        assert_eq!(infer_kind("anything", true), BrowserResultKind::Screenshot);
        assert_eq!(infer_kind("click", false), BrowserResultKind::WriteAck);
        assert_eq!(
            infer_kind("list_console_messages", false),
            BrowserResultKind::ConsoleLog
        );
        assert_eq!(
            infer_kind("list_network_requests", false),
            BrowserResultKind::NetworkLog
        );
    }

    #[test]
    fn summary_navigation_extracts_url() {
        let json = r#"{"url":"https://example.com/foo","title":"Foo"}"#;
        let s = short_summary("navigate", json, false);
        assert!(s.contains("navigated"));
        assert!(s.contains("example.com"));
    }

    #[test]
    fn summary_navigation_fallback_without_url() {
        let s = short_summary("navigate", "ok", false);
        assert!(s.contains("navigation"));
    }

    #[test]
    fn summary_console_counts_items() {
        let json = r#"[{"level":"log","text":"a"},{"level":"error","text":"b"}]"#;
        let s = short_summary("list_console_messages", json, false);
        assert!(s.contains("2"));
    }

    #[test]
    fn summary_network_counts_items_in_wrapped_shape() {
        let json = r#"{"requests":[{"url":"a"},{"url":"b"},{"url":"c"}]}"#;
        let s = short_summary("list_network_requests", json, false);
        assert!(s.contains("3"));
    }

    #[test]
    fn summary_screenshot_with_image_flag() {
        let s = short_summary("take_screenshot", "", true);
        assert!(s.contains("screenshot"));
    }

    #[test]
    fn summary_page_text_reports_length() {
        let body = "hello world";
        let s = short_summary("read_page", body, false);
        assert!(s.contains("chars"));
    }

    #[test]
    fn truncate_preserves_short() {
        assert_eq!(truncate("abc", 10), "abc");
        assert_eq!(truncate("hello world", 5), "hello…");
    }

    #[test]
    fn find_url_in_plain_text() {
        let s = "Navigated to https://foo.example/path?x=1 successfully";
        let url = find_url_hint(s).unwrap();
        assert!(url.starts_with("https://"));
    }
}
