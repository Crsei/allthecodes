use allthecodes_shell_command::fallback;
use allthecodes_shell_command::model::{
    DiagnosticSeverity, ParseMode, ReadOnlyResult, ShellDialect,
};

/// Check if a path contains a vulnerable UNC path that could leak credentials.
///
/// UNC paths like `\\host\share` can be used to leak NTLM hashes when
/// Windows attempts to authenticate with the remote host. This check
/// detects UNC paths in command arguments.
///
/// Port of Bun's `containsVulnerableUncPath`.
pub fn contains_vulnerable_unc_path(command: &str) -> bool {
    if !command.contains("\\\\") && !command.contains("//") {
        return false;
    }

    // Check for UNC patterns: \\host\share or //host/share anywhere in the line.
    // These can appear as arguments like `copy \\attacker\share\file.txt dest`.
    for line in command.lines() {
        let trimmed = line.trim();

        // Search for \\ anywhere in the line
        let mut search_start = 0;
        while let Some(pos) = trimmed[search_start..].find("\\\\") {
            let abs_pos = search_start + pos;
            let substr = &trimmed[abs_pos..];

            // Skip \\?\ extended-length paths and \\?\UNC\ paths
            if substr.starts_with("\\\\?\\") {
                search_start = abs_pos + 4;
                continue;
            }

            // Check for \\host\share pattern
            let after_slash = &substr[2..];
            if let Some(slash_pos) = after_slash.find('\\') {
                if slash_pos > 0 {
                    let host = &after_slash[..slash_pos];
                    let host_lower = host.to_lowercase();
                    if host_lower == "localhost"
                        || host_lower == "127.0.0.1"
                        || host_lower == "."
                        || host_lower == "?"
                    {
                        search_start = abs_pos + 2;
                        continue;
                    }
                    return true;
                }
            }
            search_start = abs_pos + 2;
        }

        // Search for // anywhere in the line (Samba/Unix UNC)
        let mut search_start = 0;
        while let Some(pos) = trimmed[search_start..].find("//") {
            let abs_pos = search_start + pos;
            let substr = &trimmed[abs_pos..];

            // Skip /// (empty host)
            if substr.starts_with("///") {
                search_start = abs_pos + 3;
                continue;
            }

            // Check for //host/share pattern
            let after_slash = &substr[2..];
            if let Some(slash_pos) = after_slash.find('/') {
                if slash_pos > 0 {
                    let host = &after_slash[..slash_pos];
                    let host_lower = host.to_lowercase();
                    if host_lower == "localhost"
                        || host_lower == "127.0.0.1"
                        || host_lower == "."
                        || host_lower == "?"
                    {
                        search_start = abs_pos + 2;
                        continue;
                    }
                    return true;
                }
            }
            search_start = abs_pos + 2;
        }
    }

    false
}

/// Classify a raw Bash command string as read-only or not.
///
/// This entry point is intentionally more conservative than the argv-level map:
/// Plan/Explore mode only auto-allows a single simple command. Shell syntax that
/// can hide extra execution or file writes is rejected before argv validation.
pub fn is_read_only_bash_command(command: &str) -> ReadOnlyResult {
    classify_shell_command_text(command, ShellDialect::Bash)
}

/// Classify a raw PowerShell command string as read-only or not.
///
/// This only permits simple external command invocations such as `git status`
/// or `rg pattern`. PowerShell pipelines, variables, script blocks, redirects,
/// and invocation operators remain approval-gated.
pub fn is_read_only_powershell_command(command: &str) -> ReadOnlyResult {
    classify_shell_command_text(command, ShellDialect::PowerShell)
}

fn classify_shell_command_text(command: &str, dialect: ShellDialect) -> ReadOnlyResult {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return ReadOnlyResult::ParseFailed("empty shell command".into());
    }

    if contains_vulnerable_unc_path(trimmed) {
        return ReadOnlyResult::NotReadOnly("command contains vulnerable UNC path".into());
    }

    if let Some(reason) = forbidden_shell_syntax(trimmed, dialect) {
        return ReadOnlyResult::NotReadOnly(reason);
    }

    let parsed = fallback::parse_shell_command(trimmed, ParseMode::FailClosedSecurity);
    if let Some(err) = parsed
        .diagnostics
        .iter()
        .find(|diag| diag.severity == DiagnosticSeverity::Error)
    {
        return ReadOnlyResult::ParseFailed(err.message.clone());
    }

    if parsed.segments.len() != 1 {
        return ReadOnlyResult::NotReadOnly("compound shell commands require approval".into());
    }

    let Some(segment) = parsed.segments.first() else {
        return ReadOnlyResult::ParseFailed("command did not parse into a segment".into());
    };

    if !segment.redirections.is_empty() {
        return ReadOnlyResult::NotReadOnly("shell redirections require approval".into());
    }
    if !segment.heredocs.is_empty() {
        return ReadOnlyResult::NotReadOnly("heredocs require approval".into());
    }

    let Some(simple) = &segment.command else {
        return ReadOnlyResult::ParseFailed("command did not parse into argv".into());
    };
    if simple.argv.is_empty() {
        return ReadOnlyResult::ParseFailed("empty argv".into());
    }

    super::is_read_only_shell_command(&simple.argv, trimmed)
}

fn forbidden_shell_syntax(command: &str, dialect: ShellDialect) -> Option<String> {
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;
    let chars: Vec<char> = command.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let ch = chars[i];

        if escaped {
            escaped = false;
            i += 1;
            continue;
        }

        match dialect {
            ShellDialect::PowerShell if ch == '`' => {
                return Some("PowerShell escape/dynamic syntax requires approval".into());
            }
            _ if ch == '\\' && !in_single => {
                escaped = true;
                i += 1;
                continue;
            }
            _ => {}
        }

        if ch == '\'' && !in_double {
            in_single = !in_single;
            i += 1;
            continue;
        }
        if ch == '"' && !in_single {
            in_double = !in_double;
            i += 1;
            continue;
        }

        if !in_single && !in_double {
            match ch {
                '\n' | '\r' | ';' | '|' | '&' | '<' | '>' => {
                    return Some("shell operators and redirections require approval".into());
                }
                '$' => {
                    return Some("shell expansions require approval".into());
                }
                '`' => {
                    return Some("shell command substitution requires approval".into());
                }
                '(' | ')' | '{' | '}' if dialect == ShellDialect::PowerShell => {
                    return Some(
                        "PowerShell expressions and script blocks require approval".into(),
                    );
                }
                _ => {}
            }
        }

        i += 1;
    }

    None
}
