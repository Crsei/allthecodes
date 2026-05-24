//! Cross-platform Chromium browser paths + identifiers for the first-party
//! Chrome integration ("Claude in Chrome").
//!
//! The `ChromiumBrowser` enum enumerates every browser we know how to find on
//! disk. Each browser has per-platform data paths (for extension detection)
//! and per-platform native-messaging-host paths (for installing the native
//! host manifest). Windows uses registry keys instead of file locations for
//! native messaging discovery — those live here too.
//!
//! Mirrors `claude-code-bun/src/utils/claudeInChrome/common.ts` + `setupPortable.ts`.
//!
//! Nothing in this file performs I/O; detection and manifest install live in
//! `setup.rs`. This keeps the table pure so tests can reason about it without
//! touching the real filesystem.

use std::path::PathBuf;

/// Chromium-based browsers allthecodes knows how to find and talk to.
///
/// Ordered by popularity — callers that want "the first browser we can see"
/// should iterate in [`BROWSER_DETECTION_ORDER`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChromiumBrowser {
    Chrome,
    Brave,
    Arc,
    Chromium,
    Edge,
    Vivaldi,
    Opera,
}

impl ChromiumBrowser {
    /// Human-readable name (e.g. `"Google Chrome"`).
    pub fn display_name(self) -> &'static str {
        match self {
            ChromiumBrowser::Chrome => "Google Chrome",
            ChromiumBrowser::Brave => "Brave",
            ChromiumBrowser::Arc => "Arc",
            ChromiumBrowser::Chromium => "Chromium",
            ChromiumBrowser::Edge => "Microsoft Edge",
            ChromiumBrowser::Vivaldi => "Vivaldi",
            ChromiumBrowser::Opera => "Opera",
        }
    }

    /// Short identifier used in logs and config (e.g. `"chrome"`).
    pub fn slug(self) -> &'static str {
        match self {
            ChromiumBrowser::Chrome => "chrome",
            ChromiumBrowser::Brave => "brave",
            ChromiumBrowser::Arc => "arc",
            ChromiumBrowser::Chromium => "chromium",
            ChromiumBrowser::Edge => "edge",
            ChromiumBrowser::Vivaldi => "vivaldi",
            ChromiumBrowser::Opera => "opera",
        }
    }
}

/// Iteration order for "first available browser" lookups. Mirrors
/// the bun version's `BROWSER_DETECTION_ORDER`.
pub const BROWSER_DETECTION_ORDER: &[ChromiumBrowser] = &[
    ChromiumBrowser::Chrome,
    ChromiumBrowser::Brave,
    ChromiumBrowser::Arc,
    ChromiumBrowser::Edge,
    ChromiumBrowser::Chromium,
    ChromiumBrowser::Vivaldi,
    ChromiumBrowser::Opera,
];

// ---------------------------------------------------------------------------
// Native host identifier + extension IDs
// ---------------------------------------------------------------------------

/// Native messaging host identifier written into the manifest `name` field.
/// Must match the ID hard-coded in the Anthropic Chrome extension.
pub const NATIVE_HOST_IDENTIFIER: &str = "com.anthropic.claude_code_browser_extension";

/// Production extension ID (distributed via the Chrome Web Store).
pub const PROD_EXTENSION_ID: &str = "fcoeoabgfenejglbffodgkkbkcdhcgfn";

/// Development extension ID (internal Anthropic builds).
pub const DEV_EXTENSION_ID: &str = "dihbgbndebgnbjfmelmegjepbnkhlgni";

/// Anthropic-internal extension ID.
pub const ANT_EXTENSION_ID: &str = "dngcpimnedloihjnnfngkgjoidhnaolf";

/// Chrome Web Store URL for end users to install the extension.
pub const CHROME_EXTENSION_URL: &str = "https://claude.ai/chrome";

/// Reconnect URL the browser opens after native host install.
pub const CHROME_RECONNECT_URL: &str = "https://clau.de/chrome/reconnect";

/// Permissions management URL.
pub const CHROME_PERMISSIONS_URL: &str = "https://clau.de/chrome/permissions";

/// Extension IDs we look for when detecting installation.
///
/// Dev and Ant IDs are only returned when `USER_TYPE=ant` (internal builds),
/// matching the bun behavior. End-user builds only match the prod ID.
pub fn extension_ids() -> Vec<&'static str> {
    if std::env::var("USER_TYPE").as_deref() == Ok("ant") {
        vec![PROD_EXTENSION_ID, DEV_EXTENSION_ID, ANT_EXTENSION_ID]
    } else {
        vec![PROD_EXTENSION_ID]
    }
}

/// MCP server name under which the first-party Chrome bridge registers its
/// tools. Tools appear to the model as `mcp__claude-in-chrome__*`.
pub const CLAUDE_IN_CHROME_MCP_SERVER_NAME: &str = "claude-in-chrome";

// ---------------------------------------------------------------------------
// Per-platform, per-browser config tables
// ---------------------------------------------------------------------------

/// Home-relative path segments for browser user data + native messaging.
///
/// Every field is used on *some* platform via the cfg-gated path helpers
/// below, but the dead-code lint can only see one platform's usage at a
/// time. Fields not relevant on the current platform carry
/// `#[cfg_attr(not(target_os = "…"), allow(dead_code))]` so the struct can
/// be constructed uniformly in `config_for()` on all platforms while
/// keeping the per-platform path-accessor functions warning-free.
#[derive(Debug, Clone, Copy)]
struct BrowserConfig {
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    macos_app_bundle: &'static str,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    macos_data: &'static [&'static str],
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    macos_native_messaging: &'static [&'static str],
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    linux_binaries: &'static [&'static str],
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    linux_data: &'static [&'static str],
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    linux_native_messaging: &'static [&'static str],
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    windows_data: &'static [&'static str],
    windows_registry_key: &'static str,
    /// Opera uses `AppData/Roaming` instead of `AppData/Local`.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    windows_use_roaming: bool,
}

fn config_for(browser: ChromiumBrowser) -> BrowserConfig {
    match browser {
        ChromiumBrowser::Chrome => BrowserConfig {
            macos_app_bundle: "Google Chrome",
            macos_data: &["Library", "Application Support", "Google", "Chrome"],
            macos_native_messaging: &[
                "Library",
                "Application Support",
                "Google",
                "Chrome",
                "NativeMessagingHosts",
            ],
            linux_binaries: &["google-chrome", "google-chrome-stable"],
            linux_data: &[".config", "google-chrome"],
            linux_native_messaging: &[".config", "google-chrome", "NativeMessagingHosts"],
            windows_data: &["Google", "Chrome", "User Data"],
            windows_registry_key: r"Software\Google\Chrome\NativeMessagingHosts",
            windows_use_roaming: false,
        },
        ChromiumBrowser::Brave => BrowserConfig {
            macos_app_bundle: "Brave Browser",
            macos_data: &[
                "Library",
                "Application Support",
                "BraveSoftware",
                "Brave-Browser",
            ],
            macos_native_messaging: &[
                "Library",
                "Application Support",
                "BraveSoftware",
                "Brave-Browser",
                "NativeMessagingHosts",
            ],
            linux_binaries: &["brave-browser", "brave"],
            linux_data: &[".config", "BraveSoftware", "Brave-Browser"],
            linux_native_messaging: &[
                ".config",
                "BraveSoftware",
                "Brave-Browser",
                "NativeMessagingHosts",
            ],
            windows_data: &["BraveSoftware", "Brave-Browser", "User Data"],
            windows_registry_key: r"Software\BraveSoftware\Brave-Browser\NativeMessagingHosts",
            windows_use_roaming: false,
        },
        ChromiumBrowser::Arc => BrowserConfig {
            macos_app_bundle: "Arc",
            macos_data: &["Library", "Application Support", "Arc", "User Data"],
            macos_native_messaging: &[
                "Library",
                "Application Support",
                "Arc",
                "User Data",
                "NativeMessagingHosts",
            ],
            linux_binaries: &[],
            linux_data: &[],
            linux_native_messaging: &[],
            windows_data: &["Arc", "User Data"],
            windows_registry_key: r"Software\ArcBrowser\Arc\NativeMessagingHosts",
            windows_use_roaming: false,
        },
        ChromiumBrowser::Chromium => BrowserConfig {
            macos_app_bundle: "Chromium",
            macos_data: &["Library", "Application Support", "Chromium"],
            macos_native_messaging: &[
                "Library",
                "Application Support",
                "Chromium",
                "NativeMessagingHosts",
            ],
            linux_binaries: &["chromium", "chromium-browser"],
            linux_data: &[".config", "chromium"],
            linux_native_messaging: &[".config", "chromium", "NativeMessagingHosts"],
            windows_data: &["Chromium", "User Data"],
            windows_registry_key: r"Software\Chromium\NativeMessagingHosts",
            windows_use_roaming: false,
        },
        ChromiumBrowser::Edge => BrowserConfig {
            macos_app_bundle: "Microsoft Edge",
            macos_data: &["Library", "Application Support", "Microsoft Edge"],
            macos_native_messaging: &[
                "Library",
                "Application Support",
                "Microsoft Edge",
                "NativeMessagingHosts",
            ],
            linux_binaries: &["microsoft-edge", "microsoft-edge-stable"],
            linux_data: &[".config", "microsoft-edge"],
            linux_native_messaging: &[".config", "microsoft-edge", "NativeMessagingHosts"],
            windows_data: &["Microsoft", "Edge", "User Data"],
            windows_registry_key: r"Software\Microsoft\Edge\NativeMessagingHosts",
            windows_use_roaming: false,
        },
        ChromiumBrowser::Vivaldi => BrowserConfig {
            macos_app_bundle: "Vivaldi",
            macos_data: &["Library", "Application Support", "Vivaldi"],
            macos_native_messaging: &[
                "Library",
                "Application Support",
                "Vivaldi",
                "NativeMessagingHosts",
            ],
            linux_binaries: &["vivaldi", "vivaldi-stable"],
            linux_data: &[".config", "vivaldi"],
            linux_native_messaging: &[".config", "vivaldi", "NativeMessagingHosts"],
            windows_data: &["Vivaldi", "User Data"],
            windows_registry_key: r"Software\Vivaldi\NativeMessagingHosts",
            windows_use_roaming: false,
        },
        ChromiumBrowser::Opera => BrowserConfig {
            macos_app_bundle: "Opera",
            macos_data: &["Library", "Application Support", "com.operasoftware.Opera"],
            macos_native_messaging: &[
                "Library",
                "Application Support",
                "com.operasoftware.Opera",
                "NativeMessagingHosts",
            ],
            linux_binaries: &["opera"],
            linux_data: &[".config", "opera"],
            linux_native_messaging: &[".config", "opera", "NativeMessagingHosts"],
            windows_data: &["Opera Software", "Opera Stable"],
            windows_registry_key: r"Software\Opera Software\Opera Stable\NativeMessagingHosts",
            windows_use_roaming: true,
        },
    }
}

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

/// A browser's on-disk user-data directory (where profiles and Extensions live).
#[derive(Debug, Clone)]
pub struct BrowserDataPath {
    pub browser: ChromiumBrowser,
    pub path: PathBuf,
}

/// A browser's native-messaging-hosts directory (where the manifest is written).
#[derive(Debug, Clone)]
pub struct NativeMessagingPath {
    pub browser: ChromiumBrowser,
    pub path: PathBuf,
}

/// A Windows registry key entry for native host discovery.
#[derive(Debug, Clone)]
pub struct WindowsRegistryKey {
    pub browser: ChromiumBrowser,
    /// Path under HKCU (without the `HKCU\\` prefix).
    pub key: String,
}

fn home_dir() -> Option<PathBuf> {
    dirs::home_dir()
}

#[cfg(target_os = "windows")]
fn appdata_base(use_roaming: bool) -> Option<PathBuf> {
    let home = home_dir()?;
    if use_roaming {
        Some(home.join("AppData").join("Roaming"))
    } else {
        Some(home.join("AppData").join("Local"))
    }
}

/// Build the platform-appropriate user-data directory path for a single
/// browser. Returns `None` if the browser isn't supported on this platform
/// (e.g. Arc on Linux).
pub fn data_path_for(browser: ChromiumBrowser) -> Option<PathBuf> {
    let cfg = config_for(browser);

    #[cfg(target_os = "macos")]
    {
        if cfg.macos_data.is_empty() {
            return None;
        }
        let mut p = home_dir()?;
        for seg in cfg.macos_data {
            p.push(seg);
        }
        Some(p)
    }

    #[cfg(target_os = "linux")]
    {
        if cfg.linux_data.is_empty() {
            return None;
        }
        let mut p = home_dir()?;
        for seg in cfg.linux_data {
            p.push(seg);
        }
        Some(p)
    }

    #[cfg(target_os = "windows")]
    {
        if cfg.windows_data.is_empty() {
            return None;
        }
        let mut p = appdata_base(cfg.windows_use_roaming)?;
        for seg in cfg.windows_data {
            p.push(seg);
        }
        Some(p)
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = cfg;
        None
    }
}

/// Build the native-messaging-hosts directory for a single browser on the
/// current platform. Returns `None` on Windows (Windows uses registry, not a
/// file path) and on platforms where the browser isn't supported.
pub fn native_messaging_path_for(browser: ChromiumBrowser) -> Option<PathBuf> {
    let cfg = config_for(browser);

    #[cfg(target_os = "macos")]
    {
        if cfg.macos_native_messaging.is_empty() {
            return None;
        }
        let mut p = home_dir()?;
        for seg in cfg.macos_native_messaging {
            p.push(seg);
        }
        Some(p)
    }

    #[cfg(target_os = "linux")]
    {
        if cfg.linux_native_messaging.is_empty() {
            return None;
        }
        let mut p = home_dir()?;
        for seg in cfg.linux_native_messaging {
            p.push(seg);
        }
        Some(p)
    }

    #[cfg(target_os = "windows")]
    {
        // Windows uses a single shared manifest directory pointed at by registry.
        let _ = cfg;
        None
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = cfg;
        None
    }
}

/// macOS application bundle name for launching or diagnosing a browser.
#[cfg(target_os = "macos")]
pub fn macos_app_bundle_for(browser: ChromiumBrowser) -> &'static str {
    config_for(browser).macos_app_bundle
}

/// Linux executable names used when probing for an installed browser.
#[cfg(target_os = "linux")]
pub fn linux_binary_names_for(browser: ChromiumBrowser) -> &'static [&'static str] {
    config_for(browser).linux_binaries
}

/// Enumerate every supported browser's user-data path on the current platform.
pub fn all_browser_data_paths() -> Vec<BrowserDataPath> {
    BROWSER_DETECTION_ORDER
        .iter()
        .filter_map(|&browser| data_path_for(browser).map(|path| BrowserDataPath { browser, path }))
        .collect()
}

/// Enumerate every supported browser's native-messaging-hosts directory on
/// the current platform. Empty on Windows (registry is used instead).
pub fn all_native_messaging_paths() -> Vec<NativeMessagingPath> {
    BROWSER_DETECTION_ORDER
        .iter()
        .filter_map(|&browser| {
            native_messaging_path_for(browser).map(|path| NativeMessagingPath { browser, path })
        })
        .collect()
}

/// Enumerate every supported browser's Windows registry key (empty on non-Windows).
pub fn all_windows_registry_keys() -> Vec<WindowsRegistryKey> {
    BROWSER_DETECTION_ORDER
        .iter()
        .map(|&browser| {
            let cfg = config_for(browser);
            WindowsRegistryKey {
                browser,
                key: cfg.windows_registry_key.to_string(),
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Platform capability
// ---------------------------------------------------------------------------

/// Whether the first-party Chrome integration is supported on the current OS.
///
/// Supported: macOS, Linux, Windows. Everything else (non-WSL BSD, etc.)
/// returns `false` and the `/chrome` command should tell the user so.
pub fn supports_claude_in_chrome() -> bool {
    cfg!(any(
        target_os = "macos",
        target_os = "linux",
        target_os = "windows"
    ))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_browsers_have_distinct_slugs() {
        let mut slugs: Vec<&str> = BROWSER_DETECTION_ORDER.iter().map(|b| b.slug()).collect();
        slugs.sort_unstable();
        slugs.dedup();
        assert_eq!(slugs.len(), BROWSER_DETECTION_ORDER.len());
    }

    #[test]
    fn extension_ids_default_to_prod_only() {
        // Reset USER_TYPE so we don't accidentally pick up the ant variant.
        let prev = std::env::var("USER_TYPE").ok();
        std::env::remove_var("USER_TYPE");

        let ids = extension_ids();
        assert_eq!(ids, vec![PROD_EXTENSION_ID]);

        if let Some(v) = prev {
            std::env::set_var("USER_TYPE", v);
        }
    }

    #[test]
    fn registry_keys_exist_for_every_browser() {
        let keys = all_windows_registry_keys();
        assert_eq!(keys.len(), BROWSER_DETECTION_ORDER.len());
        for k in &keys {
            assert!(k.key.starts_with("Software\\"));
            assert!(k.key.contains("NativeMessagingHosts"));
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_binary_names_cover_linux_browsers() {
        assert!(linux_binary_names_for(ChromiumBrowser::Chrome).contains(&"google-chrome"));
        assert!(linux_binary_names_for(ChromiumBrowser::Arc).is_empty());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_app_bundle_names_cover_macos_browsers() {
        assert_eq!(
            macos_app_bundle_for(ChromiumBrowser::Chrome),
            "Google Chrome"
        );
        assert_eq!(macos_app_bundle_for(ChromiumBrowser::Arc), "Arc");
    }

    #[test]
    fn data_path_resolves_on_current_platform() {
        // At least Chrome should resolve to *some* path on each supported OS.
        let p = data_path_for(ChromiumBrowser::Chrome);
        if supports_claude_in_chrome() {
            assert!(
                p.is_some(),
                "Chrome data path should resolve on supported OS"
            );
        }
    }

    #[test]
    fn detection_order_puts_chrome_first() {
        assert_eq!(BROWSER_DETECTION_ORDER[0], ChromiumBrowser::Chrome);
    }
}
