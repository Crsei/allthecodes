//! Pipe command rearrangement for eval stdin redirect compatibility.
//!
//! Port of Bun `src/utils/bash/bashPipeCommand.ts`.
//!
//! When eval wraps a piped command, the stdin redirect (`< /dev/null`) applies
//! to eval itself rather than the first command in the pipeline. This module
//! rearranges piped commands so that `< /dev/null` appears after the first
//! command: `first_cmd < /dev/null | rest_of_pipeline`.
//!
//! For commands that cannot be safely rearranged (containing backticks,
//! variable references, control structures, etc.), a fallback quotes the
//! entire command and appends `< /dev/null` after the closing quote.

use std::sync::LazyLock;

use regex::Regex;

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

/// Rearrange a piped command to place stdin redirect after the first command.
///
/// This fixes an issue where eval treats the entire piped command as a single
/// unit, causing the stdin redirect to apply to eval itself rather than the
/// first command in the pipeline.
///
/// When the command cannot be safely rearranged, falls back to
/// `quote_with_eval_stdin_redirect()` which quotes the full command and
/// appends `< /dev/null`.
pub fn rearrange_pipe_command(command: &str) -> String {
    // --- Bail conditions ---

    // Skip if command has backticks - shell-words doesn't handle them well
    if command.contains('`') {
        return quote_with_eval_stdin_redirect(command);
    }

    // Skip if command has command substitution
    if command.contains("$(") {
        return quote_with_eval_stdin_redirect(command);
    }

    // Skip if command references shell variables ($VAR, ${VAR})
    if has_shell_variable_ref(command) {
        return quote_with_eval_stdin_redirect(command);
    }

    // Skip if command contains fd redirect operators (>&, <&) that
    // shell-words treats as literal tokens rather than operators.
    if command.contains(">&") || command.contains("<&") {
        return quote_with_eval_stdin_redirect(command);
    }

    // Skip if command contains bash control structures
    if contains_control_structure(command) {
        return quote_with_eval_stdin_redirect(command);
    }

    // Join continuation lines before parsing
    let joined = join_continuation_lines(command);

    // If any newlines remain (real line separators), use fallback
    if joined.contains('\n') {
        return quote_with_eval_stdin_redirect(command);
    }

    // Try parsing with shell-words
    let tokens = match shell_words::split(&joined) {
        Ok(t) => t,
        Err(_) => return quote_with_eval_stdin_redirect(command),
    };

    // Find first pipe operator
    let first_pipe = find_first_pipe(&tokens);

    // No pipe, or pipe at position 0 (syntax error) — use fallback
    if first_pipe == 0 || first_pipe >= tokens.len() {
        return quote_with_eval_stdin_redirect(command);
    }

    // Rebuild: first_command < /dev/null | rest_of_pipeline
    let before_pipe = rebuild_tokens(&tokens[..first_pipe]);
    let after_pipe = rebuild_tokens(&tokens[first_pipe..]);

    let rearranged = format!("{} < /dev/null {}", before_pipe, after_pipe);
    single_quote_for_eval(&rearranged)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Check if command contains shell variable references ($VAR, ${VAR}).
fn has_shell_variable_ref(command: &str) -> bool {
    let bytes = command.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' {
            if let Some(&next) = bytes.get(i + 1) {
                if next.is_ascii_alphabetic() || next == b'_' || next == b'{' {
                    return true;
                }
            }
        }
        i += 1;
    }
    false
}

/// Find the index of the first pipe (`|`) token in shell-words output.
fn find_first_pipe(tokens: &[String]) -> usize {
    tokens.iter().position(|t| t == "|").unwrap_or(usize::MAX)
}

/// Rebuild a command string from shell-words tokens, preserving operator
/// tokens as-is and quoting string tokens that need it.
///
/// port of Bun's `buildCommandParts()`, simplified for Rust (flat string
/// tokens from shell-words instead of structured operator objects).
fn rebuild_tokens(tokens: &[String]) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut seen_non_env_var = false;

    let mut i = 0;
    while i < tokens.len() {
        let token = &tokens[i];

        // Pipe and compound operators: emit as-is, reset env-var context
        if token == "|" || token == "&&" || token == "||" || token == ";" {
            parts.push(token.clone());
            i += 1;
            seen_non_env_var = false;
            continue;
        }

        // Glob pattern or bare `*` handled by shell-words — emit as-is
        if token == "*" || token.contains('*') || token.contains('?') {
            parts.push(token.clone());
            i += 1;
            continue;
        }

        // Check for file descriptor redirections (e.g., 2>&1, 2>/dev/null)
        if is_fd_number(token) && i + 2 < tokens.len() && is_redirect_operator(&tokens[i + 1]) {
            let op = &tokens[i + 1];
            let target = &tokens[i + 2];

            if op == ">&" && is_fd_number(target) {
                parts.push(format!("{}>&{}", token, target));
                i += 3;
                continue;
            }
            if op == ">" && target == "/dev/null" {
                parts.push(format!("{}>/dev/null", token));
                i += 3;
                continue;
            }
            if op == ">" && target.starts_with('&') {
                let fd = &target[1..];
                if is_fd_number(fd) {
                    parts.push(format!("{}>&{}", token, fd));
                    i += 3;
                    continue;
                }
            }
        }

        // Environment variable assignments — only at command start
        if is_env_var_assignment(token) && !seen_non_env_var {
            if let Some(eq_pos) = token.find('=') {
                let name = &token[..eq_pos];
                let value = &token[eq_pos + 1..];
                parts.push(format!("{}={}", name, quote_shell_token(value)));
            } else {
                parts.push(quote_shell_token(token));
            }
            i += 1;
            continue;
        }

        // Regular argument
        seen_non_env_var = true;
        parts.push(quote_shell_token(token));
        i += 1;
    }

    parts.join(" ")
}

fn is_fd_number(s: &str) -> bool {
    matches!(s, "0" | "1" | "2")
}

fn is_redirect_operator(s: &str) -> bool {
    matches!(s, ">" | "<" | ">&" | "<&" | ">>" | "<<")
}

fn is_env_var_assignment(s: &str) -> bool {
    if let Some(eq_pos) = s.find('=') {
        if eq_pos == 0 {
            return false;
        }
        // Name part must be valid identifier chars
        s[..eq_pos]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
    } else {
        false
    }
}

/// Quote a shell token for safe reconstruction.
///
/// Safe characters (alphanumeric, `_`, `-`, `/`, `.`, `=`, `:`, `@`, `+`, `~`,
/// `,`) are returned as-is. Everything else is single-quoted, with embedded
/// single quotes escaped via the `'"'"'` pattern.
fn quote_shell_token(token: &str) -> String {
    if token.is_empty() {
        return "''".to_string();
    }
    if token
        .bytes()
        .all(|b| matches!(b, b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'-' | b'/' | b'.' | b'=' | b':' | b'@' | b'+' | b'~' | b','))
    {
        return token.to_string();
    }
    single_quote_for_eval(token)
}

/// Single-quote a string for use as an eval argument.
///
/// Escapes embedded single quotes via `'"'"'` — close single-quote, literal
/// single-quote in double-quote, reopen single-quote. This avoids shell-quote
/// bugs where `!` gets escaped to `\!`.
pub fn single_quote_for_eval(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\"'\"'"))
}

/// Quote the full command and append `< /dev/null`.
///
/// Produces: `'command' < /dev/null`
/// → eval's stdin is /dev/null, command is evaluated inside eval
fn quote_with_eval_stdin_redirect(command: &str) -> String {
    format!("{} < /dev/null", single_quote_for_eval(command))
}

// ---------------------------------------------------------------------------
// Continuation line joining
// ---------------------------------------------------------------------------

static BACKSLASH_NL_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"\\+\n"));

/// Join shell continuation lines (backslash-newline) into a single line.
///
/// Only joins when there is an odd number of backslashes before the newline
/// (the last one escapes it). Even pairs remain as literal backslashes plus
/// newline separator.
fn join_continuation_lines(command: &str) -> String {
    let Ok(regex) = BACKSLASH_NL_RE.as_ref() else {
        return command.to_string();
    };
    regex
        .replace_all(command, |caps: &regex::Captures| {
            let Some(m) = caps.get(0) else {
                return String::new();
            };
            let text = m.as_str();
            let backslash_count = text.len() - 1; // -1 for the \n
            if backslash_count % 2 == 1 {
                // Odd: last backslash escapes the newline (continuation)
                // Keep (backslash_count - 1) backslashes, drop the \n
                "\\".repeat(backslash_count.saturating_sub(1))
            } else {
                // Even: all paired, newline is a real separator — keep original
                text.to_string()
            }
        })
        .to_string()
}

// ---------------------------------------------------------------------------
// Control structure detection
// ---------------------------------------------------------------------------

/// Check if a command contains bash control structure keywords.
///
/// Detects: `for`, `while`, `until`, `if`, `case`, `select` when they appear
/// as standalone keywords (preceded by a word boundary, followed by whitespace).
fn contains_control_structure(command: &str) -> bool {
    let keywords: &[&[u8]] = &[b"for", b"while", b"until", b"if", b"case", b"select"];
    let bytes = command.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        // Skip non-word chars (operators, whitespace)
        if !bytes[i].is_ascii_alphanumeric() && bytes[i] != b'_' {
            i += 1;
            continue;
        }

        // Check if current position starts any keyword
        for &kw in keywords {
            if i + kw.len() <= bytes.len() && bytes[i..i + kw.len()] == *kw {
                let after = i + kw.len();
                // Must be followed by whitespace (end of keyword)
                if after < bytes.len()
                    && (bytes[after] == b' ' || bytes[after] == b'\t' || bytes[after] == b'\n')
                {
                    // Preceding char must not be a word char (word boundary)
                    let word_boundary =
                        i == 0 || !bytes[i - 1].is_ascii_alphanumeric() && bytes[i - 1] != b'_';
                    if word_boundary {
                        return true;
                    }
                }
            }
        }

        // Skip to next non-word char (fast-forward through current word)
        while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
            i += 1;
        }
    }

    false
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- rearrange_pipe_command ---

    #[test]
    fn test_rearrange_simple_pipe() {
        let result = rearrange_pipe_command("cat file.txt | grep pattern");
        assert!(
            result.starts_with('\''),
            "should start with single quote: {}",
            result
        );
        assert!(
            result.contains("< /dev/null"),
            "should contain stdin redirect: {}",
            result
        );
        assert!(result.contains("|"), "should preserve pipe: {}", result);
    }

    #[test]
    fn test_rearrange_no_pipe_uses_fallback() {
        let result = rearrange_pipe_command("echo hello");
        // No pipe → fallback: 'echo hello' < /dev/null
        assert!(
            result.starts_with('\''),
            "fallback should be single-quoted: {}",
            result
        );
        assert!(
            result.contains("< /dev/null"),
            "fallback should have redirect: {}",
            result
        );
    }

    #[test]
    fn test_rearrange_backtick_bail() {
        let result = rearrange_pipe_command("echo `pwd` | cat");
        assert!(
            result.ends_with("< /dev/null"),
            "backtick bail should end with redirect: {}",
            result
        );
    }

    #[test]
    fn test_rearrange_substitution_bail() {
        let result = rearrange_pipe_command("echo $(pwd) | cat");
        assert!(result.ends_with("< /dev/null"));
    }

    #[test]
    fn test_rearrange_variable_ref_bail() {
        let result = rearrange_pipe_command("echo $HOME | cat");
        assert!(result.ends_with("< /dev/null"));
    }

    #[test]
    fn test_rearrange_variable_brace_bail() {
        let result = rearrange_pipe_command("echo ${PATH} | cat");
        assert!(result.ends_with("< /dev/null"));
    }

    #[test]
    fn test_rearrange_control_structure_bail() {
        let result = rearrange_pipe_command("for i in 1 2 3; do echo $i; done | cat");
        assert!(result.ends_with("< /dev/null"));
    }

    #[test]
    fn test_rearrange_if_statement_bail() {
        let result = rearrange_pipe_command("if true; then echo hi; fi | grep hi");
        assert!(result.ends_with("< /dev/null"));
    }

    #[test]
    fn test_rearrange_pipe_with_env_var() {
        let result = rearrange_pipe_command("LANG=C sort file | head");
        assert!(result.starts_with('\''));
        assert!(result.contains("LANG=C"));
    }

    // --- join_continuation_lines ---

    #[test]
    fn test_join_continuation_odd_backslash() {
        let result = join_continuation_lines("echo hello \\\nworld");
        assert_eq!(result, "echo hello world");
    }

    #[test]
    fn test_join_continuation_triple_backslash() {
        let result = join_continuation_lines("echo hello \\\\\\\nworld");
        // Odd (3), one escapes the \n, keep 2 as literal
        assert_eq!(result, "echo hello \\\\world");
    }

    #[test]
    fn test_join_continuation_even_backslash() {
        let result = join_continuation_lines("echo hello \\\\\nworld");
        // Even (2), \n is a real separator
        assert_eq!(result, "echo hello \\\\\nworld");
    }

    #[test]
    fn test_join_continuation_no_match() {
        let result = join_continuation_lines("echo hello");
        assert_eq!(result, "echo hello");
    }

    // --- single_quote_for_eval ---

    #[test]
    fn test_single_quote_simple() {
        assert_eq!(single_quote_for_eval("hello"), "'hello'");
    }

    #[test]
    fn test_single_quote_with_embedded_single() {
        let result = single_quote_for_eval("it's fine");
        assert_eq!(result, "'it'\"'\"'s fine'");
    }

    // --- contains_control_structure ---

    #[test]
    fn test_contains_for_loop() {
        assert!(contains_control_structure("for i in 1 2 3"));
    }

    #[test]
    fn test_contains_while_loop() {
        assert!(contains_control_structure("while true; do"));
    }

    #[test]
    fn test_contains_if_statement() {
        assert!(contains_control_structure("if [ -f file ]"));
    }

    #[test]
    fn test_contains_case_statement() {
        assert!(contains_control_structure("case $x in"));
    }

    #[test]
    fn test_contains_select() {
        assert!(contains_control_structure("select name in list"));
    }

    #[test]
    fn test_contains_until() {
        assert!(contains_control_structure("until false; do"));
    }

    #[test]
    fn test_no_control_structure() {
        assert!(!contains_control_structure("echo hello"));
    }

    #[test]
    fn test_false_positive_for_in_word() {
        // "before" starts with "for" but isn't a keyword — must be at word boundary
        assert!(!contains_control_structure("echo before"));
    }

    #[test]
    fn test_false_positive_in_path() {
        // /path/to/ifconfig is not an if statement
        assert!(!contains_control_structure("/path/to/ifconfig list"));
    }

    // --- has_shell_variable_ref ---

    #[test]
    fn test_has_variable_ref_dollar() {
        assert!(has_shell_variable_ref("echo $HOME"));
    }

    #[test]
    fn test_has_variable_ref_brace() {
        assert!(has_shell_variable_ref("echo ${PATH}"));
    }

    #[test]
    fn test_has_variable_ref_no_match() {
        assert!(!has_shell_variable_ref("echo hello"));
    }

    #[test]
    fn test_has_variable_ref_uppercase() {
        assert!(has_shell_variable_ref("echo $MYVAR"));
    }

    // --- quote_shell_token ---

    #[test]
    fn test_quote_safe_token() {
        assert_eq!(quote_shell_token("hello"), "hello");
    }

    #[test]
    fn test_quote_token_with_spaces() {
        let result = quote_shell_token("hello world");
        assert_eq!(result, "'hello world'");
    }

    #[test]
    fn test_quote_empty_token() {
        assert_eq!(quote_shell_token(""), "''");
    }

    // --- quote_with_eval_stdin_redirect ---

    #[test]
    fn test_quote_with_redirect() {
        let result = quote_with_eval_stdin_redirect("echo hello | wc");
        assert_eq!(result, "'echo hello | wc' < /dev/null");
    }
}
