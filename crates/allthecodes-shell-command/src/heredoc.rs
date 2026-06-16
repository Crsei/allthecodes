//! Heredoc extraction and restoration utilities.
//!
//! Port of Bun `src/utils/bash/heredoc.ts`.
//!
//! The shell-words crate (like shell-quote) parses `<<` as two separate `<`
//! redirect operators, which breaks command splitting for heredoc syntax. This
//! module extracts heredocs before parsing and restores them after.
//!
//! Supported variations:
//! - `<<WORD`      — basic heredoc
//! - `<<'WORD'`    — single-quoted delimiter (no expansion in body)
//! - `<<"WORD"`    — double-quoted delimiter (with expansion)
//! - `<<-WORD`     — tab-stripping heredoc
//! - `<<-'WORD'`   — combined tab-stripping and quoted

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const HEREDOC_PLACEHOLDER_PREFIX: &str = "__HEREDOC_";
const HEREDOC_PLACEHOLDER_SUFFIX: &str = "__";

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Information about an extracted heredoc.
#[derive(Debug, Clone)]
pub struct HeredocInfo {
    /// Full heredoc text: operator + newline + content + closing delimiter.
    pub full_text: String,
    /// The delimiter word (without quotes or backslash).
    pub delimiter: String,
    /// Byte offset of the `<<` operator in the original command.
    pub operator_start_index: usize,
    /// Byte offset past the operator (delimiter end).
    pub operator_end_index: usize,
    /// Byte offset of the newline before heredoc content.
    pub content_start_index: usize,
    /// Byte offset past the closing delimiter (exclusive).
    pub content_end_index: usize,
}

/// Result of `extract_heredocs()`.
#[derive(Debug, Clone)]
pub struct HeredocExtractionResult {
    /// The command with heredocs replaced by `__HEREDOC_N_SALT__` placeholders.
    pub processed_command: String,
    /// Map from placeholder string to original heredoc info.
    pub heredocs: HashMap<String, HeredocInfo>,
}

/// Options for `extract_heredocs()`.
#[derive(Debug, Clone, Copy, Default)]
pub struct ExtractOptions {
    /// When true, only extract quoted/escaped heredocs (`<<'EOF'`, `<<"EOF"`,
    /// `<<\\EOF`). Unquoted heredocs (`<<EOF`) have their bodies expanded by
    /// bash and are left in place so security validators can inspect them.
    pub quoted_only: bool,
}

// ---------------------------------------------------------------------------
// Salt generation
// ---------------------------------------------------------------------------

/// Generate a unique salt string for heredoc placeholders.
///
/// Uses a mix of process-id, timestamp, and atomic counter — no `rand` dep.
/// Produces 16 hex characters, matching Bun's `randomBytes(8).toString('hex')`.
pub fn generate_placeholder_salt() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let pid = std::process::id() as u64;
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mixed = pid
        .wrapping_mul(6364136223846793005)
        .wrapping_add(time)
        .wrapping_add(count);
    format!("{:016x}", mixed)
}

// ---------------------------------------------------------------------------
// Incremental scanner state machine
// ---------------------------------------------------------------------------

/// Tracks quote/comment/escape state for byte-by-byte scanning.
///
/// Port of Bun's `advanceScan()` closure state.
#[derive(Debug, Default, Clone, Copy)]
struct ScanState {
    pos: usize,
    in_single_quote: bool,
    in_double_quote: bool,
    in_comment: bool,
    dq_escape_next: bool,
    pending_backslashes: usize,
}

impl ScanState {
    /// Advance scanning forward to `target` byte offset.
    fn advance(&mut self, command: &[u8], target: usize) {
        let len = command.len();
        while self.pos < target && self.pos < len {
            let ch = command[self.pos];

            // Physical newline clears comment state (matching Bun's quote-blind
            // lastIndexOf('\n', pos-1)+1 semantics).
            if ch == b'\n' {
                self.in_comment = false;
            }

            if self.in_single_quote {
                if ch == b'\'' {
                    self.in_single_quote = false;
                }
                self.pos += 1;
                continue;
            }

            if self.in_double_quote {
                if self.dq_escape_next {
                    self.dq_escape_next = false;
                    self.pos += 1;
                    continue;
                }
                if ch == b'\\' {
                    self.dq_escape_next = true;
                    self.pos += 1;
                    continue;
                }
                if ch == b'"' {
                    self.in_double_quote = false;
                }
                self.pos += 1;
                continue;
            }

            // Unquoted context — quote tracking is comment-blind:
            // we do NOT skip anything for being "in a comment", only `#`
            // detection itself is gated on !in_comment.
            if ch == b'\\' {
                self.pending_backslashes += 1;
                self.pos += 1;
                continue;
            }
            let escaped = self.pending_backslashes % 2 == 1;
            self.pending_backslashes = 0;
            if escaped {
                self.pos += 1;
                continue;
            }

            if ch == b'\'' {
                self.in_single_quote = true;
            } else if ch == b'"' {
                self.in_double_quote = true;
            } else if !self.in_comment && ch == b'#' {
                self.in_comment = true;
            }
            self.pos += 1;
        }
    }

    /// Whether the position at `target` is escaped by an odd backslash run.
    fn is_escaped(&self) -> bool {
        self.pending_backslashes % 2 == 1
    }
}

// ---------------------------------------------------------------------------
// Character classification
// ---------------------------------------------------------------------------

fn is_heredoc_word_char(ch: u8) -> bool {
    matches!(ch, b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_')
}

fn is_bash_metachar(ch: u8) -> bool {
    matches!(
        ch,
        b' ' | b'\t' | b'\n' | b'|' | b'&' | b';' | b'(' | b')' | b'<' | b'>'
    )
}

fn is_pst_eoftoken_char(ch: u8) -> bool {
    matches!(
        ch,
        b')' | b'}' | b'`' | b'|' | b'&' | b';' | b'(' | b'<' | b'>'
    )
}

// ---------------------------------------------------------------------------
// Heredoc position finder
// ---------------------------------------------------------------------------

/// Find all `<<` positions that could be heredoc operators (not `<<<`).
fn find_heredoc_positions(command: &[u8]) -> Vec<usize> {
    let mut positions = Vec::new();
    let len = command.len();
    let mut i = 0;

    while i + 1 < len {
        if command[i] == b'<' && command[i + 1] == b'<' {
            // Lookbehind: not preceded by `<`  (avoids matching second `<<` in `<<<`)
            if i > 0 && command[i - 1] == b'<' {
                i += 1;
                continue;
            }
            // Lookahead: not followed by `<` (avoids matching `<<<` at all)
            if i + 2 < len && command[i + 2] == b'<' {
                i += 3;
                continue;
            }
            positions.push(i);
            i += 2;
        } else {
            i += 1;
        }
    }

    positions
}

// ---------------------------------------------------------------------------
// Heredoc operator parsing
// ---------------------------------------------------------------------------

/// Result of parsing a heredoc operator at a `<<` position.
struct HeredocOp {
    has_dash: bool,
    delimiter: String,
    is_quoted: bool,
    /// Byte offset past the complete operator (delimiter end).
    operator_end: usize,
}

/// Parse the heredoc operator starting at `<<` position `start`.
///
/// Returns `None` if the syntax at this position is not a valid heredoc.
fn parse_heredoc_operator(command: &[u8], start: usize) -> Option<HeredocOp> {
    let len = command.len();
    let mut pos = start + 2; // after `<<`

    // Optional `-` for tab stripping
    let has_dash = pos < len && command[pos] == b'-';
    if has_dash {
        pos += 1;
    }

    // Skip whitespace
    while pos < len && (command[pos] == b' ' || command[pos] == b'\t') {
        pos += 1;
    }

    if pos >= len {
        return None;
    }

    // Quoted delimiter: `'WORD'` or `"WORD"`
    if command[pos] == b'\'' || command[pos] == b'"' {
        let quote = command[pos];
        pos += 1;
        let delim_start = pos;

        // Inside quotes: optional backslash followed by word chars (Bun: `\\?\w+`)
        if pos < len && command[pos] == b'\\' {
            pos += 1;
        }
        while pos < len && is_heredoc_word_char(command[pos]) {
            pos += 1;
        }

        let delimiter = std::str::from_utf8(&command[delim_start..pos])
            .ok()?
            .to_string();
        if delimiter.is_empty() {
            return None;
        }

        // Closing quote must match
        if pos >= len || command[pos] != quote {
            return None;
        }
        pos += 1;

        return Some(HeredocOp {
            has_dash,
            delimiter,
            is_quoted: true,
            operator_end: pos,
        });
    }

    // Unquoted delimiter: optional `\` then word chars (Bun: `\\?(\w+)`)
    // NOTE: For unquoted, the backslash is consumed but NOT part of delimiter.
    if pos < len && command[pos] == b'\\' {
        pos += 1; // consume the backslash (it escapes the first char)
    }
    let delim_start = pos;
    while pos < len && is_heredoc_word_char(command[pos]) {
        pos += 1;
    }

    if pos == delim_start {
        return None;
    }

    let delimiter = std::str::from_utf8(&command[delim_start..pos])
        .ok()?
        .to_string();
    Some(HeredocOp {
        has_dash,
        delimiter,
        is_quoted: false,
        operator_end: pos,
    })
}

// ---------------------------------------------------------------------------
// First unquoted newline finder
// ---------------------------------------------------------------------------

/// Find the first newline that is NOT inside a quoted string, starting from
/// `start`. Returns the offset from `start`, or `None`.
///
/// This mirrors the quote-tracking loop in Bun's `extractHeredocs` that uses
/// separate `inSingleQuote`/`inDoubleQuote` state (not the incremental
/// scanner), beginning with clean state at `operatorEndIndex`.
fn find_first_unquoted_newline(command: &[u8], start: usize) -> Option<usize> {
    let len = command.len();
    let mut in_single = false;
    let mut in_double = false;

    let mut k = start;
    while k < len {
        let ch = command[k];
        if in_single {
            if ch == b'\'' {
                in_single = false;
            }
            k += 1;
            continue;
        }
        if in_double {
            if ch == b'\\' {
                k += 2; // skip escaped char
                continue;
            }
            if ch == b'"' {
                in_double = false;
            }
            k += 1;
            continue;
        }
        // Unquoted context
        if ch == b'\n' {
            return Some(k - start);
        }
        // Count backslashes for escape detection
        let mut backslash_count = 0usize;
        for j in (start..k).rev() {
            if command[j] == b'\\' {
                backslash_count += 1;
            } else {
                break;
            }
        }
        if backslash_count % 2 == 1 {
            // escaped — skip
            k += 1;
            continue;
        }
        if ch == b'\'' {
            in_single = true;
        } else if ch == b'"' {
            in_double = true;
        }
        k += 1;
    }
    None
}

// ---------------------------------------------------------------------------
// Backslash continuation check
// ---------------------------------------------------------------------------

/// Check whether the content between `op_end` and `op_end + newline_offset`
/// ends with an odd number of backslashes, which would make `\<newline>` a
/// line continuation in bash, meaning the heredoc body starts one line later.
fn has_backslash_continuation(command: &[u8], op_end: usize, newline_offset: usize) -> bool {
    let end = op_end + newline_offset;
    if end > command.len() {
        return false;
    }
    let trailing = command[op_end..end]
        .iter()
        .rev()
        .take_while(|&&b| b == b'\\')
        .count();
    trailing % 2 == 1
}

// ---------------------------------------------------------------------------
// Closing delimiter search
// ---------------------------------------------------------------------------

/// Search for a heredoc's closing delimiter line within `content`.
///
/// Returns `(line_index, pst_eoftoken)` where:
/// - `line_index`: position in the split lines if found, or `None`
/// - `pst_eoftoken`: set when a PST_EOFTOKEN-like early closure is detected,
///   which indicates the heredoc should be skipped entirely
fn find_closing_delimiter(
    content: &str,
    delimiter: &str,
    strip_tabs: bool,
) -> (Option<usize>, bool) {
    let lines: Vec<&str> = content.split('\n').collect();

    for (i, line) in lines.iter().enumerate() {
        let candidate = if strip_tabs {
            line.trim_start_matches('\t')
        } else {
            line
        };

        if candidate == delimiter {
            return (Some(i), false);
        }

        // PST_EOFTOKEN check: if line starts with delimiter followed by a
        // shell metachar or substitution closer, bash may close the heredoc
        // early (make_cmd.c:606).
        if candidate.len() > delimiter.len() && candidate.starts_with(delimiter) {
            let after = candidate.as_bytes()[delimiter.len()];
            if is_pst_eoftoken_char(after) {
                return (None, true);
            }
        }
    }

    (None, false)
}

// ---------------------------------------------------------------------------
// Main extraction function
// ---------------------------------------------------------------------------

/// Extracts heredocs from a command string and replaces them with placeholders.
///
/// This allows shell-words (or other parsers) to process the command without
/// mangling heredoc syntax. After processing, use `restore_heredocs()` to
/// replace placeholders with original content.
///
/// # Security
///
/// The incremental scanner follows Bun's quote-tracking semantics and bails
/// on constructs that could desync its state:
/// - `$'...'` / `$"..."` ANSI-C / locale quoting
/// - Backtick command substitution before the first `<<`
/// - Unbalanced `((` arithmetic evaluation before the first `<<`
/// - Backslash-newline continuation on same-line content after operator
/// - PST_EOFTOKEN-like early heredoc closure
///
/// When extraction fails for any reason, the command passes through unchanged.
/// This is safe because the unextracted heredoc will either cause shell-words
/// to fail (falling back to raw splitting) or require manual approval.
pub fn extract_heredocs(
    command: &str,
    options: Option<&ExtractOptions>,
) -> HeredocExtractionResult {
    let command_bytes = command.as_bytes();

    // --- Quick check ---
    if !command.contains("<<") {
        return HeredocExtractionResult {
            processed_command: command.to_string(),
            heredocs: HashMap::new(),
        };
    }

    // --- Security: ANSI-C / locale quoting ---
    // $'...' or $"..." — our scanner can't handle the $ prefix on quotes.
    if command.contains("$'") || command.contains("$\"") {
        return HeredocExtractionResult {
            processed_command: command.to_string(),
            heredocs: HashMap::new(),
        };
    }

    // --- Security: backticks before first `<<` ---
    let Some(first_heredoc_pos) = command.find("<<") else {
        return HeredocExtractionResult {
            processed_command: command.to_string(),
            heredocs: HashMap::new(),
        };
    };
    if first_heredoc_pos > 0 {
        let before = &command_bytes[..first_heredoc_pos];
        if before.contains(&b'`') {
            return HeredocExtractionResult {
                processed_command: command.to_string(),
                heredocs: HashMap::new(),
            };
        }
    }

    // --- Security: unbalanced `((` before first `<<` (arithmetic context) ---
    if first_heredoc_pos > 0 {
        let before = &command[..first_heredoc_pos];
        let open_arith = before.matches("((").count();
        let close_arith = before.matches("))").count();
        if open_arith > close_arith {
            return HeredocExtractionResult {
                processed_command: command.to_string(),
                heredocs: HashMap::new(),
            };
        }
    }

    let opts = options
        .copied()
        .unwrap_or(ExtractOptions { quoted_only: false });

    // --- Find heredoc positions ---
    let positions = find_heredoc_positions(command_bytes);

    // --- Incremental scanner + operator parsing ---
    let mut scan = ScanState::default();
    let mut heredoc_matches: Vec<HeredocInfo> = Vec::new();
    let mut skipped_ranges: Vec<(usize, usize)> = Vec::new(); // (start, end)

    for &pos in &positions {
        // Advance incremental scanner to this position
        scan.advance(command_bytes, pos);

        // Skip if inside quotes
        if scan.in_single_quote || scan.in_double_quote {
            continue;
        }

        // Skip if inside comment
        if scan.in_comment {
            continue;
        }

        // Skip if preceded by odd number of backslashes (escaped `<`)
        if scan.is_escaped() {
            continue;
        }

        // Parse the heredoc operator
        let op = match parse_heredoc_operator(command_bytes, pos) {
            Some(op) => op,
            None => continue,
        };

        let HeredocOp {
            has_dash,
            delimiter,
            is_quoted,
            operator_end,
        } = op;

        // Security: verify next char after operator is a bash metachar or EOS
        if operator_end < command_bytes.len() {
            let next = command_bytes[operator_end];
            if !is_bash_metachar(next) {
                continue;
            }
        }

        // Security: determine if delimiter or content is quoted/escaped
        let is_quoted_or_escaped = is_quoted; // for unquoted, backslash is consumed, not in delimiter

        // Find first unquoted newline after operator
        let newline_offset = match find_first_unquoted_newline(command_bytes, operator_end) {
            Some(off) => off,
            None => continue,
        };
        let content_start_index = operator_end + newline_offset;

        // Security: check for backslash-newline continuation on same-line content
        if has_backslash_continuation(command_bytes, operator_end, newline_offset) {
            continue;
        }

        // Content starts after the newline
        let content_start_byte = content_start_index + 1; // skip the newline
        let after_newline = &command[content_start_byte..];

        // Find closing delimiter
        let (closing_line_idx, pst_eof) =
            find_closing_delimiter(after_newline, &delimiter, has_dash);
        if pst_eof {
            continue;
        }

        // Handle quotedOnly mode for unquoted heredocs
        if opts.quoted_only && !is_quoted_or_escaped {
            let skip_end = if let Some(closing_line) = closing_line_idx {
                let lines_up_to: Vec<&str> =
                    after_newline.split('\n').take(closing_line + 1).collect();
                content_start_byte + lines_up_to.join("\n").len()
            } else {
                command_bytes.len()
            };
            skipped_ranges.push((content_start_index, skip_end));
            continue;
        }

        // If no closing delimiter found, skip this heredoc
        let closing_line = match closing_line_idx {
            Some(idx) => idx,
            None => continue,
        };

        // Calculate content end
        let lines_up_to: Vec<&str> = after_newline.split('\n').take(closing_line + 1).collect();
        let content_length = lines_up_to.join("\n").len();
        let content_end_index = content_start_byte + content_length;

        // Security: check content range doesn't overlap with skipped ranges
        let mut overlaps = false;
        for &(sk_start, sk_end) in &skipped_ranges {
            if content_start_index < sk_end && sk_start < content_end_index {
                overlaps = true;
                break;
            }
        }
        if overlaps {
            continue;
        }

        // Build full text for restoration
        let operator_text = &command[pos..operator_end];
        let content_text = &command[content_start_index..content_end_index];
        let full_text = format!("{}{}", operator_text, content_text);

        heredoc_matches.push(HeredocInfo {
            full_text,
            delimiter,
            operator_start_index: pos,
            operator_end_index: operator_end,
            content_start_index,
            content_end_index,
        });
    }

    // --- No valid heredocs found ---
    if heredoc_matches.is_empty() {
        return HeredocExtractionResult {
            processed_command: command.to_string(),
            heredocs: HashMap::new(),
        };
    }

    // --- Filter nested heredocs ---
    let top_level: Vec<HeredocInfo> = heredoc_matches
        .iter()
        .enumerate()
        .filter(|(i, candidate)| {
            !heredoc_matches.iter().enumerate().any(|(j, other)| {
                j != *i
                    && candidate.operator_start_index > other.content_start_index
                    && candidate.operator_start_index < other.content_end_index
            })
        })
        .map(|(_, h)| h.clone())
        .collect();

    if top_level.is_empty() {
        return HeredocExtractionResult {
            processed_command: command.to_string(),
            heredocs: HashMap::new(),
        };
    }

    // --- Check for multiple heredocs sharing content start position ---
    {
        let mut seen = std::collections::HashSet::new();
        let has_shared = !top_level.iter().all(|h| seen.insert(h.content_start_index));
        if has_shared {
            return HeredocExtractionResult {
                processed_command: command.to_string(),
                heredocs: HashMap::new(),
            };
        }
    }

    // --- Sort descending by content_end_index for safe replacement ---
    let mut sorted = top_level;
    sorted.sort_by(|a, b| b.content_end_index.cmp(&a.content_end_index));

    // --- Generate salt and replace ---
    let salt = generate_placeholder_salt();
    let mut result_map: HashMap<String, HeredocInfo> = HashMap::new();
    let mut processed = command.to_string();

    for (index, info) in sorted.iter().enumerate() {
        let placeholder_index = sorted.len() - 1 - index;
        let placeholder = format!(
            "{}{}_{}{}",
            HEREDOC_PLACEHOLDER_PREFIX, placeholder_index, salt, HEREDOC_PLACEHOLDER_SUFFIX
        );

        result_map.insert(placeholder.clone(), info.clone());

        // Replace: keep pre-operator + placeholder + same-line content, skip heredoc body
        let pre = &processed[..info.operator_start_index];
        let same_line = &processed[info.operator_end_index..info.content_start_index];
        let post = &processed[info.content_end_index..];
        processed = format!("{}{}{}{}", pre, placeholder, same_line, post);
    }

    HeredocExtractionResult {
        processed_command: processed,
        heredocs: result_map,
    }
}

// ---------------------------------------------------------------------------
// Restoration
// ---------------------------------------------------------------------------

/// Replace all heredoc placeholders in a single string.
fn restore_placeholders_in_string(text: &str, heredocs: &HashMap<String, HeredocInfo>) -> String {
    let mut result = text.to_string();
    for (placeholder, info) in heredocs {
        result = result.replace(placeholder, &info.full_text);
    }
    result
}

/// Restore heredoc placeholders in an array of string parts.
///
/// Each string in `parts` is checked for placeholders and any found are
/// replaced with the original heredoc text.
pub fn restore_heredocs(parts: &[String], heredocs: &HashMap<String, HeredocInfo>) -> Vec<String> {
    if heredocs.is_empty() {
        return parts.to_vec();
    }
    parts
        .iter()
        .map(|part| restore_placeholders_in_string(part, heredocs))
        .collect()
}

// ---------------------------------------------------------------------------
// Quick check
// ---------------------------------------------------------------------------

/// Check whether a command string appears to contain heredoc syntax.
///
/// This is a quick check using a manual scanner (not a full parse).
/// Use `extract_heredocs` for precise detection.
pub fn contains_heredoc(command: &str) -> bool {
    let bytes = command.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'<' && bytes[i + 1] == b'<' {
            // Avoid matching <<< (not a heredoc)
            if (i > 0 && bytes[i - 1] == b'<') || (i + 2 < bytes.len() && bytes[i + 2] == b'<') {
                i += 1;
                continue;
            }

            // After <<, skip optional dash then whitespace
            let mut pos = i + 2;
            if pos < bytes.len() && bytes[pos] == b'-' {
                pos += 1;
            }
            while pos < bytes.len() && (bytes[pos] == b' ' || bytes[pos] == b'\t') {
                pos += 1;
            }

            // Must be followed by a quote or word character (start of delimiter)
            if pos < bytes.len()
                && (bytes[pos] == b'\'' || bytes[pos] == b'"' || is_heredoc_word_char(bytes[pos]))
            {
                return true;
            }

            i = pos;
        } else {
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

    // --- contains_heredoc ---

    #[test]
    fn test_contains_heredoc_basic() {
        assert!(contains_heredoc("cat <<EOF"));
    }

    #[test]
    fn test_contains_heredoc_quoted() {
        assert!(contains_heredoc("cat <<'EOF'"));
        assert!(contains_heredoc(r#"cat <<"EOF""#));
    }

    #[test]
    fn test_contains_heredoc_with_dash() {
        assert!(contains_heredoc("cat <<-EOF"));
    }

    #[test]
    fn test_contains_heredoc_no_match() {
        assert!(!contains_heredoc("echo hello"));
    }

    #[test]
    fn test_contains_heredoc_triple_lt() {
        // <<< is not a heredoc
        assert!(!contains_heredoc("cat <<<EOF"));
    }

    // --- extract_heredocs basic ---

    #[test]
    fn test_extract_no_heredoc() {
        let result = extract_heredocs("echo hello", None);
        assert_eq!(result.processed_command, "echo hello");
        assert!(result.heredocs.is_empty());
    }

    #[test]
    fn test_extract_simple() {
        let result = extract_heredocs("cat <<EOF\nhello world\nEOF", None);
        assert_ne!(result.processed_command, "cat <<EOF\nhello world\nEOF");
        // Should have a placeholder
        assert!(result.processed_command.contains("__HEREDOC_"));
        assert_eq!(result.heredocs.len(), 1);
    }

    #[test]
    fn test_extract_and_restore_roundtrip() {
        let original = "cat <<EOF\nhello world\nEOF";
        let result = extract_heredocs(original, None);
        let restored = restore_placeholders_in_string(&result.processed_command, &result.heredocs);
        assert_eq!(original, restored);
    }

    #[test]
    fn test_extract_preserves_same_line_content() {
        let result = extract_heredocs("cat <<EOF && echo done\nhello\nEOF", None);
        // Same-line content " && echo done" should be preserved
        assert!(result.processed_command.contains("&& echo done"));
    }

    #[test]
    fn test_extract_restore_via_tokens_roundtrip() {
        // restore_heredocs works on PARSED TOKENS (shell-words output),
        // not on the raw command string. Simulate the full pipeline:
        // extract → shell-words split → restore → join
        let original = "cat <<EOF && echo done\nhello\nEOF";
        let result = extract_heredocs(original, None);

        // Parse the processed command with shell-words
        let tokens = shell_words::split(&result.processed_command).unwrap();

        // The placeholder is the second token (after "cat"): ["cat", "__HEREDOC_...", "&&", ...]
        assert_eq!(tokens.len(), 5, "tokens: {:?}", tokens);
        assert_eq!(tokens[0], "cat");
        assert!(tokens[1].starts_with("__HEREDOC_"));
        assert_eq!(tokens[2], "&&");
        assert_eq!(tokens[3], "echo");
        assert_eq!(tokens[4], "done");

        // Restore heredocs in the parsed tokens
        let restored = restore_heredocs(&tokens, &result.heredocs);
        // The placeholder is replaced with the full heredoc text
        assert!(!restored[1].starts_with("__HEREDOC_"));
        assert!(restored[1].contains("<<EOF"));
        assert!(restored[1].contains("hello"));
        assert!(restored[1].contains("EOF"));
        assert_eq!(restored[0], "cat");
        assert_eq!(restored[2], "&&");
        assert_eq!(restored[3], "echo");
        assert_eq!(restored[4], "done");
    }

    #[test]
    fn test_extract_quoted_delimiter() {
        let original = "cat <<'EOF'\nhello world\nEOF";
        let result = extract_heredocs(original, None);
        assert!(!result.heredocs.is_empty());
        let restored = restore_placeholders_in_string(&result.processed_command, &result.heredocs);
        assert_eq!(original, restored);
    }

    #[test]
    fn test_extract_double_quoted_delimiter() {
        let original = "cat <<\"EOF\"\nhello world\nEOF";
        let result = extract_heredocs(original, None);
        assert!(!result.heredocs.is_empty());
        let restored = restore_placeholders_in_string(&result.processed_command, &result.heredocs);
        assert_eq!(original, restored);
    }

    #[test]
    fn test_extract_dash_delimiter() {
        let original = "cat <<-EOF\n\thello world\n\tEOF";
        let result = extract_heredocs(original, None);
        assert!(!result.heredocs.is_empty());
        let restored = restore_placeholders_in_string(&result.processed_command, &result.heredocs);
        assert_eq!(original, restored);
    }

    #[test]
    fn test_extract_combined_dash_quoted() {
        let original = "cat <<-'EOF'\n\thello world\n\tEOF";
        let result = extract_heredocs(original, None);
        assert!(!result.heredocs.is_empty());
        let restored = restore_placeholders_in_string(&result.processed_command, &result.heredocs);
        assert_eq!(original, restored);
    }

    // --- quoted_only mode ---

    #[test]
    fn test_quoted_only_skips_unquoted() {
        let result = extract_heredocs(
            "cat <<EOF\nhello\nEOF",
            Some(&ExtractOptions { quoted_only: true }),
        );
        // Unquoted heredoc should not be extracted in quoted_only mode
        assert!(result.heredocs.is_empty());
        assert_eq!(result.processed_command, "cat <<EOF\nhello\nEOF");
    }

    #[test]
    fn test_quoted_only_extracts_quoted() {
        let result = extract_heredocs(
            "cat <<'EOF'\nhello\nEOF",
            Some(&ExtractOptions { quoted_only: true }),
        );
        // Quoted heredoc should be extracted even in quoted_only mode
        assert!(!result.heredocs.is_empty());
    }

    // --- security bails ---

    #[test]
    fn test_security_bail_ansi_c_quoting() {
        // $'...' should prevent extraction
        let result = extract_heredocs("echo $'<<EOF'\ncontent\nEOF", None);
        assert!(result.heredocs.is_empty());
    }

    #[test]
    fn test_security_bail_locale_quoting() {
        let result = extract_heredocs("echo $\" <<EOF\ncontent\nEOF", None);
        assert!(result.heredocs.is_empty());
    }

    #[test]
    fn test_security_bail_backtick_before_heredoc() {
        let result = extract_heredocs("echo `pwd` <<EOF\ncontent\nEOF", None);
        assert!(result.heredocs.is_empty());
    }

    #[test]
    fn test_security_bail_arithmetic_context() {
        // Unbalanced (( before << suggests arithmetic shift, not heredoc
        let result = extract_heredocs("(( x = 1 << 2 ))\necho content", None);
        assert!(result.heredocs.is_empty());
    }

    #[test]
    fn test_backslash_continuation_bail() {
        // <<EOF && \<newline> cmd — continuation hides cmd from validator
        let result = extract_heredocs("cat <<EOF && \\\necho hidden\ncontent\nEOF", None);
        assert!(result.heredocs.is_empty());
    }

    // --- multiple heredocs ---

    #[test]
    fn test_multiple_heredocs_different_lines() {
        // Two separate heredoc commands on different lines
        let result = extract_heredocs("cat <<EOF\nhello\nEOF\ncat <<END\nworld\nEND", None);
        assert!(!result.heredocs.is_empty());
        assert_eq!(result.processed_command.matches("__HEREDOC_").count(), 2);
        assert_eq!(result.heredocs.len(), 2);
    }

    // --- inline heredoc content ---

    #[test]
    fn test_heredoc_in_quotes_not_extracted() {
        // << inside quoted string should not be extracted
        let result = extract_heredocs("echo \"not <<EOF a heredoc\"", None);
        assert!(result.heredocs.is_empty());
    }

    #[test]
    fn test_heredoc_in_single_quotes_not_extracted() {
        let result = extract_heredocs("echo 'not <<EOF a heredoc'", None);
        assert!(result.heredocs.is_empty());
    }

    // --- heredocs with comment ---

    #[test]
    fn test_heredoc_after_comment_not_extracted() {
        // # comment after start of line
        let cmd = "echo hello # comment <<EOF\ncontent\nEOF";
        let result = extract_heredocs(cmd, None);
        // The `<<EOF` on the same line after `#` should be in a comment
        assert!(result.heredocs.is_empty());
    }

    // --- restore_heredocs ---

    #[test]
    fn test_restore_heredocs_empty() {
        let parts = vec!["echo hello".to_string()];
        let heredocs = HashMap::new();
        let restored = restore_heredocs(&parts, &heredocs);
        assert_eq!(restored, parts);
    }

    #[test]
    fn test_restore_heredocs_parts() {
        let original = "cat <<EOF\nhello\nEOF";
        let result = extract_heredocs(original, None);
        let parts = vec![result.processed_command.clone()];
        let restored = restore_heredocs(&parts, &result.heredocs);
        assert_eq!(restored[0], original);
    }
}
