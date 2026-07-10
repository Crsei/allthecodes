use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------------------------------------------------------------------------
// Typed sub-structures for richer settings
// ---------------------------------------------------------------------------

/// Prose policy used by the Auto mode classifier.
///
/// This mirrors Claude Code's `permissions.autoMode` surface. The rules are
/// intentionally prose, not permission-rule patterns: the classifier reads
/// them as extra environment, allow, and soft-deny guidance.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct AutoModeSettings {
    pub environment: Vec<String>,
    pub allow: Vec<String>,
    pub soft_deny: Vec<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

impl AutoModeSettings {
    pub fn is_effectively_empty(&self) -> bool {
        self.environment.is_empty()
            && self.allow.is_empty()
            && self.soft_deny.is_empty()
            && self.extra.is_empty()
    }
}

/// Permissions section of settings.json.
///
/// Mirrors the Claude Code TS `PermissionsSettings` shape at a high level.
/// Missing fields are `None`; empty arrays are treated the same as missing
/// for merge purposes.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct PermissionsSettings {
    /// Permission mode. One of: `default`, `ask`, `auto`, `bypass`, `plan`.
    pub default_mode: Option<String>,
    /// Tools that are always allowed (patterns).
    pub allow: Vec<String>,
    /// Tools that are always asked before execution.
    pub ask: Vec<String>,
    /// Tools that are always denied.
    pub deny: Vec<String>,
    /// Additional working directories the tools may access.
    pub additional_directories: Vec<String>,
    /// Whether `bypass` mode should be allowed at runtime.
    pub enable_bypass_mode: Option<bool>,
    /// Skip the confirmation prompt before entering bypass permissions mode.
    pub skip_dangerous_mode_permission_prompt: Option<bool>,
    /// Whether `auto` mode should be allowed at runtime.
    pub enable_auto_mode: Option<bool>,
    /// Prose policy for Auto mode's classifier.
    pub auto_mode: Option<AutoModeSettings>,
    /// Unknown fields so forward-compat is preserved.
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

impl PermissionsSettings {
    pub fn is_effectively_empty(&self) -> bool {
        self.default_mode.is_none()
            && self.allow.is_empty()
            && self.ask.is_empty()
            && self.deny.is_empty()
            && self.additional_directories.is_empty()
            && self.enable_bypass_mode.is_none()
            && self.skip_dangerous_mode_permission_prompt.is_none()
            && self.enable_auto_mode.is_none()
            && self
                .auto_mode
                .as_ref()
                .is_none_or(AutoModeSettings::is_effectively_empty)
            && self.extra.is_empty()
    }
}

/// Filesystem paths the sandbox permits or denies access to.
///
/// Paths accept three prefix conventions (matching Claude Code):
/// - `/absolute/path` — absolute path
/// - `~/relative` — relative to `$HOME`
/// - `./relative` or bare `relative` — relative to the project root (or the
///   enclosing `~/.allthecodes/` for user settings)
///
/// Lists from every [`crate::settings::SettingsSource`] are **merged**, not
/// replaced, so users can extend managed rules without overriding them.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct SandboxFilesystemSettings {
    pub allow_read: Vec<String>,
    pub deny_read: Vec<String>,
    pub allow_write: Vec<String>,
    pub deny_write: Vec<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

impl SandboxFilesystemSettings {
    pub fn is_effectively_empty(&self) -> bool {
        self.allow_read.is_empty()
            && self.deny_read.is_empty()
            && self.allow_write.is_empty()
            && self.deny_write.is_empty()
            && self.extra.is_empty()
    }
}

/// Network restrictions applied to shell subprocesses and WebFetch.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct SandboxNetworkSettings {
    /// Hard-disable all network access (matches `--no-network`).
    pub disabled: Option<bool>,
    /// Domain allowlist. Empty list means "no restriction".
    pub allowed_domains: Vec<String>,
    /// Optional HTTP proxy port for advanced network sandboxing.
    pub http_proxy_port: Option<u16>,
    /// Optional SOCKS proxy port for advanced network sandboxing.
    pub socks_proxy_port: Option<u16>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

impl SandboxNetworkSettings {
    pub fn is_effectively_empty(&self) -> bool {
        self.disabled.is_none()
            && self.allowed_domains.is_empty()
            && self.http_proxy_port.is_none()
            && self.socks_proxy_port.is_none()
            && self.extra.is_empty()
    }
}

/// Sandbox section of settings.json.
///
/// Aligned with Claude Code's `sandbox.*` settings surface. A pragmatic
/// subset of the TypeScript reference is enforced at runtime:
/// - `enabled` + `mode` drive policy assembly
/// - `failIfUnavailable` controls the hard-fail path when the OS primitives
///   (bubblewrap / sandbox-exec / Restricted Token) are missing
/// - `allowUnsandboxedCommands` gates the `dangerouslyDisableSandbox` escape
///   hatch
/// - `excludedCommands` forces specific commands outside the sandbox
/// - `filesystem.*` controls subprocess fs access (merged with Read/Edit
///   permission rules)
/// - `network.*` controls subprocess + WebFetch network access
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct SandboxSettings {
    /// Enable sandbox for shell / tool execution. Defaults to `false`.
    pub enabled: Option<bool>,
    /// Sandbox profile identifier: `read-only`, `workspace`, or `full` (off).
    pub mode: Option<String>,
    /// When `true` and the OS primitive is unavailable, fail hard instead of
    /// falling back to unsandboxed execution (defaults to `true` when enabled).
    pub fail_if_unavailable: Option<bool>,
    /// When `false`, reject the `dangerouslyDisableSandbox` escape hatch
    /// regardless of permission rules (defaults to `false` when enabled).
    pub allow_unsandboxed_commands: Option<bool>,
    /// When `true` in managed settings, only managed `allowRead` entries are
    /// respected; user/project/local entries are ignored. `denyRead` still
    /// merges from every source.
    pub allow_managed_read_paths_only: Option<bool>,
    /// When `true` in managed settings, only managed `allowedDomains` entries
    /// are respected; user/project/local entries are ignored.
    pub allow_managed_domains_only: Option<bool>,
    /// Commands (glob patterns / prefixes) that must run outside the sandbox.
    pub excluded_commands: Vec<String>,
    /// Commands that are pre-approved for sandboxed execution. When the
    /// active mode is `workspace`, commands in this list are run without
    /// going through the ask/allow flow.
    pub allowed_commands: Vec<String>,
    /// Filesystem allow/deny lists (merged with Read/Edit permission rules).
    pub filesystem: SandboxFilesystemSettings,
    /// Network policy (allowed domains + --no-network).
    pub network: SandboxNetworkSettings,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

impl SandboxSettings {
    pub fn is_effectively_empty(&self) -> bool {
        self.enabled.is_none()
            && self.mode.is_none()
            && self.fail_if_unavailable.is_none()
            && self.allow_unsandboxed_commands.is_none()
            && self.allow_managed_read_paths_only.is_none()
            && self.allow_managed_domains_only.is_none()
            && self.excluded_commands.is_empty()
            && self.allowed_commands.is_empty()
            && self.filesystem.is_effectively_empty()
            && self.network.is_effectively_empty()
            && self.extra.is_empty()
    }
}

/// Status-line configuration.
///
/// Aligned with `customize-status-line.md`: a `command` type lets the user
/// feed a JSON payload to an external script on stdin and render its stdout
/// as the terminal footer.
///
/// # Fields
///
/// - `type` — `"none"`, `"minimal"`, `"command"`, or `"script"`.
///   `"command"` is the one driven by [`crate::ui::status_line`].
/// - `command` — shell command executed for `type = command`. The runtime
///   pipes a [`crate::ui::status_line::StatusLinePayload`] JSON blob to its
///   stdin and captures stdout.
/// - `script` — path to a script (equivalent to `command` when the path is
///   executable; kept separate for schema clarity).
/// - `enabled` — explicit on/off switch. When `Some(false)` the runtime
///   always falls back to the default footer regardless of `type`.
/// - `padding` — spaces of left padding added to the rendered output.
/// - `refreshIntervalMs` — minimum gap between refreshes. Defaults to
///   `300` ms when absent (see [`Self::effective_refresh_ms`]).
/// - `timeoutMs` — how long to wait for stdout before killing the child.
///   Defaults to `2000` ms.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct StatusLineSettings {
    /// Status-line type, e.g. `none`, `minimal`, `command`, `script`.
    pub r#type: Option<String>,
    /// Inline command to execute for `command` type.
    pub command: Option<String>,
    /// Path to a script for `script` type.
    pub script: Option<String>,
    /// Optional format template (for legacy `minimal` rendering — unused by
    /// the command runner which trusts the script's stdout verbatim).
    pub format: Option<String>,
    /// Explicit on/off. When `Some(false)` the runner short-circuits even if
    /// `command`/`script` is set.
    pub enabled: Option<bool>,
    /// Spaces of left padding added to each rendered line.
    pub padding: Option<u16>,
    /// Minimum milliseconds between refreshes.
    #[serde(rename = "refreshIntervalMs")]
    pub refresh_interval_ms: Option<u64>,
    /// Milliseconds to wait for stdout before killing the child.
    #[serde(rename = "timeoutMs")]
    pub timeout_ms: Option<u64>,
    /// Forward-compat / passthrough.
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

impl StatusLineSettings {
    /// Effective refresh interval (ms). Returns the user value or the
    /// conservative default of 300 ms. Clamped to `[100, 5_000]` so a typo
    /// can't pin the CPU at 100 % or leave the bar frozen for minutes.
    pub fn effective_refresh_ms(&self) -> u64 {
        const DEFAULT_MS: u64 = 300;
        const MIN_MS: u64 = 100;
        const MAX_MS: u64 = 5_000;
        self.refresh_interval_ms
            .unwrap_or(DEFAULT_MS)
            .clamp(MIN_MS, MAX_MS)
    }

    /// Effective subprocess timeout (ms). Returns the user value or the
    /// default of 2000 ms. Clamped to `[100, 30_000]`.
    pub fn effective_timeout_ms(&self) -> u64 {
        const DEFAULT_MS: u64 = 2_000;
        const MIN_MS: u64 = 100;
        const MAX_MS: u64 = 30_000;
        self.timeout_ms.unwrap_or(DEFAULT_MS).clamp(MIN_MS, MAX_MS)
    }

    /// The command the runner should spawn, if any. Prefers `command`;
    /// falls back to `script`.
    pub fn runnable_command(&self) -> Option<&str> {
        self.command
            .as_deref()
            .or(self.script.as_deref())
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }

    /// True when the caller has opted into the `command` runner. A missing
    /// `type` still counts as enabled when a `command` is set — this matches
    /// the spec's "set a command, get a status line" ergonomics.
    pub fn is_command_mode(&self) -> bool {
        if matches!(self.enabled, Some(false)) {
            return false;
        }
        let ty = self
            .r#type
            .as_deref()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if ty == "none" {
            return false;
        }
        if matches!(ty.as_str(), "command" | "script") {
            return self.runnable_command().is_some();
        }
        // No explicit type → treat presence of a command as opt-in.
        ty.is_empty() && self.runnable_command().is_some()
    }
}

/// Spinner-tip configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct SpinnerTipsSettings {
    pub enabled: Option<bool>,
    pub interval_ms: Option<u64>,
    pub custom_tips: Vec<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}
