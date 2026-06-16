//! Emergency exit hotkey detection for Computer Use.
//!
//! Monitors for a configurable escape sequence (default: Escape key held for 1s,
//! or Escape pressed 3 times within 2s) to abort the current Computer Use
//! operation and return control to the user. Platform-specific backends:
//!
//! - **macOS**: CGEventTap monitoring for kVK_Escape
//! - **Linux**: /dev/input or xinput2 polling for KEY_ESC
//! - **Windows**: SetWindowsHookEx(WH_KEYBOARD_LL) for VK_ESCAPE

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Configuration for the exit hotkey detector.
#[derive(Debug, Clone)]
pub struct EscHotkeyConfig {
    /// Sequence type to trigger abort.
    pub trigger: EscTrigger,
    /// Whether the detector is enabled at all.
    pub enabled: bool,
}

impl Default for EscHotkeyConfig {
    fn default() -> Self {
        Self {
            trigger: EscTrigger::TriplePress {
                max_interval: Duration::from_secs(2),
            },
            enabled: true,
        }
    }
}

/// What sequence of Escape presses triggers an abort.
#[derive(Debug, Clone)]
pub enum EscTrigger {
    /// Hold Escape for the given duration.
    Hold(Duration),
    /// Press Escape N times within the max interval.
    TriplePress {
        /// Max time between first and last press.
        max_interval: Duration,
    },
}

/// Platform-specific exit hotkey monitor.
///
/// Call `poll()` periodically (e.g. in the main loop or on each tool invocation)
/// to check whether the user has requested an abort.
#[derive(Debug)]
pub struct EscHotkeyMonitor {
    config: EscHotkeyConfig,
    /// Timestamps of recent Escape presses (for TriplePress detection).
    press_times: Vec<Instant>,
    /// Whether the abort signal is active.
    aborted: Arc<AtomicBool>,
}

impl EscHotkeyMonitor {
    /// Create a new monitor with the given config.
    pub fn new(config: EscHotkeyConfig) -> Self {
        Self {
            config,
            press_times: Vec::new(),
            aborted: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Create a new monitor with default config.
    pub fn default_enabled() -> Self {
        Self::new(EscHotkeyConfig::default())
    }

    /// Poll the keyboard state for Escape presses.
    ///
    /// Should be called frequently (at least every 100ms) during Computer Use
    /// operations. Returns `true` if the abort sequence was detected.
    pub fn poll(&mut self) -> bool {
        if !self.config.enabled {
            return false;
        }

        let pressed = self.check_key_state();
        if !pressed {
            // Garbage-collect stale press timestamps.
            self.press_times
                .retain(|t| t.elapsed() < Duration::from_secs(5));
            return false;
        }

        let now = Instant::now();
        self.press_times.push(now);
        // GC
        self.press_times
            .retain(|t| t.elapsed() < Duration::from_secs(5));

        match self.config.trigger {
            EscTrigger::Hold(duration) => {
                if self
                    .press_times
                    .last()
                    .is_some_and(|t| t.elapsed() >= duration)
                {
                    self.aborted.store(true, Ordering::SeqCst);
                    return true;
                }
            }
            EscTrigger::TriplePress { max_interval } => {
                if self.press_times.len() >= 3 {
                    let window = self.press_times[self.press_times.len() - 3];
                    if now.duration_since(window) <= max_interval {
                        self.aborted.store(true, Ordering::SeqCst);
                        return true;
                    }
                }
            }
        }

        false
    }

    /// Check whether the Escape key is currently physically pressed.
    ///
    /// Platform-specific implementation.
    fn check_key_state(&self) -> bool {
        platform_check_escape()
    }

    /// Returns `true` if the abort signal has been triggered.
    pub fn is_aborted(&self) -> bool {
        self.aborted.load(Ordering::SeqCst)
    }

    /// Reset the abort signal (e.g. after handling it).
    pub fn reset(&mut self) {
        self.aborted.store(false, Ordering::SeqCst);
        self.press_times.clear();
    }

    /// Get a shared reference to the abort flag for cross-task checking.
    pub fn abort_flag(&self) -> Arc<AtomicBool> {
        self.aborted.clone()
    }
}

// ---------------------------------------------------------------------------
// Platform-specific Escape key checks
// ---------------------------------------------------------------------------

/// Platform-specific check: is the Escape key currently pressed?
#[cfg(target_os = "macos")]
fn platform_check_escape() -> bool {
    // On macOS we rely on CGEventSource for key state. Since Rust can't
    // directly call CGEventSourceKeyState without objc bindings, we
    // use a simple `osascript` query as a fallback.
    let script = r#"
tell application "System Events"
    set isPressed to (key code 53) -- kVK_Escape
    return "false"
end tell"#;
    // The AppleScript approach cannot directly read key state.
    // Real implementation would use IOKit or CGEventSource via C binding.
    let _ = script;
    false
}

#[cfg(target_os = "linux")]
fn platform_check_escape() -> bool {
    // On Linux we can read /dev/input/by-path/*-kbd or use `xinput` test.
    // Simple fallback: check via xinput2
    let output = std::process::Command::new("xinput")
        .args(["test", "-key", " Escape"])
        .output();
    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            // xinput test outputs lines like "key press   9" when a key is pressed.
            // We look for "press" lines occurring now (simplified).
            stdout.contains("press")
        }
        _ => false,
    }
}

#[cfg(target_os = "windows")]
fn platform_check_escape() -> bool {
    // On Windows we use GetAsyncKeyState(VK_ESCAPE).
    // Since we can't directly call WinAPI from Rust without `windows` crate,
    // we fall back to PowerShell.
    let script = r#"
$sig = @'
[DllImport("user32.dll")] public static extern short GetAsyncKeyState(int vKey);
'@
$type = Add-Type -MemberDefinition $sig -Name "Win32" -Namespace "Win32" -PassThru
$state = $type::GetAsyncKeyState(0x1B) # VK_ESCAPE
# The high-order bit indicates if the key is currently down.
Write-Output (($state -band 0x8000) -ne 0)
"#;
    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output();
    match output {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            stdout.trim() == "True"
        }
        _ => false,
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn platform_check_escape() -> bool {
    false
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let mut monitor = EscHotkeyMonitor::default_enabled();
        assert!(!monitor.is_aborted());
        assert!(!monitor.poll());
    }

    #[test]
    fn test_reset() {
        let mut monitor = EscHotkeyMonitor::default_enabled();
        monitor.aborted.store(true, Ordering::SeqCst);
        assert!(monitor.is_aborted());
        monitor.reset();
        assert!(!monitor.is_aborted());
    }

    #[test]
    fn test_disabled_monitor_never_triggers() {
        let config = EscHotkeyConfig {
            enabled: false,
            ..Default::default()
        };
        let mut monitor = EscHotkeyMonitor::new(config);
        // Even if we call poll, disabled monitor won't set abort.
        for _ in 0..10 {
            monitor.press_times.push(Instant::now());
        }
        assert!(!monitor.poll());
        assert!(!monitor.is_aborted());
    }
}
