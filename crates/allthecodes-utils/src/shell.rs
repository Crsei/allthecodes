//! Shell detection and environment initialization.
//!
//! Detects the user's default shell, provides platform-aware shell
//! configuration, and exports environment helpers used by shell tools.
//!
//! Reference: TypeScript `src/utils/shell/` directory.

use std::env;
use std::path::PathBuf;

// =============================================================================
// Shell provider types
// =============================================================================

/// Supported shell types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellKind {
    Bash,
    Zsh,
    Fish,
    PowerShell,
    Cmd,
    Sh,
    Unknown,
}

impl ShellKind {
    /// Display name for the shell.
    pub fn display_name(&self) -> &'static str {
        match self {
            ShellKind::Bash => "bash",
            ShellKind::Zsh => "zsh",
            ShellKind::Fish => "fish",
            ShellKind::PowerShell => "powershell",
            ShellKind::Cmd => "cmd",
            ShellKind::Sh => "sh",
            ShellKind::Unknown => "unknown",
        }
    }

    /// Whether this shell uses POSIX-compatible syntax.
    pub fn is_posix(&self) -> bool {
        matches!(self, ShellKind::Bash | ShellKind::Zsh | ShellKind::Sh)
    }
}

/// Shell configuration for command execution.
#[derive(Debug, Clone)]
pub struct ShellConfig {
    /// Shell kind.
    pub kind: ShellKind,
    /// Path to the shell executable.
    pub path: String,
    /// Arguments to pass for non-interactive command execution (e.g., ["-c"]).
    pub exec_args: Vec<String>,
    /// Whether stdin redirect to /dev/null is needed.
    pub needs_stdin_redirect: bool,
}

// =============================================================================
// Shell detection
// =============================================================================

/// Detect the default shell for the current platform.
///
/// On Unix: checks $SHELL, falls back to /bin/sh.
/// On Windows: uses bash (Git Bash) if available, otherwise cmd.
pub fn detect_default_shell() -> ShellConfig {
    if cfg!(target_os = "windows") {
        detect_windows_shell()
    } else {
        detect_unix_shell()
    }
}

/// Detect the shell on Unix systems.
fn detect_unix_shell() -> ShellConfig {
    let shell_path = env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    let kind = shell_kind_from_path(&shell_path);

    ShellConfig {
        kind,
        path: shell_path,
        exec_args: vec!["-c".to_string()],
        needs_stdin_redirect: true,
    }
}

/// Detect the shell on Windows.
///
/// Prefers Git Bash if available (POSIX commands work consistently),
/// falls back to cmd.exe. PowerShell is detected but not default
/// because Claude Code's system prompt assumes Unix shell syntax.
fn detect_windows_shell() -> ShellConfig {
    // Check for Git Bash
    if let Some(git_bash) = find_git_bash() {
        return ShellConfig {
            kind: ShellKind::Bash,
            path: git_bash,
            exec_args: vec!["-c".to_string()],
            needs_stdin_redirect: true,
        };
    }

    // Check COMSPEC (usually cmd.exe)
    let cmd_path = env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string());

    ShellConfig {
        kind: ShellKind::Cmd,
        path: cmd_path,
        exec_args: vec!["/C".to_string()],
        needs_stdin_redirect: false,
    }
}

/// Determine the shell kind from its executable path.
fn shell_kind_from_path(path: &str) -> ShellKind {
    let lower = path.to_lowercase();
    if lower.contains("bash") {
        ShellKind::Bash
    } else if lower.contains("zsh") {
        ShellKind::Zsh
    } else if lower.contains("fish") {
        ShellKind::Fish
    } else if lower.contains("pwsh") || lower.contains("powershell") {
        ShellKind::PowerShell
    } else if lower.ends_with("sh") || lower.ends_with("sh.exe") {
        ShellKind::Sh
    } else if lower.contains("cmd") {
        ShellKind::Cmd
    } else {
        ShellKind::Unknown
    }
}

/// Try to find Git Bash on Windows.
fn find_git_bash() -> Option<String> {
    // Common Git Bash locations
    let candidates = [
        "C:\\Program Files\\Git\\bin\\bash.exe",
        "C:\\Program Files (x86)\\Git\\bin\\bash.exe",
    ];

    for path in &candidates {
        if std::path::Path::new(path).exists() {
            return Some(path.to_string());
        }
    }

    // Check if bash is on PATH
    if which_bash().is_some() {
        return Some("bash".to_string());
    }

    None
}

/// Check if bash is available via PATH.
fn which_bash() -> Option<PathBuf> {
    env::var_os("PATH").and_then(|paths| {
        env::split_paths(&paths).find_map(|dir| {
            let candidate = dir.join("bash.exe");
            if candidate.exists() {
                Some(candidate)
            } else {
                let candidate = dir.join("bash");
                if candidate.exists() {
                    Some(candidate)
                } else {
                    None
                }
            }
        })
    })
}

// =============================================================================
// Environment initialization
// =============================================================================

/// Build a minimal environment for spawned shell processes.
///
/// Inherits the current process environment but ensures critical
/// variables are set (e.g., TERM, LANG) for consistent behavior.
pub fn build_shell_env() -> Vec<(String, String)> {
    let mut env_vars: Vec<(String, String)> = Vec::new();

    // Ensure TERM is set (for colored output in subprocesses)
    if env::var("TERM").is_err() {
        env_vars.push(("TERM".to_string(), "xterm-256color".to_string()));
    }

    // Ensure LANG is set for consistent UTF-8 handling
    if env::var("LANG").is_err() && env::var("LC_ALL").is_err() {
        env_vars.push(("LANG".to_string(), "en_US.UTF-8".to_string()));
    }

    // Disable pager in git and other tools
    env_vars.push(("GIT_PAGER".to_string(), "cat".to_string()));
    env_vars.push(("PAGER".to_string(), "cat".to_string()));

    // Mark that we're running inside Claude Code
    env_vars.push(("CLAUDE_CODE".to_string(), "1".to_string()));

    env_vars
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shell_kind_from_path() {
        assert_eq!(shell_kind_from_path("/bin/bash"), ShellKind::Bash);
        assert_eq!(shell_kind_from_path("/usr/bin/zsh"), ShellKind::Zsh);
        assert_eq!(shell_kind_from_path("/usr/bin/fish"), ShellKind::Fish);
        assert_eq!(shell_kind_from_path("pwsh"), ShellKind::PowerShell);
        assert_eq!(shell_kind_from_path("/bin/sh"), ShellKind::Sh);
        assert_eq!(shell_kind_from_path("cmd.exe"), ShellKind::Cmd);
        assert_eq!(shell_kind_from_path("unknown_shell"), ShellKind::Unknown);
    }

    #[test]
    fn test_shell_kind_posix() {
        assert!(ShellKind::Bash.is_posix());
        assert!(ShellKind::Zsh.is_posix());
        assert!(ShellKind::Sh.is_posix());
        assert!(!ShellKind::PowerShell.is_posix());
        assert!(!ShellKind::Cmd.is_posix());
        assert!(!ShellKind::Fish.is_posix());
    }

    #[test]
    fn test_detect_default_shell() {
        let config = detect_default_shell();
        // Should always return a valid shell
        assert!(!config.path.is_empty());
        assert!(!config.exec_args.is_empty());
    }

    #[test]
    fn test_build_shell_env() {
        let env = build_shell_env();
        // Should always include GIT_PAGER and CLAUDE_CODE
        assert!(env.iter().any(|(k, _)| k == "GIT_PAGER"));
        assert!(env.iter().any(|(k, _)| k == "CLAUDE_CODE"));
    }

    #[test]
    fn test_shell_display_name() {
        assert_eq!(ShellKind::Bash.display_name(), "bash");
        assert_eq!(ShellKind::PowerShell.display_name(), "powershell");
    }
}
