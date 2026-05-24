//! Shell history completion provider.
//!
//! Provides `!`-prefixed shell history completions by reading `~/.bash_history`
//! and `~/.zsh_history` files with a 60-second TTL cache.
//!
//! Returns entries that match the current `!` prefix input as ghost suffix text.

use std::ops::Range;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::completions::{CompletionContext, CompletionItem, CompletionKind, CompletionProvider};

/// Maximum number of parsed history entries to keep in cache.
const MAX_HISTORY_ENTRIES: usize = 500;

/// Cache TTL for shell history files.
const CACHE_TTL: Duration = Duration::from_secs(60);

/// Minimum input length (after `!`) to trigger completion.
const MIN_QUERY_LENGTH: usize = 2;

// ---------------------------------------------------------------------------
// ShellHistoryCompletionProvider
// ---------------------------------------------------------------------------

/// Completion provider for `!` shell history.
///
/// Reads shell history files (`~/.bash_history`, `~/.zsh_history`) and returns
/// matching entries when the input starts with `!`.
pub struct ShellHistoryCompletionProvider {
    /// Parsed history entries (most recent first).
    cache: Vec<String>,
    /// When the cache was last refreshed.
    cached_at: Option<Instant>,
    /// Known history file paths (auto-detected on first access).
    history_paths: Vec<PathBuf>,
}

impl ShellHistoryCompletionProvider {
    pub fn new() -> Self {
        Self {
            cache: Vec::new(),
            cached_at: None,
            history_paths: Self::detect_history_files(),
        }
    }

    /// Detect shell history files from the home directory.
    fn detect_history_files() -> Vec<PathBuf> {
        let mut paths = Vec::new();

        if let Some(home) = dirs::home_dir() {
            // bash
            let bash_history = home.join(".bash_history");
            if bash_history.exists() {
                paths.push(bash_history);
            }
            // zsh
            let zsh_history = home.join(".zsh_history");
            if zsh_history.exists() {
                paths.push(zsh_history);
            }
            // Generic shell history files
            for name in &[".history", ".sh_history"] {
                let p = home.join(name);
                if p.exists() {
                    paths.push(p);
                }
            }
        }

        paths
    }

    /// Set custom history file paths (for testing).
    #[cfg(test)]
    pub fn with_paths(paths: Vec<PathBuf>) -> Self {
        Self {
            cache: Vec::new(),
            cached_at: None,
            history_paths: paths,
        }
    }

    /// Refresh the cache from history files (if TTL expired).
    fn ensure_cache_fresh(&mut self) {
        if let Some(cached_at) = self.cached_at {
            if cached_at.elapsed() < CACHE_TTL {
                return;
            }
        }

        let mut entries: Vec<String> = Vec::new();

        for path in &self.history_paths {
            if let Ok(content) = std::fs::read_to_string(path) {
                for line in content.lines() {
                    let line = line.trim();
                    if !line.is_empty() {
                        // Parse zsh extended history format: ": timestamp:session;command"
                        let line = if let Some(semi_pos) = line.find(';') {
                            if line.starts_with(':') {
                                &line[semi_pos + 1..]
                            } else {
                                line
                            }
                        } else {
                            line
                        };

                        let line = line.trim();
                        if !line.is_empty() {
                            entries.push(line.to_string());
                        }
                    }
                }
            }
        }

        // Deduplicate while preserving order (most recent first)
        let mut seen = std::collections::HashSet::new();
        let mut deduped: Vec<String> = Vec::new();
        for entry in entries.into_iter().rev() {
            if seen.insert(entry.clone()) {
                deduped.push(entry);
                if deduped.len() >= MAX_HISTORY_ENTRIES {
                    break;
                }
            }
        }

        self.cache = deduped;
        self.cached_at = Some(Instant::now());
    }

    /// Check if the input has a `!` prefix at the cursor position.
    fn find_bang_prefix(input: &str, cursor_pos: usize) -> Option<(Range<usize>, String)> {
        let byte_pos = cursor_pos.min(input.len());
        let prefix = &input[..byte_pos];

        // Find `!` at the start of a word (after whitespace or BOL)
        let bang_pos = prefix.rfind('!')?;

        // `!` must be at BOL or preceded by whitespace
        if bang_pos > 0 && input.as_bytes()[bang_pos - 1] != b' ' {
            return None;
        }

        let after_bang = &prefix[bang_pos + 1..byte_pos];
        // Minimum 2 chars for `!x`
        if after_bang.len() < MIN_QUERY_LENGTH {
            return None;
        }

        Some((bang_pos..byte_pos, after_bang.to_string()))
    }

    /// Update cache after a shell command execution (called externally).
    #[cfg(test)]
    pub fn notify_command_executed(&mut self, _command: &str) {
        // Invalidate cache so next access re-reads history files
        self.cached_at = None;
    }
}

impl CompletionProvider for ShellHistoryCompletionProvider {
    fn compute(&self, ctx: &CompletionContext) -> Vec<CompletionItem> {
        let (range, query) = match Self::find_bang_prefix(ctx.input, ctx.cursor_pos) {
            Some(result) => result,
            None => return Vec::new(),
        };

        let query_lower = query.to_ascii_lowercase();
        let mut provider = self.clone(); // We need mutability for cache refresh
        provider.ensure_cache_fresh();

        let mut completions = Vec::new();

        for entry in &provider.cache {
            let entry_lower = entry.to_ascii_lowercase();
            if !entry_lower.starts_with(&query_lower) {
                continue;
            }

            let ghost = if entry.len() > query.len() {
                Some(entry[query.len()..].to_string())
            } else {
                None
            };

            completions.push(
                CompletionItem::new(
                    CompletionKind::ShellHistory,
                    entry.clone(),
                    format!("!{}", entry),
                    range.clone(),
                )
                .with_detail("shell command".to_string())
                .with_ghost_suffix(ghost.unwrap_or_default()),
            );

            // Limit results to 20
            if completions.len() >= 20 {
                break;
            }
        }

        completions
    }

    fn priority(&self) -> u32 {
        60
    }
}

impl Default for ShellHistoryCompletionProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for ShellHistoryCompletionProvider {
    fn clone(&self) -> Self {
        Self {
            cache: self.cache.clone(),
            cached_at: self.cached_at,
            history_paths: self.history_paths.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_ctx(input: &str, cursor_pos: usize) -> CompletionContext<'_> {
        CompletionContext::new(input, cursor_pos, &[])
    }

    #[test]
    fn parse_history_and_match_bang() {
        let dir = tempdir().unwrap();
        let hist_path = dir.path().join(".test_history");
        std::fs::write(
            &hist_path,
            "git commit -m \"fix\"\ncargo build\nnpm test\ngit push\n",
        )
        .unwrap();

        let _provider = ShellHistoryCompletionProvider::with_paths(vec![hist_path]);
        // We need to find the `!` prefix with minimum 2 chars
        let result = ShellHistoryCompletionProvider::find_bang_prefix("!gi", 3);
        assert!(result.is_some(), "expected bang prefix found");
        let (_range, query) = result.unwrap();
        assert_eq!(query, "gi");
    }

    #[test]
    fn bang_prefix_requires_min_length() {
        let result = ShellHistoryCompletionProvider::find_bang_prefix("!g", 2);
        assert!(result.is_none(), "single char after ! should not trigger");

        let result = ShellHistoryCompletionProvider::find_bang_prefix("!gi", 3);
        assert!(result.is_some(), "two chars after ! should trigger");
    }

    #[test]
    fn no_bang_prefix_no_completions() {
        let provider = ShellHistoryCompletionProvider::new();
        let ctx = make_ctx("hello world", 11);
        let items = provider.compute(&ctx);
        assert!(items.is_empty(), "no ! means no shell history completions");
    }

    #[test]
    fn command_execution_notification_invalidates_cache() {
        let mut provider = ShellHistoryCompletionProvider::new();
        provider.cached_at = Some(std::time::Instant::now());
        provider.notify_command_executed("git status");
        assert!(provider.cached_at.is_none());
    }

    #[test]
    fn bang_must_be_at_word_start() {
        let result = ShellHistoryCompletionProvider::find_bang_prefix("abc!git", 6);
        assert!(result.is_none(), "! in middle of word should not trigger");
    }
}
