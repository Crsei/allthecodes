//! WebSearchTool implementation.

use std::time::Instant;

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::tool::*;
use allthecodes_types::message::AssistantMessage;

use super::providers::{detect_provider_from_settings, search_brave, search_tavily};
use super::{
    build_cache_key, cache_ttl_secs, filter_results_unified, format_results_text, SearchProvider,
    SearchResultEntry, DEFAULT_MAX_RESULTS, MAX_QUERY_LENGTH, MAX_RESULTS_CAP, SEARCH_CACHE,
    SEARCH_TIMEOUT,
};

// ---------------------------------------------------------------------------
// Tool implementation
// ---------------------------------------------------------------------------

pub struct WebSearchTool;

impl WebSearchTool {
    /// Fetch raw results from the search provider.
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

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "WebSearch"
    }

    async fn description(&self, _: &Value) -> String {
        "Search the web for current information.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The search query (2-400 characters)."
                },
                "max_results": {
                    "type": "number",
                    "description": "Maximum number of results to return (default 5, max 20).",
                    "default": DEFAULT_MAX_RESULTS
                },
                "allowed_domains": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Only return results from these domains."
                },
                "blocked_domains": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Exclude results from these domains."
                }
            },
            "required": ["query"]
        })
    }

    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }

    fn is_read_only(&self, _: &Value) -> bool {
        true
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        let query = input.get("query").and_then(|v| v.as_str()).unwrap_or("");
        if query.len() < 2 {
            return ValidationResult::Error {
                message: "Search query must be at least 2 characters".to_string(),
                error_code: 400,
            };
        }
        if query.len() > MAX_QUERY_LENGTH {
            return ValidationResult::Error {
                message: format!("Search query exceeds {} character limit", MAX_QUERY_LENGTH),
                error_code: 400,
            };
        }
        ValidationResult::Ok
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let query = input.get("query").and_then(|v| v.as_str()).unwrap_or("");

        let max_results = input
            .get("max_results")
            .and_then(|v| v.as_u64())
            .map(|n| (n as u32).min(MAX_RESULTS_CAP))
            .unwrap_or(DEFAULT_MAX_RESULTS);

        let allowed_domains: Vec<String> = input
            .get("allowed_domains")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let blocked_domains: Vec<String> = input
            .get("blocked_domains")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let app_state = (ctx.get_app_state)();
        let provider = match detect_provider_from_settings(&app_state.settings) {
            Some(p) => p,
            None => {
                return Ok(ToolResult {
                    data: json!({
                        "error": "WebSearch requires webSearchProvider and the matching API key in allthecodes settings.",
                    }),
                    new_messages: vec![],
                    ..Default::default()
                });
            }
        };

        let start = Instant::now();

        let provider_name = match &provider {
            SearchProvider::Tavily(_) => "tavily",
            SearchProvider::Brave(_) => "brave",
        };

        let ttl = cache_ttl_secs();
        let cache_key = build_cache_key(query, max_results, provider_name);

        // Check cache first, fetch on miss
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
                            ..Default::default()
                        });
                    }
                }
            }
        } else {
            // TTL=0 disables cache
            match self.fetch_results(&provider, query, max_results).await {
                Ok(results) => results,
                Err(e) => {
                    return Ok(ToolResult {
                        data: json!({
                            "error": format!("{} search failed: {}", provider_name, e),
                            "query": query,
                        }),
                        new_messages: vec![],
                        ..Default::default()
                    });
                }
            }
        };

        let results = filter_results_unified(raw_results, &allowed_domains, &blocked_domains);
        let duration_secs = start.elapsed().as_secs_f64();
        let results_text = format_results_text(&results);
        let taint = allthecodes_types::security::TaintContext::from_marks([
            allthecodes_types::security::TaintMark::from_content(
                allthecodes_types::security::UntrustedSourceKind::WebContent,
                format!("web-search:{provider_name}"),
                results_text.as_bytes(),
            ),
        ]);

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
            taint,
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        let now = chrono::Utc::now();
        let month_year = now.format("%B %Y");
        format!(
            r#"Search the web for current information. The current date is {month_year}.
Search provider: configured in allthecodes settings.

When presenting search results, you MUST include a "Sources:" section at the end
with markdown hyperlinks to the sources used.

Parameters:
- query: The search query (required, 2-400 chars)
- max_results: Number of results (default 5, max 20)
- allowed_domains: Only return results from these domains
- blocked_domains: Exclude results from these domains

Example sources section:
Sources:
- [Title](https://example.com/page)"#
        )
    }

    fn user_facing_name(&self, input: Option<&Value>) -> String {
        if let Some(q) = input.and_then(|v| v.get("query")).and_then(|v| v.as_str()) {
            let short = if q.len() > 30 { &q[..30] } else { q };
            format!("WebSearch(\"{}\")", short)
        } else {
            "WebSearch".to_string()
        }
    }
}

// ---------------------------------------------------------------------------
// Inline unit tests (logic that lives in this file)
// The broader web_search integration tests are in tests.rs.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // -----------------------------------------------------------------------
    // user_facing_name
    // -----------------------------------------------------------------------

    #[test]
    fn user_facing_name_short_query_unchanged() {
        let tool = WebSearchTool;
        let input = json!({"query": "rust"});
        assert_eq!(tool.user_facing_name(Some(&input)), "WebSearch(\"rust\")");
    }

    #[test]
    fn user_facing_name_truncates_at_30_chars() {
        let tool = WebSearchTool;
        // 31-character query — should be truncated to exactly 30 chars
        let query = "a".repeat(31);
        let input = json!({"query": query});
        let name = tool.user_facing_name(Some(&input));
        // Expected: WebSearch("<30 'a' chars>")
        let expected = format!("WebSearch(\"{}\")", "a".repeat(30));
        assert_eq!(name, expected);
    }

    #[test]
    fn user_facing_name_exactly_30_chars_not_truncated() {
        let tool = WebSearchTool;
        let query = "b".repeat(30);
        let input = json!({"query": query});
        let name = tool.user_facing_name(Some(&input));
        let expected = format!("WebSearch(\"{}\")", "b".repeat(30));
        assert_eq!(name, expected);
    }

    #[test]
    fn user_facing_name_no_input_returns_bare_name() {
        let tool = WebSearchTool;
        assert_eq!(tool.user_facing_name(None), "WebSearch");
    }

    #[test]
    fn user_facing_name_input_without_query_field() {
        let tool = WebSearchTool;
        let input = json!({"max_results": 5});
        assert_eq!(tool.user_facing_name(Some(&input)), "WebSearch");
    }

    // -----------------------------------------------------------------------
    // input_json_schema
    // -----------------------------------------------------------------------

    #[test]
    fn schema_required_contains_query() {
        let tool = WebSearchTool;
        let schema = tool.input_json_schema();
        let required = schema["required"]
            .as_array()
            .expect("required should be array");
        let names: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();
        assert!(
            names.contains(&"query"),
            "schema required should include 'query'"
        );
    }

    #[test]
    fn schema_has_blocked_domains_property() {
        let tool = WebSearchTool;
        let schema = tool.input_json_schema();
        assert!(
            schema["properties"]["blocked_domains"].is_object(),
            "schema should have blocked_domains property"
        );
    }

    #[test]
    fn schema_max_results_has_default() {
        let tool = WebSearchTool;
        let schema = tool.input_json_schema();
        let default_val = schema["properties"]["max_results"]["default"]
            .as_u64()
            .expect("max_results should have numeric default");
        assert_eq!(default_val, DEFAULT_MAX_RESULTS as u64);
    }

    #[test]
    fn schema_type_is_object() {
        let tool = WebSearchTool;
        let schema = tool.input_json_schema();
        assert_eq!(schema["type"].as_str().unwrap_or(""), "object");
    }

    // -----------------------------------------------------------------------
    // validate_input boundary conditions
    // -----------------------------------------------------------------------

    fn make_test_ctx() -> crate::tool::ToolUseContext {
        use crate::tool::ToolAppState as AppState;
        use crate::tool::{FileStateCache, ToolUseOptions};
        use std::sync::Arc;
        let state = AppState::default();
        crate::tool::ToolUseContext {
            cwd: ".".to_string(),
            options: ToolUseOptions {
                debug: false,
                main_loop_model: "test".into(),
                verbose: false,
                is_non_interactive_session: true,
                custom_system_prompt: None,
                append_system_prompt: None,
                max_budget_usd: None,
            },
            abort_signal: tokio::sync::watch::channel(false).1,
            read_file_state: FileStateCache::default(),
            get_app_state: Arc::new(move || state.clone()),
            set_app_state: Arc::new(|_| {}),
            session_id: "test-session".to_string(),
            langfuse_session_id: "test-session".to_string(),
            messages: vec![],
            agent_id: None,
            agent_type: None,
            query_tracking: None,
            permission_callback: None,
            ask_user_callback: None,
            permission_event_callback: None,
            bg_agent_tx: None,
            hook_runner: Arc::new(allthecodes_types::hooks::NoopHookRunner::new()),
            command_dispatcher: Arc::new(allthecodes_types::commands::NoopCommandDispatcher::new()),
            available_tools: vec![],
            execute_deferred_tool: None,
            taint_context: Default::default(),
        }
    }

    #[tokio::test]
    async fn validate_input_empty_query_is_error() {
        let tool = WebSearchTool;
        let ctx = make_test_ctx();
        let result = tool.validate_input(&json!({"query": ""}), &ctx).await;
        assert!(
            matches!(result, ValidationResult::Error { .. }),
            "empty query should fail"
        );
    }

    #[tokio::test]
    async fn validate_input_one_char_query_is_error() {
        let tool = WebSearchTool;
        let ctx = make_test_ctx();
        let result = tool.validate_input(&json!({"query": "x"}), &ctx).await;
        assert!(
            matches!(result, ValidationResult::Error { .. }),
            "single-char query should fail"
        );
    }

    #[tokio::test]
    async fn validate_input_two_char_query_is_ok() {
        let tool = WebSearchTool;
        let ctx = make_test_ctx();
        // Minimum valid length is exactly 2
        let result = tool.validate_input(&json!({"query": "ab"}), &ctx).await;
        assert!(
            matches!(result, ValidationResult::Ok),
            "2-char query should pass"
        );
    }

    #[tokio::test]
    async fn validate_input_max_length_query_is_ok() {
        let tool = WebSearchTool;
        let ctx = make_test_ctx();
        let query = "q".repeat(MAX_QUERY_LENGTH);
        let result = tool.validate_input(&json!({"query": query}), &ctx).await;
        assert!(
            matches!(result, ValidationResult::Ok),
            "query at MAX_QUERY_LENGTH chars should pass"
        );
    }

    #[tokio::test]
    async fn validate_input_over_max_length_is_error() {
        let tool = WebSearchTool;
        let ctx = make_test_ctx();
        let query = "q".repeat(MAX_QUERY_LENGTH + 1);
        let result = tool.validate_input(&json!({"query": query}), &ctx).await;
        assert!(
            matches!(result, ValidationResult::Error { .. }),
            "query over MAX_QUERY_LENGTH should fail"
        );
    }

    #[tokio::test]
    async fn validate_input_missing_query_field_is_error() {
        let tool = WebSearchTool;
        let ctx = make_test_ctx();
        // No "query" field — defaults to "" which is < 2 chars
        let result = tool.validate_input(&json!({}), &ctx).await;
        assert!(
            matches!(result, ValidationResult::Error { .. }),
            "missing query should fail"
        );
    }

    // -----------------------------------------------------------------------
    // Flags
    // -----------------------------------------------------------------------

    #[test]
    fn tool_is_read_only_and_concurrency_safe() {
        let tool = WebSearchTool;
        assert!(tool.is_read_only(&json!({})));
        assert!(tool.is_concurrency_safe(&json!({})));
    }

    #[test]
    fn tool_name_is_web_search() {
        let tool = WebSearchTool;
        assert_eq!(tool.name(), "WebSearch");
    }
}
