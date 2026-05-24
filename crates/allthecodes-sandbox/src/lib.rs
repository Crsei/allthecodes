//! Sandbox runtime — OS-level isolation + Rust-level policy checks for
//! shell subprocesses and WebFetch.
//!
//! Corresponds to: issue #8 + `docs/claude-code-configuration/sandboxing.md`.
//!
//! # Layout
//!
//! - [`mode`] — [`SandboxMode`] enum (`read-only`, `workspace`, `full`)
//! - [`policy`] — [`SandboxPolicy`] assembled from `SandboxSettings` +
//!   permission rules
//! - [`runner`] — [`SandboxRunner`] trait, factory, and platform back-ends
//! - [`filesystem`] — path resolution + `allow/deny` matching
//! - [`network`] — domain allowlist matching
//! - [`availability`] — detects whether the OS primitive (bubblewrap,
//!   sandbox-exec, Windows Restricted Token) is usable on this host
//!
//! # Status per platform
//!
//! | Platform | OS-level enforcement |
//! |----------|---------------------|
//! | Linux / WSL2 | bubblewrap wrapper (when `bwrap` is on `$PATH`) |
//! | macOS | `sandbox-exec` (Seatbelt) wrapper |
//! | Windows | Rust-level policy checks only; no upstream OS-level parity |
//!
//! On every platform, path-based allow/deny checks and network-domain checks
//! run **in-process before spawn** as a best-effort second line of defence,
//! even when the OS primitive isn't available. `sandbox.failIfUnavailable`
//! controls whether the missing OS primitive is a hard error.

pub mod availability;
pub mod errors;
pub mod filesystem;
pub mod mode;
pub mod network;
pub mod policy;
pub mod runner;

// Re-export only what the rest of the crate consumes. Deeper entry points
// (e.g. `SandboxPolicyBuilder` for tests) can still reach the submodules
// directly via `crate::sandbox::policy::SandboxPolicyBuilder`.
pub use availability::{Availability, Mechanism};
pub use errors::SandboxError;
pub use mode::SandboxMode;
pub use network::NetworkDecision;
pub use runner::{make_runner, policy_from_app_state, preflight_shell_command};
