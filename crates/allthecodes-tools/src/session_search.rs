use std::path::PathBuf;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};

use allthecodes_session::storage::{
    load_session_info, search_sessions, search_workspace_sessions, SessionSearchResult,
};
use allthecodes_types::message::{AssistantMessage, ToolResultContent};

use crate::tool::{Tool, ToolProgress, ToolResult, ToolUseContext, ValidationResult};

const DEFAULT_LIMIT: usize = 20;
const MAX_LIMIT: usize = 20;

pub struct SessionSearchTool;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchScope {
    Workspace,
    All,
}

impl SearchScope {
    fn as_str(self) -> &'static str {
        match self {
            Self::Workspace => "workspace",
            Self::All => "all",
        }
    }

    fn parse(raw: Option<&str>) -> Result<Self> {
        match raw
            .unwrap_or("workspace")
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "" | "workspace" => Ok(Self::Workspace),
            "all" => Ok(Self::All),
            _ => Err(anyhow!("scope must be 'workspace' or 'all'")),
        }
    }
}

#[derive(Debug, Clone)]
struct SessionSearchInput {
    query: String,
    limit: usize,
    scope: SearchScope,
}

fn parse_input(input: &Value) -> Result<SessionSearchInput> {
    let query = input
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    if query.is_empty() {
        return Err(anyhow!("query is required"));
    }

    let limit = input
        .get("limit")
        .and_then(Value::as_i64)
        .unwrap_or(DEFAULT_LIMIT as i64)
        .clamp(1, MAX_LIMIT as i64) as usize;
    let scope = SearchScope::parse(input.get("scope").and_then(Value::as_str))?;

    Ok(SessionSearchInput {
        query,
        limit,
        scope,
    })
}

#[async_trait]
impl Tool for SessionSearchTool {
    fn name(&self) -> &str {
        "SessionSearch"
    }

    async fn description(&self, _input: &Value) -> String {
        "Search saved allthecodes sessions and return compact resume-oriented hits.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search text to find in saved allthecodes sessions."
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": MAX_LIMIT,
                    "description": "Maximum number of hits to return."
                },
                "scope": {
                    "type": "string",
                    "enum": ["workspace", "all"],
                    "description": "Search only the current workspace by default, or all saved sessions."
                }
            },
            "required": ["query"],
            "additionalProperties": false
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        match parse_input(input) {
            Ok(_) => ValidationResult::Ok,
            Err(err) => ValidationResult::Error {
                message: err.to_string(),
                error_code: 400,
            },
        }
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolUseContext,
        _parent_message: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let parsed = parse_input(&input)?;
        let hits = match parsed.scope {
            SearchScope::Workspace => {
                let cwd = workspace_cwd(ctx);
                search_workspace_sessions(&cwd, &parsed.query, parsed.limit)?
            }
            SearchScope::All => search_sessions(&parsed.query, parsed.limit)?,
        };
        let output = format_results(parsed.scope, &parsed.query, &hits);

        Ok(ToolResult::with_content(
            json!({
                "query": parsed.query,
                "scope": parsed.scope.as_str(),
                "limit": parsed.limit,
                "hits": hits,
            }),
            ToolResultContent::Text(output.clone()),
            output,
        ))
    }

    async fn prompt(&self) -> String {
        "Use SessionSearch to find relevant saved allthecodes sessions by text. \
         Results are summaries only; use /resume <session_id> when a hit should be reopened."
            .to_string()
    }

    fn user_facing_name(&self, _input: Option<&Value>) -> String {
        "SessionSearch".to_string()
    }
}

fn workspace_cwd(ctx: &ToolUseContext) -> PathBuf {
    load_session_info(&ctx.session_id)
        .ok()
        .and_then(|info| {
            let cwd = info.cwd.trim();
            if cwd.is_empty() {
                None
            } else {
                Some(PathBuf::from(cwd))
            }
        })
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn format_results(scope: SearchScope, query: &str, hits: &[SessionSearchResult]) -> String {
    if hits.is_empty() {
        let qualifier = match scope {
            SearchScope::Workspace => "in the current workspace",
            SearchScope::All => "across all saved sessions",
        };
        return format!(
            "No saved session messages matched \"{}\" {}.",
            one_line(query),
            qualifier
        );
    }

    let mut output = format!(
        "SessionSearch found {} saved session hit(s) in {} scope.",
        hits.len(),
        scope.as_str()
    );
    for (index, hit) in hits.iter().enumerate() {
        let role = hit.role.as_deref().unwrap_or(hit.msg_type.as_str());
        output.push_str(&format!(
            "\n{}. session `{}` message #{} ({})",
            index + 1,
            hit.session_id,
            hit.message_index,
            role
        ));
        if !hit.title.trim().is_empty() {
            output.push_str(&format!("\n   title: {}", one_line(&hit.title)));
        }
        if !hit.cwd.trim().is_empty() {
            output.push_str(&format!("\n   cwd: {}", hit.cwd));
        }
        output.push_str(&format!("\n   snippet: {}", one_line(&hit.snippet)));
        output.push_str(&format!(
            "\n   Use /resume {} to continue this session.",
            hit.session_id
        ));
    }
    output
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use allthecodes_types::commands::NoopCommandDispatcher;
    use allthecodes_types::hooks::NoopHookRunner;
    use allthecodes_types::message::{
        AssistantMessage, Message, MessageContent, ToolResultContent, UserMessage,
    };
    use serde_json::json;
    use tempfile::TempDir;
    use tokio::sync::watch;
    use uuid::Uuid;

    use crate::tool::{FileStateCache, Tool, ToolAppState, ToolUseContext, ToolUseOptions, Tools};

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &std::path::Path) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(previous) = &self.previous {
                std::env::set_var(self.key, previous);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    fn user_message(text: &str, timestamp: i64) -> Message {
        Message::User(UserMessage {
            uuid: Uuid::new_v4(),
            timestamp,
            role: "user".to_string(),
            content: MessageContent::Text(text.to_string()),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        })
    }

    fn parent_message() -> AssistantMessage {
        AssistantMessage {
            uuid: Uuid::new_v4(),
            timestamp: 0,
            role: "assistant".to_string(),
            content: vec![],
            usage: None,
            stop_reason: None,
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        }
    }

    fn test_context(session_id: &str) -> (ToolUseContext, watch::Sender<bool>) {
        let (abort_tx, abort_rx) = watch::channel(false);
        let state = ToolAppState::default();
        let ctx = ToolUseContext {
            cwd: ".".to_string(),
            options: ToolUseOptions {
                debug: false,
                main_loop_model: "test-model".to_string(),
                verbose: false,
                is_non_interactive_session: false,
                custom_system_prompt: None,
                append_system_prompt: None,
                max_budget_usd: None,
            },
            abort_signal: abort_rx,
            read_file_state: FileStateCache::default(),
            get_app_state: Arc::new(move || state.clone()),
            set_app_state: Arc::new(|_updater| {}),
            session_id: session_id.to_string(),
            langfuse_session_id: session_id.to_string(),
            messages: vec![],
            agent_id: None,
            agent_type: None,
            query_tracking: None,
            permission_callback: None,
            ask_user_callback: None,
            permission_event_callback: None,
            bg_agent_tx: None,
            hook_runner: Arc::new(NoopHookRunner),
            command_dispatcher: Arc::new(NoopCommandDispatcher),
            available_tools: Tools::new(),
            execute_deferred_tool: None,
        };
        (ctx, abort_tx)
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn session_search_tool_returns_bounded_hits() {
        let home = TempDir::new().expect("temp home");
        let workspace = TempDir::new().expect("temp workspace");
        let _home_guard = EnvGuard::set("ALLTHECODES_HOME", home.path());

        for index in 0..6 {
            let session_id = format!("tool-session-{index}");
            allthecodes_session::storage::save_session(
                &session_id,
                &[user_message(
                    &format!("known phrase appears in result {index}"),
                    100 + index,
                )],
                workspace.path().to_str().expect("workspace path"),
            )
            .expect("save session");
        }

        let tool = SessionSearchTool;
        let (ctx, _abort_tx) = test_context("tool-session-0");
        let result = tool
            .call(
                json!({"query": "known phrase", "limit": 5}),
                &ctx,
                &parent_message(),
                None,
            )
            .await
            .expect("tool result");

        let hits = result
            .data
            .get("hits")
            .and_then(|value| value.as_array())
            .expect("hits array");
        assert_eq!(hits.len(), 5);

        let Some(ToolResultContent::Text(output)) = result.model_content else {
            panic!("expected text model content");
        };
        assert!(output.contains("tool-session-"));
        assert!(output.contains("known phrase"));
        assert!(output.contains("Use /resume tool-session-"));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn session_search_tool_rejects_empty_query() {
        let home = TempDir::new().expect("temp home");
        let _home_guard = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let tool = SessionSearchTool;
        let (ctx, _abort_tx) = test_context("empty-query-session");

        let err = tool
            .call(json!({"query": "   "}), &ctx, &parent_message(), None)
            .await
            .expect_err("empty query should fail");

        assert!(err.to_string().contains("query is required"));
    }
}
