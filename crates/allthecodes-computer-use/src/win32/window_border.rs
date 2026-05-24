//! Window border management for Windows.
//!
//! Provides utilities for:
//! - Detecting and drawing window borders
//! - Reading window frame metrics
//! - Managing window styles (resizable, bordered, etc.)
//! - Identifying window chrome vs. client area
//! - Highlighting the active window for visual feedback

use super::shared::{run_powershell, Hwnd};

/// Border metrics for a window.
#[derive(Debug, Clone)]
pub struct WindowBorderMetrics {
    /// Thickness of the left border in pixels.
    pub left: u32,
    /// Thickness of the right border in pixels.
    pub right: u32,
    /// Thickness of the top border (including title bar) in pixels.
    pub top: u32,
    /// Thickness of the bottom border in pixels.
    pub bottom: u32,
    /// Whether the window has a title bar.
    pub has_title_bar: bool,
    /// Title bar height in pixels (if present).
    pub title_bar_height: u32,
    /// Whether the window is resizable.
    pub resizable: bool,
}

/// Get border metrics for a given window.
pub async fn get_border_metrics(hwnd: Hwnd) -> anyhow::Result<WindowBorderMetrics> {
    let script = format!(
        r#"
$sig = @'
[DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr hWnd, int nIndex);
[DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT lpRect);
[DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr hWnd, out RECT lpRect);
public struct RECT {{ public int Left; public int Top; public int Right; public int Bottom; }}
'@
$type = Add-Type -MemberDefinition $sig -Name "Win32Border" -Namespace "Win32" -PassThru

$GWL_STYLE = -16
$WS_THICKFRAME = 0x00040000
$WS_CAPTION = 0x00C00000

$style = $type::GetWindowLong([IntPtr]{hwnd}, $GWL_STYLE)
$resizable = ($style -band $WS_THICKFRAME) -ne 0
$hasCaption = ($style -band $WS_CAPTION) -eq $WS_CAPTION

$winRect = New-Object Win32.RECT
$type::GetWindowRect([IntPtr]{hwnd}, [ref]$winRect) | Out-Null
$clientRect = New-Object Win32.RECT
$type::GetClientRect([IntPtr]{hwnd}, [ref]$clientRect) | Out-Null

$borderLeft = $clientRect.Left - $winRect.Left
$borderTop = $clientRect.Top - $winRect.Top
$borderRight = $winRect.Right - $clientRect.Right
$borderBottom = $winRect.Bottom - $clientRect.Bottom

Write-Output "$borderLeft|$borderTop|$borderRight|$borderBottom|$hasCaption|$borderTop|$resizable"
"#,
        hwnd = hwnd.0
    );

    let output = run_powershell(&script).await?;
    let parts: Vec<&str> = output.split('|').collect();
    if parts.len() >= 7 {
        Ok(WindowBorderMetrics {
            left: parts[0].parse().unwrap_or(8),
            right: parts[2].parse().unwrap_or(8),
            top: parts[1].parse().unwrap_or(30),
            bottom: parts[3].parse().unwrap_or(8),
            has_title_bar: parts[4] == "True",
            title_bar_height: parts[5].parse().unwrap_or(30),
            resizable: parts[6] == "True",
        })
    } else {
        // Reasonable defaults
        Ok(WindowBorderMetrics {
            left: 8,
            right: 8,
            top: 30,
            bottom: 8,
            has_title_bar: true,
            title_bar_height: 30,
            resizable: true,
        })
    }
}

/// Get the client area rectangle of a window (excluding borders and title bar).
pub async fn get_client_rect(hwnd: Hwnd) -> anyhow::Result<(i32, i32, i32, i32)> {
    let script = format!(
        r#"
$sig = @'
[DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr hWnd, out RECT lpRect);
[DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr hWnd, out POINT lpPoint);
public struct RECT {{ public int Left; public int Top; public int Right; public int Bottom; }}
public struct POINT {{ public int X; public int Y; }}
'@
$type = Add-Type -MemberDefinition $sig -Name "Win32ClientRect" -Namespace "Win32" -PassThru

$rect = New-Object Win32.RECT
$type::GetClientRect([IntPtr]{hwnd}, [ref]$rect) | Out-Null

# Convert client coords to screen coords
$topLeft = New-Object Win32.POINT
$type::ClientToScreen([IntPtr]{hwnd}, [ref]$topLeft) | Out-Null

Write-Output "$($topLeft.X) $($topLeft.Y) $($topLeft.X + $rect.Right) $($topLeft.Y + $rect.Bottom)"
"#,
        hwnd = hwnd.0
    );
    let output = run_powershell(&script).await?;
    let parts: Vec<i32> = output
        .split_whitespace()
        .filter_map(|s| s.parse().ok())
        .collect();
    if parts.len() == 4 {
        Ok((parts[0], parts[1], parts[2], parts[3]))
    } else {
        anyhow::bail!("Failed to parse client rect: {}", output);
    }
}

/// Convert screen coordinates to client coordinates for a given window.
pub async fn screen_to_client(
    hwnd: Hwnd,
    screen_x: i32,
    screen_y: i32,
) -> anyhow::Result<(i32, i32)> {
    let script = format!(
        r#"
$sig = @'
[DllImport("user32.dll")] public static extern bool ScreenToClient(IntPtr hWnd, out POINT lpPoint);
public struct POINT {{ public int X; public int Y; }}
'@
$type = Add-Type -MemberDefinition $sig -Name "Win32ScreenToClient" -Namespace "Win32" -PassThru

$pt = New-Object Win32.POINT
$pt.X = {x}
$pt.Y = {y}
$type::ScreenToClient([IntPtr]{hwnd}, [ref]$pt) | Out-Null
Write-Output "$($pt.X) $($pt.Y)"
"#,
        hwnd = hwnd.0,
        x = screen_x,
        y = screen_y
    );
    let output = run_powershell(&script).await?;
    let parts: Vec<i32> = output
        .split_whitespace()
        .filter_map(|s| s.parse().ok())
        .collect();
    if parts.len() == 2 {
        Ok((parts[0], parts[1]))
    } else {
        anyhow::bail!("Failed to convert coordinates: {}", output);
    }
}

/// Highlight a window by briefly inverting its border (for visual feedback).
///
/// Uses Windows `FlashWindow` API to draw attention to a window.
pub async fn flash_window(hwnd: Hwnd) -> anyhow::Result<()> {
    let script = format!(
        r#"
$sig = @'
[DllImport("user32.dll")] public static extern bool FlashWindow(IntPtr hWnd, bool bInvert);
'@
$type = Add-Type -MemberDefinition $sig -Name "Win32Flash" -Namespace "Win32" -PassThru
$type::FlashWindow([IntPtr]{hwnd}, $true) | Out-Null
"#,
        hwnd = hwnd.0
    );
    run_powershell(&script).await?;
    Ok(())
}

/// Get the window style flags.
pub async fn get_window_style(hwnd: Hwnd) -> anyhow::Result<u32> {
    let script = format!(
        r#"
$sig = @'
[DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr hWnd, int nIndex);
'@
$type = Add-Type -MemberDefinition $sig -Name "Win32GetStyle" -Namespace "Win32" -PassThru
$style = $type::GetWindowLong([IntPtr]{hwnd}, -16)
Write-Output $style
"#,
        hwnd = hwnd.0
    );
    let output = run_powershell(&script).await?;
    output
        .trim()
        .parse()
        .map_err(|e| anyhow::anyhow!("Failed to parse window style: {}", e))
}

/// Check whether a specific style flag is set on a window.
pub async fn has_window_style(hwnd: Hwnd, style_flag: u32) -> anyhow::Result<bool> {
    let style = get_window_style(hwnd).await?;
    Ok(style & style_flag != 0)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_border_metrics_null_window() {
        // This will fail since HWND 0 is invalid, but should not panic.
        let result = get_border_metrics(Hwnd(0)).await;
        assert!(result.is_err());
    }
}
