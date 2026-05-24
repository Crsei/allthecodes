//! Shared Win32 utilities — constants, error codes, and helpers.
//!
//! Provides common Win32 API constants and PowerShell-based helper functions
//! used by other Win32 Computer Use modules.

/// Represents a Windows window handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hwnd(pub isize);

impl Hwnd {
    /// Null/invalid window handle.
    pub const NULL: Hwnd = Hwnd(0);
}

/// Standard Win32 error codes relevant to Computer Use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Win32Error {
    Success,
    AccessDenied,
    InvalidParam,
    WindowNotFound,
    ComError,
    PowerShellFailed,
    Unknown(u32),
}

impl From<u32> for Win32Error {
    fn from(code: u32) -> Self {
        match code {
            0 => Win32Error::Success,
            5 => Win32Error::AccessDenied,
            87 => Win32Error::InvalidParam,
            _ => Win32Error::Unknown(code),
        }
    }
}

/// ShowWindow command constants.
pub mod show_window_cmd {
    pub const SW_HIDE: u32 = 0;
    pub const SW_SHOWNORMAL: u32 = 1;
    pub const SW_SHOWMINIMIZED: u32 = 2;
    pub const SW_SHOWMAXIMIZED: u32 = 3;
    pub const SW_RESTORE: u32 = 9;
    pub const SW_SHOWDEFAULT: u32 = 10;
}

/// Virtual key codes relevant to Computer Use.
pub mod vk_codes {
    pub const VK_ESCAPE: u32 = 0x1B;
    pub const VK_RETURN: u32 = 0x0D;
    pub const VK_TAB: u32 = 0x09;
    pub const VK_CONTROL: u32 = 0x11;
    pub const VK_MENU: u32 = 0x12; // Alt
    pub const VK_SHIFT: u32 = 0x10;
    pub const VK_DELETE: u32 = 0x2E;
    pub const VK_BACK: u32 = 0x08; // Backspace
    pub const VK_SPACE: u32 = 0x20;
}

/// Run a PowerShell script and return stdout.
pub async fn run_powershell(script: &str) -> anyhow::Result<String> {
    let output = tokio::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to run PowerShell: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(anyhow::anyhow!(
            "PowerShell failed (exit {}): {} {}",
            output.status.code().unwrap_or(-1),
            stderr.trim(),
            stdout.trim()
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Get the active foreground window handle via PowerShell.
pub async fn get_foreground_window() -> anyhow::Result<Hwnd> {
    let script = r#"
$sig = @'
[DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
'@
$type = Add-Type -MemberDefinition $sig -Name "Win32GetFG" -Namespace "Win32" -PassThru
$hwnd = $type::GetForegroundWindow()
Write-Output $hwnd.ToInt64()
"#;
    let output = run_powershell(script).await?;
    let hwnd: isize = output
        .trim()
        .parse()
        .map_err(|e| anyhow::anyhow!("Failed to parse window handle '{}': {}", output.trim(), e))?;
    Ok(Hwnd(hwnd))
}

/// Get the window title for a given window handle via PowerShell.
pub async fn get_window_text(hwnd: Hwnd) -> anyhow::Result<String> {
    let script = format!(
        r#"
$sig = @'
[DllImport("user32.dll")] public static extern int GetWindowText(IntPtr hWnd, System.Text.StringBuilder lpString, int nMaxCount);
[DllImport("user32.dll")] public static extern int GetWindowTextLength(IntPtr hWnd);
'@
$type = Add-Type -MemberDefinition $sig -Name "Win32GetText" -Namespace "Win32" -PassThru
$length = $type::GetWindowTextLength([IntPtr]{hwnd})
$sb = New-Object System.Text.StringBuilder($length + 1)
$type::GetWindowText([IntPtr]{hwnd}, $sb, $sb.Capacity) | Out-Null
Write-Output $sb.ToString()
"#,
        hwnd = hwnd.0
    );
    run_powershell(&script)
}

/// Get the window class name for a given window handle.
pub async fn get_window_class(hwnd: Hwnd) -> anyhow::Result<String> {
    let script = format!(
        r#"
$sig = @'
[DllImport("user32.dll")] public static extern int GetClassName(IntPtr hWnd, System.Text.StringBuilder lpClassName, int nMaxCount);
'@
$type = Add-Type -MemberDefinition $sig -Name "Win32GetClass" -Namespace "Win32" -PassThru
$sb = New-Object System.Text.StringBuilder(256)
$type::GetClassName([IntPtr]{hwnd}, $sb, $sb.Capacity) | Out-Null
Write-Output $sb.ToString()
"#,
        hwnd = hwnd.0
    );
    run_powershell(&script)
}

/// Enumerate all top-level windows.
pub async fn enum_windows() -> anyhow::Result<Vec<(Hwnd, String)>> {
    let script = r#"
$sig = @'
[DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc lpEnumFunc, int lParam);
public delegate bool EnumWindowsProc(IntPtr hWnd, int lParam);
[DllImport("user32.dll")] public static extern int GetWindowText(IntPtr hWnd, System.Text.StringBuilder lpString, int nMaxCount);
[DllImport("user32.dll")] public static extern int GetWindowTextLength(IntPtr hWnd);
'@
$type = Add-Type -MemberDefinition $sig -Name "Win32EnumWin" -Namespace "Win32" -PassThru
$windows = New-Object System.Collections.ArrayList
$callback = [Win32.EnumWindowsProc]{
    param($hWnd, $lParam)
    $length = [Win32EnumWin]::GetWindowTextLength($hWnd)
    if ($length -gt 0) {
        $sb = New-Object System.Text.StringBuilder($length + 1)
        [Win32EnumWin]::GetWindowText($hWnd, $sb, $sb.Capacity) | Out-Null
        $windows.Add(@{Handle = $hWnd.ToInt64(); Title = $sb.ToString()}) | Out-Null
    }
    return $true
}
[Win32EnumWin]::EnumWindows($callback, 0) | Out-Null
foreach ($w in $windows) { Write-Output "$($w.Handle)|$($w.Title)" }
"#;
    let output = run_powershell(script).await?;
    let mut windows = Vec::new();
    for line in output.lines() {
        if let Some((handle_str, title)) = line.split_once('|') {
            if let Ok(h) = handle_str.trim().parse::<isize>() {
                windows.push((Hwnd(h), title.to_string()));
            }
        }
    }
    Ok(windows)
}

/// Get the window rectangle (bounds) of a given window.
pub async fn get_window_rect(hwnd: Hwnd) -> anyhow::Result<(i32, i32, i32, i32)> {
    let script = format!(
        r#"
$sig = @'
[DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT lpRect);
public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }
'@
$type = Add-Type -MemberDefinition $sig -Name "Win32GetRect" -Namespace "Win32" -PassThru
$rect = New-Object Win32.RECT
[Win32GetRect]::GetWindowRect([IntPtr]{hwnd}, [ref]$rect) | Out-Null
Write-Output "$($rect.Left) $($rect.Top) $($rect.Right) $($rect.Bottom)"
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
        anyhow::bail!("Failed to parse window rect: {}", output);
    }
}

/// Bring a window to the foreground.
pub async fn set_foreground_window(hwnd: Hwnd) -> anyhow::Result<()> {
    let script = format!(
        r#"
$sig = @'
[DllImport("user32.dll")] public static extern bool ShowWindowAsync(IntPtr hWnd, int nCmdShow);
[DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
'@
$type = Add-Type -MemberDefinition $sig -Name "Win32Focus" -Namespace "Win32" -PassThru
[Win32Focus]::ShowWindowAsync([IntPtr]{hwnd}, 9) | Out-Null
[Win32Focus]::SetForegroundWindow([IntPtr]{hwnd}) | Out-Null
"#,
        hwnd = hwnd.0
    );
    run_powershell(&script).await?;
    Ok(())
}

/// Check if a window is minimized.
pub async fn is_iconic(hwnd: Hwnd) -> anyhow::Result<bool> {
    let script = format!(
        r#"
$sig = @'
[DllImport("user32.dll")] public static extern bool IsIconic(IntPtr hWnd);
'@
$type = Add-Type -MemberDefinition $sig -Name "Win32Iconic" -Namespace "Win32" -PassThru
$result = $type::IsIconic([IntPtr]{hwnd})
if ($result) {{ Write-Output "true" }} else {{ Write-Output "false" }}
"#,
        hwnd = hwnd.0
    );
    let output = run_powershell(&script).await?;
    Ok(output.trim() == "true")
}

/// Get the process ID associated with a window handle.
pub async fn get_window_process_id(hwnd: Hwnd) -> anyhow::Result<u32> {
    let script = format!(
        r#"
$sig = @'
[DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint lpdwProcessId);
'@
$type = Add-Type -MemberDefinition $sig -Name "Win32GetPID" -Namespace "Win32" -PassThru
$pid = 0
[Win32GetPID]::GetWindowThreadProcessId([IntPtr]{hwnd}, [ref]$pid) | Out-Null
Write-Output $pid
"#,
        hwnd = hwnd.0
    );
    let output = run_powershell(&script).await?;
    output
        .trim()
        .parse()
        .map_err(|e| anyhow::anyhow!("Failed to parse PID: {}", e))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_run_powershell_simple() {
        let result = run_powershell("Write-Output 'hello'").await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "hello");
    }

    #[test]
    fn test_hwnd_null() {
        assert_eq!(Hwnd::NULL, Hwnd(0));
    }

    #[test]
    fn test_win32_error_conversion() {
        assert_eq!(Win32Error::from(0), Win32Error::Success);
        assert_eq!(Win32Error::from(5), Win32Error::AccessDenied);
    }
}
