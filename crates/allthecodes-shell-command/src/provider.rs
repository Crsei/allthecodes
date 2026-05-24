//! Shell provider abstraction: encapsulates shell-specific command assembly.
//!
//! Each `ShellProvider` implementation knows how to:
//! - Normalize command strings for its shell dialect
//! - Set up environment variables for spawned child processes
//!
//! Tool code (`BashTool`, `PowerShellTool`) uses a provider to build the
//! `tokio::process::Command` and delegates shell-specific logic here.
//!
//! Reference: Bun `src/utils/bash/provider.ts` and `src/utils/shell/`.

use std::fmt::Debug;

use base64::Engine;
use regex::Regex;

use crate::model::ShellDialect;

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// Contract for shell-specific command construction.
///
/// Each implementation knows its executable path, invocation arguments,
/// stdin redirect policy, command normalization rules, and environment.
pub trait ShellProvider: Debug + Send + Sync {
    /// Which shell dialect this provider handles.
    fn dialect(&self) -> ShellDialect;

    /// Path to the shell executable.
    fn shell_path(&self) -> &str;

    /// Arguments for non-interactive command execution (e.g., `["-c"]`).
    fn exec_args(&self) -> &[String];

    /// Whether stdin should be redirected to `/dev/null` to prevent hangs.
    fn needs_stdin_redirect(&self) -> bool;

    /// Normalize a command string for this shell dialect.
    ///
    /// Applies transformations such as Windows null-redirect rewriting for
    /// POSIX shells, stdin redirect insertion, or PowerShell encoded-command
    /// wrapping.
    fn normalize_command(&self, command: &str) -> String;

    /// Build environment variables for the spawned shell process.
    ///
    /// Default implementation sets `GIT_PAGER=cat`, `PAGER=cat`,
    /// `CLAUDE_CODE=1`, and fills in `TERM` / `LANG` when the host
    /// environment does not already define them.
    fn build_env(&self) -> Vec<(String, String)> {
        let mut env = Vec::new();

        if std::env::var("TERM").is_err() {
            env.push(("TERM".to_string(), "xterm-256color".to_string()));
        }
        if std::env::var("LANG").is_err() && std::env::var("LC_ALL").is_err() {
            env.push(("LANG".to_string(), "en_US.UTF-8".to_string()));
        }

        env.push(("GIT_PAGER".to_string(), "cat".to_string()));
        env.push(("PAGER".to_string(), "cat".to_string()));
        env.push(("CLAUDE_CODE".to_string(), "1".to_string()));

        env
    }
}

// ---------------------------------------------------------------------------
// BashProvider
// ---------------------------------------------------------------------------

/// Provider for POSIX-compatible shells (bash, zsh, sh).
///
/// Normalisation includes:
/// - Rewriting Windows `>nul` / `2>nul` redirects to `/dev/null`
/// - Adding `< /dev/null` when no heredoc or stdin redirect exists
#[derive(Debug)]
pub struct BashProvider {
    path: String,
    args: Vec<String>,
    do_stdin_redirect: bool,
    dialect: ShellDialect,
}

impl BashProvider {
    /// Create a new provider for a detected POSIX shell.
    ///
    /// `path` — the shell executable (e.g. `/bin/bash`, `/bin/zsh`)
    /// `args` — invocation arguments (typically `["-c"]`)
    /// `do_stdin_redirect` — whether to inject `< /dev/null`
    pub fn new(path: String, args: Vec<String>, do_stdin_redirect: bool) -> Self {
        BashProvider {
            dialect: dialect_from_path(&path),
            path,
            args,
            do_stdin_redirect,
        }
    }
}

impl ShellProvider for BashProvider {
    fn dialect(&self) -> ShellDialect {
        self.dialect
    }

    fn shell_path(&self) -> &str {
        &self.path
    }

    fn exec_args(&self) -> &[String] {
        &self.args
    }

    fn needs_stdin_redirect(&self) -> bool {
        self.do_stdin_redirect
    }

    fn normalize_command(&self, command: &str) -> String {
        let cmd = if self.dialect.is_posix() {
            rewrite_windows_null_redirect(command)
        } else {
            command.to_string()
        };
        if self.do_stdin_redirect && should_add_stdin_redirect(&cmd) {
            format!("{} < /dev/null", cmd)
        } else {
            cmd
        }
    }
}

// ---------------------------------------------------------------------------
// PowerShellProvider
// ---------------------------------------------------------------------------

/// Provider for PowerShell (pwsh / powershell.exe).
///
/// On Windows uses `powershell.exe -NoProfile -NonInteractive -EncodedCommand`.
/// On non-Windows uses `pwsh -NoProfile -NonInteractive -EncodedCommand`.
#[derive(Debug)]
pub struct PowerShellProvider {
    path: String,
    args: Vec<String>,
}

impl PowerShellProvider {
    /// Create a new provider with the platform-appropriate executable.
    pub fn new() -> Self {
        let exe = if cfg!(target_os = "windows") {
            "powershell.exe"
        } else {
            "pwsh"
        };
        PowerShellProvider {
            path: exe.to_string(),
            args: vec![
                "-NoProfile".to_string(),
                "-NonInteractive".to_string(),
                "-EncodedCommand".to_string(),
            ],
        }
    }

    /// Check whether PowerShell is available on the current system.
    pub fn is_available() -> bool {
        if cfg!(target_os = "windows") {
            true
        } else {
            std::process::Command::new("pwsh")
                .arg("--version")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
        }
    }
}

impl Default for PowerShellProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl ShellProvider for PowerShellProvider {
    fn dialect(&self) -> ShellDialect {
        ShellDialect::PowerShell
    }

    fn shell_path(&self) -> &str {
        &self.path
    }

    fn exec_args(&self) -> &[String] {
        &self.args
    }

    fn needs_stdin_redirect(&self) -> bool {
        false
    }

    fn normalize_command(&self, command: &str) -> String {
        encode_powershell_command(command)
    }
}

// ---------------------------------------------------------------------------
// Normalisation helpers
// ---------------------------------------------------------------------------

fn dialect_from_path(path: &str) -> ShellDialect {
    let lower = path.to_lowercase();
    if lower.contains("bash") {
        ShellDialect::Bash
    } else if lower.contains("zsh") {
        ShellDialect::Zsh
    } else if lower.contains("fish") {
        ShellDialect::Fish
    } else if lower.contains("pwsh") || lower.contains("powershell") {
        ShellDialect::PowerShell
    } else if lower.ends_with("sh") || lower.ends_with("sh.exe") {
        ShellDialect::Sh
    } else if lower.contains("cmd") {
        ShellDialect::Cmd
    } else {
        ShellDialect::Unknown
    }
}

fn encode_powershell_command(command: &str) -> String {
    let mut bytes = Vec::with_capacity(command.len() * 2);
    for unit in command.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// Rewrite Windows CMD-style `>nul` / `2>nul` to POSIX `/dev/null`.
///
/// Port of `allthecodes_utils::bash::rewrite_windows_null_redirect`.
static NUL_RE: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
    Regex::new(r"(\d?&?>+\s*)[Nn][Uu][Ll]([\s|&;)\n]|$)").expect("invalid nul redirect regex")
});

fn rewrite_windows_null_redirect(command: &str) -> String {
    NUL_RE.replace_all(command, "${1}/dev/null${2}").to_string()
}

/// Check whether a command needs `< /dev/null` added.
fn should_add_stdin_redirect(command: &str) -> bool {
    if super::fallback::contains_heredoc(command) {
        return false;
    }
    if has_stdin_redirect(command) {
        return false;
    }
    true
}

/// Detect a `<` stdin redirect token (not `<<` heredoc or `<(` process subst).
fn has_stdin_redirect(command: &str) -> bool {
    let chars: Vec<char> = command.chars().collect();
    for (i, &ch) in chars.iter().enumerate() {
        if ch != '<' {
            continue;
        }
        if i > 0 {
            let prev = chars[i - 1];
            if !matches!(prev, ' ' | '\t' | '\n' | ';' | '&' | '|') {
                continue;
            }
        }
        if let Some(&next) = chars.get(i + 1) {
            if next == '<' || next == '(' {
                continue;
            }
        }
        return true;
    }
    false
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- BashProvider ---

    #[test]
    fn test_bash_provider_adds_stdin_redirect() {
        let p = BashProvider::new("/bin/bash".into(), vec!["-c".into()], true);
        let cmd = p.normalize_command("echo hello");
        assert_eq!(cmd, "echo hello < /dev/null");
    }

    #[test]
    fn test_bash_provider_stdin_redirect_already_present() {
        let p = BashProvider::new("/bin/bash".into(), vec!["-c".into()], true);
        let cmd = p.normalize_command("sort < input.txt");
        assert_eq!(cmd, "sort < input.txt");
    }

    #[test]
    fn test_bash_provider_with_heredoc() {
        let p = BashProvider::new("/bin/bash".into(), vec!["-c".into()], true);
        let cmd = p.normalize_command("cat << EOF\nhello\nEOF");
        assert!(!cmd.contains("/dev/null"));
    }

    #[test]
    fn test_bash_provider_no_stdin_redirect_needed() {
        let p = BashProvider::new("/bin/sh".into(), vec!["-c".into()], false);
        let cmd = p.normalize_command("echo hi");
        assert_eq!(cmd, "echo hi");
    }

    #[test]
    fn test_bash_provider_nul_redirect() {
        let p = BashProvider::new("/bin/bash".into(), vec!["-c".into()], false);
        let cmd = p.normalize_command("dir >nul");
        assert_eq!(cmd, "dir >/dev/null");
    }

    #[test]
    fn test_bash_provider_does_not_rewrite_cmd_null_redirect() {
        let p = BashProvider::new("cmd.exe".into(), vec!["/C".into()], false);
        let cmd = p.normalize_command("dir >nul");
        assert_eq!(cmd, "dir >nul");
        assert_eq!(p.dialect(), ShellDialect::Cmd);
    }

    #[test]
    fn test_bash_provider_build_env() {
        let p = BashProvider::new("/bin/bash".into(), vec!["-c".into()], true);
        let env = p.build_env();
        assert!(env.iter().any(|(k, _)| k == "GIT_PAGER"));
        assert!(env.iter().any(|(k, _)| k == "CLAUDE_CODE"));
    }

    // --- PowerShellProvider ---

    #[test]
    fn test_powershell_provider_path() {
        let p = PowerShellProvider::new();
        if cfg!(target_os = "windows") {
            assert_eq!(p.shell_path(), "powershell.exe");
        } else {
            assert_eq!(p.shell_path(), "pwsh");
        }
    }

    #[test]
    fn test_powershell_provider_args() {
        let p = PowerShellProvider::new();
        assert_eq!(
            p.exec_args(),
            &["-NoProfile", "-NonInteractive", "-EncodedCommand"]
        );
    }

    #[test]
    fn test_powershell_provider_no_stdin_redirect() {
        let p = PowerShellProvider::new();
        assert!(!p.needs_stdin_redirect());
    }

    #[test]
    fn test_powershell_provider_normalize() {
        let p = PowerShellProvider::new();
        let cmd = p.normalize_command("Get-Date");
        assert_ne!(cmd, "Get-Date");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(cmd.as_bytes())
            .expect("encoded command should be base64");
        let utf16: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        assert_eq!(String::from_utf16(&utf16).unwrap(), "Get-Date");
    }

    // --- Helpers ---

    #[test]
    fn test_has_stdin_redirect_basic() {
        assert!(has_stdin_redirect("sort < input.txt"));
        assert!(!has_stdin_redirect("cat << EOF"));
        assert!(!has_stdin_redirect("diff <(echo a) <(echo b)"));
    }

    #[test]
    fn test_rewrite_nul_redirect_simple() {
        let r = rewrite_windows_null_redirect("dir >nul");
        assert_eq!(r, "dir >/dev/null");
    }

    #[test]
    fn test_rewrite_nul_redirect_stderr() {
        let r = rewrite_windows_null_redirect("cmd 2>nul");
        assert_eq!(r, "cmd 2>/dev/null");
    }

    #[test]
    fn test_rewrite_nul_redirect_no_match() {
        let r = rewrite_windows_null_redirect("echo hello");
        assert_eq!(r, "echo hello");
    }
}
