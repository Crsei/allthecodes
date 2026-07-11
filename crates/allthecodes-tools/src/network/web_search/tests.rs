use super::providers::{detect_provider_from_keys, detect_provider_from_settings};
use super::tool::WebSearchTool;
use super::*;

use serde_json::json;

use crate::tool::*;

#[test]
fn test_matches_domain() {
    assert!(matches_domain(
        "https://docs.rust-lang.org/std",
        "rust-lang.org"
    ));
    assert!(matches_domain("https://rust-lang.org/", "rust-lang.org"));
    assert!(!matches_domain(
        "https://evil-rust-lang.org/",
        "rust-lang.org"
    ));
    assert!(!matches_domain("https://example.com/", "rust-lang.org"));
}

#[test]
fn test_format_results_empty() {
    let text = format_results_text(&[]);
    assert_eq!(text, "No results found.");
}

#[test]
fn test_format_results_entries() {
    let results = vec![SearchResultEntry {
        title: "Example".into(),
        url: "https://example.com".into(),
        description: "An example site".into(),
        age: Some("2 days ago".into()),
    }];
    let text = format_results_text(&results);
    assert!(text.contains("**Example**"));
    assert!(text.contains("https://example.com"));
    assert!(text.contains("An example site"));
    assert!(text.contains("2 days ago"));
}

#[tokio::test]
async fn test_web_search_tool_basics() {
    let tool = WebSearchTool;
    assert_eq!(tool.name(), "WebSearch");
    assert!(tool.is_read_only(&json!({})));
    assert!(tool.is_concurrency_safe(&json!({})));

    let schema = tool.input_json_schema();
    assert!(schema["properties"]["query"].is_object());
    assert!(schema["properties"]["max_results"].is_object());
    assert!(schema["properties"]["allowed_domains"].is_object());
}

#[tokio::test]
async fn test_validate_input_short_query() {
    let tool = WebSearchTool;
    let ctx = make_test_ctx();
    let result = tool.validate_input(&json!({"query": "a"}), &ctx).await;
    match result {
        ValidationResult::Error { .. } => {}
        _ => panic!("expected error for short query"),
    }
}

#[tokio::test]
async fn test_validate_input_valid() {
    let tool = WebSearchTool;
    let ctx = make_test_ctx();
    let result = tool
        .validate_input(&json!({"query": "rust programming"}), &ctx)
        .await;
    match result {
        ValidationResult::Ok => {}
        _ => panic!("expected Ok for valid query"),
    }
}

#[test]
fn test_user_facing_name() {
    let tool = WebSearchTool;
    let input = json!({"query": "how to write rust"});
    assert_eq!(
        tool.user_facing_name(Some(&input)),
        "WebSearch(\"how to write rust\")"
    );
}

#[test]
fn test_detect_provider_tavily_first() {
    let provider = detect_provider_from_keys(Some("tvly-test".into()), Some("brave-test".into()));
    assert!(matches!(provider, Some(SearchProvider::Tavily(_))));
}

#[test]
fn test_detect_provider_brave_fallback() {
    let provider = detect_provider_from_keys(None, Some("brave-test".into()));
    assert!(matches!(provider, Some(SearchProvider::Brave(_))));
}

#[test]
fn test_detect_provider_from_settings_honors_explicit_provider() {
    let settings = allthecodes_config::runtime_settings::SettingsJson {
        web_search_provider: Some("brave".into()),
        web_search_tavily_api_key: Some("tvly-test".into()),
        web_search_brave_api_key: Some("brave-test".into()),
        ..Default::default()
    };

    let provider = detect_provider_from_settings(&settings);
    assert!(matches!(provider, Some(SearchProvider::Brave(_))));
}

#[test]
fn test_detect_provider_none() {
    let provider = detect_provider_from_keys(None, None);
    assert!(provider.is_none());
}

#[test]
fn test_filter_results_unified_allowed() {
    let results = vec![
        SearchResultEntry {
            title: "Rust".into(),
            url: "https://rust-lang.org".into(),
            description: "".into(),
            age: None,
        },
        SearchResultEntry {
            title: "Go".into(),
            url: "https://go.dev".into(),
            description: "".into(),
            age: None,
        },
    ];
    let filtered = filter_results_unified(results, &["rust-lang.org".to_string()], &[]);
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].title, "Rust");
}

#[test]
fn test_tavily_response_deserialization() {
    let json_str = r#"{
        "results": [
            {
                "title": "Rust Programming",
                "url": "https://rust-lang.org",
                "content": "A systems programming language",
                "score": 0.95,
                "published_date": "2024-01-15"
            }
        ]
    }"#;
    let resp: TavilySearchResponse = serde_json::from_str(json_str).unwrap();
    assert_eq!(resp.results.len(), 1);
    assert_eq!(resp.results[0].title, "Rust Programming");
    assert_eq!(resp.results[0].content, "A systems programming language");
    assert_eq!(
        resp.results[0].published_date.as_deref(),
        Some("2024-01-15")
    );
}

fn make_test_ctx() -> ToolUseContext {
    use crate::tool::ToolAppState as AppState;
    use std::sync::Arc;
    let state = AppState::default();
    ToolUseContext {
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
    }
}

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

    cache.put(
        "q1|5|tavily",
        vec![SearchResultEntry {
            title: "A".into(),
            url: "https://a.com".into(),
            description: "".into(),
            age: None,
        }],
    );
    cache.put(
        "q2|5|tavily",
        vec![SearchResultEntry {
            title: "B".into(),
            url: "https://b.com".into(),
            description: "".into(),
            age: None,
        }],
    );

    assert_eq!(cache.get("q1|5|tavily", 300).unwrap()[0].title, "A");
    assert_eq!(cache.get("q2|5|tavily", 300).unwrap()[0].title, "B");
    assert!(cache.get("q3|5|tavily", 300).is_none());
}

#[test]
fn test_build_cache_key() {
    let k1 = build_cache_key("rust", 5, "tavily");
    let k2 = build_cache_key("rust", 10, "tavily");
    let k3 = build_cache_key("rust", 5, "brave");
    let k4 = build_cache_key("go", 5, "tavily");

    assert_ne!(k1, k2, "different max_results should differ");
    assert_ne!(k1, k3, "different provider should differ");
    assert_ne!(k1, k4, "different query should differ");
    assert_eq!(
        k1,
        build_cache_key("rust", 5, "tavily"),
        "same inputs same key"
    );
}

#[test]
fn test_search_cache_evicts_oldest_at_capacity() {
    let cache = SearchCache::new();

    // Fill to MAX_CACHE_ENTRIES
    for i in 0..MAX_CACHE_ENTRIES {
        cache.put(
            &format!("query{}|5|tavily", i),
            vec![SearchResultEntry {
                title: format!("Result {}", i),
                url: format!("https://example.com/{}", i),
                description: "".into(),
                age: None,
            }],
        );
    }

    // First entry should still exist
    assert!(cache.get("query0|5|tavily", 300).is_some());

    // Add one more — should evict the oldest (query0)
    cache.put(
        "overflow|5|tavily",
        vec![SearchResultEntry {
            title: "Overflow".into(),
            url: "https://overflow.com".into(),
            description: "".into(),
            age: None,
        }],
    );

    // query0 should be evicted, overflow should exist
    assert!(
        cache.get("query0|5|tavily", 300).is_none(),
        "oldest should be evicted"
    );
    assert!(
        cache.get("overflow|5|tavily", 300).is_some(),
        "new entry should exist"
    );
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
