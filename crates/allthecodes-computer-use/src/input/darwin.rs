//! macOS input simulation via osascript (AppleScript) and cliclick.

use super::{CursorPosition, InputAction, MouseButton};

pub async fn execute(action: InputAction) -> anyhow::Result<String> {
    match action {
        InputAction::Click { x, y, button } => {
            // Use cliclick if available, otherwise osascript
            let btn_char = match button {
                MouseButton::Left => "c",
                MouseButton::Right => "rc",
                MouseButton::Middle => "c", // cliclick doesn't support middle directly
            };
            let btn_name = match button {
                MouseButton::Left => "left",
                MouseButton::Right => "right",
                MouseButton::Middle => "middle",
            };

            // Try cliclick first
            let result = tokio::process::Command::new("cliclick")
                .args([&format!("{}:{},{}", btn_char, x, y)])
                .output()
                .await;

            match result {
                Ok(output) if output.status.success() => {
                    Ok(format!("{} click at ({}, {})", btn_name, x, y))
                }
                _ => {
                    // Fallback to osascript
                    let script = format!(
                        r#"tell application "System Events" to click at {{{}, {}}}"#,
                        x, y
                    );
                    run_osascript(&script).await?;
                    Ok(format!("{} click at ({}, {})", btn_name, x, y))
                }
            }
        }
        InputAction::DoubleClick { x, y } => {
            let result = tokio::process::Command::new("cliclick")
                .args([&format!("dc:{},{}", x, y)])
                .output()
                .await;

            match result {
                Ok(output) if output.status.success() => {
                    Ok(format!("double click at ({}, {})", x, y))
                }
                _ => {
                    let script = format!(
                        r#"tell application "System Events" to click at {{{}, {}}}
tell application "System Events" to click at {{{}, {}}}"#,
                        x, y, x, y
                    );
                    run_osascript(&script).await?;
                    Ok(format!("double click at ({}, {})", x, y))
                }
            }
        }
        InputAction::TypeText { ref text } => {
            let script = format!(
                r#"tell application "System Events" to keystroke "{}""#,
                text.replace('\\', "\\\\").replace('"', "\\\"")
            );
            run_osascript(&script).await?;
            Ok("typed text".to_string())
        }
        InputAction::KeyPress { ref key } => {
            let script = map_key_to_applescript(key);
            run_osascript(&script).await?;
            Ok(format!("pressed key: {}", key))
        }
        InputAction::Scroll { x, y, amount } => {
            // Move cursor first, then scroll
            let _ = tokio::process::Command::new("cliclick")
                .args([&format!("m:{},{}", x, y)])
                .output()
                .await;

            let script = format!(
                r#"tell application "System Events" to scroll area 1 by {}"#,
                amount
            );
            let _ = run_osascript(&script).await;
            Ok(format!("scrolled {} at ({}, {})", amount, x, y))
        }
        InputAction::MouseMove { x, y } => {
            let result = tokio::process::Command::new("cliclick")
                .args([&format!("m:{},{}", x, y)])
                .output()
                .await;

            match result {
                Ok(output) if output.status.success() => {
                    Ok(format!("moved mouse to ({}, {})", x, y))
                }
                _ => {
                    anyhow::bail!(
                        "Mouse move requires cliclick on macOS. Install with: brew install cliclick"
                    )
                }
            }
        }
    }
}

pub async fn cursor_position() -> anyhow::Result<CursorPosition> {
    // Try cliclick first
    let result = tokio::process::Command::new("cliclick")
        .args(["p:"])
        .output()
        .await;

    if let Ok(output) = result {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            // cliclick outputs "X,Y\n"
            let trimmed = stdout.trim();
            let mut parts = trimmed.split(',');
            if let (Some(x_str), Some(y_str)) = (parts.next(), parts.next()) {
                if let (Ok(x), Ok(y)) = (x_str.trim().parse(), y_str.trim().parse()) {
                    return Ok(CursorPosition { x, y });
                }
            }
        }
    }

    // Fallback to AppleScript (less reliable)
    let script = r#"
tell application "System Events"
    set mousePos to position of the mouse
    return (item 1 of mousePos) & " " & (item 2 of mousePos)
end tell"#;
    let output = run_osascript(script).await?;
    let mut parts = output.trim().split_whitespace();
    let x: i32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let y: i32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    Ok(CursorPosition { x, y })
}

async fn run_osascript(script: &str) -> anyhow::Result<String> {
    let output = tokio::process::Command::new("osascript")
        .args(["-e", script])
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to run osascript: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("osascript failed: {}", stderr.trim());
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn map_key_to_applescript(key: &str) -> String {
    let parts: Vec<&str> = key.split('+').collect();

    if parts.len() > 1 {
        let modifiers: Vec<&str> = parts[..parts.len() - 1]
            .iter()
            .map(|m| match m.to_lowercase().as_str() {
                "ctrl" | "control" => "control down",
                "alt" | "option" => "option down",
                "shift" => "shift down",
                "cmd" | "command" | "super" => "command down",
                _ => "command down",
            })
            .collect();
        let key_part = parts.last().unwrap();
        let mod_str = modifiers.join(", ");
        return format!(
            r#"tell application "System Events" to key code {} using {{{}}}"#,
            key_name_to_code(key_part),
            mod_str
        );
    }

    let key_lower = key.to_lowercase();
    match key_lower.as_str() {
        "return" | "enter" => r#"tell application "System Events" to key code 36"#.to_string(),
        "escape" | "esc" => r#"tell application "System Events" to key code 53"#.to_string(),
        "tab" => r#"tell application "System Events" to key code 48"#.to_string(),
        "space" => r#"tell application "System Events" to key code 49"#.to_string(),
        "backspace" | "delete" => r#"tell application "System Events" to key code 51"#.to_string(),
        _ => {
            format!(r#"tell application "System Events" to keystroke "{}""#, key)
        }
    }
}

fn key_name_to_code(key: &str) -> u32 {
    match key.to_lowercase().as_str() {
        "a" => 0,
        "c" => 8,
        "v" => 9,
        "x" => 7,
        "z" => 6,
        "s" => 1,
        "f" => 3,
        "w" => 13,
        "t" => 17,
        "tab" => 48,
        "return" | "enter" => 36,
        "escape" | "esc" => 53,
        "space" => 49,
        _ => 0, // fallback
    }
}
