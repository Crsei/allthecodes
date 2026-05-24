//! Linux input simulation via xdotool (X11) or ydotool (Wayland).

use super::{CursorPosition, InputAction, MouseButton};

pub async fn execute(action: InputAction) -> anyhow::Result<String> {
    match action {
        InputAction::Click { x, y, button } => {
            let btn_num = match button {
                MouseButton::Left => 1,
                MouseButton::Middle => 2,
                MouseButton::Right => 3,
            };
            let btn_name = match button {
                MouseButton::Left => "left",
                MouseButton::Right => "right",
                MouseButton::Middle => "middle",
            };
            run_xdotool(&[
                "mousemove",
                &x.to_string(),
                &y.to_string(),
                "click",
                &btn_num.to_string(),
            ])
            .await?;
            Ok(format!("{} click at ({}, {})", btn_name, x, y))
        }
        InputAction::DoubleClick { x, y } => {
            run_xdotool(&[
                "mousemove",
                &x.to_string(),
                &y.to_string(),
                "click",
                "--repeat",
                "2",
                "1",
            ])
            .await?;
            Ok(format!("double click at ({}, {})", x, y))
        }
        InputAction::TypeText { ref text } => {
            run_xdotool(&["type", "--clearmodifiers", text]).await?;
            Ok("typed text".to_string())
        }
        InputAction::KeyPress { ref key } => {
            // xdotool uses format like "ctrl+c", "Return", "alt+Tab"
            let xdotool_key = map_key_to_xdotool(key);
            run_xdotool(&["key", "--clearmodifiers", &xdotool_key]).await?;
            Ok(format!("pressed key: {}", key))
        }
        InputAction::Scroll { x, y, amount } => {
            // Move cursor first
            run_xdotool(&["mousemove", &x.to_string(), &y.to_string()]).await?;
            // xdotool button 4 = scroll up, button 5 = scroll down
            let (button, count) = if amount > 0 {
                ("5", amount)
            } else {
                ("4", -amount)
            };
            for _ in 0..count {
                run_xdotool(&["click", button]).await?;
            }
            Ok(format!("scrolled {} at ({}, {})", amount, x, y))
        }
        InputAction::MouseMove { x, y } => {
            run_xdotool(&["mousemove", &x.to_string(), &y.to_string()]).await?;
            Ok(format!("moved mouse to ({}, {})", x, y))
        }
    }
}

pub async fn cursor_position() -> anyhow::Result<CursorPosition> {
    let output = run_xdotool(&["getmouselocation"]).await?;
    // Output format: "x:123 y:456 screen:0 window:12345"
    let mut x = 0i32;
    let mut y = 0i32;
    for part in output.split_whitespace() {
        if let Some(val) = part.strip_prefix("x:") {
            x = val.parse().unwrap_or(0);
        } else if let Some(val) = part.strip_prefix("y:") {
            y = val.parse().unwrap_or(0);
        }
    }
    Ok(CursorPosition { x, y })
}

async fn run_xdotool(args: &[&str]) -> anyhow::Result<String> {
    let output = tokio::process::Command::new("xdotool")
        .args(args)
        .output()
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "xdotool not available: {}. Install with: sudo apt install xdotool",
                e
            )
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("xdotool failed: {}", stderr.trim());
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn map_key_to_xdotool(key: &str) -> String {
    // xdotool uses X11 keysym names and "+" for combos.
    // Most common keys map directly; unknowns pass through as-is.
    let parts: Vec<&str> = key.split('+').collect();
    parts
        .iter()
        .map(|p| {
            // Rebind to a named local so `as_str()`'s borrow lives long enough.
            let p_lower = p.to_lowercase();
            match p_lower.as_str() {
                "ctrl" | "control" => "ctrl".to_string(),
                "alt" => "alt".to_string(),
                "shift" => "shift".to_string(),
                "super" | "cmd" | "command" | "meta" => "super".to_string(),
                "return" | "enter" => "Return".to_string(),
                "escape" | "esc" => "Escape".to_string(),
                "tab" => "Tab".to_string(),
                "backspace" | "back" => "BackSpace".to_string(),
                "delete" | "del" => "Delete".to_string(),
                "space" => "space".to_string(),
                "up" => "Up".to_string(),
                "down" => "Down".to_string(),
                "left" => "Left".to_string(),
                "right" => "Right".to_string(),
                "home" => "Home".to_string(),
                "end" => "End".to_string(),
                "pageup" | "page_up" => "Prior".to_string(),
                "pagedown" | "page_down" => "Next".to_string(),
                // Unknown: preserve original case from the input.
                _ => (*p).to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join("+")
}
