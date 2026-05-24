//! Platform-native screenshot capture.
//!
//! Returns a base64-encoded PNG image of the full screen (or a specified region).

/// Screenshot result.
pub struct ScreenshotResult {
    /// Base64-encoded PNG image data.
    pub base64_png: String,
    /// Screen width in pixels.
    pub width: u32,
    /// Screen height in pixels.
    pub height: u32,
}

/// Capture a screenshot of the entire screen.
///
/// Returns the image as a base64-encoded PNG.
pub async fn capture_screenshot() -> anyhow::Result<ScreenshotResult> {
    platform::capture_full_screen().await
}

// ---------------------------------------------------------------------------
// Platform dispatch
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
#[path = "win32.rs"]
mod platform;

#[cfg(target_os = "macos")]
#[path = "darwin.rs"]
mod platform;

#[cfg(target_os = "linux")]
#[path = "linux.rs"]
mod platform;

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
mod platform {
    use super::ScreenshotResult;
    pub async fn capture_full_screen() -> anyhow::Result<ScreenshotResult> {
        anyhow::bail!("Screenshot not supported on this platform")
    }
}
