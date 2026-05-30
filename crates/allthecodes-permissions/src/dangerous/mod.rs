//! Dangerous command detection.
//!
//! Identifies shell commands that could cause irreversible damage to the system.
//! Returns a human-readable reason string when a dangerous pattern is detected.

use regex::Regex;

pub(crate) mod auto_mode;
pub(crate) mod powershell;
pub(crate) mod shell;

pub use auto_mode::{
    restore_auto_mode_stripped_permissions, restore_dangerous_permissions_after_auto_mode,
    set_permission_mode_with_auto_mode_safety, strip_dangerous_permissions_for_active_auto_mode,
    strip_dangerous_permissions_for_auto_mode, AutoModePermissionStrip, AutoModeRuntimeTransition,
};
pub use powershell::is_dangerous_powershell_command;
pub use shell::is_dangerous_command;

/// A single danger pattern: compiled regex + human-readable reason.
pub(crate) struct DangerPattern {
    regex: Regex,
    reason: &'static str,
}
