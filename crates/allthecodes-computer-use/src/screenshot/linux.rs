//! Linux screenshot via `scrot` (X11) or `grim` (Wayland).

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

    // Try grim (Wayland) first, then scrot (X11).
    // `or_else` can't short-circuit across awaits, so we dispatch manually.
    match try_grim(&tmp_path).await {
        Ok(()) => {}
        Err(_) => try_scrot(&tmp_path).await?,
    }

    let png_bytes = tokio::fs::read(tmp_file.path()).await?;

    let (width, height) = parse_png_dimensions(&png_bytes).unwrap_or((1920, 1080));
    let base64_png = base64_encode(&png_bytes);

    Ok(ScreenshotResult {
        base64_png,
        width,
        height,
    })
}

async fn try_grim(output_path: &str) -> anyhow::Result<()> {
    let output = tokio::process::Command::new("grim")
        .arg(output_path)
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("grim not available: {}", e))?;

    if !output.status.success() {
        anyhow::bail!("grim failed: {}", String::from_utf8_lossy(&output.stderr));
    }
    Ok(())
}

async fn try_scrot(output_path: &str) -> anyhow::Result<()> {
    let output = tokio::process::Command::new("scrot")
        .arg(output_path)
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("scrot not available: {}", e))?;

    if !output.status.success() {
        anyhow::bail!("scrot failed: {}", String::from_utf8_lossy(&output.stderr));
    }
    Ok(())
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
