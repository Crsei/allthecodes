//! Visual input indicator overlay for Windows.
//!
//! Draws a small visual indicator (crosshair, circle, or highlight) at the
//! current mouse position to make Computer Use actions visible to the user.
//! This is particularly useful during automated operations where the user
//! needs to see where clicks will occur.
//!
//! The indicator is implemented as a small always-on-top, click-through window
//! that appears briefly during Computer Use operations and then fades out.
//!
//! On non-Windows platforms, this module provides no-op stubs.

use super::shared::run_powershell;
use std::time::Duration;

/// Style of the input indicator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndicatorStyle {
    /// Small crosshair centered on the cursor.
    Crosshair,
    /// Semi-transparent circle.
    Circle,
    /// Highlight box around the target area.
    Highlight,
}

/// Configuration for the input indicator.
#[derive(Debug, Clone)]
pub struct IndicatorConfig {
    /// Visual style.
    pub style: IndicatorStyle,
    /// Size in pixels (radius or half-side).
    pub size: u32,
    /// Duration to show the indicator before hiding.
    pub display_duration: Duration,
    /// Color as 0xRRGGBB hex value.
    pub color: u32,
    /// Whether the indicator is enabled.
    pub enabled: bool,
}

impl Default for IndicatorConfig {
    fn default() -> Self {
        Self {
            style: IndicatorStyle::Circle,
            size: 10,
            display_duration: Duration::from_millis(300),
            color: 0xFF4444, // red
            enabled: true,
        }
    }
}

/// Show an input indicator at the given screen coordinates.
///
/// The indicator appears briefly and then auto-hides.
pub async fn show_indicator(x: i32, y: i32, config: &IndicatorConfig) -> anyhow::Result<()> {
    if !config.enabled {
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    {
        platform_show_indicator(x, y, config).await?;
    }

    // Non-Windows: indicator not supported, silently no-op.
    let _ = (x, y, config);
    Ok(())
}

/// Show the default indicator at a position (uses default config).
pub async fn show_default_indicator(x: i32, y: i32) -> anyhow::Result<()> {
    show_indicator(x, y, &IndicatorConfig::default()).await
}

/// Show an indicator at the current cursor position.
pub async fn show_indicator_at_cursor(config: &IndicatorConfig) -> anyhow::Result<()> {
    let pos = crate::input::get_cursor_position().await?;
    show_indicator(pos.x, pos.y, config).await
}

/// Hide the indicator immediately.
pub async fn hide_indicator() -> anyhow::Result<()> {
    #[cfg(target_os = "windows")]
    {
        // We can use a simple PowerShell script to remove the overlay window.
        let script = r#"
$sig = @'
[DllImport("user32.dll")] public static extern IntPtr FindWindow(string lpClassName, string lpWindowName);
[DllImport("user32.dll")] public static extern bool DestroyWindow(IntPtr hWnd);
'@
$type = Add-Type -MemberDefinition $sig -Name "Win32HideIndicator" -Namespace "Win32" -PassThru
$hwnd = $type::FindWindow("CCRust_InputIndicator", $null)
if ($hwnd -ne [IntPtr]::Zero) { $type::DestroyWindow($hwnd) | Out-Null }
"#;
        let _ = run_powershell(script).await;
    }

    Ok(())
}

/// Platform-specific indicator drawing for Windows.
#[cfg(target_os = "windows")]
async fn platform_show_indicator(x: i32, y: i32, config: &IndicatorConfig) -> anyhow::Result<()> {
    let color_hex = format!("0x{:06X}", config.color & 0xFFFFFF);
    let size = config.size;
    let display_ms = config.display_duration.as_millis().min(5000) as u32;

    // Draw an always-on-top, transparent, click-through form.
    let script = format!(
        r#"
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

$form = New-Object System.Windows.Forms.Form
$form.Text = "CCRust_InputIndicator"
$form.ShowInTaskbar = $false
$form.FormBorderStyle = "None"
$form.BackColor = [System.Drawing.Color]::FromArgb(120, [System.Drawing.Color]::FromArgb({color}))
$form.TransparencyKey = $form.BackColor
$form.TopMost = $true

# Make click-through using WS_EX_TRANSPARENT + WS_EX_LAYERED
$form.ShowInTaskbar = $false

# Draw the indicator shape
$form.Paint.Add({{
    param($sender, $e)
    $g = $e.Graphics
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $pen = New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb(200, {color}), 2)

    if ({style} -eq 0) {{  # Crosshair
        $g.DrawLine($pen, $form.Width/2, 0, $form.Width/2, $form.Height)
        $g.DrawLine($pen, 0, $form.Height/2, $form.Width, $form.Height/2)
        $g.DrawEllipse($pen, $form.Width/4, $form.Height/4, $form.Width/2, $form.Height/2)
    }} elseif ({style} -eq 1) {{  # Circle
        $g.DrawEllipse($pen, 2, 2, $form.Width-4, $form.Height-4)
        $fillBrush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(60, {color}))
        $g.FillEllipse($fillBrush, 2, 2, $form.Width-4, $form.Height-4)
    }} else {{  # Highlight
        $g.DrawRectangle($pen, 2, 2, $form.Width-4, $form.Height-4)
        $fillBrush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(30, {color}))
        $g.FillRectangle($fillBrush, 2, 2, $form.Width-4, $form.Height-4)
    }}
}})

# Position centered on target
$sizeVal = {size}
$form.Width = $sizeVal * 2 + 4
$form.Height = $sizeVal * 2 + 4
$form.StartPosition = "Manual"
$form.Left = {x} - $sizeVal - 2
$form.Top = {y} - $sizeVal - 2

# WS_EX_TRANSPARENT | WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE
$exStyle = 0x80020 | 0x80000 | 0x80 | 0x08000000
$sig = @'
[DllImport("user32.dll")] public static extern int SetWindowLong(IntPtr hWnd, int nIndex, int dwNewLong);
[DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr hWnd, int nIndex);
'@
$type = Add-Type -MemberDefinition $sig -Name "Win32WS" -Namespace "Win32" -PassThru

$form.Add_Shown({{
    $hWnd = $form.Handle
    $currentStyle = [Win32WS]::GetWindowLong($hWnd, -20)
    [Win32WS]::SetWindowLong($hWnd, -20, $currentStyle -bor {exStyle}) | Out-Null
}})

$form.Show()
$form.Refresh()
Start-Sleep -Milliseconds {display}
$form.Close()
"#,
        x = x,
        y = y,
        size = size,
        color = color_hex,
        style = config.style as u32,
        display = display_ms,
        exStyle = 0x80020 | 0x80000 | 0x80 | 0x08000000u64,
    );

    run_powershell(&script).await?;
    Ok(())
}

/// Create the exStyle bitmask for the indicator window.
#[cfg(target_os = "windows")]
fn indicator_ex_style() -> u32 {
    // WS_EX_TRANSPARENT (0x20) | WS_EX_LAYERED (0x80000) | WS_EX_TOOLWINDOW (0x80) | WS_EX_NOACTIVATE (0x08000000)
    0x08088020
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_show_hide_does_not_panic() {
        hide_indicator().await.unwrap_or(());
        show_default_indicator(100, 100).await.unwrap_or(());
        hide_indicator().await.unwrap_or(());
    }

    #[test]
    fn test_indicator_config_default() {
        let config = IndicatorConfig::default();
        assert_eq!(config.style, IndicatorStyle::Circle);
        assert!(config.enabled);
    }

    #[test]
    fn test_disabled_indicator_skips_show() {
        let config = IndicatorConfig {
            enabled: false,
            ..Default::default()
        };
        // When disabled, show_indicator returns Ok immediately without platform call.
        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(show_indicator(0, 0, &config));
        assert!(result.is_ok());
    }
}
