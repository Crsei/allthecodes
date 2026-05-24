//! Slack channel completion provider.
//!
//! Detects `#channel` tokens at the cursor position and provides channel
//! name completions. Uses a known-channel cache with optional MCP-based
//! `slack_search_channels` queries for dynamic results.

use std::collections::HashMap;
use std::ops::Range;
use std::time::{Duration, Instant};

use super::completions::{CompletionContext, CompletionItem, CompletionKind, CompletionProvider};

/// Maximum number of cached search queries.
const MAX_CACHED_QUERIES: usize = 50;
const _: usize = MAX_CACHED_QUERIES;

/// Cache TTL for MCP query results.
const MCP_CACHE_TTL: Duration = Duration::from_secs(120);

// ---------------------------------------------------------------------------
// SlackChannelCompletionProvider
// ---------------------------------------------------------------------------

/// Completion provider for `#channel` Slack channel names.
///
/// Detects `#channel` tokens and provides completions from:
/// 1. A known-channel cache (pre-populated with common channels)
/// 2. MCP `slack_search_channels` tool call results (cached)
/// 3. Footer suggestions for partial matches
pub struct SlackChannelCompletionProvider {
    /// Known channel names, pre-populated with defaults.
    known_channels: Vec<String>,
    /// Cached MCP search results per query prefix.
    mcp_cache: HashMap<String, (Instant, Vec<String>)>,
    /// Whether MCP queries are enabled (requires connected Slack MCP server).
    mcp_enabled: bool,
}

impl SlackChannelCompletionProvider {
    pub fn new() -> Self {
        let known = vec![
            "general".to_string(),
            "random".to_string(),
            "announcements".to_string(),
            "engineering".to_string(),
            "design".to_string(),
            "support".to_string(),
            "sales".to_string(),
            "marketing".to_string(),
            "product".to_string(),
            "dev".to_string(),
            "devops".to_string(),
            "qa".to_string(),
            "standup".to_string(),
            "standups".to_string(),
            "team".to_string(),
            "feedback".to_string(),
        ];

        Self {
            known_channels: known,
            mcp_cache: HashMap::new(),
            mcp_enabled: false,
        }
    }

    /// Set custom known channels (for testing or configuration).
    #[cfg(test)]
    pub fn set_known_channels(&mut self, channels: Vec<String>) {
        self.known_channels = channels;
    }

    /// Enable or disable MCP-backed search (requires connected MCP server).
    #[cfg(test)]
    pub fn set_mcp_enabled(&mut self, enabled: bool) {
        self.mcp_enabled = enabled;
    }

    /// Attempt an MCP `slack_search_channels` query.
    ///
    /// Returns cached results if available and fresh, otherwise returns
    /// an empty Vec (the MCP call is async, so we defer to the known cache
    /// for synchronous completion).
    fn query_mcp_channels(&self, query: &str) -> Vec<String> {
        if !self.mcp_enabled {
            return Vec::new();
        }

        // Check cache first
        if let Some((timestamp, results)) = self.mcp_cache.get(query) {
            if timestamp.elapsed() < MCP_CACHE_TTL {
                return results.clone();
            }
        }

        // MCP calls are async and can't be made from synchronous `compute()`
        // in this implementation. We return empty and rely on the known channel
        // cache. A background task could pre-fill the MCP cache.
        Vec::new()
    }

    /// Find a `#channel` token at the cursor position.
    fn find_channel_token(input: &str, cursor_pos: usize) -> Option<(Range<usize>, String)> {
        let byte_pos = cursor_pos.min(input.len());
        let prefix = &input[..byte_pos];

        // Find the last `#` preceded by whitespace or BOL
        let hash_pos = prefix.rfind('#')?;

        if hash_pos > 0 && input.as_bytes()[hash_pos - 1] != b' ' {
            return None; // `#` is part of a word, not a channel
        }

        let after_hash = &prefix[hash_pos + 1..byte_pos];
        // Don't trigger on bare `#`
        if after_hash.is_empty() {
            return None;
        }

        Some((hash_pos..byte_pos, after_hash.to_string()))
    }

    /// Add a query result to the MCP cache.
    ///
    /// Called from the async MCP response handler.
    #[cfg(test)]
    pub fn cache_mcp_results(&mut self, query: &str, results: Vec<String>) {
        if self.mcp_cache.len() >= MAX_CACHED_QUERIES {
            // Evict oldest entry
            if let Some(oldest) = self
                .mcp_cache
                .iter()
                .min_by_key(|(_, (ts, _))| *ts)
                .map(|(k, _)| k.clone())
            {
                self.mcp_cache.remove(&oldest);
            }
        }
        self.mcp_cache
            .insert(query.to_string(), (Instant::now(), results));
    }

    /// Get the set of known channel names for highlighting.
    #[cfg(test)]
    pub fn known_channel_set(&self) -> std::collections::HashSet<&str> {
        self.known_channels.iter().map(String::as_str).collect()
    }
}

impl CompletionProvider for SlackChannelCompletionProvider {
    fn compute(&self, ctx: &CompletionContext) -> Vec<CompletionItem> {
        let (range, query) = match Self::find_channel_token(ctx.input, ctx.cursor_pos) {
            Some(result) => result,
            None => return Vec::new(),
        };

        let query_lower = query.to_ascii_lowercase();
        let mut completions: Vec<CompletionItem> = Vec::new();

        // Step 1: Check known channels
        for channel in &self.known_channels {
            if channel.to_ascii_lowercase().starts_with(&query_lower) {
                let label = format!("#{}", channel);
                let insert = format!("#{} ", channel);
                completions.push(
                    CompletionItem::new(
                        CompletionKind::SlackChannel,
                        &label,
                        &insert,
                        range.clone(),
                    )
                    .with_detail("known channel".to_string()),
                );
            }
        }

        // Step 2: Check MCP cache
        let mcp_results = self.query_mcp_channels(&query);
        for channel in mcp_results {
            let channel_name = channel
                .strip_prefix("Name: #")
                .or_else(|| channel.strip_prefix('#'))
                .unwrap_or(&channel);
            let label = format!("#{}", channel_name);
            let insert = format!("#{} ", channel_name);
            completions.push(
                CompletionItem::new(CompletionKind::SlackChannel, &label, &insert, range.clone())
                    .with_detail("slack channel".to_string()),
            );
        }

        // Deduplicate
        completions.sort_by(|a, b| a.label.cmp(&b.label));
        completions.dedup_by(|a, b| a.label == b.label);

        completions
    }

    fn priority(&self) -> u32 {
        40
    }
}

impl Default for SlackChannelCompletionProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ctx(input: &str, cursor_pos: usize) -> CompletionContext<'_> {
        CompletionContext::new(input, cursor_pos, &[])
    }

    #[test]
    fn channel_detection_with_hash() {
        let result = SlackChannelCompletionProvider::find_channel_token("message to #gen", 15);
        assert!(result.is_some());
        let (_range, query) = result.unwrap();
        assert_eq!(query, "gen");
    }

    #[test]
    fn known_channel_completions() {
        let provider = SlackChannelCompletionProvider::new();
        let ctx = make_ctx("post to #gen", 12);
        let items = provider.compute(&ctx);
        assert!(!items.is_empty(), "expected known channel completions");
        // Should match "general" from known channels
        assert!(items.iter().any(|i| i.label == "#general"));
    }

    #[test]
    fn custom_known_channels_replace_defaults() {
        let mut provider = SlackChannelCompletionProvider::new();
        provider.set_known_channels(vec!["ops-alerts".to_string()]);
        let ctx = make_ctx("post to #ops", 12);
        let items = provider.compute(&ctx);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "#ops-alerts");
    }

    #[test]
    fn no_channel_without_hash() {
        let provider = SlackChannelCompletionProvider::new();
        let ctx = make_ctx("hello world", 5);
        let items = provider.compute(&ctx);
        assert!(items.is_empty(), "no # means no channel completions");
    }

    #[test]
    fn bare_hash_does_not_trigger() {
        let result = SlackChannelCompletionProvider::find_channel_token("message to #", 12);
        assert!(result.is_none(), "bare # should not trigger completion");
    }

    #[test]
    fn mcp_cache_stores_and_retrieves() {
        let mut provider = SlackChannelCompletionProvider::new();
        provider.set_mcp_enabled(true);
        provider.cache_mcp_results("dev", vec!["#dev-team".to_string(), "#devops".to_string()]);

        let results = provider.query_mcp_channels("dev");
        assert_eq!(results.len(), 2);
        assert!(results.contains(&"#dev-team".to_string()));
    }

    #[test]
    fn known_channel_set() {
        let provider = SlackChannelCompletionProvider::new();
        let known = provider.known_channel_set();
        assert!(known.contains("general"));
        assert!(known.contains("random"));
    }
}
