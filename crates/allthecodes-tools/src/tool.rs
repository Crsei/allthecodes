use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::hash::{Hash, Hasher};
use std::pin::Pin;
use std::sync::Arc;

use anyhow::Result;
use parking_lot::RwLock;
use serde_json::Value;

pub use allthecodes_types::callbacks::{
    AskUserCallback, AskUserRequestPayload, PermissionCallback, PermissionEventCallback,
    PermissionEventPayload, PermissionRequestPayload, PermissionResponsePayload, ToolProgress,
};
pub use allthecodes_types::permissions::ToolPermissionRulesBySource;
pub use allthecodes_types::permissions::{
    AdditionalWorkingDirectory, PermissionMode, StrippedPermissionRule, ToolPermissionContext,
};

/// Tool-facing projection of engine application state.
///
/// This intentionally contains only the fields tool implementations need. The
/// engine keeps its own richer AppState and maps this projection at the tool
/// execution boundary.
#[derive(Debug, Clone)]
pub struct ToolAppState {
    pub settings: allthecodes_config::runtime_settings::SettingsJson,
    pub verbose: bool,
    pub main_loop_model: String,
    pub main_loop_backend: String,
    pub advisor_model: Option<String>,
    pub tool_permission_context: ToolPermissionContext,
    pub thinking_enabled: Option<bool>,
    pub fast_mode: bool,
    pub effort_value: Option<String>,
    pub team_context: Option<allthecodes_types::teams::TeamContext>,
    pub hooks: HashMap<String, serde_json::Value>,
    pub plan_workflow: Option<allthecodes_types::plan_workflow::PlanWorkflowRecord>,
    pub surfaced_memory_keys: HashSet<String>,
    pub kairos_active: bool,
    pub is_brief_only: bool,
    pub is_assistant_mode: bool,
    pub autonomous_tick_ms: Option<u64>,
    pub terminal_focus: bool,
}

impl Default for ToolAppState {
    fn default() -> Self {
        Self {
            settings: allthecodes_config::runtime_settings::SettingsJson::default(),
            verbose: false,
            main_loop_model: "claude-sonnet-4-20250514".to_string(),
            main_loop_backend: "native".to_string(),
            advisor_model: None,
            tool_permission_context: ToolPermissionContext {
                mode: PermissionMode::Default,
                additional_working_directories: HashMap::new(),
                always_allow_rules: HashMap::new(),
                always_deny_rules: HashMap::new(),
                always_ask_rules: HashMap::new(),
                session_allow_rules: HashMap::new(),
                auto_mode_stripped_always_allow_rules: Vec::new(),
                auto_mode_stripped_session_allow_rules: Vec::new(),
                is_bypass_permissions_mode_available: false,
                is_auto_mode_available: None,
                pre_plan_mode: None,
            },
            thinking_enabled: None,
            fast_mode: false,
            effort_value: None,
            team_context: None,
            hooks: HashMap::new(),
            plan_workflow: None,
            surfaced_memory_keys: HashSet::new(),
            kairos_active: false,
            is_brief_only: false,
            is_assistant_mode: false,
            autonomous_tick_ms: None,
            terminal_focus: true,
        }
    }
}

/// Tool input validation result.
#[derive(Debug, Clone)]
pub enum ValidationResult {
    Ok,
    Error { message: String, error_code: i32 },
}

/// Permission check result.
#[derive(Debug, Clone)]
pub enum PermissionResult {
    Allow { updated_input: Value },
    Deny { message: String },
    Ask { message: String },
}

/// Tool execution result.
#[derive(Debug, Clone, Default)]
pub struct ToolResult {
    pub data: Value,
    pub model_content: Option<allthecodes_types::message::ToolResultContent>,
    pub display_preview: Option<String>,
    pub new_messages: Vec<allthecodes_types::message::Message>,
    pub shell: Option<allthecodes_types::ShellExecutionOutput>,
}

impl ToolResult {
    pub fn with_content(
        data: Value,
        model_content: allthecodes_types::message::ToolResultContent,
        display_preview: String,
    ) -> Self {
        Self {
            data,
            model_content: Some(model_content),
            display_preview: Some(display_preview),
            new_messages: vec![],
            shell: None,
        }
    }
}

/// File state cache used by read/write/edit tools.
#[derive(Debug, Clone, Default)]
pub struct FileStateCache {
    pub entries: Arc<RwLock<HashMap<String, FileCacheEntry>>>,
}

#[derive(Debug, Clone)]
pub struct FileCacheEntry {
    pub content_hash: u64,
    pub last_read_timestamp: i64,
}

impl FileStateCache {
    pub fn get(&self, path: &str) -> Option<FileCacheEntry> {
        self.entries.read().get(path).cloned()
    }

    pub fn insert(&self, path: String, entry: FileCacheEntry) {
        self.entries.write().insert(path, entry);
    }

    pub fn invalidate(&self, path: &str) {
        self.entries.write().remove(path);
    }

    pub fn hash_content(content: &[u8]) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        content.hash(&mut hasher);
        hasher.finish()
    }
}

pub type AppStateUpdater = Box<dyn FnOnce(ToolAppState) -> ToolAppState>;
pub type SetAppState = Arc<dyn Fn(AppStateUpdater) + Send + Sync>;

#[derive(Debug, Clone)]
pub struct DeferredToolExecutionRequest {
    pub tool_use_id: String,
    pub tool_name: String,
    pub input: Value,
}

#[derive(Debug, Clone)]
pub struct DeferredToolExecutionResult {
    pub tool_use_id: String,
    pub tool_name: String,
    pub result: ToolResult,
    pub is_error: bool,
}

pub type DeferredToolExecutor = Arc<
    dyn Fn(
            DeferredToolExecutionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<DeferredToolExecutionResult>> + Send>>
        + Send
        + Sync,
>;

/// Context passed to every tool call.
pub struct ToolUseContext {
    pub options: ToolUseOptions,
    pub abort_signal: tokio::sync::watch::Receiver<bool>,
    pub read_file_state: FileStateCache,
    pub get_app_state: Arc<dyn Fn() -> ToolAppState + Send + Sync>,
    pub set_app_state: SetAppState,
    pub session_id: String,
    pub langfuse_session_id: String,
    pub messages: Vec<allthecodes_types::message::Message>,
    pub agent_id: Option<String>,
    pub agent_type: Option<String>,
    pub query_tracking: Option<QueryChainTracking>,
    pub permission_callback: Option<PermissionCallback>,
    pub ask_user_callback: Option<AskUserCallback>,
    pub permission_event_callback: Option<PermissionEventCallback>,
    pub bg_agent_tx: Option<allthecodes_types::agent_channel::AgentSender>,
    pub hook_runner: Arc<dyn allthecodes_types::hooks::HookRunner>,
    pub command_dispatcher: Arc<dyn allthecodes_types::commands::CommandDispatcher>,
    pub available_tools: Tools,
    pub execute_deferred_tool: Option<DeferredToolExecutor>,
}

/// Parent runtime state observable at the exact tool-call boundary. The
/// engine's AgentTool uses this snapshot for fork construction without making
/// the generic tools crate depend on engine-only QuerySource/ThinkingConfig
/// enums.
#[derive(Debug, Clone, PartialEq)]
pub struct ParentRuntimeSnapshot {
    pub rendered_system_prompt: Option<String>,
    pub query_source: Option<String>,
    pub thinking_config: Option<serde_json::Value>,
    pub available_tool_names: Vec<String>,
}

impl ToolUseContext {
    pub fn parent_runtime_snapshot(&self) -> ParentRuntimeSnapshot {
        let app_state = (self.get_app_state)();
        let rendered_system_prompt = [
            self.options.custom_system_prompt.as_deref(),
            self.options.append_system_prompt.as_deref(),
        ]
        .into_iter()
        .flatten()
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>();
        let rendered_system_prompt =
            (!rendered_system_prompt.is_empty()).then(|| rendered_system_prompt.join("\n\n"));
        let query_source = self.agent_id.as_ref().map_or_else(
            || {
                Some(if self.options.is_non_interactive_session {
                    "non_interactive".to_string()
                } else {
                    "interactive".to_string()
                })
            },
            |agent_id| Some(format!("agent:{agent_id}")),
        );
        let thinking_config = app_state.thinking_enabled.map(|enabled| {
            serde_json::json!({
                "enabled": enabled,
                "effort": app_state.effort_value,
            })
        });
        let available_tool_names = self
            .available_tools
            .iter()
            .map(|tool| tool.name().to_string())
            .collect();

        // TODO(upstream fork parity): clone ContentReplacementState when an
        // equivalent becomes part of allthecodes' tool-call context.
        ParentRuntimeSnapshot {
            rendered_system_prompt,
            query_source,
            thinking_config,
            available_tool_names,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToolUseOptions {
    pub debug: bool,
    pub main_loop_model: String,
    pub verbose: bool,
    pub is_non_interactive_session: bool,
    pub custom_system_prompt: Option<String>,
    pub append_system_prompt: Option<String>,
    pub max_budget_usd: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct QueryChainTracking {
    pub chain_id: String,
    pub depth: usize,
}

#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;

    async fn description(&self, input: &Value) -> String;

    fn input_json_schema(&self) -> Value;

    fn is_enabled(&self) -> bool {
        true
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        false
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        false
    }

    fn is_destructive(&self, _input: &Value) -> bool {
        false
    }

    async fn validate_input(&self, _input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        ValidationResult::Ok
    }

    async fn check_permissions(&self, input: &Value, _ctx: &ToolUseContext) -> PermissionResult {
        PermissionResult::Allow {
            updated_input: input.clone(),
        }
    }

    fn backfill_observable_input(&self, _input: &mut serde_json::Map<String, Value>) {}

    async fn call(
        &self,
        input: Value,
        ctx: &ToolUseContext,
        parent_message: &allthecodes_types::message::AssistantMessage,
        on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult>;

    async fn prompt(&self) -> String;

    fn user_facing_name(&self, _input: Option<&Value>) -> String {
        self.name().to_string()
    }

    fn mcp_server_name(&self) -> Option<&str> {
        None
    }

    fn max_result_size_chars(&self) -> usize {
        100_000
    }

    fn get_path(&self, _input: &Value) -> Option<String> {
        None
    }

    fn interrupt_behavior(&self) -> InterruptBehavior {
        InterruptBehavior::Block
    }

    fn to_auto_classifier_input(&self, _input: &Value) -> Value {
        Value::String(String::new())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterruptBehavior {
    Cancel,
    Block,
}

pub type Tools = Vec<Arc<dyn Tool>>;
