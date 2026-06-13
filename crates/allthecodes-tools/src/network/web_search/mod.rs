//! WebSearch tool — search the web via Tavily or Brave Search API.
//!
//! Corresponds to TypeScript: tools/WebSearchTool/WebSearchTool.ts
//!
//! Supports two providers configured through allthecodes settings:
//!   1. Tavily Search API
//!   2. Brave Search API
//!
//! At least one provider API key must be saved in settings.

mod providers;
mod tool;

#[cfg(test)]
mod tests;

pub use tool::WebSearchTool;

use std::collections::HashMap;
use std::time::Instant;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Tavily Search API endpoint.
const TAVILY_API_URL: &str = "https://api.tavily.com/search";

/// Brave Search API endpoint.
const BRAVE_API_URL: &str = "https://api.search.brave.com/res/v1/web/search";

/// Maximum number of results to request.
const DEFAULT_MAX_RESULTS: u32 = 5;

/// Absolute cap on results per query.
const MAX_RESULTS_CAP: u32 = 20;

/// Maximum search query length.
const MAX_QUERY_LENGTH: usize = 400;

/// Request timeout.
const SEARCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

// ---------------------------------------------------------------------------
// Search cache
// ---------------------------------------------------------------------------

/// Environment variable for configuring cache TTL.
const CACHE_TTL_ENV: &str = "ALLTHECODES_SEARCH_CACHE_TTL";

/// Default cache TTL in seconds (5 minutes).
const DEFAULT_CACHE_TTL_SECS: u64 = 300;

/// Maximum number of cached search results.
const MAX_CACHE_ENTRIES: usize = 128;

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
        // Evict oldest entry if at capacity
        if map.len() >= MAX_CACHE_ENTRIES && !map.contains_key(key) {
            if let Some(oldest_key) = map
                .iter()
                .min_by_key(|(_, e)| e.inserted_at)
                .map(|(k, _)| k.clone())
            {
                map.remove(&oldest_key);
            }
        }
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
static SEARCH_CACHE: std::sync::LazyLock<SearchCache> = std::sync::LazyLock::new(SearchCache::new);

/// Build the cache key from query parameters.
fn build_cache_key(query: &str, max_results: u32, provider_name: &str) -> String {
    format!("{}|{}|{}", query, max_results, provider_name)
}

// ---------------------------------------------------------------------------
// Brave Search response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct BraveSearchResponse {
    #[serde(default)]
    web: Option<BraveWebResults>,
}

#[derive(Debug, Deserialize)]
struct BraveWebResults {
    #[serde(default)]
    results: Vec<BraveWebResult>,
}

#[derive(Debug, Deserialize)]
struct BraveWebResult {
    title: String,
    url: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    age: Option<String>,
}

// ---------------------------------------------------------------------------
// Tavily Search response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct TavilySearchResponse {
    #[serde(default)]
    results: Vec<TavilySearchResult>,
}

#[derive(Debug, Deserialize)]
struct TavilySearchResult {
    title: String,
    url: String,
    #[serde(default)]
    content: String,
    #[serde(default)]
    published_date: Option<String>,
}

// ---------------------------------------------------------------------------
// Search provider abstraction
// ---------------------------------------------------------------------------

/// Which search provider to use.
enum SearchProvider {
    Tavily(String),
    Brave(String),
}

// ---------------------------------------------------------------------------
// Result formatting
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
struct SearchResultEntry {
    title: String,
    url: String,
    description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    age: Option<String>,
}

fn format_results_text(results: &[SearchResultEntry]) -> String {
    if results.is_empty() {
        return "No results found.".to_string();
    }
    let mut lines = Vec::new();
    for (i, r) in results.iter().enumerate() {
        lines.push(format!("{}. **{}**", i + 1, r.title));
        lines.push(format!("   {}", r.url));
        if !r.description.is_empty() {
            lines.push(format!("   {}", r.description));
        }
        if let Some(age) = &r.age {
            lines.push(format!("   ({})", age));
        }
        lines.push(String::new());
    }
    lines.join("\n")
}

// ---------------------------------------------------------------------------
// Domain filtering
// ---------------------------------------------------------------------------

fn matches_domain(url: &str, domain: &str) -> bool {
    if let Ok(parsed) = url::Url::parse(url) {
        if let Some(host) = parsed.host_str() {
            return host == domain || host.ends_with(&format!(".{}", domain));
        }
    }
    false
}

/// Filter unified SearchResultEntry results by domain rules.
fn filter_results_unified(
    results: Vec<SearchResultEntry>,
    allowed_domains: &[String],
    blocked_domains: &[String],
) -> Vec<SearchResultEntry> {
    results
        .into_iter()
        .filter(|r| {
            if !allowed_domains.is_empty() {
                return allowed_domains.iter().any(|d| matches_domain(&r.url, d));
            }
            if !blocked_domains.is_empty() {
                return !blocked_domains.iter().any(|d| matches_domain(&r.url, d));
            }
            true
        })
        .collect()
}
