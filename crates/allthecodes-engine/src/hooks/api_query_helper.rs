//! API query hook helper — framework for running side-channel LLM queries.
//!
//! Port of TypeScript `apiQueryHookHelper.ts`.
//!
//! NOTE: This is a structural stub. Full implementation requires integration
//! with the query engine (cc-engine's query loop) and the API client
//! (cc-api). The TypeScript version uses `queryModelWithoutStreaming()`
//! to make non-interactive LLM calls.
//!
//! The trait and types are provided here so they can be wired into the
//! post-sampling hook pipeline. The actual execution will be implemented
//! when the query engine is fully extracted into cc-engine.

use serde_json::Value;

use super::post_sampling::ReplHookContext;

/// Context for API query hooks, extending the REPL hook context.
#[derive(Debug, Clone)]
pub struct ApiQueryHookContext {
    pub repl: ReplHookContext,
    pub query_message_count: Option<usize>,
}

impl ApiQueryHookContext {
    pub fn new(repl: ReplHookContext) -> Self {
        Self {
            repl,
            query_message_count: None,
        }
    }
}

/// Result of an API query hook.
#[derive(Debug, Clone)]
pub struct ApiQueryResult<TResult> {
    pub query_name: String,
    pub result: TResult,
    pub message_id: String,
    pub model: String,
    pub uuid: String,
}

/// Configuration for an API query hook.
pub struct ApiQueryHookConfig<TResult> {
    pub name: String,
    pub should_run: Box<dyn Fn(&ApiQueryHookContext) -> bool + Send + Sync>,
    pub build_messages: Box<dyn Fn(&ApiQueryHookContext) -> Vec<Value> + Send + Sync>,
    pub system_prompt: Option<String>,
    pub use_tools: bool,
    pub parse_response: Box<dyn Fn(&str) -> TResult + Send + Sync>,
    pub log_result: Box<dyn Fn(&ApiQueryResult<TResult>, &ApiQueryHookContext) + Send + Sync>,
    pub get_model: Box<dyn Fn() -> String + Send + Sync>,
}

/// Create an API query hook closure that can be registered as a post-sampling hook.
///
/// The returned closure implements the `PostSamplingHook` signature but wraps
/// the API query logic. When the query engine integration is complete, this
/// will make actual LLM calls.
pub fn create_api_query_hook<TResult: 'static>(
    _config: ApiQueryHookConfig<TResult>,
) -> Box<dyn Fn(&ReplHookContext) + Send + Sync> {
    // Structural stub: posts to a channel/SPSC for async execution
    Box::new(move |_ctx: &ReplHookContext| {
        // TODO: Implement full API query when cc-engine has access to
        // cc-api's streaming/non-streaming query functions.
        // The TypeScript version does:
        //   1. config.shouldRun(ctx) — quick check
        //   2. config.buildMessages(ctx) — build message array
        //   3. queryModelWithoutStreaming({ messages, systemPrompt, ... })
        //   4. config.parseResponse(content) — parse result
        //   5. config.logResult(result, ctx) — log/update state
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_creation() {
        let repl = ReplHookContext {
            messages: vec![],
            system_prompt: "test".into(),
            user_context: std::collections::HashMap::new(),
            system_context: std::collections::HashMap::new(),
            query_source: None,
        };
        let ctx = ApiQueryHookContext::new(repl);
        assert_eq!(ctx.repl.system_prompt, "test");
        assert!(ctx.query_message_count.is_none());
    }
}
