//! Read-only shell command validation.
//!
//! Port of Bun's `readOnlyCommandValidation.ts` for classifying shell commands
//! as read-only or not. Used by Plan mode and Explore mode to decide whether
//! a Bash/PowerShell command can execute without user approval.
//!
//! # Design
//!
//! Each known command (git, gh, docker, rg, etc.) has a config of safe flags.
//! Unknown flags or positional arguments that could be dangerous cause the
//! command to be classified as NOT read-only.
//!
//! This is FAIL-CLOSED: any command we can't classify as read-only is treated
//! as potentially dangerous.

mod commands;
mod flag_validation;
mod shell_classifier;
#[cfg(test)]
mod tests;
mod types;

pub use types::{ExternalCommandConfig, FlagArgType};

use allthecodes_shell_command::model::ReadOnlyResult;

use commands::{
    make_docker_read_only_commands, make_external_readonly_commands, make_gh_read_only_commands,
    make_git_read_only_commands, make_pyright_read_only_commands, make_rg_read_only_commands,
};
use flag_validation::{match_readonly_command, validate_flags};
pub use shell_classifier::{
    contains_vulnerable_unc_path, is_read_only_bash_command, is_read_only_powershell_command,
};

/// Determine whether a shell command is read-only.
///
/// Returns `ReadOnlyResult::ReadOnly` if the command is safe to execute
/// without user approval (Plan mode, Explore mode, etc.).
///
/// The check uses known read-only command maps for git, gh, docker, rg,
/// pyright, and common external commands. Unknown commands are classified
/// as `Unsupported`.
pub fn is_read_only_shell_command(argv: &[String], raw_command: &str) -> ReadOnlyResult {
    if argv.is_empty() {
        return ReadOnlyResult::Unsupported("empty argv".into());
    }

    // Check UNC path vulnerability
    if contains_vulnerable_unc_path(raw_command) {
        return ReadOnlyResult::NotReadOnly("command contains vulnerable UNC path".into());
    }

    let command_base = std::path::Path::new(&argv[0])
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or(&argv[0]);

    let _remaining: Vec<String> = if argv.len() > 1 {
        argv[1..].to_vec()
    } else {
        vec![]
    };

    // Determine command name for flag validation
    let command_name = match command_base {
        "git" | "gh" | "docker" => Some(command_base),
        _ => None,
    };

    // Try matching against known maps

    // 1. GIT
    if command_base == "git" || command_base == "git.exe" {
        let git_map = make_git_read_only_commands();
        if let Some((config, args)) = match_readonly_command(argv, &git_map) {
            if validate_flags(&args, config, command_name, raw_command) {
                return ReadOnlyResult::ReadOnly;
            }
            return ReadOnlyResult::NotReadOnly(
                "git command or flags are not read-only safe".into(),
            );
        }
        // No matching git subcommand — treat as write
        return ReadOnlyResult::NotReadOnly(format!(
            "unsupported git subcommand: {}",
            argv.get(1).map(|s| s.as_str()).unwrap_or("(none)")
        ));
    }

    // 2. GH
    if command_base == "gh" || command_base == "gh.exe" {
        let gh_map = make_gh_read_only_commands();
        if let Some((config, args)) = match_readonly_command(argv, &gh_map) {
            if validate_flags(&args, config, command_name, raw_command) {
                return ReadOnlyResult::ReadOnly;
            }
            return ReadOnlyResult::NotReadOnly(
                "gh command or flags are not read-only safe".into(),
            );
        }
        return ReadOnlyResult::NotReadOnly(format!(
            "unsupported gh subcommand: {}",
            argv.get(1).map(|s| s.as_str()).unwrap_or("(none)")
        ));
    }

    // 3. DOCKER
    if command_base == "docker" || command_base == "docker.exe" {
        let docker_map = make_docker_read_only_commands();
        if let Some((config, args)) = match_readonly_command(argv, &docker_map) {
            if validate_flags(&args, config, command_name, raw_command) {
                return ReadOnlyResult::ReadOnly;
            }
            return ReadOnlyResult::NotReadOnly(
                "docker command or flags are not read-only safe".into(),
            );
        }
        return ReadOnlyResult::NotReadOnly(format!(
            "unsupported docker subcommand: {}",
            argv.get(1).map(|s| s.as_str()).unwrap_or("(none)")
        ));
    }

    // 4. RIPGREP
    if command_base == "rg" || command_base == "ripgrep" {
        let rg_map = make_rg_read_only_commands();
        if let Some((config, args)) = match_readonly_command(argv, &rg_map) {
            if validate_flags(&args, config, command_name, raw_command) {
                return ReadOnlyResult::ReadOnly;
            }
            return ReadOnlyResult::NotReadOnly("rg flags are not read-only safe".into());
        }
    }

    // 5. PYRIGHT
    if command_base == "pyright" || command_base == "pyright-langserver" {
        let pyright_map = make_pyright_read_only_commands();
        if let Some((config, args)) = match_readonly_command(argv, &pyright_map) {
            if validate_flags(&args, config, command_name, raw_command) {
                return ReadOnlyResult::ReadOnly;
            }
            return ReadOnlyResult::NotReadOnly("pyright flags are not read-only safe".into());
        }
    }

    // 6. known external read-only commands
    let ext_map = make_external_readonly_commands();
    if let Some((config, args)) = match_readonly_command(argv, &ext_map) {
        // For simple commands with no flag restrictions (empty safe_flags
        // and no additional check), any remaining args are fine — the
        // command itself is inherently read-only. Combined short flags
        // like `ls -la` are accepted without expanding each flag.
        if config.safe_flags.is_empty() && config.additional_check.is_none() {
            return ReadOnlyResult::ReadOnly;
        }
        // Check that the command is EXACTLY a known external command
        // (no subcommand that might be dangerous)
        if argv.len() == 1 {
            return ReadOnlyResult::ReadOnly;
        }
        // With args: check that remaining args are all flags or simple paths
        let remaining = &argv[1..];
        // Allow non-flag args for simple commands (they're file paths)
        // Block redirects and shell metacharacters (handled by the caller)
        if remaining.iter().all(|a| !a.starts_with('-') || a == "--") {
            return ReadOnlyResult::ReadOnly;
        }
        // If there are flag-like args, validate them
        if validate_flags(&args, config, None, raw_command) {
            return ReadOnlyResult::ReadOnly;
        }
    }

    ReadOnlyResult::Unsupported(format!("unknown or unsupported command: {}", command_base))
}
