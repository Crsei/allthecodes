//! Sandbox error types.
//!
//! Distinguishes "the sandbox blocked this" from "the command itself
//! failed" — callers (BashTool, WebFetchTool) surface the former as a
//! policy error and the latter as an ordinary non-zero exit.

use std::fmt;
use std::path::PathBuf;

/// Reason a command or request was rejected by the sandbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxError {
    /// Sandbox mode forbids writes and the command needs to write.
    ReadOnlyModeDeniesWrite { path: PathBuf },
    /// Path matched a `denyWrite` entry.
    WriteDenied { path: PathBuf, rule: String },
    /// Path matched a `denyRead` entry and no `allowRead` re-allow.
    ReadDenied { path: PathBuf, rule: String },
    /// Write destination is outside the workspace and no `allowWrite` entry
    /// covers it.
    WriteOutsideWorkspace { path: PathBuf },
    /// Network access is globally disabled (`sandbox.network.disabled=true`
    /// or `--no-network`).
    NetworkDisabled,
    /// Domain is not in the `allowedDomains` list.
    DomainNotAllowed { host: String },
    /// `sandbox.failIfUnavailable=true` and the OS primitive is missing.
    PrimitiveUnavailable {
        platform: &'static str,
        detail: String,
    },
    /// Command matched `excludedCommands`, meaning it cannot be sandboxed
    /// and `sandbox.allowUnsandboxedCommands=false` rejected the fallback.
    EscapeHatchDisabled { command: String },
    /// Free-form policy violation — fallback variant for callers emitting
    /// custom messages.
    Policy { message: String },
}

impl fmt::Display for SandboxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SandboxError::ReadOnlyModeDeniesWrite { path } => write!(
                f,
                "sandbox: read-only mode denies write to {}",
                path.display()
            ),
            SandboxError::WriteDenied { path, rule } => write!(
                f,
                "sandbox: write to {} denied by rule '{}'",
                path.display(),
                rule
            ),
            SandboxError::ReadDenied { path, rule } => write!(
                f,
                "sandbox: read of {} denied by rule '{}'",
                path.display(),
                rule
            ),
            SandboxError::WriteOutsideWorkspace { path } => write!(
                f,
                "sandbox: write to {} is outside the workspace and has no allowWrite entry",
                path.display()
            ),
            SandboxError::NetworkDisabled => f.write_str("sandbox: network access is disabled"),
            SandboxError::DomainNotAllowed { host } => write!(
                f,
                "sandbox: host '{}' is not in sandbox.network.allowedDomains",
                host
            ),
            SandboxError::PrimitiveUnavailable { platform, detail } => write!(
                f,
                "sandbox: OS-level sandbox primitive unavailable on {} ({}). \
                 Set sandbox.failIfUnavailable=false to fall back to unsandboxed \
                 execution, or install the required tooling.",
                platform, detail
            ),
            SandboxError::EscapeHatchDisabled { command } => write!(
                f,
                "sandbox: command '{}' cannot be sandboxed and \
                 sandbox.allowUnsandboxedCommands=false rejects the fallback",
                command
            ),
            SandboxError::Policy { message } => write!(f, "sandbox: {}", message),
        }
    }
}

impl std::error::Error for SandboxError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_has_sandbox_prefix() {
        let e = SandboxError::NetworkDisabled;
        assert!(e.to_string().starts_with("sandbox:"));
    }

    #[test]
    fn display_includes_path() {
        let e = SandboxError::WriteDenied {
            path: PathBuf::from("/etc/passwd"),
            rule: "denyWrite".into(),
        };
        assert!(e.to_string().contains("/etc/passwd"));
        assert!(e.to_string().contains("denyWrite"));
    }
}
