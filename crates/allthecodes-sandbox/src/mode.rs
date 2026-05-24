//! Sandbox modes — `read-only`, `workspace`, `full` (disabled).

use std::fmt;
use std::str::FromStr;

/// Sandbox mode controlling the default level of isolation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SandboxMode {
    /// All filesystem writes are blocked (no subprocess write access).
    /// Reads follow the default allow/deny rules.
    ReadOnly,
    /// Writes allowed within the working directory (and any explicit
    /// `sandbox.filesystem.allowWrite` entries); reads allowed broadly
    /// unless explicitly denied.
    Workspace,
    /// Sandbox disabled; commands run unrestricted. Equivalent to
    /// `enabled: false`. Kept explicit so users can pin the intent.
    Full,
}

impl SandboxMode {
    pub fn as_str(self) -> &'static str {
        match self {
            SandboxMode::ReadOnly => "read-only",
            SandboxMode::Workspace => "workspace",
            SandboxMode::Full => "full",
        }
    }

    /// Default mode applied when `sandbox.enabled = true` but `sandbox.mode`
    /// is unset.
    pub fn default_enabled() -> Self {
        SandboxMode::Workspace
    }

    pub fn allows_writes(self) -> bool {
        matches!(self, SandboxMode::Workspace | SandboxMode::Full)
    }

    pub fn is_active(self) -> bool {
        !matches!(self, SandboxMode::Full)
    }
}

impl fmt::Display for SandboxMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for SandboxMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "read-only" | "readonly" | "ro" => Ok(SandboxMode::ReadOnly),
            "workspace" | "ws" => Ok(SandboxMode::Workspace),
            "full" | "off" | "disabled" | "none" => Ok(SandboxMode::Full),
            other => Err(format!(
                "unknown sandbox mode '{}'; expected read-only | workspace | full",
                other
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_round_trip() {
        for mode in [
            SandboxMode::ReadOnly,
            SandboxMode::Workspace,
            SandboxMode::Full,
        ] {
            let parsed: SandboxMode = mode.as_str().parse().unwrap();
            assert_eq!(parsed, mode);
        }
    }

    #[test]
    fn parse_aliases() {
        assert_eq!(
            "ReadOnly".parse::<SandboxMode>().unwrap(),
            SandboxMode::ReadOnly
        );
        assert_eq!("ws".parse::<SandboxMode>().unwrap(), SandboxMode::Workspace);
        assert_eq!("off".parse::<SandboxMode>().unwrap(), SandboxMode::Full);
    }

    #[test]
    fn parse_unknown() {
        assert!("sudo".parse::<SandboxMode>().is_err());
    }

    #[test]
    fn allows_writes() {
        assert!(!SandboxMode::ReadOnly.allows_writes());
        assert!(SandboxMode::Workspace.allows_writes());
        assert!(SandboxMode::Full.allows_writes());
    }

    #[test]
    fn is_active() {
        assert!(SandboxMode::ReadOnly.is_active());
        assert!(SandboxMode::Workspace.is_active());
        assert!(!SandboxMode::Full.is_active());
    }
}
