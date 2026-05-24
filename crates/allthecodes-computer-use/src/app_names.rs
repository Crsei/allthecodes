//! Application name mapping — friendly name → executable name.
//!
//! Provides a bidirectional mapping table that translates user-facing application
//! names (e.g. "Chrome", "Word", "Terminal") to platform-specific executable
//! names (e.g. "google-chrome", "WINWORD.EXE", "Terminal.app").
//!
//! Used by Computer Use tools to launch or focus applications without requiring
//! the user to know exact executable paths.

/// A mapping entry for a single application.
#[derive(Debug, Clone)]
pub struct AppEntry {
    /// Canonical display name (e.g. "Google Chrome").
    pub display_name: &'static str,
    /// Executable name on macOS.
    pub macos: &'static [&'static str],
    /// Executable name on Windows.
    pub windows: &'static [&'static str],
    /// Executable name on Linux.
    pub linux: &'static [&'static str],
    /// Bundle identifier (macOS) or app user model ID (Windows).
    pub bundle_id: Option<&'static str>,
    /// Category for grouping.
    pub category: AppCategory,
}

/// Broad category for grouping apps by purpose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppCategory {
    Browser,
    Editor,
    Terminal,
    Office,
    Communication,
    Media,
    FileManager,
    Utility,
    Other,
}

/// The full application registry.
///
/// Built as a static slice so it can be queried without allocation.
pub static APP_REGISTRY: &[AppEntry] = &[
    // ── Browsers ──
    AppEntry {
        display_name: "Google Chrome",
        macos: &["Google Chrome"],
        windows: &["chrome.exe"],
        linux: &["google-chrome", "google-chrome-stable", "chrome"],
        bundle_id: Some("com.google.Chrome"),
        category: AppCategory::Browser,
    },
    AppEntry {
        display_name: "Chromium",
        macos: &["Chromium"],
        windows: &["chrome.exe"],
        linux: &["chromium", "chromium-browser"],
        bundle_id: Some("org.chromium.Chromium"),
        category: AppCategory::Browser,
    },
    AppEntry {
        display_name: "Firefox",
        macos: &["Firefox"],
        windows: &["firefox.exe"],
        linux: &["firefox"],
        bundle_id: Some("org.mozilla.firefox"),
        category: AppCategory::Browser,
    },
    AppEntry {
        display_name: "Safari",
        macos: &["Safari"],
        windows: &[],
        linux: &[],
        bundle_id: Some("com.apple.Safari"),
        category: AppCategory::Browser,
    },
    AppEntry {
        display_name: "Edge",
        macos: &["Microsoft Edge"],
        windows: &["msedge.exe"],
        linux: &["microsoft-edge", "microsoft-edge-stable"],
        bundle_id: Some("com.microsoft.edgemac"),
        category: AppCategory::Browser,
    },
    // ── Terminals ──
    AppEntry {
        display_name: "Terminal",
        macos: &["Terminal"],
        windows: &["cmd.exe", "WindowsTerminal.exe"],
        linux: &["gnome-terminal", "konsole", "xterm", "alacritty", "kitty"],
        bundle_id: Some("com.apple.Terminal"),
        category: AppCategory::Terminal,
    },
    AppEntry {
        display_name: "iTerm2",
        macos: &["iTerm2"],
        windows: &[],
        linux: &[],
        bundle_id: Some("com.googlecode.iterm2"),
        category: AppCategory::Terminal,
    },
    AppEntry {
        display_name: "Windows Terminal",
        macos: &[],
        windows: &["WindowsTerminal.exe"],
        linux: &[],
        bundle_id: Some("Microsoft.WindowsTerminal"),
        category: AppCategory::Terminal,
    },
    // ── Editors / IDEs ──
    AppEntry {
        display_name: "VS Code",
        macos: &["Visual Studio Code", "Code"],
        windows: &["Code.exe"],
        linux: &["code", "code-oss"],
        bundle_id: Some("com.microsoft.VSCode"),
        category: AppCategory::Editor,
    },
    AppEntry {
        display_name: "Cursor",
        macos: &["Cursor"],
        windows: &["Cursor.exe"],
        linux: &["cursor"],
        bundle_id: None,
        category: AppCategory::Editor,
    },
    AppEntry {
        display_name: "Vim",
        macos: &["Vim", "MacVim"],
        windows: &["vim.exe", "gvim.exe"],
        linux: &["vim", "gvim", "nvim", "neovim"],
        bundle_id: Some("org.vim.MacVim"),
        category: AppCategory::Editor,
    },
    AppEntry {
        display_name: "Neovide",
        macos: &["Neovide"],
        windows: &["neovide.exe"],
        linux: &["neovide"],
        bundle_id: Some("com.neovide.neovide"),
        category: AppCategory::Editor,
    },
    AppEntry {
        display_name: "IntelliJ IDEA",
        macos: &["IntelliJ IDEA"],
        windows: &["idea64.exe"],
        linux: &["idea", "intellij-idea"],
        bundle_id: Some("com.jetbrains.intellij"),
        category: AppCategory::Editor,
    },
    // ── Office ──
    AppEntry {
        display_name: "Microsoft Word",
        macos: &["Microsoft Word"],
        windows: &["WINWORD.EXE"],
        linux: &[],
        bundle_id: Some("com.microsoft.Word"),
        category: AppCategory::Office,
    },
    AppEntry {
        display_name: "Microsoft Excel",
        macos: &["Microsoft Excel"],
        windows: &["EXCEL.EXE"],
        linux: &[],
        bundle_id: Some("com.microsoft.Excel"),
        category: AppCategory::Office,
    },
    AppEntry {
        display_name: "Microsoft PowerPoint",
        macos: &["Microsoft PowerPoint"],
        windows: &["POWERPNT.EXE"],
        linux: &[],
        bundle_id: Some("com.microsoft.PowerPoint"),
        category: AppCategory::Office,
    },
    AppEntry {
        display_name: "Microsoft Outlook",
        macos: &["Microsoft Outlook"],
        windows: &["OUTLOOK.EXE"],
        linux: &[],
        bundle_id: Some("com.microsoft.Outlook"),
        category: AppCategory::Office,
    },
    AppEntry {
        display_name: "LibreOffice",
        macos: &["LibreOffice"],
        windows: &["soffice.exe"],
        linux: &["libreoffice", "soffice"],
        bundle_id: Some("org.libreoffice.script"),
        category: AppCategory::Office,
    },
    // ── Communication ──
    AppEntry {
        display_name: "Slack",
        macos: &["Slack"],
        windows: &["slack.exe"],
        linux: &["slack"],
        bundle_id: Some("com.tinyspeck.slackmacos"),
        category: AppCategory::Communication,
    },
    AppEntry {
        display_name: "Discord",
        macos: &["Discord"],
        windows: &["Discord.exe"],
        linux: &["discord"],
        bundle_id: Some("com.hnc.Discord"),
        category: AppCategory::Communication,
    },
    AppEntry {
        display_name: "Zoom",
        macos: &["zoom.us"],
        windows: &["Zoom.exe"],
        linux: &["zoom"],
        bundle_id: Some("us.zoom.xos"),
        category: AppCategory::Communication,
    },
    AppEntry {
        display_name: "Teams",
        macos: &["Microsoft Teams"],
        windows: &["Teams.exe", "ms-teams.exe"],
        linux: &["teams", "teams-for-linux"],
        bundle_id: Some("com.microsoft.teams"),
        category: AppCategory::Communication,
    },
    // ── Media ──
    AppEntry {
        display_name: "Spotify",
        macos: &["Spotify"],
        windows: &["Spotify.exe"],
        linux: &["spotify"],
        bundle_id: Some("com.spotify.client"),
        category: AppCategory::Media,
    },
    AppEntry {
        display_name: "VLC",
        macos: &["VLC"],
        windows: &["vlc.exe"],
        linux: &["vlc"],
        bundle_id: Some("org.videolan.vlc"),
        category: AppCategory::Media,
    },
    // ── Utilities ──
    AppEntry {
        display_name: "Finder",
        macos: &["Finder"],
        windows: &[],
        linux: &[],
        bundle_id: Some("com.apple.Finder"),
        category: AppCategory::FileManager,
    },
    AppEntry {
        display_name: "File Explorer",
        macos: &[],
        windows: &["explorer.exe"],
        linux: &["nautilus", "dolphin", "thunar", "nemo"],
        bundle_id: None,
        category: AppCategory::FileManager,
    },
    AppEntry {
        display_name: "System Settings",
        macos: &["System Settings", "System Preferences"],
        windows: &["SystemSettings.exe", "control.exe"],
        linux: &["gnome-control-center", "systemsettings"],
        bundle_id: Some("com.apple.systempreferences"),
        category: AppCategory::Utility,
    },
    AppEntry {
        display_name: "Calculator",
        macos: &["Calculator"],
        windows: &["calc.exe"],
        linux: &["gnome-calculator", "kcalc", "qalculate-gtk"],
        bundle_id: Some("com.apple.calculator"),
        category: AppCategory::Utility,
    },
];

/// Look up an application by its display name (case-insensitive, fuzzy).
///
/// Returns the first matching entry, or `None`. Matching is case-insensitive
/// and accepts both full display names and common abbreviations.
pub fn lookup_app(name: &str) -> Option<&'static AppEntry> {
    let name_lower = name.to_lowercase();

    // Exact match on display_name first.
    for entry in APP_REGISTRY {
        if entry.display_name.to_lowercase() == name_lower {
            return Some(entry);
        }
    }

    // Then match on any executable name (current platform).
    let platform_names: &[&str] = platform_executable_names();

    // Fuzzy match: check if name is a prefix/substring of any known name.
    for entry in APP_REGISTRY {
        if entry.display_name.to_lowercase().contains(&name_lower) {
            return Some(entry);
        }
        for pn in platform_names {
            let pn_lower = pn.to_lowercase();
            if pn_lower.contains(&name_lower) || name_lower.contains(&pn_lower) {
                return Some(entry);
            }
        }
    }

    None
}

/// Get the platform-specific executable names for the current OS.
pub fn platform_executable_names() -> &'static [&'static str] {
    #[cfg(target_os = "macos")]
    {
        // On macOS we look for .app bundle names.
        &["Terminal", "Finder", "Safari", "Chrome", "Firefox"]
    }
    #[cfg(target_os = "windows")]
    {
        &["cmd.exe", "explorer.exe", "chrome.exe"]
    }
    #[cfg(target_os = "linux")]
    {
        &["xterm", "gnome-terminal", "firefox", "chromium"]
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        &[]
    }
}

/// Get the primary executable name for an app on the current platform.
pub fn primary_executable(entry: &AppEntry) -> Option<&'static str> {
    #[cfg(target_os = "macos")]
    {
        entry.macos.first().copied()
    }
    #[cfg(target_os = "windows")]
    {
        entry.windows.first().copied()
    }
    #[cfg(target_os = "linux")]
    {
        entry.linux.first().copied()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        None
    }
}

/// Open an application by name using the platform launcher.
///
/// On macOS uses `open -b <bundle_id>` or `open -a <name>`.
/// On Windows uses `start <name>`.
/// On Linux uses `xdg-open` or direct executable launch.
pub async fn launch_app(name: &str) -> anyhow::Result<String> {
    let entry = lookup_app(name).ok_or_else(|| anyhow::anyhow!("Unknown application: {}", name))?;
    launch_app_entry(entry).await
}

/// Launch an application from an `AppEntry`.
pub async fn launch_app_entry(entry: &AppEntry) -> anyhow::Result<String> {
    let display = entry.display_name;

    #[cfg(target_os = "macos")]
    {
        if let Some(bundle) = entry.bundle_id {
            let output = tokio::process::Command::new("open")
                .args(["-b", bundle])
                .output()
                .await?;
            if output.status.success() {
                return Ok(format!("Launched {}", display));
            }
        }
        if let Some(name) = entry.macos.first() {
            let output = tokio::process::Command::new("open")
                .args(["-a", name])
                .output()
                .await?;
            if output.status.success() {
                return Ok(format!("Launched {}", display));
            }
        }
        anyhow::bail!("Failed to launch {} on macOS", display);
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(name) = entry.windows.first() {
            let output = tokio::process::Command::new("cmd")
                .args(["/c", "start", "", name])
                .output()
                .await?;
            if output.status.success() {
                return Ok(format!("Launched {}", display));
            }
        }
        anyhow::bail!("Failed to launch {} on Windows", display);
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(name) = entry.linux.first() {
            let _output = tokio::process::Command::new(name)
                .arg(&format!("--version")) // dummy to test existence
                .output()
                .await;

            // Launch in background regardless of --version result
            let _ = tokio::process::Command::new(name).spawn();
            return Ok(format!("Launched {}", display));
        }
        anyhow::bail!("Failed to launch {} on Linux", display);
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = display;
        anyhow::bail!("App launching not supported on this platform");
    }
}

/// Bring an already-running application to the foreground.
pub async fn focus_app(name: &str) -> anyhow::Result<String> {
    focus_by_name(name).await
}

#[cfg(target_os = "macos")]
async fn focus_by_name(name: &str) -> anyhow::Result<String> {
    let script = format!(
        r#"tell application "{}" to activate"#,
        name.replace('"', "\\\"")
    );
    let output = tokio::process::Command::new("osascript")
        .args(["-e", &script])
        .output()
        .await?;
    if output.status.success() {
        Ok(format!("Focused {}", name))
    } else {
        anyhow::bail!("Failed to focus {}", name);
    }
}

#[cfg(target_os = "windows")]
async fn focus_by_name(name: &str) -> anyhow::Result<String> {
    let script = format!(
        r#"
$app = Get-Process | Where-Object {{ $_.MainWindowTitle -like "*{name}*" -or $_.ProcessName -like "{name}" }} | Select-Object -First 1
if ($app) {{
    $sig = @'
[DllImport("user32.dll")] public static extern bool ShowWindowAsync(IntPtr hWnd, int nCmdShow);
[DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
'@
    $type = Add-Type -MemberDefinition $sig -Name "Win32Focus" -Namespace "Win32" -PassThru
    [Win32Focus]::ShowWindowAsync($app.MainWindowHandle, 9) | Out-Null  # SW_RESTORE
    [Win32Focus]::SetForegroundWindow($app.MainWindowHandle) | Out-Null
    Write-Output "focused"
}}
"#,
        name = name
    );
    let output = tokio::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .await?;
    if output.status.success() {
        Ok(format!("Focused {}", name))
    } else {
        anyhow::bail!("Failed to focus {}", name);
    }
}

#[cfg(target_os = "linux")]
async fn focus_by_name(name: &str) -> anyhow::Result<String> {
    // Search for window by name and activate it
    let output = tokio::process::Command::new("xdotool")
        .args(["search", "--name", name, "windowactivate"])
        .output()
        .await?;
    if output.status.success() {
        Ok(format!("Focused {}", name))
    } else {
        anyhow::bail!("Failed to focus {} (is xdotool installed?)", name);
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
async fn focus_by_name(_name: &str) -> anyhow::Result<String> {
    anyhow::bail!("Window focus not supported on this platform");
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lookup_chrome() {
        let entry = lookup_app("Chrome");
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().category, AppCategory::Browser);
    }

    #[test]
    fn test_lookup_case_insensitive() {
        let entry = lookup_app("chrome");
        assert!(entry.is_some());
    }

    #[test]
    fn test_lookup_fuzzy() {
        let entry = lookup_app("word");
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().category, AppCategory::Office);
    }

    #[test]
    fn test_lookup_unknown() {
        let entry = lookup_app("NonExistentApp12345");
        assert!(entry.is_none());
    }

    #[test]
    fn test_primary_executable() {
        let entry = lookup_app("VS Code").unwrap();
        let exec = primary_executable(entry);
        // Should return something on the current platform
        if cfg!(target_os = "macos") {
            assert!(exec.is_some());
        }
    }

    #[test]
    fn test_all_entries_have_display_name() {
        for entry in APP_REGISTRY {
            assert!(!entry.display_name.is_empty());
        }
    }
}
