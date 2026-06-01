# Web Search Cache — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an in-memory TTL cache to the web search tool so repeated queries don't make redundant API calls.

**Architecture:** A process-global `LazyLock<Mutex<HashMap>>` cache stores raw search results keyed by `query|max_results|provider_name`. Domain filtering is applied post-cache so different domain filters on the same query share the cached results. TTL defaults to 300s, configurable via `CC_RUST_SEARCH_CACHE_TTL` env var. Expired entries are lazily evicted on lookup.

**Tech Stack:** `std::sync::LazyLock`, `parking_lot::Mutex`, `std::time::Instant`.

---

## File Structure

| Action | File | Responsibility |
|--------|------|---------------|
| Modify | `src/tools/web_search.rs` | Add `SearchCache` + wire into `call()` + remove dead `filter_results` |

---

### Task 1: Add SearchCache struct and global instance

**Files:**
- Modify: `src/tools/web_search.rs`

- [ ] **Step 1: Write tests for the cache**

Add these tests at the bottom of the existing `#[cfg(test)] mod tests` block in `web_search.rs`:

```rust
#[test]
fn test_search_cache_miss_then_hit() {
    let cache = SearchCache::new();
    let key = "rust programming|5|tavily";

    assert!(cache.get(key, 300).is_none(), "should miss on empty cache");

    let entries = vec![SearchResultEntry {
        title: "Rust".into(),
        url: "https://rust-lang.org".into(),
        description: "Systems lang".into(),
        age: None,
    }];
    cache.put(key, entries.clone());

    let hit = cache.get(key, 300);
    assert!(hit.is_some(), "should hit after put");
    assert_eq!(hit.unwrap().len(), 1);
    assert_eq!(hit.unwrap()[0].title, "Rust");
}

#[test]
fn test_search_cache_expired() {
    let cache = SearchCache::new();
    let key = "old query|5|brave";

    let entries = vec![SearchResultEntry {
        title: "Old".into(),
        url: "https://old.com".into(),
        description: "".into(),
        age: None,
    }];
    cache.put(key, entries);

    // TTL of 0 means immediately expired
    assert!(cache.get(key, 0).is_none(), "should miss with 0 TTL");
}

#[test]
fn test_search_cache_different_keys() {
    let cache = SearchCache::new();

    cache.put("q1|5|tavily", vec![SearchResultEntry {
        title: "A".into(),
        url: "https://a.com".into(),
        description: "".into(),
        age: None,
    }]);
    cache.put("q2|5|tavily", vec![SearchResultEntry {
        title: "B".into(),
        url: "https://b.com".into(),
        description: "".into(),
        age: None,
    }]);

    assert_eq!(cache.get("q1|5|tavily", 300).unwrap()[0].title, "A");
    assert_eq!(cache.get("q2|5|tavily", 300).unwrap()[0].title, "B");
    assert!(cache.get("q3|5|tavily", 300).is_none());
}

#[test]
fn test_cache_ttl_from_env() {
    let orig = std::env::var(CACHE_TTL_ENV).ok();

    // Default
    std::env::remove_var(CACHE_TTL_ENV);
    assert_eq!(cache_ttl_secs(), DEFAULT_CACHE_TTL_SECS);

    // Custom
    std::env::set_var(CACHE_TTL_ENV, "120");
    assert_eq!(cache_ttl_secs(), 120);

    // Invalid falls back to default
    std::env::set_var(CACHE_TTL_ENV, "not_a_number");
    assert_eq!(cache_ttl_secs(), DEFAULT_CACHE_TTL_SECS);

    // Zero is valid (disables cache)
    std::env::set_var(CACHE_TTL_ENV, "0");
    assert_eq!(cache_ttl_secs(), 0);

    // Restore
    match orig {
        Some(v) => std::env::set_var(CACHE_TTL_ENV, v),
        None => std::env::remove_var(CACHE_TTL_ENV),
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --bin claude-code-rs tools::web_search::tests -- --nocapture 2>&1 | tail -10`

Expected: FAIL — `SearchCache`, `CACHE_TTL_ENV`, `DEFAULT_CACHE_TTL_SECS`, `cache_ttl_secs` not found.

- [ ] **Step 3: Add `Clone` derive to `SearchResultEntry`**

`SearchResultEntry` currently derives `Debug, Serialize`. The cache needs to clone entries on `get()`. Change the derive line:

From:
```rust
#[derive(Debug, Serialize)]
struct SearchResultEntry {
```

To:
```rust
#[derive(Debug, Clone, Serialize)]
struct SearchResultEntry {
```

- [ ] **Step 4: Add cache constants, TTL helper, and `SearchCache` struct**

Add this block after the existing constants section (after `SEARCH_TIMEOUT` around line 47, before the Brave response types):

```rust
// ---------------------------------------------------------------------------
// Search cache
// ---------------------------------------------------------------------------

/// Environment variable for configuring cache TTL.
const CACHE_TTL_ENV: &str = "CC_RUST_SEARCH_CACHE_TTL";

/// Default cache TTL in seconds (5 minutes).
const DEFAULT_CACHE_TTL_SECS: u64 = 300;

/// Read the cache TTL from environment, falling back to default.
fn cache_ttl_secs() -> u64 {
    std::env::var(CACHE_TTL_ENV)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(DEFAULT_CACHE_TTL_SECS)
}

struct CacheEntry {
    results: Vec<SearchResultEntry>,
    inserted_at: Instant,
}

/// In-memory TTL cache for search results.
///
/// Keyed by `"{query}|{max_results}|{provider}"`. Stores raw results
/// before domain filtering so different domain filters share cache.
struct SearchCache {
    entries: parking_lot::Mutex<HashMap<String, CacheEntry>>,
}

impl SearchCache {
    fn new() -> Self {
        Self {
            entries: parking_lot::Mutex::new(HashMap::new()),
        }
    }

    /// Look up cached results. Returns `None` if missing or expired.
    /// Lazily removes expired entry on miss.
    fn get(&self, key: &str, ttl_secs: u64) -> Option<Vec<SearchResultEntry>> {
        let mut map = self.entries.lock();
        if let Some(entry) = map.get(key) {
            if entry.inserted_at.elapsed().as_secs() < ttl_secs {
                return Some(entry.results.clone());
            }
            // Expired — remove lazily
            map.remove(key);
        }
        None
    }

    /// Store results in the cache.
    fn put(&self, key: &str, results: Vec<SearchResultEntry>) {
        let mut map = self.entries.lock();
        map.insert(
            key.to_string(),
            CacheEntry {
                results,
                inserted_at: Instant::now(),
            },
        );
    }
}

/// Global search cache instance.
static SEARCH_CACHE: std::sync::LazyLock<SearchCache> =
    std::sync::LazyLock::new(SearchCache::new);
```

- [ ] **Step 5: Add `HashMap` import**

At the top of the file, add `use std::collections::HashMap;` to the existing imports. Change:

```rust
use std::time::Instant;
```

To:

```rust
use std::collections::HashMap;
use std::time::Instant;
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test --bin claude-code-rs tools::web_search::tests -- --nocapture`

Expected: All tests PASS including the 4 new cache tests.

- [ ] **Step 7: Commit**

```bash
git add src/tools/web_search.rs
git commit -m "feat: add SearchCache struct for web search result caching

In-memory TTL cache keyed by query|max_results|provider. Default 5min
TTL configurable via CC_RUST_SEARCH_CACHE_TTL env var. Lazy expiry on
lookup. Not yet wired into call()."
```

---

### Task 2: Wire cache into `call()` and remove dead code

**Files:**
- Modify: `src/tools/web_search.rs`

- [ ] **Step 1: Write integration test for cache hit behavior**

Add this test to the existing test module:

```rust
#[test]
fn test_build_cache_key() {
    // Verify key format: different queries, max_results, providers produce different keys
    let k1 = build_cache_key("rust", 5, "tavily");
    let k2 = build_cache_key("rust", 10, "tavily");
    let k3 = build_cache_key("rust", 5, "brave");
    let k4 = build_cache_key("go", 5, "tavily");

    assert_ne!(k1, k2, "different max_results should differ");
    assert_ne!(k1, k3, "different provider should differ");
    assert_ne!(k1, k4, "different query should differ");
    assert_eq!(k1, build_cache_key("rust", 5, "tavily"), "same inputs same key");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bin claude-code-rs tools::web_search::tests::test_build_cache_key -- --nocapture`

Expected: FAIL — `build_cache_key` not found.

- [ ] **Step 3: Add `build_cache_key` helper**

Add this function right after the `SEARCH_CACHE` static (before the `// Search provider abstraction` section):

```rust
/// Build the cache key from query parameters.
fn build_cache_key(query: &str, max_results: u32, provider_name: &str) -> String {
    format!("{}|{}|{}", query, max_results, provider_name)
}
```

- [ ] **Step 4: Wire cache into `call()`**

Modify the `call()` method. The change inserts a cache check after provider detection and returns cached results on hit, or stores fresh results on miss.

Replace the body of `call()` from `let start = Instant::now();` through the final `Ok(ToolResult { ... })` (lines ~414-466) with:

```rust
        let start = Instant::now();

        let provider_name = match &provider {
            SearchProvider::Tavily(_) => "tavily",
            SearchProvider::Brave(_) => "brave",
        };

        let ttl = cache_ttl_secs();
        let cache_key = build_cache_key(query, max_results, provider_name);

        // Check cache first
        let raw_results = if ttl > 0 {
            if let Some(cached) = SEARCH_CACHE.get(&cache_key, ttl) {
                cached
            } else {
                let fetched = self.fetch_results(&provider, query, max_results).await?;
                SEARCH_CACHE.put(&cache_key, fetched.clone());
                fetched
            }
        } else {
            // TTL=0 disables cache
            self.fetch_results(&provider, query, max_results).await?
        };

        let results = filter_results_unified(raw_results, &allowed_domains, &blocked_domains);
        let duration_secs = start.elapsed().as_secs_f64();
        let results_text = format_results_text(&results);

        Ok(ToolResult {
            data: json!({
                "query": query,
                "provider": provider_name,
                "resultCount": results.len(),
                "results": results,
                "formattedResults": results_text,
                "durationSeconds": (duration_secs * 100.0).round() / 100.0,
            }),
            new_messages: vec![],
        })
```

- [ ] **Step 5: Extract `fetch_results` method on `WebSearchTool`**

Add this method to the `WebSearchTool` impl block (before `call()` or after `prompt()`). This extracts the provider dispatch logic that was previously inline in `call()`:

```rust
impl WebSearchTool {
    /// Fetch raw results from the search provider. Errors are returned as
    /// ToolResult (user-visible) rather than propagated, matching the
    /// existing error handling pattern.
    async fn fetch_results(
        &self,
        provider: &SearchProvider,
        query: &str,
        max_results: u32,
    ) -> Result<Vec<SearchResultEntry>> {
        let client = reqwest::Client::builder()
            .timeout(SEARCH_TIMEOUT)
            .build()
            .context("Failed to build HTTP client")?;

        match provider {
            SearchProvider::Tavily(key) => search_tavily(&client, key, query, max_results).await,
            SearchProvider::Brave(key) => search_brave(&client, key, query, max_results).await,
        }
    }
}
```

Then update `call()` to handle fetch errors as ToolResult (keeping existing behavior). Wrap the cache section:

Replace the cache block in `call()` with:

```rust
        // Check cache first
        let raw_results = if ttl > 0 {
            if let Some(cached) = SEARCH_CACHE.get(&cache_key, ttl) {
                cached
            } else {
                match self.fetch_results(&provider, query, max_results).await {
                    Ok(results) => {
                        SEARCH_CACHE.put(&cache_key, results.clone());
                        results
                    }
                    Err(e) => {
                        return Ok(ToolResult {
                            data: json!({
                                "error": format!("{} search failed: {}", provider_name, e),
                                "query": query,
                            }),
                            new_messages: vec![],
                        });
                    }
                }
            }
        } else {
            match self.fetch_results(&provider, query, max_results).await {
                Ok(results) => results,
                Err(e) => {
                    return Ok(ToolResult {
                        data: json!({
                            "error": format!("{} search failed: {}", provider_name, e),
                            "query": query,
                        }),
                        new_messages: vec![],
                    });
                }
            }
        };
```

- [ ] **Step 6: Remove dead `filter_results` function**

Delete the entire `filter_results` function (lines ~249-273, the one with `#[allow(dead_code)]` that takes `Vec<BraveWebResult>`). This eliminates the last build warning.

Also remove the two tests that use it (`test_filter_results_allowed` and `test_filter_results_blocked`) since they test the dead function. The equivalent functionality is covered by `test_filter_results_unified_allowed`.

- [ ] **Step 7: Run all tests**

Run: `cargo test --bin claude-code-rs tools::web_search::tests -- --nocapture`

Expected: All tests PASS.

- [ ] **Step 8: Build release and verify zero warnings**

Run: `cargo build --release 2>&1 | grep warning`

Expected: No warnings at all (the `filter_results` warning was the only one).

- [ ] **Step 9: Commit**

```bash
git add src/tools/web_search.rs
git commit -m "feat: wire search cache into WebSearchTool.call()

Cache check before API call, store on miss. Extract fetch_results()
method. Remove dead filter_results() function (was the only build
warning). TTL=0 disables caching entirely."
```

---

### Task 3: Update SDK tracker

**Files:**
- Modify: `docs/sdk-work-tracker.md`

- [ ] **Step 1: Update P1-3 status**

Change the P1-3 line from:

```markdown
| P1-3 | Web 搜索缓存层 | ❌ | — | Brave API 直连无缓存 |
```

To:

```markdown
| P1-3 | Web 搜索缓存层 | ✅ | — | 进程级内存 TTL 缓存, 默认 5min, CC_RUST_SEARCH_CACHE_TTL 可配置 |
```

Add to the "已完成的对标项" table:

```markdown
| P1-3 Web 搜索缓存层 | 2026-04-10 | SearchCache — query\|max_results\|provider key, 5min default TTL |
```

- [ ] **Step 2: Commit**

```bash
git add docs/sdk-work-tracker.md
git commit -m "docs: update P1-3 web search cache status to completed"
```
