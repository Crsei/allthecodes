//! Windows screenshot via PowerShell + System.Drawing.
//!
//! Captures the entire virtual screen and returns a base64-encoded PNG.

use super::ScreenshotResult;

pub async fn capture_full_screen() -> anyhow::Result<ScreenshotResult> {
    // PowerShell script that captures the full virtual screen to a PNG,
    // then outputs "WIDTH HEIGHT\n<base64>" to stdout.
    let ps_script = r#"
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms

$bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$bmp = New-Object System.Drawing.Bitmap($bounds.Width, $bounds.Height)
$gfx = [System.Drawing.Graphics]::FromImage($bmp)
$gfx.CopyFromScreen($bounds.Location, [System.Drawing.Point]::Empty, $bounds.Size)
$gfx.Dispose()

$ms = New-Object System.IO.MemoryStream
$bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()

$bytes = $ms.ToArray()
$ms.Dispose()

Write-Output "$($bounds.Width) $($bounds.Height)"
Write-Output ([Convert]::ToBase64String($bytes))
"#;

    let output = tokio::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", ps_script])
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to run PowerShell screenshot: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("PowerShell screenshot failed: {}", stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut lines = stdout.trim().lines();

    let dims_line = lines
        .next()
        .ok_or_else(|| anyhow::anyhow!("No dimensions in screenshot output"))?;
    let mut dims = dims_line.split_whitespace();
    let width: u32 = dims.next().and_then(|s| s.parse().ok()).unwrap_or(1920);
    let height: u32 = dims.next().and_then(|s| s.parse().ok()).unwrap_or(1080);

    let base64_png = lines
        .next()
        .ok_or_else(|| anyhow::anyhow!("No base64 data in screenshot output"))?
        .to_string();

    Ok(ScreenshotResult {
        base64_png,
        width,
        height,
    })
}
