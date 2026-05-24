//! Bash-specific permission matcher.
//!
//! Permission rules like `Bash(git diff:*)`, `Bash(prefix:git)`,
//! `Bash(cargo test*)` need to evaluate against the *meaningful* command
//! tokens of a shell invocation, not just the first whitespace-separated
//! word. That requires:
//!
//! 1. **Compound command tokenisation** — split on `&&`, `||`, `;`, `|`,
//!    `&`, and newlines so a rule that allows `git status` doesn't
//!    accidentally allow `git status && rm -rf /`.
//! 2. **Wrapper stripping** — peel off `sudo`, `nohup`, `nice`, `time`,
//!    `env A=B`, `xargs`, `bash -c`, `sh -c` so a rule on `cargo` matches
//!    `sudo nohup cargo build` and a `bash -c "git status"` rule matches.
//! 3. **Pattern matching** — exact / glob / `prefix:` semantics applied
//!    to each surviving sub-command.
//!
//! Living in a dedicated module keeps the BashTool free of permission
//! concerns and lets us unit-test the matcher in isolation.

use crate::rules::glob_match_public;
use allthecodes_utils::bash::{extract_command_name, parse_command, split_compound_command};

/// Wrapper executables whose first non-flag argument is the command we
/// actually want to permission-check.
const PROCESS_WRAPPERS: &[&str] = &[
    "sudo", "doas", "nohup", "nice", "ionice", "time", "stdbuf", "command", "exec",
];

/// Wrappers that take `-c <script>` and run the script in a fresh shell.
/// We extract the script and re-tokenise it.
const SHELL_DASH_C: &[&str] = &["bash", "sh", "zsh", "ksh", "dash"];

/// Wrappers that prefix a command line: their first non-flag argument is
/// the command we want to inspect (e.g. `timeout 30 cargo test`).
const ARG_FORWARDERS: &[&str] = &["watch", "timeout", "parallel"];

/// Tokenise a shell command into the set of "interesting" sub-commands
/// for permission purposes.
///
/// Always returns at least one entry (the original command, trimmed) so
/// callers don't need a special empty-vec branch.
pub fn extract_command_tokens(command: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut frontier: Vec<String> = split_compound_command(command);
    if frontier.is_empty() {
        frontier.push(command.trim().to_string());
    }

    while let Some(raw) = frontier.pop() {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Some wrappers (`bash -c "git status && pwd"`) wrap a whole new
        // compound script. After stripping, we may need to re-tokenise.
        match strip_wrappers(trimmed) {
            StripResult::Inner(inner) => {
                // Wrapped script — re-feed through the splitter, then loop.
                let nested = split_compound_command(&inner);
                if nested.is_empty() {
                    out.push(inner);
                } else {
                    for n in nested {
                        frontier.push(n);
                    }
                }
            }
            StripResult::Plain(cmd) => {
                if !out.contains(&cmd) {
                    out.push(cmd);
                }
            }
        }
    }

    if out.is_empty() {
        out.push(command.trim().to_string());
    }
    out
}

/// Result of stripping a wrapper from one sub-command.
enum StripResult {
    /// A plain command (wrapper layers removed).
    Plain(String),
    /// A wrapped script that needs to be re-tokenised by the caller.
    Inner(String),
}

fn strip_wrappers(command: &str) -> StripResult {
    let words = match parse_command(command) {
        Ok(w) => w,
        Err(_) => return StripResult::Plain(command.to_string()),
    };
    let mut idx = 0;
    while idx < words.len() {
        let head = basename(&words[idx]);
        // Skip env var assignments: `FOO=bar BAZ=qux cargo build`
        if words[idx].contains('=') && !words[idx].starts_with('-') {
            let before = words[idx].split('=').next().unwrap_or("");
            if before
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                idx += 1;
                continue;
            }
        }
        // `env [opts] NAME=val ... CMD ...`
        if head == "env" {
            idx += 1;
            while idx < words.len() {
                let w = &words[idx];
                if w.starts_with('-') || (w.contains('=') && !w.starts_with('-')) {
                    idx += 1;
                } else {
                    break;
                }
            }
            continue;
        }
        // `sh -c "<script>"` / `bash -c ...` — extract the script and ask
        // the caller to re-tokenise.
        if SHELL_DASH_C.contains(&head) {
            // Look for `-c <script>`
            let mut j = idx + 1;
            while j < words.len() {
                if words[j] == "-c" && j + 1 < words.len() {
                    return StripResult::Inner(words[j + 1].clone());
                }
                if words[j].starts_with('-') {
                    j += 1;
                    continue;
                }
                // First positional argument is the script (POSIX `sh script`).
                return StripResult::Plain(words[j..].join(" "));
            }
            // sh with no script — treat as the bare shell.
            return StripResult::Plain(words[idx..].join(" "));
        }
        // Generic process wrapper — drop and continue with the rest.
        if PROCESS_WRAPPERS.contains(&head) {
            idx += 1;
            // Skip wrapper-specific flags (anything starting with `-`).
            while idx < words.len() && words[idx].starts_with('-') {
                idx += 1;
            }
            continue;
        }
        // Bare `xargs` is stripped, but `xargs -n1 ...` stays as `xargs`
        // per the permission docs.
        if head == "xargs" {
            let xargs_idx = idx;
            idx += 1;
            if idx < words.len() && words[idx].starts_with('-') {
                return StripResult::Plain(words[xargs_idx..].join(" "));
            }
            continue;
        }
        // Argument forwarder: `timeout 30 cmd ...`.
        if ARG_FORWARDERS.contains(&head) {
            idx += 1;
            // `timeout` consumes a duration argument and may take flags.
            while idx < words.len()
                && (words[idx].starts_with('-') || is_duration_token(&words[idx]))
            {
                idx += 1;
            }
            continue;
        }
        // Reached the actual command.
        return StripResult::Plain(words[idx..].join(" "));
    }
    StripResult::Plain(command.to_string())
}

fn basename(arg: &str) -> &str {
    let raw = std::path::Path::new(arg)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or(arg);
    raw
}

fn is_duration_token(s: &str) -> bool {
    // Accept `30`, `30s`, `2m`, `1.5h` style.
    let mut chars = s.chars();
    let mut saw_digit = false;
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() || c == '.' {
            saw_digit = true;
        } else if matches!(c, 's' | 'm' | 'h' | 'd') && chars.next().is_none() {
            return saw_digit;
        } else {
            return false;
        }
    }
    saw_digit
}

/// Apply a `Bash(...)` rule pattern to a full command string.
///
/// `pattern` is the inner text of `Bash(<pattern>)`:
///   - `prefix:foo`    — matches if any token's first word equals or
///     begins with `foo`.
///   - `<exact>`       — exact match against any token after wrapper
///     stripping.
///   - `<glob with *>` — glob match against any token (or the joined
///     compound command).
///   - empty / `*`     — matches everything (any Bash command).
///
/// Returns true iff the pattern matches the command.
pub fn bash_pattern_matches(pattern: &str, command: &str) -> bool {
    let pat = pattern.trim();
    if pat.is_empty() || pat == "*" {
        return true;
    }

    let tokens = extract_command_tokens(command);

    if let Some(prefix) = pat.strip_prefix("prefix:") {
        let prefix = prefix.trim();
        return tokens.iter().any(|tok| {
            let head = extract_command_name(tok).unwrap_or_default();
            head == prefix || tok.starts_with(prefix)
        });
    }

    if pat.contains('*') {
        // Whole-command glob first, then per-token to keep `Bash(cargo *)`
        // working against `cargo test`.
        if glob_match_public(command, pat) {
            return true;
        }
        return tokens.iter().any(|t| glob_match_public(t, pat));
    }

    // Exact match: against any sub-command, or its first word.
    tokens.iter().any(|t| {
        if t == pat {
            return true;
        }
        let head = extract_command_name(t).unwrap_or_default();
        head == pat
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenises_simple_command() {
        let toks = extract_command_tokens("git status");
        assert_eq!(toks, vec!["git status"]);
    }

    #[test]
    fn tokenises_compound() {
        let toks = extract_command_tokens("git status && rm -rf /");
        assert!(toks.iter().any(|t| t.starts_with("git status")));
        assert!(toks.iter().any(|t| t.starts_with("rm")));
    }

    #[test]
    fn tokenises_pipe_and_semicolon() {
        let toks = extract_command_tokens("ls | grep foo; cargo test");
        assert!(toks.iter().any(|t| t.contains("ls")));
        assert!(toks.iter().any(|t| t.starts_with("cargo")));
    }

    #[test]
    fn strips_sudo_wrapper() {
        let toks = extract_command_tokens("sudo cargo build");
        assert!(toks.contains(&"cargo build".to_string()));
    }

    #[test]
    fn strips_env_assignments() {
        let toks = extract_command_tokens("FOO=bar BAZ=qux cargo test --release");
        assert!(
            toks.iter().any(|t| t.starts_with("cargo test")),
            "tokens were {:?}",
            toks
        );
    }

    #[test]
    fn strips_bash_dash_c_and_re_tokenises() {
        let toks = extract_command_tokens(r#"bash -c "git status && pwd""#);
        assert!(toks.iter().any(|t| t.starts_with("git status")));
        assert!(toks.iter().any(|t| t == "pwd"));
    }

    #[test]
    fn strips_xargs_and_timeout() {
        let toks = extract_command_tokens("timeout 30 cargo test");
        assert!(toks.contains(&"cargo test".to_string()));
        let toks = extract_command_tokens("xargs cargo build");
        assert!(toks.contains(&"cargo build".to_string()));
    }

    #[test]
    fn does_not_strip_xargs_with_flags() {
        let toks = extract_command_tokens("xargs -n1 cargo build");
        assert!(toks.contains(&"xargs -n1 cargo build".to_string()));
        assert!(!toks.contains(&"cargo build".to_string()));
    }

    #[test]
    fn pattern_prefix_matches_after_strip() {
        assert!(bash_pattern_matches("prefix:cargo", "sudo cargo build"));
        assert!(bash_pattern_matches("prefix:git", "git status && rm -rf /"));
        assert!(!bash_pattern_matches("prefix:git", "cargo test"));
    }

    #[test]
    fn pattern_glob_matches_token() {
        assert!(bash_pattern_matches("cargo *", "cargo test --release"));
        assert!(bash_pattern_matches("*test*", "FOO=1 cargo test --release"));
        assert!(!bash_pattern_matches("cargo *", "rm -rf /"));
    }

    #[test]
    fn pattern_exact_matches_any_subcommand() {
        assert!(bash_pattern_matches("git", "git status && pwd"));
        assert!(bash_pattern_matches("pwd", "git status && pwd"));
        assert!(!bash_pattern_matches("ls", "git status && pwd"));
    }

    #[test]
    fn pattern_wildcard_matches_anything() {
        assert!(bash_pattern_matches("*", "rm -rf /"));
        assert!(bash_pattern_matches("", "rm -rf /"));
    }

    #[test]
    fn deny_via_compound_token() {
        // The point of compound tokenisation: a deny rule on `rm` MUST
        // catch the second arm of an `&&`.
        assert!(bash_pattern_matches("rm", "git status && rm -rf /tmp/x"));
    }
}
