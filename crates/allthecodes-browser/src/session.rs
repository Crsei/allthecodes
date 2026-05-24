//! `ChromeSession` — orchestrates the first-party Chrome subsystem's lifecycle.
//!
//! A session wraps the decision "should Chrome be on this run?", performs
//! the one-time setup (extension detection + manifest/shim install), and
//! updates [`state`] so `/chrome` and the system-prompt assembler can see
//! what's happening.
//!
//! Issue #4 (this PR) implements the *skeleton*: enable/disable, detection,
//! manifest install. The actual socket connection to Chrome lives in #5.
//! `connect()` is therefore a stub that moves the state machine to
//! `Enabled` (ready for #5 to flip to `Connecting → Connected`).

use anyhow::Result;
use tracing::{debug, info, warn};

use super::setup::{self, ExtensionDetection};
use super::state::{self, ChromeConnectionState};

/// How the Chrome subsystem was asked to run for this session.
///
/// Resolved once during startup by combining the CLI flag, env var, and
/// saved config (in that order). The outcome is immutable for the rest of
/// the session — `/chrome reconnect` does not change *this*, it only nudges
/// the connection state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChromeEnablement {
    /// Explicitly off (`--no-chrome`, or `ALLTHECODES_ENABLE_CFC=false`, or
    /// saved-config `claudeInChromeDefaultEnabled: false` with no opposite
    /// CLI signal).
    Disabled,
    /// Explicitly on (`--chrome` or `ALLTHECODES_ENABLE_CFC=true`).
    Enabled,
}

impl ChromeEnablement {
    pub fn is_enabled(self) -> bool {
        matches!(self, ChromeEnablement::Enabled)
    }
}

/// Resolve the session's enablement from (CLI flag, env, config).
///
/// Precedence (highest first):
/// 1. `cli_chrome == Some(true/false)` — `--chrome` / `--no-chrome`
/// 2. `ALLTHECODES_ENABLE_CFC` env var (truthy / falsy)
/// 3. `config_default` — `claudeInChromeDefaultEnabled` from settings.json
/// 4. Default: disabled.
///
/// `cli_chrome` uses `Option<bool>`: `None` = flag not given, `Some(true)` =
/// `--chrome`, `Some(false)` = `--no-chrome`.
pub fn resolve_enablement(
    cli_chrome: Option<bool>,
    config_default: Option<bool>,
) -> ChromeEnablement {
    if let Some(v) = cli_chrome {
        return if v {
            ChromeEnablement::Enabled
        } else {
            ChromeEnablement::Disabled
        };
    }

    match std::env::var("ALLTHECODES_ENABLE_CFC")
        .ok()
        .as_deref()
        .map(env_truthy_falsy)
    {
        Some(Some(true)) => return ChromeEnablement::Enabled,
        Some(Some(false)) => return ChromeEnablement::Disabled,
        _ => {}
    }

    match config_default {
        Some(true) => ChromeEnablement::Enabled,
        Some(false) => ChromeEnablement::Disabled,
        None => ChromeEnablement::Disabled,
    }
}

/// Map env-var string to `Some(true)` / `Some(false)` / `None`.
fn env_truthy_falsy(raw: &str) -> Option<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        "" => None,
        _ => None,
    }
}

/// `ChromeSession` owns the one-shot startup work for the Chrome subsystem.
pub struct ChromeSession {
    enablement: ChromeEnablement,
}

impl ChromeSession {
    /// Create a new session; no I/O yet.
    pub fn new(enablement: ChromeEnablement) -> Self {
        Self { enablement }
    }

    /// Perform startup: run extension detection, write native-host manifest
    /// + wrapper script, and transition state to `Enabled`.
    ///
    /// Returns `Ok(())` even if sub-steps fail (extension not installed,
    /// native-host install denied by the OS, etc.) — we record the error in
    /// state and let `/chrome` surface it rather than crashing the whole
    /// allthecodes process over a best-effort feature.
    pub fn start(&self) -> Result<()> {
        if !self.enablement.is_enabled() {
            state::set_connection(ChromeConnectionState::Disabled);
            return Ok(());
        }

        info!("claude-in-chrome: starting subsystem");

        // Detection is pure filesystem reads; cheap and safe to run sync.
        let detection: ExtensionDetection = setup::detect_extension_installed();
        state::set_extension_detection(detection.is_installed, detection.browser);

        // Best-effort native host install. Errors are recorded, not
        // propagated, because a session without a native host is still
        // useful — the user can install the extension later and re-run.
        //
        // In this skeleton PR the binary path points at the current
        // allthecodes binary with `--chrome-native-host` (wired in #5). That's
        // intentional: the manifest files will be correct the moment #5
        // ships, without requiring a second install step.
        match install_native_host() {
            Ok(updated) => {
                if updated > 0 {
                    debug!(
                        updated,
                        "claude-in-chrome: native host manifest(s) installed/updated"
                    );
                }
            }
            Err(e) => {
                let msg = format!("native host install failed: {e}");
                state::set_connection(ChromeConnectionState::Error(msg));
                warn!(error = %e, "claude-in-chrome: native host install failed");
                return Ok(());
            }
        }

        state::clear_error();
        state::set_connection(ChromeConnectionState::Enabled);

        info!(
            extension_installed = detection.is_installed,
            detected_browser = detection.browser.map(|b| b.slug()),
            "claude-in-chrome: subsystem enabled"
        );

        Ok(())
    }

    /// Trigger a reconnect attempt.
    ///
    /// In #4 this is a stub: re-runs detection and re-installs the manifest
    /// (idempotent), so the user can fix their setup and hit "reconnect"
    /// without restarting allthecodes. #5 will additionally re-open the socket
    /// and re-handshake with the extension.
    pub fn reconnect(&self) -> Result<()> {
        if !self.enablement.is_enabled() {
            return Ok(());
        }
        info!("claude-in-chrome: reconnect requested");
        self.start()
    }
}

/// Install the native host manifest pointing at the current allthecodes binary.
///
/// The `--chrome-native-host` flag it's wired to is the entry point #5 will
/// add. Until then, the manifest is correct-by-construction; nothing reads it
/// because no extension can talk to a flag the binary doesn't yet honor.
fn install_native_host() -> Result<usize> {
    let exe = std::env::current_exe()?;
    let command = format!("\"{}\" --chrome-native-host", exe.display());
    let wrapper = setup::create_wrapper_script(&command)?;
    setup::install_native_host_manifest(&wrapper)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn with_clean_env<F: FnOnce()>(f: F) {
        let prev = std::env::var("ALLTHECODES_ENABLE_CFC").ok();
        std::env::remove_var("ALLTHECODES_ENABLE_CFC");
        f();
        if let Some(v) = prev {
            std::env::set_var("ALLTHECODES_ENABLE_CFC", v);
        }
    }

    #[test]
    fn cli_flag_beats_env_and_config() {
        with_clean_env(|| {
            std::env::set_var("ALLTHECODES_ENABLE_CFC", "true");
            let got = resolve_enablement(Some(false), Some(true));
            assert_eq!(
                got,
                ChromeEnablement::Disabled,
                "--no-chrome overrides env and config"
            );
            std::env::remove_var("ALLTHECODES_ENABLE_CFC");
        });
    }

    #[test]
    fn env_beats_config() {
        with_clean_env(|| {
            std::env::set_var("ALLTHECODES_ENABLE_CFC", "0");
            let got = resolve_enablement(None, Some(true));
            assert_eq!(got, ChromeEnablement::Disabled);
            std::env::remove_var("ALLTHECODES_ENABLE_CFC");
        });
    }

    #[test]
    fn config_used_when_no_cli_or_env() {
        with_clean_env(|| {
            let got = resolve_enablement(None, Some(true));
            assert_eq!(got, ChromeEnablement::Enabled);
        });
    }

    #[test]
    fn default_disabled() {
        with_clean_env(|| {
            let got = resolve_enablement(None, None);
            assert_eq!(got, ChromeEnablement::Disabled);
        });
    }

    #[test]
    fn env_truthy_parsing() {
        assert_eq!(env_truthy_falsy("1"), Some(true));
        assert_eq!(env_truthy_falsy("true"), Some(true));
        assert_eq!(env_truthy_falsy("YES"), Some(true));
        assert_eq!(env_truthy_falsy("0"), Some(false));
        assert_eq!(env_truthy_falsy("false"), Some(false));
        assert_eq!(env_truthy_falsy("off"), Some(false));
        assert_eq!(env_truthy_falsy(""), None);
        assert_eq!(env_truthy_falsy("maybe"), None);
    }
}
