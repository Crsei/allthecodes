//! Runtime state for the first-party Chrome integration.
//!
//! One `ChromeState` per process, stored behind a `parking_lot::RwLock` so
//! the `/chrome` command and the native-host connection loop can read the
//! status without contention.
//!
//! The fields here are *intentionally narrow*: they track things the user
//! asks about ("is Chrome enabled? is the extension installed? are we
//! connected?") and leave the heavy lifting (manifest writes, socket I/O)
//! to other modules.

use std::sync::OnceLock;

use parking_lot::RwLock;

use super::common::ChromiumBrowser;

/// Coarse connection state for the Chrome subsystem.
///
/// Lifecycle: `Disabled` → `Enabled` → `Connecting` → (`Connected` | `Error(_)`).
/// Disconnect flips back to `Enabled` (the user can run `/chrome reconnect`).
///
/// `Connecting` and `Connected` are not constructed in the #4 skeleton — they
/// belong to the native-host transport layer in #5. They're declared here so
/// the state machine is complete and `/chrome` can display them when #5
/// lands without a second data-model change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChromeConnectionState {
    /// `--no-chrome` or never-enabled session.
    Disabled,
    /// `--chrome` (or auto-enabled): subsystem is on but no native-host
    /// session exists yet.
    Enabled,
    /// Attempting to connect to the extension via the native host. (#5)
    Connecting,
    /// Native host ↔ extension handshake complete; browser tools are live. (#5)
    Connected {
        /// Which browser provided the extension that connected.
        browser: ChromiumBrowser,
    },
    /// Connection failed or dropped; message is user-facing.
    Error(String),
}

impl ChromeConnectionState {
    /// One-word label suitable for `/chrome` status.
    pub fn label(&self) -> &'static str {
        match self {
            ChromeConnectionState::Disabled => "disabled",
            ChromeConnectionState::Enabled => "enabled (not yet connected)",
            ChromeConnectionState::Connecting => "connecting",
            ChromeConnectionState::Connected { .. } => "connected",
            ChromeConnectionState::Error(_) => "error",
        }
    }

    pub fn is_active(&self) -> bool {
        matches!(
            self,
            ChromeConnectionState::Enabled
                | ChromeConnectionState::Connecting
                | ChromeConnectionState::Connected { .. }
        )
    }
}

/// Full runtime state snapshot for the Chrome subsystem.
///
/// Cloned cheaply (enum + bool + `Option<String>`), so readers can grab a
/// snapshot without holding the lock while they format output.
#[derive(Debug, Clone)]
pub struct ChromeState {
    /// Current connection state.
    pub connection: ChromeConnectionState,
    /// Whether the Anthropic Chrome extension was detected on disk.
    ///
    /// Detection runs asynchronously during session setup; `None` means
    /// "haven't checked yet". See `setup::detect_extension_installed`.
    pub extension_installed: Option<bool>,
    /// Which browser the extension was detected in (if any).
    pub detected_browser: Option<ChromiumBrowser>,
    /// The last error reported by the connection loop (for `/chrome` to show).
    pub last_error: Option<String>,
}

impl Default for ChromeState {
    fn default() -> Self {
        Self {
            connection: ChromeConnectionState::Disabled,
            extension_installed: None,
            detected_browser: None,
            last_error: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Global singleton
// ---------------------------------------------------------------------------
//
// A process-wide state lives behind a RwLock. The alternative — threading a
// handle through every caller — balloons the API surface for a feature that
// genuinely is process-singleton (there's at most one Chrome session per
// allthecodes run).

static CHROME_STATE: OnceLock<RwLock<ChromeState>> = OnceLock::new();

fn state() -> &'static RwLock<ChromeState> {
    CHROME_STATE.get_or_init(|| RwLock::new(ChromeState::default()))
}

/// Read the current state.
pub fn snapshot() -> ChromeState {
    state().read().clone()
}

/// Set the connection state.
pub fn set_connection(new: ChromeConnectionState) {
    let mut s = state().write();
    // Mirror Error(_) into last_error for convenience.
    if let ChromeConnectionState::Error(ref msg) = new {
        s.last_error = Some(msg.clone());
    }
    s.connection = new;
}

/// Mark extension-installed status + which browser it was found in.
pub fn set_extension_detection(installed: bool, browser: Option<ChromiumBrowser>) {
    let mut s = state().write();
    s.extension_installed = Some(installed);
    s.detected_browser = browser;
}

/// Clear the last error (e.g. after a successful reconnect).
pub fn clear_error() {
    let mut s = state().write();
    s.last_error = None;
}

/// Convenience: are we currently enabled in any form?
pub fn is_enabled() -> bool {
    state().read().connection.is_active()
}

/// Reset state — tests only. Public across crates so tests in the root
/// crate (chrome_cmd etc.) can exercise the command flow with a clean
/// Chrome subsystem snapshot; `#[doc(hidden)]` keeps it out of rustdoc.
#[doc(hidden)]
pub fn reset_for_tests() {
    let mut s = state().write();
    *s = ChromeState::default();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn test_lock() -> std::sync::MutexGuard<'static, ()> {
        use std::sync::{Mutex, OnceLock};
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn default_is_disabled() {
        let _guard = test_lock();
        reset_for_tests();
        let s = snapshot();
        assert_eq!(s.connection, ChromeConnectionState::Disabled);
        assert!(!s.connection.is_active());
    }

    #[test]
    fn set_connection_transitions() {
        let _guard = test_lock();
        reset_for_tests();

        set_connection(ChromeConnectionState::Enabled);
        assert!(is_enabled());

        set_connection(ChromeConnectionState::Connecting);
        assert!(is_enabled());

        set_connection(ChromeConnectionState::Connected {
            browser: ChromiumBrowser::Chrome,
        });
        assert!(is_enabled());

        set_connection(ChromeConnectionState::Error("boom".into()));
        assert!(!is_enabled());
        assert_eq!(snapshot().last_error.as_deref(), Some("boom"));

        clear_error();
        assert!(snapshot().last_error.is_none());
    }

    #[test]
    fn extension_detection_stored() {
        let _guard = test_lock();
        reset_for_tests();

        set_extension_detection(true, Some(ChromiumBrowser::Chrome));
        let s = snapshot();
        assert_eq!(s.extension_installed, Some(true));
        assert_eq!(s.detected_browser, Some(ChromiumBrowser::Chrome));
    }
}
