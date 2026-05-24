//! Input method loader — detect and select the best available input method.
//!
//! Each platform may have multiple ways to simulate input (e.g., direct API,
//! CLI tool, script host). This module probes availability and returns a
//! priority-ordered list of working methods.

use std::time::Duration;

/// A detected input method with its capabilities.
#[derive(Debug, Clone)]
pub struct InputMethod {
    /// Name of the method (e.g. "cliclick", "osascript", "xdotool", "PowerShell").
    pub name: &'static str,
    /// Whether this is the preferred (most reliable) method.
    pub preferred: bool,
    /// Whether this method supports mouse clicks.
    pub supports_clicks: bool,
    /// Whether this method supports keyboard input.
    pub supports_keyboard: bool,
    /// Whether this method supports scrolling.
    pub supports_scroll: bool,
    /// Whether this method supports cursor movement.
    pub supports_move: bool,
    /// Whether this method requires accessibility permissions.
    pub requires_accessibility: bool,
}

/// Probe for available input methods on the current platform.
///
/// Returns methods in priority order (most preferred first).
pub async fn detect_input_methods() -> Vec<InputMethod> {
    platform_detect().await
}

/// Get the single best available input method.
pub async fn best_input_method() -> Option<InputMethod> {
    let methods = detect_input_methods().await;
    methods
        .iter()
        .find(|m| m.preferred)
        .cloned()
        .or_else(|| methods.into_iter().next())
}

/// Check whether at least one input method is available.
pub async fn has_any_input_method() -> bool {
    best_input_method().await.is_some()
}

/// Check availability of a specific CLI tool.
#[cfg(any(target_os = "macos", target_os = "windows"))]
async fn check_tool(name: &str, args: &[&str]) -> bool {
    tokio::process::Command::new(name)
        .args(args)
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}

async fn check_tool_timeout(name: &str, args: &[&str], timeout: Duration) -> bool {
    let fut = tokio::process::Command::new(name).args(args).output();
    tokio::time::timeout(timeout, fut)
        .await
        .ok()
        .and_then(|r| r.ok())
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Platform detection
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
async fn platform_detect() -> Vec<InputMethod> {
    let has_cliclick = check_tool("cliclick", &["-h"]).await;
    let has_osascript = check_tool("osascript", &["-e", "return 1"]).await;

    let mut methods = Vec::new();

    if has_cliclick {
        methods.push(InputMethod {
            name: "cliclick",
            preferred: true,
            supports_clicks: true,
            supports_keyboard: true,
            supports_scroll: true,
            supports_move: true,
            requires_accessibility: false, // cliclick uses CGEvent directly
        });
    }

    if has_osascript {
        methods.push(InputMethod {
            name: "osascript",
            preferred: !has_cliclick,
            supports_clicks: true,
            supports_keyboard: true,
            supports_scroll: true,
            supports_move: true,
            requires_accessibility: true,
        });
    }

    // AppleScript via /usr/bin/osascript is always available.
    if methods.is_empty() {
        methods.push(InputMethod {
            name: "osascript",
            preferred: true,
            supports_clicks: true,
            supports_keyboard: true,
            supports_scroll: true,
            supports_move: true,
            requires_accessibility: true,
        });
    }

    methods
}

#[cfg(target_os = "windows")]
async fn platform_detect() -> Vec<InputMethod> {
    let has_powershell = check_tool("powershell", &["-NoProfile", "-Command", "1+1"]).await;

    vec![InputMethod {
        name: "PowerShell",
        preferred: true,
        supports_clicks: true,
        supports_keyboard: true,
        supports_scroll: true,
        supports_move: true,
        requires_accessibility: false,
    }]
}

#[cfg(target_os = "linux")]
async fn platform_detect() -> Vec<InputMethod> {
    let has_xdotool =
        check_tool_timeout("xdotool", &["--version"], Duration::from_millis(500)).await;
    let mut methods = Vec::new();

    if has_xdotool {
        methods.push(InputMethod {
            name: "xdotool",
            preferred: true,
            supports_clicks: true,
            supports_keyboard: true,
            supports_scroll: true,
            supports_move: true,
            requires_accessibility: false,
        });
    }

    methods
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
async fn platform_detect() -> Vec<InputMethod> {
    vec![]
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_detect_does_not_panic() {
        let methods = detect_input_methods().await;
        // On CI/unknown platforms this may be empty; that's fine.
        assert!(methods.len() <= 3);
    }

    #[tokio::test]
    async fn test_has_any_does_not_panic() {
        let _ = has_any_input_method().await;
    }
}
