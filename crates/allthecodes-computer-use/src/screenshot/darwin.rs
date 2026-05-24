//! macOS screenshot via `screencapture` CLI tool.

use super::ScreenshotResult;

pub async fn capture_full_screen() -> anyhow::Result<ScreenshotResult> {
    let tmp_file = tempfile::Builder::new()
        .prefix("cc_rust_screenshot_")
        .suffix(".png")
        .tempfile()?;
    let tmp_path = tmp_file
        .path()
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("screenshot temp path is not valid UTF-8"))?
        .to_string();

    // Capture screenshot using screencapture
    let output = tokio::process::Command::new("screencapture")
        .args(["-x", "-C", &tmp_path])
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to run screencapture: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("screencapture failed: {}", stderr.trim());
    }

    // Read the file and convert to base64
    let png_bytes = tokio::fs::read(tmp_file.path()).await?;

    // Get dimensions from the PNG header (width at bytes 16-19, height at 20-23, big-endian)
    let (width, height) = parse_png_dimensions(&png_bytes).unwrap_or((1920, 1080));

    let base64_png = base64_encode(&png_bytes);

    Ok(ScreenshotResult {
        base64_png,
        width,
        height,
    })
}

fn parse_png_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    if data.len() < 24 || &data[0..8] != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    let width = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
    let height = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
    Some((width, height))
}

fn base64_encode(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(data)
}
