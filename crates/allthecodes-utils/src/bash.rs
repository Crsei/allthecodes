//! Bash/shell command utilities.
//!
//! Provides command parsing, shell escaping, dangerous pattern detection
//! (bash-specific parsing beyond `permissions/dangerous.rs`), heredoc
//! detection, stdin redirect handling, and working directory validation.
//!
//! Reference: TypeScript `src/utils/bash/` directory.

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use std::sync::LazyLock;

use allthecodes_config::constants::bash::{default_timeout, max_timeout, MAX_COMMAND_LENGTH};

use crate::safe_regex::SafeRegex;

// =============================================================================
// Command parsing
// =============================================================================

/// Parse a shell command string into individual words/arguments.
///
/// Uses the `shell-words` crate which handles quoting, escaping, and
/// argument splitting according to POSIX shell rules.
pub fn parse_command(cmd: &str) -> Result<Vec<String>> {
    shell_words::split(cmd).context("Failed to parse shell command")
}

/// Split a compound shell command (with `&&`, `||`, `;`) into sub-commands.
///
/// This is a simplified splitter that respects quoting but does not attempt
/// full shell AST parsing. It handles the common cases for permission
/// checks on compound commands.
pub fn split_compound_command(command: &str) -> Vec<String> {
    let mut commands = Vec::new();
    let mut current = String::new();
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut chars = command.chars().peekable();

    while let Some(ch) = chars.next() {
        // Handle escape sequences
        if ch == '\\' && !in_single_quote {
            current.push(ch);
            if let Some(next) = chars.next() {
                current.push(next);
            }
            continue;
        }

        // Track quoting state
        if ch == '\'' && !in_double_quote {
            in_single_quote = !in_single_quote;
            current.push(ch);
            continue;
        }
        if ch == '"' && !in_single_quote {
            in_double_quote = !in_double_quote;
            current.push(ch);
            continue;
        }

        // Only split on operators when not inside quotes
        if !in_single_quote && !in_double_quote {
            // Check for && or ||
            if (ch == '&' || ch == '|') && chars.peek() == Some(&ch) {
                let trimmed = current.trim().to_string();
                if !trimmed.is_empty() {
                    commands.push(trimmed);
                }
                current.clear();
                chars.next(); // consume the second & or |
                continue;
            }

            // Check for ;
            if ch == ';' {
                let trimmed = current.trim().to_string();
                if !trimmed.is_empty() {
                    commands.push(trimmed);
                }
                current.clear();
                continue;
            }

            // Check for | (single pipe, not ||)
            if ch == '|' {
                // Single pipe is part of a pipeline, keep it in the current command
                current.push(ch);
                continue;
            }
        }

        current.push(ch);
    }

    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() {
        commands.push(trimmed);
    }

    commands
}

/// Extract the first command name from a shell command string.
///
/// Skips leading environment variable assignments (`VAR=val`) and
/// returns the base command name (without path).
pub fn extract_command_name(command: &str) -> Option<String> {
    let words = parse_command(command).ok()?;
    for word in &words {
        // Skip environment variable assignments (VAR=value)
        if word.contains('=') && !word.starts_with('-') {
            let before_eq = word.split('=').next().unwrap_or("");
            if before_eq
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                continue;
            }
        }
        // Return the basename of the command
        let name = Path::new(word)
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or(word);
        return Some(name.to_string());
    }
    None
}

// =============================================================================
// Dangerous command detection (bash-specific)
// =============================================================================

/// Patterns that indicate commands containing heredoc syntax.
/// Note: Rust regex doesn't support backreferences, so we use separate
/// patterns for single-quoted, double-quoted, and unquoted delimiters.
static HEREDOC_SINGLE_QUOTED: LazyLock<SafeRegex> =
    LazyLock::new(|| SafeRegex::new(r"<<-?\s*'\w+'"));
static HEREDOC_DOUBLE_QUOTED: LazyLock<SafeRegex> =
    LazyLock::new(|| SafeRegex::new(r#"<<-?\s*"\w+""#));
static HEREDOC_UNQUOTED: LazyLock<SafeRegex> = LazyLock::new(|| SafeRegex::new(r"<<-?\s*\\?\w+"));

/// Patterns for detecting unterminated/malformed quoting.
///
/// Returns `true` if the command has an odd number of unescaped
/// single or double quotes (outside of the other quote type).
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
            i += 2; // skip escaped character
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

/// Patterns to exclude bit-shift operators from heredoc detection.
static BIT_SHIFT_DIGIT: LazyLock<SafeRegex> = LazyLock::new(|| SafeRegex::new(r"\d\s*<<\s*\d"));
static ARITH_SHIFT: LazyLock<SafeRegex> = LazyLock::new(|| SafeRegex::new(r"\$\(\(.*<<.*\)\)"));

/// Check if a command contains heredoc syntax (`<<EOF`, `<<'EOF'`, etc.).
pub fn contains_heredoc(command: &str) -> bool {
    if !command.contains("<<") {
        return false;
    }
    // Exclude bit-shift operators like `1 << 2`, `[[ 1 << 2 ]]`, `$(( ... << ... ))`
    if BIT_SHIFT_DIGIT.is_match(command) || ARITH_SHIFT.is_match(command) {
        return false;
    }
    HEREDOC_SINGLE_QUOTED.is_match(command)
        || HEREDOC_DOUBLE_QUOTED.is_match(command)
        || HEREDOC_UNQUOTED.is_match(command)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HeredocSpec {
    delimiter: String,
    strip_tabs: bool,
}

/// Validate that shell heredocs have a matching closing delimiter.
///
/// This is intentionally line-based: bash reads heredoc bodies after the full
/// command line, and a closing delimiter must appear on its own line. The
/// parser skips quoted strings, comments, here-strings (`<<<`), and simple
/// arithmetic shift forms so normal command text is not rejected as heredoc
/// syntax.
pub fn validate_heredocs(command: &str) -> Result<(), String> {
    if !command.contains("<<") {
        return Ok(());
    }

    let mut pending: std::collections::VecDeque<HeredocSpec> = std::collections::VecDeque::new();

    for line in command.lines() {
        if let Some(spec) = pending.front() {
            let candidate = if spec.strip_tabs {
                line.trim_start_matches('\t')
            } else {
                line
            };

            if candidate == spec.delimiter {
                pending.pop_front();
            }
            continue;
        }

        for spec in parse_heredoc_specs_on_line(line)? {
            pending.push_back(spec);
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

fn parse_heredoc_specs_on_line(line: &str) -> Result<Vec<HeredocSpec>, String> {
    let chars: Vec<char> = line.chars().collect();
    let mut specs = Vec::new();
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

        let (spec, next_index) = parse_heredoc_spec(&chars, i)?;
        specs.push(spec);
        i = next_index;
    }

    Ok(specs)
}

fn parse_heredoc_spec(
    chars: &[char],
    operator_index: usize,
) -> Result<(HeredocSpec, usize), String> {
    let mut i = operator_index + 2;
    let strip_tabs = if chars.get(i) == Some(&'-') {
        i += 1;
        true
    } else {
        false
    };

    while matches!(chars.get(i), Some(' ' | '\t')) {
        i += 1;
    }

    let delimiter = match chars.get(i) {
        Some('\'') | Some('"') => {
            let quote = chars[i];
            i += 1;
            let start = i;
            while i < chars.len() && chars[i] != quote {
                i += 1;
            }
            if i >= chars.len() {
                return Err("Heredoc delimiter quote is not closed".to_string());
            }
            let delimiter: String = chars[start..i].iter().collect();
            i += 1;
            delimiter
        }
        Some('\\') => {
            i += 1;
            let start = i;
            while i < chars.len() && is_heredoc_word_char(chars[i]) {
                i += 1;
            }
            chars[start..i].iter().collect()
        }
        Some(_) => {
            let start = i;
            while i < chars.len() && is_heredoc_word_char(chars[i]) {
                i += 1;
            }
            chars[start..i].iter().collect()
        }
        None => String::new(),
    };

    if delimiter.is_empty() {
        return Err("Heredoc operator is missing a delimiter".to_string());
    }

    Ok((
        HeredocSpec {
            delimiter,
            strip_tabs,
        },
        i,
    ))
}

fn is_heredoc_word_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn looks_like_arithmetic_shift(chars: &[char], operator_index: usize) -> bool {
    let before_digit = previous_non_whitespace(chars, operator_index)
        .map(|ch| ch.is_ascii_digit())
        .unwrap_or(false);
    let after_digit = next_non_whitespace(chars, operator_index + 2)
        .map(|ch| ch.is_ascii_digit())
        .unwrap_or(false);

    if before_digit && after_digit {
        return true;
    }

    let before: String = chars[..operator_index].iter().collect();
    before.matches("((").count() > before.matches("))").count()
}

fn previous_non_whitespace(chars: &[char], before: usize) -> Option<char> {
    chars[..before]
        .iter()
        .rev()
        .find(|ch| !ch.is_whitespace())
        .copied()
}

fn next_non_whitespace(chars: &[char], after: usize) -> Option<char> {
    chars[after..]
        .iter()
        .find(|ch| !ch.is_whitespace())
        .copied()
}

/// Patterns for detecting multiline strings inside quotes.
static SINGLE_QUOTE_MULTILINE: LazyLock<SafeRegex> =
    LazyLock::new(|| SafeRegex::new(r"'(?:[^'\\]|\\.)*\n(?:[^'\\]|\\.)*'"));
static DOUBLE_QUOTE_MULTILINE: LazyLock<SafeRegex> =
    LazyLock::new(|| SafeRegex::new(r#""(?:[^"\\]|\\.)*\n(?:[^"\\]|\\.)*""#));

/// Check if a command contains multiline strings inside quotes.
pub fn contains_multiline_string(command: &str) -> bool {
    SINGLE_QUOTE_MULTILINE.is_match(command) || DOUBLE_QUOTE_MULTILINE.is_match(command)
}

/// Detect if a command already has a stdin redirect (e.g. `< file`).
///
/// Returns `true` for patterns like `< file`, `</dev/null`, but NOT for
/// `<<EOF` (heredoc) or `<(cmd)` (process substitution).
pub fn has_stdin_redirect(command: &str) -> bool {
    // Rust regex doesn't support lookahead, so we find `<` preceded by
    // whitespace/operator or at start, then manually check the next char
    // isn't `<` (heredoc) or `(` (process substitution).
    let chars: Vec<char> = command.chars().collect();
    for (i, &ch) in chars.iter().enumerate() {
        if ch != '<' {
            continue;
        }
        // Must be preceded by start-of-string, whitespace, or operator
        if i > 0 {
            let prev = chars[i - 1];
            if !matches!(prev, ' ' | '\t' | '\n' | ';' | '&' | '|') {
                continue;
            }
        }
        // Next char must NOT be < or (
        if let Some(&next) = chars.get(i + 1) {
            if next == '<' || next == '(' {
                continue;
            }
        }
        // Skip optional whitespace after <, then there must be a non-whitespace char
        let mut j = i + 1;
        while j < chars.len() && (chars[j] == ' ' || chars[j] == '\t') {
            j += 1;
        }
        if j < chars.len() && !chars[j].is_whitespace() {
            return true;
        }
    }
    false
}

/// Determine if a stdin redirect (`< /dev/null`) should be added to a command.
///
/// Returns `false` for heredocs (would interfere with the terminator)
/// and commands that already have a stdin redirect.
pub fn should_add_stdin_redirect(command: &str) -> bool {
    if contains_heredoc(command) {
        return false;
    }
    if has_stdin_redirect(command) {
        return false;
    }
    true
}

/// Rewrite Windows CMD-style `>nul` redirects to POSIX `/dev/null`.
///
/// The model occasionally hallucinates Windows CMD syntax even though
/// our shell is always POSIX. On Git Bash, `2>nul` creates a literal
/// file named `nul` which is problematic on Windows.
pub fn rewrite_windows_null_redirect(command: &str) -> String {
    // Rust regex doesn't support lookahead. We match the full pattern including
    // the trailing boundary character and put it back, or handle end-of-string.
    static NUL_RE: LazyLock<SafeRegex> =
        LazyLock::new(|| SafeRegex::new(r"(\d?&?>+\s*)[Nn][Uu][Ll]([\s|&;)\n]|$)"));
    NUL_RE.replace_all(command, "${1}/dev/null${2}").to_string()
}

/// Check if parsed tokens contain unbalanced brackets/braces/quotes
/// that suggest the command was misinterpreted by the shell parser.
///
/// This is a defense-in-depth measure against command injection via
/// ambiguous shell parsing (e.g., JSON-like strings with semicolons).
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

// =============================================================================
// Timeout handling
// =============================================================================

/// Resolve the effective timeout for a bash command.
///
/// If `requested_ms` is provided (from the tool input), it is clamped
/// to the maximum allowed timeout. Otherwise the default is used.
pub fn resolve_timeout(requested_ms: Option<u64>) -> Duration {
    match requested_ms {
        Some(ms) if ms > 0 => {
            let requested = Duration::from_millis(ms);
            let max = max_timeout();
            if requested > max {
                max
            } else {
                requested
            }
        }
        _ => default_timeout(),
    }
}

// =============================================================================
// Working directory validation
// =============================================================================

/// Validate that the given path is a usable working directory.
///
/// Checks that the path exists, is a directory, and is readable.
/// Returns an error with a descriptive message if validation fails.
pub fn validate_working_directory(path: &str) -> Result<()> {
    let p = Path::new(path);

    if !p.exists() {
        anyhow::bail!("Working directory does not exist: {}", path);
    }
    if !p.is_dir() {
        anyhow::bail!("Path is not a directory: {}", path);
    }

    // Check readability by attempting to read the directory
    match std::fs::read_dir(p) {
        Ok(_) => Ok(()),
        Err(e) => anyhow::bail!("Cannot read working directory {}: {}", path, e),
    }
}

/// Check if a command length is within the parseable limit.
pub fn is_command_parseable(command: &str) -> bool {
    !command.is_empty() && command.len() <= MAX_COMMAND_LENGTH
}

// =============================================================================
// Command prefix extraction
// =============================================================================

/// Extract a command prefix suitable for permission rule matching.
///
/// For simple commands, returns the command name (and optionally the
/// first subcommand). For compound commands (`&&`, `||`, `;`), returns
/// prefixes for each sub-command.
///
/// Examples:
///   - `git push origin main` -> `["git push"]`
///   - `npm install && npm test` -> `["npm install", "npm test"]`
///   - `ls -la` -> `["ls"]`
pub fn extract_command_prefixes(command: &str) -> Vec<String> {
    let subcommands = split_compound_command(command);
    let mut prefixes = Vec::new();

    for subcmd in &subcommands {
        if let Some(prefix) = extract_single_command_prefix(subcmd) {
            if !prefixes.contains(&prefix) {
                prefixes.push(prefix);
            }
        }
    }

    prefixes
}

/// Extract the prefix for a single (non-compound) command.
fn extract_single_command_prefix(command: &str) -> Option<String> {
    let words = parse_command(command).ok()?;
    if words.is_empty() {
        return None;
    }

    // Skip env var assignments
    let mut cmd_start = 0;
    for (i, word) in words.iter().enumerate() {
        if word.contains('=') && !word.starts_with('-') {
            let before = word.split('=').next().unwrap_or("");
            if before
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                cmd_start = i + 1;
                continue;
            }
        }
        break;
    }

    if cmd_start >= words.len() {
        return None;
    }

    let cmd_name = &words[cmd_start];

    // For known commands with subcommands, include the subcommand
    let cmds_with_subcommands = [
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

    let base = Path::new(cmd_name)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or(cmd_name);

    if cmds_with_subcommands.contains(&base)
        && cmd_start + 1 < words.len()
        && !words[cmd_start + 1].starts_with('-')
    {
        Some(format!("{} {}", base, words[cmd_start + 1]))
    } else {
        Some(base.to_string())
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- parse_command ---

    #[test]
    fn test_parse_simple() {
        let args = parse_command("echo hello world").unwrap();
        assert_eq!(args, vec!["echo", "hello", "world"]);
    }

    #[test]
    fn test_parse_quoted() {
        let args = parse_command(r#"echo "hello world" 'foo bar'"#).unwrap();
        assert_eq!(args, vec!["echo", "hello world", "foo bar"]);
    }

    #[test]
    fn test_parse_empty() {
        let args = parse_command("").unwrap();
        assert!(args.is_empty());
    }

    // --- split_compound_command ---

    #[test]
    fn test_split_simple() {
        let parts = split_compound_command("echo hello");
        assert_eq!(parts, vec!["echo hello"]);
    }

    #[test]
    fn test_split_and() {
        let parts = split_compound_command("echo hello && echo world");
        assert_eq!(parts, vec!["echo hello", "echo world"]);
    }

    #[test]
    fn test_split_semicolon() {
        let parts = split_compound_command("cd /tmp; ls -la");
        assert_eq!(parts, vec!["cd /tmp", "ls -la"]);
    }

    #[test]
    fn test_split_quoted_operators() {
        // Operators inside quotes should not split
        let parts = split_compound_command("echo 'hello && world'");
        assert_eq!(parts, vec!["echo 'hello && world'"]);
    }

    #[test]
    fn test_split_pipe_is_not_split() {
        // Single pipe is part of a pipeline, not a split point
        let parts = split_compound_command("cat file | grep pattern");
        assert_eq!(parts, vec!["cat file | grep pattern"]);
    }

    // --- extract_command_name ---

    #[test]
    fn test_extract_command_name_simple() {
        assert_eq!(extract_command_name("git status"), Some("git".into()));
    }

    #[test]
    fn test_extract_command_name_with_env() {
        assert_eq!(
            extract_command_name("LANG=C sort file.txt"),
            Some("sort".into()),
        );
    }

    #[test]
    fn test_extract_command_name_with_path() {
        assert_eq!(
            extract_command_name("/usr/bin/python3 script.py"),
            Some("python3".into()),
        );
    }

    // --- unterminated quotes ---

    #[test]
    fn test_unterminated_double() {
        assert!(has_unterminated_quotes(r#"echo "hello"#));
    }

    #[test]
    fn test_terminated_quotes() {
        assert!(!has_unterminated_quotes(r#"echo "hello" world"#));
    }

    #[test]
    fn test_escaped_quote_not_unterminated() {
        assert!(!has_unterminated_quotes(r#"echo \"hello"#));
    }

    // --- heredoc detection ---

    #[test]
    fn test_contains_heredoc_basic() {
        assert!(contains_heredoc("cat <<EOF\nhello\nEOF"));
    }

    #[test]
    fn test_contains_heredoc_quoted() {
        assert!(contains_heredoc("cat <<'EOF'\nhello\nEOF"));
    }

    #[test]
    fn test_no_heredoc() {
        assert!(!contains_heredoc("echo hello"));
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
    fn test_validate_heredoc_missing_closer() {
        let err = validate_heredocs("cat <<EOF\nhello").unwrap_err();
        assert!(err.contains("EOF"));
    }

    #[test]
    fn test_validate_heredoc_quoted_and_dash_forms() {
        assert!(validate_heredocs("cat <<'EOF'\n$HOME\nEOF").is_ok());
        assert!(validate_heredocs("cat <<-EOF\n\tbody\n\tEOF").is_ok());
    }

    #[test]
    fn test_validate_heredoc_multiple_on_one_command_line() {
        let command = "cat <<A <<B\nfirst\nA\nsecond\nB";
        assert!(validate_heredocs(command).is_ok());
    }

    #[test]
    fn test_validate_heredoc_ignores_quoted_text_and_shifts() {
        assert!(validate_heredocs(r#"echo "<<EOF""#).is_ok());
        assert!(validate_heredocs("echo $((1 << 2))").is_ok());
        assert!(validate_heredocs("echo 1 << 2").is_ok());
    }

    #[test]
    fn test_validate_heredoc_rejects_malformed_delimiter() {
        let err = validate_heredocs("cat <<'EOF\nhello\nEOF").unwrap_err();
        assert!(err.contains("quote"));
    }

    // --- stdin redirect ---

    #[test]
    fn test_has_stdin_redirect() {
        assert!(has_stdin_redirect("sort < file.txt"));
        assert!(has_stdin_redirect("cat </dev/null"));
    }

    #[test]
    fn test_no_stdin_redirect() {
        assert!(!has_stdin_redirect("echo hello"));
    }

    #[test]
    fn test_heredoc_not_stdin_redirect() {
        assert!(!has_stdin_redirect("cat <<EOF"));
    }

    // --- windows nul redirect ---

    #[test]
    fn test_rewrite_nul() {
        assert_eq!(rewrite_windows_null_redirect("ls 2>nul"), "ls 2>/dev/null");
    }

    #[test]
    fn test_rewrite_nul_uppercase() {
        assert_eq!(rewrite_windows_null_redirect("cmd >NUL"), "cmd >/dev/null");
    }

    #[test]
    fn test_no_rewrite_null() {
        // >null should NOT be rewritten (it's not nul)
        let cmd = "cmd >null";
        assert_eq!(rewrite_windows_null_redirect(cmd), cmd);
    }

    // --- validate_working_directory ---

    #[test]
    fn test_validate_existing_dir() {
        assert!(validate_working_directory(".").is_ok());
    }

    #[test]
    fn test_validate_nonexistent_dir() {
        assert!(validate_working_directory("/nonexistent/path/xyz").is_err());
    }

    // --- resolve_timeout ---

    #[test]
    fn test_resolve_timeout_default() {
        let t = resolve_timeout(None);
        assert_eq!(t.as_millis(), 120_000);
    }

    #[test]
    fn test_resolve_timeout_custom() {
        let t = resolve_timeout(Some(5000));
        assert_eq!(t.as_millis(), 5000);
    }

    // --- command prefixes ---

    #[test]
    fn test_extract_prefixes_simple() {
        let prefixes = extract_command_prefixes("ls -la");
        assert_eq!(prefixes, vec!["ls"]);
    }

    #[test]
    fn test_extract_prefixes_with_subcommand() {
        let prefixes = extract_command_prefixes("git push origin main");
        assert_eq!(prefixes, vec!["git push"]);
    }

    #[test]
    fn test_extract_prefixes_compound() {
        let prefixes = extract_command_prefixes("npm install && npm test");
        assert_eq!(prefixes, vec!["npm install", "npm test"]);
    }

    // --- malformed tokens ---

    #[test]
    fn test_malformed_unbalanced_braces() {
        assert!(has_malformed_tokens("{hello"));
    }

    #[test]
    fn test_balanced_braces() {
        assert!(!has_malformed_tokens("{hello}"));
    }

    // --- is_command_parseable ---

    #[test]
    fn test_parseable_command() {
        assert!(is_command_parseable("echo hello"));
    }

    #[test]
    fn test_empty_not_parseable() {
        assert!(!is_command_parseable(""));
    }

    #[test]
    fn test_too_long_not_parseable() {
        let long = "a".repeat(MAX_COMMAND_LENGTH + 1);
        assert!(!is_command_parseable(&long));
    }
}
