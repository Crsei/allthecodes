//! Desktop host adapter — platform integration for Computer Use.
//!
//! Provides a unified interface for:
//! - Platform detection and capability reporting
//! - Permission/accessibility status checks
//! - Screen information (resolution, DPI, display count)
//! - Desktop environment detection
//! - OS version information

/// Comprehensive information about the host platform's Computer Use capabilities.
#[derive(Debug, Clone)]
pub struct HostCapabilities {
    /// OS type.
    pub os: OsType,
    /// OS version string (e.g. "14.5" on macOS, "10.0.22631" on Windows).
    pub os_version: String,
    /// Whether the platform supports native Computer Use tools.
    pub supported: bool,
    /// Screenshot capability.
    pub screenshot: CapabilityLevel,
    /// Input simulation capability.
    pub input: CapabilityLevel,
    /// Application launching capability.
    pub app_launch: CapabilityLevel,
    /// Window management capability.
    pub window_management: CapabilityLevel,
    /// Accessibility permissions granted (macOS) or not applicable.
    pub accessibility_granted: PermissionState,
    /// Screen recording permissions granted (macOS) or not applicable.
    pub screen_recording_granted: PermissionState,
    /// Desktop environment on Linux.
    pub desktop_environment: Option<String>,
    /// Display count.
    pub display_count: u32,
    /// Whether Wayland is in use (on Linux).
    pub is_wayland: bool,
    /// DPI scaling factor (1.0 = 100%).
    pub dpi_scale: f64,
}

/// Operating system type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OsType {
    MacOS,
    Windows,
    Linux,
    Unknown,
}

/// How well a capability is supported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityLevel {
    /// Full native support.
    Full,
    /// Works via fallback / external tool.
    Partial,
    /// Not supported.
    None,
}

/// Whether a permission has been granted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionState {
    /// Permission granted.
    Granted,
    /// Permission denied / not granted yet.
    Denied,
    /// Not applicable on this platform.
    NotApplicable,
    /// Unknown — could not determine.
    Unknown,
}

/// Detect the OS type.
pub fn detect_os() -> OsType {
    #[cfg(target_os = "macos")]
    {
        OsType::MacOS
    }
    #[cfg(target_os = "windows")]
    {
        OsType::Windows
    }
    #[cfg(target_os = "linux")]
    {
        OsType::Linux
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        OsType::Unknown
    }
}

/// Probe the host for all capabilities.
pub async fn probe_capabilities() -> HostCapabilities {
    let os = detect_os();
    let os_version = detect_os_version().await;
    let desktop_env = detect_desktop_environment().await;
    let is_wayland = desktop_env.as_deref() == Some("wayland");

    match os {
        OsType::MacOS => HostCapabilities {
            os,
            os_version,
            supported: true,
            screenshot: CapabilityLevel::Full,
            input: CapabilityLevel::Full,
            app_launch: CapabilityLevel::Full,
            window_management: CapabilityLevel::Full,
            accessibility_granted: check_accessibility_permission().await,
            screen_recording_granted: check_screen_recording_permission().await,
            desktop_environment: None,
            display_count: probe_display_count().await,
            is_wayland: false,
            dpi_scale: probe_dpi_scale().await,
        },
        OsType::Windows => HostCapabilities {
            os,
            os_version,
            supported: true,
            screenshot: CapabilityLevel::Full,
            input: CapabilityLevel::Partial, // via PowerShell
            app_launch: CapabilityLevel::Full,
            window_management: CapabilityLevel::Full,
            accessibility_granted: PermissionState::NotApplicable,
            screen_recording_granted: PermissionState::NotApplicable,
            desktop_environment: None,
            display_count: probe_display_count().await,
            is_wayland: false,
            dpi_scale: probe_dpi_scale().await,
        },
        OsType::Linux => HostCapabilities {
            os,
            os_version,
            supported: true,
            screenshot: if is_wayland {
                CapabilityLevel::Partial // grim
            } else {
                CapabilityLevel::Full // scrot
            },
            input: if is_wayland {
                CapabilityLevel::None
            } else {
                CapabilityLevel::Full // xdotool
            },
            app_launch: CapabilityLevel::Partial, // xdg-open
            window_management: if is_wayland {
                CapabilityLevel::None
            } else {
                CapabilityLevel::Full // wmctrl / xdotool
            },
            accessibility_granted: PermissionState::NotApplicable,
            screen_recording_granted: PermissionState::NotApplicable,
            desktop_environment: desktop_env,
            display_count: probe_display_count().await,
            is_wayland,
            dpi_scale: probe_dpi_scale().await,
        },
        OsType::Unknown => HostCapabilities {
            os,
            os_version,
            supported: false,
            screenshot: CapabilityLevel::None,
            input: CapabilityLevel::None,
            app_launch: CapabilityLevel::None,
            window_management: CapabilityLevel::None,
            accessibility_granted: PermissionState::NotApplicable,
            screen_recording_granted: PermissionState::NotApplicable,
            desktop_environment: None,
            display_count: 0,
            is_wayland: false,
            dpi_scale: 1.0,
        },
    }
}

async fn detect_os_version() -> String {
    #[cfg(target_os = "macos")]
    {
        let output = tokio::process::Command::new("sw_vers")
            .arg("-productVersion")
            .output()
            .await;
        match output {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
            _ => "unknown".to_string(),
        }
    }

    #[cfg(target_os = "windows")]
    {
        let output = tokio::process::Command::new("cmd")
            .args(["/c", "ver"])
            .output()
            .await;
        match output {
            Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
            _ => "unknown".to_string(),
        }
    }

    #[cfg(target_os = "linux")]
    {
        let output = tokio::process::Command::new("uname")
            .args(["-r"])
            .output()
            .await;
        match output {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
            _ => "unknown".to_string(),
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        "unknown".to_string()
    }
}

async fn detect_desktop_environment() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        // Check XDG_SESSION_DESKTOP or similar env vars
        if let Ok(de) = std::env::var("XDG_SESSION_DESKTOP") {
            return Some(de);
        }
        if let Ok(de) = std::env::var("XDG_CURRENT_DESKTOP") {
            return Some(de);
        }
        if let Ok(de) = std::env::var("DESKTOP_SESSION") {
            return Some(de);
        }
        // Check Wayland
        if let Ok(wayland) = std::env::var("WAYLAND_DISPLAY") {
            if !wayland.is_empty() {
                return Some("wayland".to_string());
            }
        }
        None
    }

    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

async fn check_accessibility_permission() -> PermissionState {
    #[cfg(target_os = "macos")]
    {
        // Use `tccutil` or `osascript` to check accessibility permissions.
        let script = r#"
tell application "System Events"
    set isEnabled to UI elements enabled
    return isEnabled as string
end tell"#;
        let output = tokio::process::Command::new("osascript")
            .args(["-e", script])
            .output()
            .await;
        match output {
            Ok(o) if o.status.success() => {
                let stdout = String::from_utf8_lossy(&o.stdout);
                if stdout.trim() == "true" {
                    PermissionState::Granted
                } else {
                    PermissionState::Denied
                }
            }
            _ => PermissionState::Unknown,
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        PermissionState::NotApplicable
    }
}

async fn check_screen_recording_permission() -> PermissionState {
    #[cfg(target_os = "macos")]
    {
        // Screen Recording permission is required for CGWindow-based screenshots
        // on macOS 10.15+. We check by attempting a small screenshot.
        let output = tokio::process::Command::new("screencapture")
            .args(["-x", "-C", "/dev/null"])
            .output()
            .await;
        match output {
            Ok(o) if o.status.success() => PermissionState::Granted,
            Ok(_) => PermissionState::Denied,
            _ => PermissionState::Unknown,
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        PermissionState::NotApplicable
    }
}

async fn probe_display_count() -> u32 {
    #[cfg(target_os = "macos")]
    {
        let script = r#"
tell application "System Events"
    set displays to (every display)
    return count of displays
end tell"#;
        let output = tokio::process::Command::new("osascript")
            .args(["-e", script])
            .output()
            .await;
        if let Ok(o) = output {
            if let Ok(s) = String::from_utf8(o.stdout) {
                if let Ok(n) = s.trim().parse::<u32>() {
                    return n;
                }
            }
        }
        1
    }

    #[cfg(target_os = "windows")]
    {
        let script = r#"
Add-Type -AssemblyName System.Windows.Forms
Write-Output ([System.Windows.Forms.Screen]::AllScreens.Length)
"#;
        let output = tokio::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .output()
            .await;
        if let Ok(o) = output {
            if let Ok(s) = String::from_utf8(o.stdout) {
                if let Ok(n) = s.trim().parse::<u32>() {
                    return n;
                }
            }
        }
        1
    }

    #[cfg(target_os = "linux")]
    {
        let output = tokio::process::Command::new("xrandr")
            .args(["--listmonitors"])
            .output()
            .await;
        if let Ok(o) = output {
            let stdout = String::from_utf8_lossy(&o.stdout);
            // First line: "Monitors: N"
            if let Some(line) = stdout.lines().next() {
                if let Some(n_str) = line.split_whitespace().nth(1) {
                    if let Ok(n) = n_str.parse::<u32>() {
                        return n;
                    }
                }
            }
        }
        1
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        0
    }
}

async fn probe_dpi_scale() -> f64 {
    #[cfg(target_os = "macos")]
    {
        let script = r#"
tell application "System Events"
    set displaySettings to (get display settings of every display)
    return (scale factor of item 1 of displaySettings) as string
end tell"#;
        let output = tokio::process::Command::new("osascript")
            .args(["-e", script])
            .output()
            .await;
        if let Ok(o) = output {
            if let Ok(s) = String::from_utf8(o.stdout) {
                if let Ok(n) = s.trim().parse::<f64>() {
                    return n;
                }
            }
        }
        2.0 // Retina default
    }

    #[cfg(target_os = "windows")]
    {
        let script = r#"
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public class DPI {
    [DllImport("shcore.dll")] public static extern int GetDpiForMonitor(IntPtr hMonitor, int dpiType, out uint dpiX, out uint dpiY);
}
"@
# Fallback: read the registry or environment
Write-Output "1.0"
"#;
        let _ = script;
        1.0
    }

    #[cfg(target_os = "linux")]
    {
        if let Ok(gdms) = std::env::var("GDK_SCALE") {
            if let Ok(n) = gdms.parse::<f64>() {
                return n;
            }
        }
        1.0
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        1.0
    }
}

/// Format a summary string of host capabilities for system prompts.
pub fn format_capabilities_summary(caps: &HostCapabilities) -> String {
    format!(
        "OS: {os} {ver}\n\
         Screenshot: {ss:?}\n\
         Input: {input:?}\n\
         App Launch: {launch:?}\n\
         Window Management: {wm:?}\n\
         Accessibility: {acc:?}\n\
         Screen Recording: {sr:?}\n\
         Displays: {displays}\n\
         DPI Scale: {dpi:.1}x\n\
         {wayland}",
        os = match caps.os {
            OsType::MacOS => "macOS",
            OsType::Windows => "Windows",
            OsType::Linux => "Linux",
            OsType::Unknown => "Unknown",
        },
        ver = caps.os_version,
        ss = caps.screenshot,
        input = caps.input,
        launch = caps.app_launch,
        wm = caps.window_management,
        acc = caps.accessibility_granted,
        sr = caps.screen_recording_granted,
        displays = caps.display_count,
        dpi = caps.dpi_scale,
        wayland = if caps.is_wayland {
            "Wayland: yes"
        } else {
            "Wayland: no"
        },
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_os_returns_something() {
        let os = detect_os();
        // At least it should be one of the known values
        match os {
            OsType::MacOS | OsType::Windows | OsType::Linux | OsType::Unknown => {}
        }
    }

    #[tokio::test]
    async fn test_probe_capabilities_does_not_panic() {
        let caps = probe_capabilities().await;
        // Should not crash on any platform
        match caps.os {
            OsType::MacOS | OsType::Windows | OsType::Linux | OsType::Unknown => {}
        }
    }

    #[test]
    fn test_format_capabilities() {
        let caps = HostCapabilities {
            os: OsType::MacOS,
            os_version: "14.5".to_string(),
            supported: true,
            screenshot: CapabilityLevel::Full,
            input: CapabilityLevel::Full,
            app_launch: CapabilityLevel::Full,
            window_management: CapabilityLevel::Full,
            accessibility_granted: PermissionState::Granted,
            screen_recording_granted: PermissionState::Granted,
            desktop_environment: None,
            display_count: 2,
            is_wayland: false,
            dpi_scale: 2.0,
        };
        let s = format_capabilities_summary(&caps);
        assert!(s.contains("macOS"));
        assert!(s.contains("2.0x"));
        assert!(s.contains("Displays: 2"));
    }
}
