//! Path completion provider.
//!
//! Provides file-system path completions for the TUI input prompt.
//! Uses `std::fs::read_dir` with an LRU cache and TTL for directory scans.
//! Supports `~` expansion, hidden-file filtering, and directory-first sorting.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use lru::LruCache;
use std::num::NonZeroUsize;

use super::completions::{CompletionContext, CompletionItem, CompletionKind, CompletionProvider};

// ---------------------------------------------------------------------------
// Cache
// ---------------------------------------------------------------------------

/// Cached directory listing with a timestamp for TTL-based expiry.
#[derive(Clone)]
struct DirCacheEntry {
    /// File names in this directory (sorted: directories first, then files).
    entries: Vec<CacheEntry>,
    /// When this cache entry was created.
    cached_at: Instant,
}

/// A single entry in a directory listing.
#[derive(Clone)]
struct CacheEntry {
    name: String,
    is_dir: bool,
}

/// TTL for directory scan cache (default 5 seconds).
const DEFAULT_CACHE_TTL: Duration = Duration::from_secs(5);

/// Maximum number of cached directories.
const MAX_CACHED_DIRS: usize = 64;

// ---------------------------------------------------------------------------
// PathCompletionProvider
// ---------------------------------------------------------------------------

/// Completion provider for file-system paths.
///
/// Completes partial file paths in the input prompt. The provider detects
/// path-like tokens (starting with `.`, `/`, `~`, or containing `/`) and
/// produces matching file/directory completions.
pub struct PathCompletionProvider {
    include_hidden: bool,
    include_files: bool,
    cache: LruCache<PathBuf, DirCacheEntry>,
    cache_ttl: Duration,
}

impl PathCompletionProvider {
    pub fn new() -> Self {
        Self {
            include_hidden: false,
            include_files: true,
            cache: LruCache::new(NonZeroUsize::new(MAX_CACHED_DIRS).unwrap()),
            cache_ttl: DEFAULT_CACHE_TTL,
        }
    }

    /// Whether to include hidden files (dotfiles) in completions.
    #[cfg(test)]
    pub fn set_include_hidden(&mut self, v: bool) {
        self.include_hidden = v;
    }

    /// Whether to include regular files (non-directories) in completions.
    #[cfg(test)]
    pub fn set_include_files(&mut self, v: bool) {
        self.include_files = v;
    }

    /// Set the cache TTL for directory scans.
    #[cfg(test)]
    pub fn set_cache_ttl(&mut self, ttl: Duration) {
        self.cache_ttl = ttl;
    }

    /// Parse a partial path from the input at the cursor position.
    ///
    /// Returns `(base_dir, prefix, full_range)` where:
    /// - `base_dir` is the directory to scan (resolved)
    /// - `prefix` is the partial file/directory name to filter by
    /// - `full_range` is the byte range in the input to replace
    fn parse_partial_path(
        &self,
        input: &str,
        cursor_pos: usize,
    ) -> Option<(PathBuf, String, Range<usize>)> {
        let byte_pos = cursor_pos.min(input.len());
        let prefix = &input[..byte_pos];

        // Find the start of the last path-like token
        let start = prefix[..byte_pos]
            .rfind(|c: char| c.is_whitespace())
            .map(|p| p + 1)
            .unwrap_or(0);

        let token = &input[start..byte_pos];

        // Skip empty or single-character tokens that don't look like paths
        if token.is_empty()
            || (!token.contains('/') && !token.starts_with('.') && !token.starts_with('~'))
        {
            return None;
        }

        let expanded = self.expand_path(token);
        let (base_dir, filename_prefix) = if expanded.ends_with('/') || token.ends_with('/') {
            // The token ends with `/`, so scan the directory
            let dir = PathBuf::from(&expanded);
            (dir, String::new())
        } else {
            // Split on last `/` to get base_dir and filename prefix
            match expanded.rsplit_once('/') {
                Some((dir, fname)) => (PathBuf::from(dir), fname.to_string()),
                None => {
                    // No directory separator — use cwd
                    (PathBuf::from("."), expanded)
                }
            }
        };

        Some((base_dir, filename_prefix, start..byte_pos))
    }

    /// Expand `~` to the home directory and normalize separators.
    fn expand_path(&self, path: &str) -> String {
        if path.starts_with('~') {
            if let Some(home) = dirs::home_dir() {
                let home_str = home.to_string_lossy().to_string();
                return path.replacen('~', &home_str, 1);
            }
        }
        path.to_string()
    }

    /// Get or refresh the directory listing cache.
    fn get_or_scan_dir(&mut self, dir: &Path) -> Vec<CacheEntry> {
        // Check cache
        if let Some(entry) = self.cache.get(dir) {
            if entry.cached_at.elapsed() < self.cache_ttl {
                return entry.entries.clone();
            }
        }

        // Scan directory
        let entries = match std::fs::read_dir(dir) {
            Ok(rd) => {
                let mut entries: Vec<CacheEntry> = rd
                    .filter_map(|entry| entry.ok())
                    .filter(|entry| {
                        let name = entry.file_name();
                        let name_str = name.to_string_lossy();
                        // Skip dotfiles unless include_hidden
                        if !self.include_hidden && name_str.starts_with('.') {
                            return false;
                        }
                        // Skip files if not include_files
                        if !self.include_files {
                            if let Ok(ft) = entry.file_type() {
                                if ft.is_file() {
                                    return false;
                                }
                            }
                        }
                        true
                    })
                    .map(|entry| {
                        let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
                        CacheEntry {
                            name: entry.file_name().to_string_lossy().to_string(),
                            is_dir,
                        }
                    })
                    .collect();

                // Sort: directories first, then files, both alphabetical
                entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.cmp(&b.name)));

                entries
            }
            Err(_) => Vec::new(),
        };

        let cache_entry = DirCacheEntry {
            entries: entries.clone(),
            cached_at: Instant::now(),
        };
        self.cache.put(dir.to_path_buf(), cache_entry);

        entries
    }
}

impl CompletionProvider for PathCompletionProvider {
    fn compute(&self, ctx: &CompletionContext) -> Vec<CompletionItem> {
        let (base_dir, prefix, range) = match self.parse_partial_path(ctx.input, ctx.cursor_pos) {
            Some(result) => result,
            None => return Vec::new(),
        };

        let prefix_lower = prefix.to_ascii_lowercase();
        let mut completions = Vec::new();

        // We need mutability for cache — use a clone for the cache part
        let mut provider = self.clone();
        let entries = provider.get_or_scan_dir(&base_dir);

        for entry in &entries {
            let name = &entry.name;
            if !prefix_lower.is_empty() && !name.to_ascii_lowercase().starts_with(&prefix_lower) {
                continue;
            }

            let suffix = if entry.is_dir { "/" } else { " " };
            let insert = format!("{}{}", name, suffix);
            let label = if entry.is_dir {
                format!("{}/", name)
            } else {
                name.clone()
            };

            let mut item = CompletionItem::new(CompletionKind::Path, label, insert, range.clone());
            if entry.is_dir {
                item.detail = Some("directory".to_string());
            }
            completions.push(item);
        }

        completions
    }

    fn priority(&self) -> u32 {
        80
    }
}

impl Default for PathCompletionProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for PathCompletionProvider {
    fn clone(&self) -> Self {
        // Re-create the cache since LruCache doesn't Clone
        let mut new_cache = LruCache::new(NonZeroUsize::new(MAX_CACHED_DIRS).unwrap());
        // Copy all cache entries
        for (dir, entry) in self.cache.iter() {
            let _ = new_cache.put(
                dir.clone(),
                DirCacheEntry {
                    entries: entry.entries.clone(),
                    cached_at: entry.cached_at,
                },
            );
        }
        Self {
            include_hidden: self.include_hidden,
            include_files: self.include_files,
            cache: new_cache,
            cache_ttl: self.cache_ttl,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn make_ctx(input: &str, cursor_pos: usize) -> CompletionContext<'_> {
        CompletionContext::new(input, cursor_pos, &[])
    }

    #[test]
    fn normal_path_completion() {
        let dir = tempdir().unwrap();
        let _src = fs::create_dir(dir.path().join("src"));
        fs::write(dir.path().join("main.rs"), "").unwrap();

        // Test with a relative path token using the temp dir
        let provider = PathCompletionProvider::new();
        let input = format!("cd {}/", dir.path().to_string_lossy());
        let result = provider.parse_partial_path(&input, input.len());
        assert!(result.is_some(), "expected parse_partial_path to succeed");

        let (base_dir, prefix, _range) = result.unwrap();
        // base_dir should be the temp dir path (directory before the last /)
        assert_eq!(base_dir, dir.path().to_path_buf());
        // prefix should be empty since the token ends with /
        assert_eq!(prefix, "");
    }

    #[test]
    fn tilda_expansion() {
        let provider = PathCompletionProvider::new();
        let expanded = provider.expand_path("~/docs");
        assert!(!expanded.starts_with('~'));
        assert!(expanded.ends_with("/docs") || expanded.ends_with("\\docs"));
    }

    #[test]
    fn hidden_files_excluded_by_default() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join(".hidden"), "").unwrap();
        fs::write(dir.path().join("visible"), "").unwrap();

        let mut provider = PathCompletionProvider::new();
        // Reset to fresh cache
        let entries = provider.get_or_scan_dir(dir.path());
        assert!(
            !entries.iter().any(|e| e.name == ".hidden"),
            "hidden files should be excluded by default"
        );
        assert!(
            entries.iter().any(|e| e.name == "visible"),
            "visible files should be included"
        );
    }

    #[test]
    fn hidden_files_included_when_option_set() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join(".hidden"), "").unwrap();
        fs::write(dir.path().join("visible"), "").unwrap();

        let mut provider = PathCompletionProvider::new();
        provider.set_include_hidden(true);
        let entries = provider.get_or_scan_dir(dir.path());
        assert!(
            entries.iter().any(|e| e.name == ".hidden"),
            "hidden files should be included when option set"
        );
        assert!(
            entries.iter().any(|e| e.name == "visible"),
            "visible files should be included"
        );
    }

    #[test]
    fn files_can_be_excluded_from_path_entries() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("file.txt"), "").unwrap();
        fs::create_dir(dir.path().join("folder")).unwrap();

        let mut provider = PathCompletionProvider::new();
        provider.set_include_files(false);
        let entries = provider.get_or_scan_dir(dir.path());
        assert!(entries.iter().any(|entry| entry.name == "folder"));
        assert!(!entries.iter().any(|entry| entry.name == "file.txt"));
    }

    #[test]
    fn empty_directory_returns_no_completions() {
        let dir = tempdir().unwrap();
        let _provider = PathCompletionProvider::new();

        // Manually clone for Rust borrow rules
        let mut p = PathCompletionProvider::new();
        let entries = p.get_or_scan_dir(dir.path());
        assert!(entries.is_empty(), "empty dir should have no entries");
    }

    #[test]
    fn lru_cache_expiry() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("test.txt"), "").unwrap();

        let mut provider = PathCompletionProvider::new();
        provider.set_cache_ttl(Duration::from_nanos(1)); // Very short TTL

        // First scan
        let entries1 = provider.get_or_scan_dir(dir.path());
        assert_eq!(entries1.len(), 1);

        // Wait for TTL expiry
        std::thread::sleep(Duration::from_micros(10));

        // Add a new file
        fs::write(dir.path().join("new.txt"), "").unwrap();

        // Second scan should pick up the new file because TTL expired
        let entries2 = provider.get_or_scan_dir(dir.path());
        assert_eq!(entries2.len(), 2);
    }

    #[test]
    fn prefix_filtering() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("foobar.txt"), "").unwrap();
        fs::write(dir.path().join("baz.txt"), "").unwrap();

        let _provider = PathCompletionProvider::new();
        let _ctx = make_ctx(
            &format!("cd {}", dir.path().join("foo").to_string_lossy()),
            0,
        );
        // parse_partial_path at cursor 0 won't find a path token,
        // but we can test the filtering logic directly
        let mut p = PathCompletionProvider::new();
        let entries = p.get_or_scan_dir(dir.path());
        let foo_entries: Vec<_> = entries
            .iter()
            .filter(|e| e.name.to_ascii_lowercase().starts_with("foo"))
            .collect();
        assert_eq!(foo_entries.len(), 1);
        assert_eq!(foo_entries[0].name, "foobar.txt");
    }

    #[test]
    fn directory_first_sorting() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a_file.txt"), "").unwrap();
        fs::create_dir(dir.path().join("a_dir")).unwrap();

        let mut provider = PathCompletionProvider::new();
        let entries = provider.get_or_scan_dir(dir.path());
        assert_eq!(entries[0].name, "a_dir", "directories should come first");
        assert_eq!(
            entries[1].name, "a_file.txt",
            "files should come after directories"
        );
    }
}
