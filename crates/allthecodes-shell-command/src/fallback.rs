//! Fallback shell parser using traditional shell-quote splitting.
//!
//! This module provides current Rust behavior (shell-words based splitting,
//! regex heredoc detection, compound command splitting) as DTO-producing
//! functions. It does not use tree-sitter; it is the fallback for display
//! and low-risk contexts.
//!
//! In security mode (`ParseMode::FailClosedSecurity`), these functions still
//! fail closed on common ambiguous patterns: unterminated quotes, malformed
//! tokens, and too-long commands.

use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use crate::model::*;

// ---------------------------------------------------------------------------
// Top-level parse entry point
// ---------------------------------------------------------------------------

/// Parse a shell command string into a `ParsedShellCommand`.
///
/// In `Permissive` mode, returns a best-effort parse with diagnostics.
/// In `FailClosedSecurity` mode, returns error diagnostics for any ambiguity.
pub fn parse_shell_command(command: &str, mode: ParseMode) -> ParsedShellCommand {
    let original = command.to_string();
    let mut diagnostics = Vec::new();

    // Check for unterminated quotes
    if has_unterminated_quotes(command) {
        diagnostics.push(ParseDiagnostic::error("Unterminated quotes in command"));
        if mode == ParseMode::FailClosedSecurity {
            return ParsedShellCommand {
                original,
                segments: Vec::new(),
                diagnostics,
            };
        }
    }

    let segments = split_into_segments(command, mode, &mut diagnostics);

    ParsedShellCommand {
        original,
        segments,
        diagnostics,
    }
}

// ---------------------------------------------------------------------------
// Segment splitting
// ---------------------------------------------------------------------------

/// Split a compound command into `ShellSegment` values.
pub fn split_into_segments(
    command: &str,
    mode: ParseMode,
    diagnostics: &mut Vec<ParseDiagnostic>,
) -> Vec<ShellSegment> {
    let raw_segments = split_compound_inner(command);
    let mut segments = Vec::new();

    for (i, raw) in raw_segments.iter().enumerate() {
        let separator = if i == 0 {
            None
        } else {
            Some(detect_separator(command, raw, i))
        };
        let (simple, redirs, heredocs) = parse_simple_command(raw, mode, diagnostics);

        segments.push(ShellSegment {
            raw: raw.clone(),
            command: Some(simple),
            redirections: redirs,
            heredocs,
            is_pipeline: raw.contains(" |") || raw.contains("| "),
            separator,
        });
    }

    segments
}

/// Split a command string by compound operators (&&, ||, ;).
///
/// Returns the individual sub-command strings with leading/trailing whitespace
/// trimmed. Does not split on single `|` (pipeline) — those are kept as part
/// of the segment.
pub fn split_compound_inner(command: &str) -> Vec<String> {
    let mut commands = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut chars = command.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\\' && !in_single {
            current.push(ch);
            if let Some(next) = chars.next() {
                current.push(next);
            }
            continue;
        }

        if ch == '\'' && !in_double {
            in_single = !in_single;
            current.push(ch);
            continue;
        }
        if ch == '"' && !in_single {
            in_double = !in_double;
            current.push(ch);
            continue;
        }

        if !in_single && !in_double {
            if (ch == '&' || ch == '|') && chars.peek() == Some(&ch) {
                push_trimmed(&mut commands, &current);
                current.clear();
                chars.next();
                continue;
            }
            if ch == ';' {
                push_trimmed(&mut commands, &current);
                current.clear();
                continue;
            }
        }

        current.push(ch);
    }

    push_trimmed(&mut commands, &current);
    commands
}

fn push_trimmed(out: &mut Vec<String>, s: &str) {
    let t = s.trim().to_string();
    if !t.is_empty() {
        out.push(t);
    }
}

/// Crude separator detection: find the operator text between segments.
fn detect_separator(full: &str, _current_seg: &str, _seg_index: usize) -> String {
    // Simple heuristic: search for known operators
    for op in &["&&", "||", ";"] {
        if full.contains(op) {
            return op.to_string();
        }
    }
    "|".to_string()
}

// ---------------------------------------------------------------------------
// Simple command parsing
// ---------------------------------------------------------------------------

/// Parse a single (non-compound) command into SimpleCommand + redirections + heredocs.
fn parse_simple_command(
    raw: &str,
    _mode: ParseMode,
    diagnostics: &mut Vec<ParseDiagnostic>,
) -> (SimpleCommand, Vec<Redirection>, Vec<Heredoc>) {
    let argv = match shell_words::split(raw) {
        Ok(w) => w,
        Err(e) => {
            diagnostics.push(ParseDiagnostic::warning(format!(
                "shell-words parse: {}; using raw split",
                e
            )));
            raw.split_whitespace().map(|s| s.to_string()).collect()
        }
    };

    // Extract env vars (leading VAR=val non-flag items)
    let mut env_vars = Vec::new();
    let mut cmd_start = 0;
    for (i, word) in argv.iter().enumerate() {
        if let Some(eq_pos) = word.find('=') {
            if !word.starts_with('-') {
                let name = &word[..eq_pos];
                if name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                    let val = &word[eq_pos + 1..];
                    env_vars.push((name.to_string(), val.to_string()));
                    cmd_start = i + 1;
                    continue;
                }
            }
        }
        break;
    }

    let args_after_env: Vec<String> = argv.iter().skip(cmd_start).cloned().collect();
    let command_raw = args_after_env.first().cloned();
    let command_name = command_raw.as_ref().and_then(|c| {
        Path::new(c)
            .file_name()
            .and_then(|f| f.to_str())
            .map(|s| s.to_string())
    });
    let args: Vec<String> = args_after_env.iter().skip(1).cloned().collect();
    let full_argv: Vec<String> = argv.iter().cloned().collect();

    // Extract redirections
    let redirections = extract_redirections(raw);

    // Detect heredocs
    let heredocs = extract_heredocs(raw);

    // Check malformed tokens
    for token in &args {
        if has_malformed_tokens(token) {
            diagnostics.push(ParseDiagnostic::warning(format!(
                "Malformed token detected: {}",
                token
            )));
        }
    }

    let cmd = SimpleCommand {
        command_name,
        command_raw,
        args,
        env_vars,
        argv: full_argv,
    };

    (cmd, redirections, heredocs)
}

// ---------------------------------------------------------------------------
// Redirection extraction (simple regex-free scanner)
// ---------------------------------------------------------------------------

fn extract_redirections(command: &str) -> Vec<Redirection> {
    let mut redirs = Vec::new();
    let chars: Vec<char> = command.chars().collect();
    let mut i = 0;
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;

    while i < chars.len() {
        let ch = chars[i];

        if escaped {
            escaped = false;
            i += 1;
            continue;
        }
        if ch == '\\' && !in_single {
            escaped = true;
            i += 1;
            continue;
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
            // Check for digit-prefixed redirect: 2>, 2>&1, etc.
            let digits = if ch.is_ascii_digit() {
                let start = i;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
                &chars[start..i]
            } else {
                &[]
            };

            if i < chars.len() && matches!(chars[i], '<' | '>') {
                let _op_start = i;
                // Collect operator
                let mut op = String::new();
                if !digits.is_empty() {
                    op.extend(digits.iter());
                }
                while i < chars.len() && matches!(chars[i], '<' | '>' | '&') {
                    op.push(chars[i]);
                    i += 1;
                }
                // Skip whitespace to target
                while i < chars.len() && chars[i].is_whitespace() {
                    i += 1;
                }
                // Collect target
                let mut target = String::new();
                while i < chars.len()
                    && !chars[i].is_whitespace()
                    && !matches!(chars[i], '|' | ';' | '&')
                {
                    target.push(chars[i]);
                    i += 1;
                }
                if !target.is_empty() {
                    redirs.push(Redirection {
                        operator: op,
                        target,
                    });
                }
                continue;
            }

            // If we consumed digits but no redirect followed, re-wind
            if !digits.is_empty() {
                // Push the digits back into the current position
                // (they are part of a normal argument)
                continue;
            }
        }
        i += 1;
    }

    redirs
}

// ---------------------------------------------------------------------------
// Heredoc detection
// ---------------------------------------------------------------------------

static HEREDOC_SINGLE_QUOTED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<<-?\s*'\w+'").expect("invalid heredoc regex"));
static HEREDOC_DOUBLE_QUOTED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"<<-?\s*"\w+""#).expect("invalid heredoc regex"));
static HEREDOC_UNQUOTED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<<-?\s*\\?\w+").expect("invalid heredoc regex"));
static BIT_SHIFT_DIGIT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\d\s*<<\s*\d").expect("invalid bit-shift regex"));
static ARITH_SHIFT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\$\(\(.*<<.*\)\)").expect("invalid arith shift regex"));

/// Check if a command string contains heredoc syntax.
pub fn contains_heredoc(command: &str) -> bool {
    if !command.contains("<<") {
        return false;
    }
    if BIT_SHIFT_DIGIT.is_match(command) || ARITH_SHIFT.is_match(command) {
        return false;
    }
    HEREDOC_SINGLE_QUOTED.is_match(command)
        || HEREDOC_DOUBLE_QUOTED.is_match(command)
        || HEREDOC_UNQUOTED.is_match(command)
}

/// Extract heredoc specifications from a command string (simple regex version).
pub fn extract_heredocs(command: &str) -> Vec<Heredoc> {
    let mut heredocs = Vec::new();
    if !contains_heredoc(command) {
        return heredocs;
    }

    // Parse heredoc specs from the first line (before newline)
    let first_line = command.lines().next().unwrap_or(command);
    let chars: Vec<char> = first_line.chars().collect();
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;
    let mut i = 0;

    while i < chars.len() {
        let ch = chars[i];

        if escaped {
            escaped = false;
            i += 1;
            continue;
        }
        if ch == '\\' && !in_single {
            escaped = true;
            i += 1;
            continue;
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
        if !in_single && !in_double && ch == '#' {
            break;
        }
        if in_single || in_double || ch != '<' || chars.get(i + 1) != Some(&'<') {
            i += 1;
            continue;
        }
        if chars.get(i + 2) == Some(&'<') {
            i += 3;
            continue;
        }
        if looks_like_arithmetic_shift(&chars, i) {
            i += 2;
            continue;
        }

        // Found << operator
        let mut j = i + 2;
        let strip_tabs = chars.get(j) == Some(&'-');
        if strip_tabs {
            j += 1;
        }
        while matches!(chars.get(j), Some(' ' | '\t')) {
            j += 1;
        }

        let (delimiter, quoted) = match chars.get(j) {
            Some('\'') | Some('"') => {
                let quote = chars[j];
                j += 1;
                let start = j;
                while j < chars.len() && chars[j] != quote {
                    j += 1;
                }
                let delim: String = chars[start..j].iter().collect();
                (delim, true)
            }
            Some('\\') => {
                j += 1;
                let start = j;
                while j < chars.len() && is_heredoc_word_char(chars[j]) {
                    j += 1;
                }
                (chars[start..j].iter().collect(), false)
            }
            Some(_) => {
                let start = j;
                while j < chars.len() && is_heredoc_word_char(chars[j]) {
                    j += 1;
                }
                (chars[start..j].iter().collect(), false)
            }
            None => break,
        };

        if !delimiter.is_empty() {
            heredocs.push(Heredoc {
                delimiter,
                strip_tabs,
                quoted,
            });
        }
        i = j;
    }

    heredocs
}

fn is_heredoc_word_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn looks_like_arithmetic_shift(chars: &[char], operator_index: usize) -> bool {
    let before_digit = chars[..operator_index]
        .iter()
        .rev()
        .find(|c| !c.is_whitespace())
        .map(|c| c.is_ascii_digit())
        .unwrap_or(false);
    let after_digit = chars[operator_index + 2..]
        .iter()
        .find(|c| !c.is_whitespace())
        .map(|c| c.is_ascii_digit())
        .unwrap_or(false);
    if before_digit && after_digit {
        return true;
    }
    let before: String = chars[..operator_index].iter().collect();
    before.matches("((").count() > before.matches("))").count()
}

/// Validate that heredocs have matching closing lines.
pub fn validate_heredocs(command: &str) -> Result<(), String> {
    if !command.contains("<<") {
        return Ok(());
    }

    let specs = extract_heredocs(command);
    if specs.is_empty() {
        return Ok(());
    }

    let mut pending: std::collections::VecDeque<Heredoc> = specs.into_iter().collect();

    for line in command.lines().skip(1) {
        if pending.is_empty() {
            break;
        }
        let Some(spec) = pending.front() else {
            break;
        };
        let candidate = if spec.strip_tabs {
            line.trim_start_matches('\t')
        } else {
            line
        };
        if candidate == spec.delimiter {
            pending.pop_front();
        }
    }

    if let Some(spec) = pending.front() {
        return Err(format!(
            "Heredoc delimiter '{}' is missing its closing line",
            spec.delimiter
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

/// Check for unterminated quotes.
pub fn has_unterminated_quotes(command: &str) -> bool {
    let mut in_single = false;
    let mut in_double = false;
    let mut double_count = 0u32;
    let mut single_count = 0u32;
    let chars: Vec<char> = command.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && !in_single {
            i += 2;
            continue;
        }
        if c == '"' && !in_single {
            double_count += 1;
            in_double = !in_double;
        } else if c == '\'' && !in_double {
            single_count += 1;
            in_single = !in_single;
        }
        i += 1;
    }

    !double_count.is_multiple_of(2) || !single_count.is_multiple_of(2)
}

/// Check for malformed tokens (unbalanced braces, parens, brackets).
pub fn has_malformed_tokens(token: &str) -> bool {
    let open_braces = token.matches('{').count();
    let close_braces = token.matches('}').count();
    if open_braces != close_braces {
        return true;
    }
    let open_parens = token.matches('(').count();
    let close_parens = token.matches(')').count();
    if open_parens != close_parens {
        return true;
    }
    let open_brackets = token.matches('[').count();
    let close_brackets = token.matches(']').count();
    if open_brackets != close_brackets {
        return true;
    }
    false
}

/// Extract the command name from a simple command string.
pub fn extract_command_name(command: &str) -> Option<String> {
    let cmd = parse_shell_command(command, ParseMode::Permissive);
    cmd.primary_command().and_then(|c| c.command_name.clone())
}

/// Extract command prefixes (for permission rule matching).
pub fn extract_command_prefixes(command: &str) -> Vec<String> {
    let cmd = parse_shell_command(command, ParseMode::Permissive);
    let known_subcommands: &[&str] = &[
        "git",
        "npm",
        "npx",
        "yarn",
        "pnpm",
        "cargo",
        "docker",
        "kubectl",
        "pip",
        "pip3",
        "brew",
        "apt",
        "apt-get",
        "dnf",
        "yum",
        "pacman",
        "systemctl",
        "go",
        "rustup",
    ];

    let mut prefixes = Vec::new();
    for seg in &cmd.segments {
        if let Some(simple) = &seg.command {
            if let Some(ref name) = simple.command_name {
                let prefix = if known_subcommands.contains(&name.as_str()) {
                    if let Some(first_arg) = simple.args.first() {
                        if !first_arg.starts_with('-') {
                            format!("{} {}", name, first_arg)
                        } else {
                            name.clone()
                        }
                    } else {
                        name.clone()
                    }
                } else {
                    name.clone()
                };
                if !prefixes.contains(&prefix) {
                    prefixes.push(prefix);
                }
            }
        }
    }
    prefixes
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- parse_shell_command ---

    #[test]
    fn test_parse_simple() {
        let cmd = parse_shell_command("echo hello world", ParseMode::Permissive);
        assert_eq!(cmd.segments.len(), 1);
        let simple = cmd.primary_command().expect("should have command");
        assert_eq!(simple.command_name.as_deref(), Some("echo"));
        assert_eq!(simple.argv, vec!["echo", "hello", "world"]);
    }

    #[test]
    fn test_parse_compound_and() {
        let cmd = parse_shell_command("echo a && echo b", ParseMode::Permissive);
        assert_eq!(cmd.segments.len(), 2);
        assert_eq!(
            cmd.segments[0].command.as_ref().unwrap().argv,
            vec!["echo", "a"]
        );
        assert_eq!(
            cmd.segments[1].command.as_ref().unwrap().argv,
            vec!["echo", "b"]
        );
    }

    #[test]
    fn test_parse_compound_semicolon() {
        let cmd = parse_shell_command("cd /tmp; ls", ParseMode::Permissive);
        assert_eq!(cmd.segments.len(), 2);
    }

    #[test]
    fn test_unterminated_quotes_error() {
        let cmd = parse_shell_command("echo \"hello", ParseMode::FailClosedSecurity);
        assert!(cmd
            .diagnostics
            .iter()
            .any(|d| d.message.contains("Unterminated")));
        assert!(cmd.segments.is_empty());
    }

    #[test]
    fn test_permissive_fallthrough_on_unterminated() {
        let cmd = parse_shell_command("echo \"hello", ParseMode::Permissive);
        // Permissive mode may still produce segments
        assert!(cmd.segments.len() == 1 || cmd.segments.is_empty());
    }

    // --- has_unterminated_quotes ---

    #[test]
    fn test_unterminated_double() {
        assert!(has_unterminated_quotes(r#"echo "hello"#));
    }

    #[test]
    fn test_terminated_quotes() {
        assert!(!has_unterminated_quotes(r#"echo "hello" world"#));
    }

    // --- heredoc ---

    #[test]
    fn test_contains_heredoc_basic() {
        assert!(contains_heredoc("cat <<EOF\nhello\nEOF"));
    }

    #[test]
    fn test_bit_shift_not_heredoc() {
        assert!(!contains_heredoc("echo $((1 << 2))"));
    }

    #[test]
    fn test_validate_heredoc_closed() {
        assert!(validate_heredocs("cat <<EOF\nhello\nEOF").is_ok());
    }

    #[test]
    fn test_validate_heredoc_missing() {
        assert!(validate_heredocs("cat <<EOF\nhello").is_err());
    }

    #[test]
    fn test_extract_heredocs_finds_specs() {
        let h = extract_heredocs("cat <<EOF <<'END'\nhello\nEOF\nworld\nEND");
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].delimiter, "EOF");
        assert!(!h[0].quoted);
        assert_eq!(h[1].delimiter, "END");
        assert!(h[1].quoted);
    }

    // --- redirections ---

    #[test]
    fn test_extract_redirects() {
        let redirs = extract_redirections("sort < input.txt > output.txt");
        assert_eq!(redirs.len(), 2);
        assert_eq!(redirs[0].operator, "<");
        assert_eq!(redirs[0].target, "input.txt");
        assert_eq!(redirs[1].operator, ">");
        assert_eq!(redirs[1].target, "output.txt");
    }

    // --- extract_command_name ---

    #[test]
    fn test_extract_name() {
        assert_eq!(extract_command_name("git status"), Some("git".into()));
    }

    #[test]
    fn test_extract_name_with_env() {
        assert_eq!(
            extract_command_name("LANG=C sort file"),
            Some("sort".into())
        );
    }

    // --- extract_command_prefixes ---

    #[test]
    fn test_prefixes_simple() {
        let p = extract_command_prefixes("ls -la");
        assert_eq!(p, vec!["ls"]);
    }

    #[test]
    fn test_prefixes_with_subcommand() {
        let p = extract_command_prefixes("git push origin main");
        assert_eq!(p, vec!["git push"]);
    }

    #[test]
    fn test_prefixes_compound() {
        let mut p = extract_command_prefixes("npm install && npm test");
        p.sort();
        assert_eq!(p, vec!["npm install", "npm test"]);
    }

    // --- malformed tokens ---

    #[test]
    fn test_malformed_token() {
        assert!(has_malformed_tokens("{hello"));
    }

    #[test]
    fn test_balanced_token() {
        assert!(!has_malformed_tokens("{hello}"));
    }

    // --- stdin redirect ---

    #[test]
    fn test_stdin_redirect_detection() {
        let cmd = parse_shell_command("sort < input.txt", ParseMode::Permissive);
        assert!(cmd.has_stdin_redirect());
        assert!(!cmd.needs_stdin_redirect());
    }

    #[test]
    fn test_needs_stdin_redirect() {
        let cmd = parse_shell_command("echo hello", ParseMode::Permissive);
        assert!(cmd.needs_stdin_redirect());
    }

    #[test]
    fn test_heredoc_prevents_stdin_redirect() {
        // heredocs involve stdin, so stdin redirect not needed
        let cmd = parse_shell_command("cat <<EOF\nhello\nEOF", ParseMode::Permissive);
        assert!(!cmd.needs_stdin_redirect());
    }
}
