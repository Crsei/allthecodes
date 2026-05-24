//! Known managed settings key names, file names, and environment variables
//! for the MDM (Managed Device Management) settings layer.

/// Environment variable pointing to a custom managed settings file path.
pub const ALLTHECODES_MANAGED_SETTINGS: &str = "ALLTHECODES_MANAGED_SETTINGS";

/// Environment variable that forces policy enforcement regardless of the
/// managed file's `enforcement.overridable` flag.
pub const ALLTHECODES_ENFORCE_POLICY: &str = "ALLTHECODES_ENFORCE_POLICY";

/// Default file name for managed settings (platform-dependent paths are
/// resolved by `settings::managed_settings_path()`).
pub const MANAGED_SETTINGS_FILE_NAME: &str = "managed-settings.json";

/// Top-level managed policy key names that are extracted from the managed
/// settings file's extra fields.
pub const MANAGED_POLICY_KEYS: &[&str] = &["policy", "blocklist", "allowlist", "enforcement"];

/// Known settings sub-keys that, when present in the managed layer, always
/// win regardless of the normal settings priority order. These correspond to
/// security-sensitive fields where managed policy must be authoritative.
pub const MANAGED_POLICY_FIELDS: &[&str] = &[
    "permissions.deny",
    "permissions.allow",
    "permissions.ask",
    "sandbox.filesystem.allowRead",
    "sandbox.filesystem.denyRead",
    "sandbox.filesystem.allowWrite",
    "sandbox.filesystem.denyWrite",
    "sandbox.network.allowedDomains",
    "sandbox.mode",
    "sandbox.enabled",
    "hooks",
];
