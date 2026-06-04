//! Post-sampling hook registry.
//!
//! Post-sampling hooks are called after the model generates a response,
//! before the result is processed by the query loop. They allow for
//! side-channel analysis (e.g., skill improvement detection).
//!
//! Port of TypeScript `postSamplingHooks.ts`.

/// Context passed to post-sampling hooks.
#[derive(Debug, Clone)]
pub struct ReplHookContext {
    pub messages: Vec<serde_json::Value>,
    pub system_prompt: String,
    pub user_context: std::collections::HashMap<String, String>,
    pub system_context: std::collections::HashMap<String, String>,
    pub query_source: Option<String>,
}

/// A post-sampling hook is called after model sampling completes.
pub type PostSamplingHook = Box<dyn Fn(&ReplHookContext) + Send + Sync>;

use std::sync::{LazyLock, Mutex};

/// Global registry of post-sampling hooks.
static POST_SAMPLING_HOOKS: LazyLock<Mutex<Vec<PostSamplingHook>>> =
    LazyLock::new(|| Mutex::new(Vec::new()));

/// Register a post-sampling hook.
pub fn register_post_sampling_hook(hook: PostSamplingHook) {
    POST_SAMPLING_HOOKS.lock().expect("POST_SAMPLING_HOOKS lock poisoned").push(hook);
}

/// Clear all registered post-sampling hooks (for testing).
pub fn clear_post_sampling_hooks() {
    POST_SAMPLING_HOOKS.lock().expect("POST_SAMPLING_HOOKS lock poisoned").clear();
}

/// Execute all registered post-sampling hooks.
pub fn execute_post_sampling_hooks(context: &ReplHookContext) {
    let hooks = POST_SAMPLING_HOOKS.lock().expect("POST_SAMPLING_HOOKS lock poisoned");
    for hook in hooks.iter() {
        hook(context);
    }
}

/// Number of registered post-sampling hooks.
pub fn post_sampling_hook_count() -> usize {
    POST_SAMPLING_HOOKS.lock().expect("POST_SAMPLING_HOOKS lock poisoned").len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_and_clear() {
        clear_post_sampling_hooks();
        assert_eq!(post_sampling_hook_count(), 0);

        register_post_sampling_hook(Box::new(|_ctx| {}));
        assert_eq!(post_sampling_hook_count(), 1);

        clear_post_sampling_hooks();
        assert_eq!(post_sampling_hook_count(), 0);
    }

    #[test]
    fn test_execute_hooks() {
        clear_post_sampling_hooks();

        let called = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let called_clone = called.clone();

        register_post_sampling_hook(Box::new(move |_ctx| {
            called_clone.store(true, std::sync::atomic::Ordering::Relaxed);
        }));

        let ctx = ReplHookContext {
            messages: vec![],
            system_prompt: String::new(),
            user_context: std::collections::HashMap::new(),
            system_context: std::collections::HashMap::new(),
            query_source: None,
        };

        execute_post_sampling_hooks(&ctx);

        assert!(called.load(std::sync::atomic::Ordering::Relaxed));
    }
}
