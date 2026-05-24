//! Windows input simulation via PowerShell + System.Windows.Forms / WinAPI.

use super::{CursorPosition, InputAction, MouseButton};

pub async fn execute(action: InputAction) -> anyhow::Result<String> {
    let ps_script = match action {
        InputAction::Click { x, y, button } => {
            let btn_code = match button {
                MouseButton::Left => 0x0002 | 0x0004, // MOUSEEVENTF_LEFTDOWN | LEFTUP
                MouseButton::Right => 0x0008 | 0x0010, // MOUSEEVENTF_RIGHTDOWN | RIGHTUP
                MouseButton::Middle => 0x0020 | 0x0040, // MOUSEEVENTF_MIDDLEDOWN | MIDDLEUP
            };
            let btn_name = match button {
                MouseButton::Left => "left",
                MouseButton::Right => "right",
                MouseButton::Middle => "middle",
            };
            format!(
                r#"
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public class WinInput {{
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int X, int Y);
    [DllImport("user32.dll")] public static extern void mouse_event(int dwFlags, int dx, int dy, int dwData, int dwExtraInfo);
}}
"@
[WinInput]::SetCursorPos({x}, {y})
Start-Sleep -Milliseconds 50
[WinInput]::mouse_event({down}, 0, 0, 0, 0)
Start-Sleep -Milliseconds 30
[WinInput]::mouse_event({up}, 0, 0, 0, 0)
Write-Output "{btn_name} click at ({x}, {y})"
"#,
                x = x,
                y = y,
                down = btn_code & 0x002A, // down flags
                up = btn_code & 0x0054,   // up flags
                btn_name = btn_name,
            )
        }
        InputAction::DoubleClick { x, y } => {
            format!(
                r#"
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public class WinInput2 {{
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int X, int Y);
    [DllImport("user32.dll")] public static extern void mouse_event(int dwFlags, int dx, int dy, int dwData, int dwExtraInfo);
}}
"@
[WinInput2]::SetCursorPos({x}, {y})
Start-Sleep -Milliseconds 50
[WinInput2]::mouse_event(0x0002, 0, 0, 0, 0)
[WinInput2]::mouse_event(0x0004, 0, 0, 0, 0)
Start-Sleep -Milliseconds 60
[WinInput2]::mouse_event(0x0002, 0, 0, 0, 0)
[WinInput2]::mouse_event(0x0004, 0, 0, 0, 0)
Write-Output "double click at ({x}, {y})"
"#,
                x = x,
                y = y,
            )
        }
        InputAction::TypeText { ref text } => {
            // Use SendKeys for simple text, with escaping
            let escaped = text
                .replace('{', "{{}")
                .replace('}', "{}}")
                .replace('+', "{+}")
                .replace('^', "{^}")
                .replace('%', "{%}")
                .replace('~', "{~}")
                .replace('(', "{(}")
                .replace(')', "{)}");
            format!(
                r#"
Add-Type -AssemblyName System.Windows.Forms
[System.Windows.Forms.SendKeys]::SendWait("{text}")
Write-Output "typed text"
"#,
                text = escaped,
            )
        }
        InputAction::KeyPress { ref key } => {
            // Map common key names to SendKeys format
            let send_key = map_key_to_sendkeys(key);
            format!(
                r#"
Add-Type -AssemblyName System.Windows.Forms
[System.Windows.Forms.SendKeys]::SendWait("{key}")
Write-Output "pressed key: {orig}"
"#,
                key = send_key,
                orig = key,
            )
        }
        InputAction::Scroll { x, y, amount } => {
            // Positive amount = scroll down, negative = scroll up
            // MOUSEEVENTF_WHEEL = 0x0800, dwData = amount * 120
            let wheel_delta = -amount * 120; // Windows convention: negative = down
            format!(
                r#"
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public class WinScroll {{
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int X, int Y);
    [DllImport("user32.dll")] public static extern void mouse_event(int dwFlags, int dx, int dy, int dwData, int dwExtraInfo);
}}
"@
[WinScroll]::SetCursorPos({x}, {y})
Start-Sleep -Milliseconds 50
[WinScroll]::mouse_event(0x0800, 0, 0, {delta}, 0)
Write-Output "scrolled {amount} at ({x}, {y})"
"#,
                x = x,
                y = y,
                delta = wheel_delta,
                amount = amount,
            )
        }
        InputAction::MouseMove { x, y } => {
            format!(
                r#"
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public class WinMove {{
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int X, int Y);
}}
"@
[WinMove]::SetCursorPos({x}, {y})
Write-Output "moved mouse to ({x}, {y})"
"#,
                x = x,
                y = y,
            )
        }
    };

    run_powershell(&ps_script).await
}

pub async fn cursor_position() -> anyhow::Result<CursorPosition> {
    let ps_script = r#"
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public struct POINT { public int X; public int Y; }
public class WinCursor {
    [DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT lpPoint);
}
"@
$pt = New-Object POINT
[WinCursor]::GetCursorPos([ref]$pt) | Out-Null
Write-Output "$($pt.X) $($pt.Y)"
"#;

    let output = run_powershell(ps_script).await?;
    let mut parts = output.split_whitespace();
    let x: i32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let y: i32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    Ok(CursorPosition { x, y })
}

async fn run_powershell(script: &str) -> anyhow::Result<String> {
    let output = tokio::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to run PowerShell: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("PowerShell command failed: {}", stderr.trim());
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Map common key names to Windows SendKeys format.
fn map_key_to_sendkeys(key: &str) -> String {
    // Handle modifier combos like "ctrl+c", "alt+tab"
    let parts: Vec<&str> = key.split('+').collect();
    if parts.len() > 1 {
        let mut result = String::new();
        for (i, part) in parts.iter().enumerate() {
            let is_last = i == parts.len() - 1;
            match part.to_lowercase().as_str() {
                "ctrl" | "control" => result.push('^'),
                "alt" => result.push('%'),
                "shift" => result.push('+'),
                _ if is_last => result.push_str(&map_single_key(part)),
                _ => result.push_str(&map_single_key(part)),
            }
        }
        return result;
    }

    map_single_key(key)
}

fn map_single_key(key: &str) -> String {
    match key.to_lowercase().as_str() {
        "return" | "enter" => "~".to_string(),
        "escape" | "esc" => "{ESC}".to_string(),
        "tab" => "{TAB}".to_string(),
        "backspace" | "back" => "{BACKSPACE}".to_string(),
        "delete" | "del" => "{DELETE}".to_string(),
        "space" => " ".to_string(),
        "up" => "{UP}".to_string(),
        "down" => "{DOWN}".to_string(),
        "left" => "{LEFT}".to_string(),
        "right" => "{RIGHT}".to_string(),
        "home" => "{HOME}".to_string(),
        "end" => "{END}".to_string(),
        "pageup" | "page_up" => "{PGUP}".to_string(),
        "pagedown" | "page_down" => "{PGDN}".to_string(),
        "f1" => "{F1}".to_string(),
        "f2" => "{F2}".to_string(),
        "f3" => "{F3}".to_string(),
        "f4" => "{F4}".to_string(),
        "f5" => "{F5}".to_string(),
        "f6" => "{F6}".to_string(),
        "f7" => "{F7}".to_string(),
        "f8" => "{F8}".to_string(),
        "f9" => "{F9}".to_string(),
        "f10" => "{F10}".to_string(),
        "f11" => "{F11}".to_string(),
        "f12" => "{F12}".to_string(),
        other if other.len() == 1 => other.to_string(),
        other => format!("{{{}}}", other.to_uppercase()),
    }
}
