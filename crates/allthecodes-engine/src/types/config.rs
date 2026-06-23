use super::tool::{QueryChainTracking, ToolPermissionContext, Tools};
use allthecodes_types::message::Message;

/// Thinking/extended-thinking configuration.
///
/// Controls whether the model produces `thinking` content blocks.
#[derive(Debug, Clone)]
pub enum ThinkingConfig {
    /// Thinking is disabled entirely.
    Disabled,
    /// Adaptive: the model decides when to think.
    Adaptive,
    /// Thinking is enabled with an optional token budget.
    Enabled { budget_tokens: Option<usize> },
}

/// 查询配置 — 每次 query() 调用时快照一次
///
/// 对应 TypeScript: query/config.ts 的 QueryConfig
#[derive(Debug, Clone)]
pub struct QueryConfig {
    pub session_id: String,
    pub gates: QueryGates,
}

/// 运行时特性开关 (env/statsig 快照)
#[derive(Debug, Clone)]
pub struct QueryGates {
    /// 流式工具执行 (边流式边执行已完成的工具)
    pub streaming_tool_execution: bool,
    /// 产出工具使用摘要
    pub emit_tool_use_summaries: bool,
    /// 快速模式
    pub fast_mode_enabled: bool,
    /// Deferred tool loading: send only the stable core tool schemas and use
    /// SearchExtraTools/ExecuteExtraTool for hidden tools.
    pub deferred_tool_loading: bool,
}

impl Default for QueryGates {
    fn default() -> Self {
        Self {
            streaming_tool_execution: false,
            emit_tool_use_summaries: false,
            fast_mode_enabled: false,
            deferred_tool_loading: true,
        }
    }
}

impl QueryGates {
    pub fn from_env(fast_mode_enabled: bool) -> Self {
        Self::from_env_iter(fast_mode_enabled, std::env::vars())
    }

    pub fn from_env_iter(
        fast_mode_enabled: bool,
        iter: impl IntoIterator<Item = (String, String)>,
    ) -> Self {
        let env: std::collections::HashMap<String, String> = iter.into_iter().collect();
        Self {
            streaming_tool_execution: env_flag_enabled(
                &env,
                "ALLTHECODES_STREAMING_TOOL_EXECUTION",
                "CC_RUST_STREAMING_TOOL_EXECUTION",
            ),
            emit_tool_use_summaries: env_flag_enabled(
                &env,
                "ALLTHECODES_EMIT_TOOL_USE_SUMMARIES",
                "CC_RUST_EMIT_TOOL_USE_SUMMARIES",
            ),
            fast_mode_enabled,
            deferred_tool_loading: env_flag_enabled_by_default(
                &env,
                "ALLTHECODES_DEFERRED_TOOL_LOADING",
                "CC_RUST_DEFERRED_TOOL_LOADING",
            ),
        }
    }
}

fn env_flag_enabled(
    env: &std::collections::HashMap<String, String>,
    name: &str,
    legacy_name: &str,
) -> bool {
    env.get(name)
        .or_else(|| env.get(legacy_name))
        .map(|value| flag_value_enabled(value))
        .unwrap_or(false)
}

fn flag_value_enabled(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn env_flag_enabled_by_default(
    env: &std::collections::HashMap<String, String>,
    name: &str,
    legacy_name: &str,
) -> bool {
    env.get(name)
        .or_else(|| env.get(legacy_name))
        .map(|value| !flag_value_disabled(value))
        .unwrap_or(true)
}

fn flag_value_disabled(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "0" | "false" | "no" | "off"
    )
}

/// query() 函数的参数
///
/// 对应 TypeScript: query.ts 的 QueryParams
pub struct QueryParams {
    pub messages: Vec<Message>,
    pub system_prompt: Vec<String>,
    pub user_context: std::collections::HashMap<String, String>,
    pub system_context: std::collections::HashMap<String, String>,
    pub fallback_model: Option<String>,
    pub query_source: QuerySource,
    pub max_output_tokens_override: Option<usize>,
    pub max_turns: Option<usize>,
    pub skip_cache_write: Option<bool>,
    pub task_budget: Option<TaskBudget>,
    pub gates: QueryGates,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubmitMessageOverrides {
    pub model: Option<String>,
    pub thinking_enabled: Option<bool>,
    pub effort: Option<String>,
    pub allowed_tools: Option<Vec<String>>,
    pub skill_ids: Option<Vec<String>>,
    pub context_mode: Option<SubmitContextMode>,
    pub system_prompt_append_parts: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitContextMode {
    Inherit,
    Compact,
    Isolated,
}

impl SubmitContextMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "inherit" => Some(Self::Inherit),
            "compact" => Some(Self::Compact),
            "isolated" => Some(Self::Isolated),
            _ => None,
        }
    }
}

/// 查询来源
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuerySource {
    Sdk,
    ReplMainThread,
    Compact,
    SessionMemory,
    Agent(String),
    ProactiveTick,
    ScheduledTask,
    WebhookEvent,
    ChannelNotification,
}

impl QuerySource {
    pub fn as_str(&self) -> &str {
        match self {
            QuerySource::Sdk => "sdk",
            QuerySource::ReplMainThread => "repl_main_thread",
            QuerySource::Compact => "compact",
            QuerySource::SessionMemory => "session_memory",
            QuerySource::Agent(_) => "agent:",
            QuerySource::ProactiveTick => "proactive_tick",
            QuerySource::ScheduledTask => "scheduled_task",
            QuerySource::WebhookEvent => "webhook_event",
            QuerySource::ChannelNotification => "channel_notification",
        }
    }

    pub fn as_label(&self) -> String {
        match self {
            QuerySource::Agent(id) => format!("agent:{}", id),
            _ => self.as_str().to_string(),
        }
    }

    pub fn starts_with_agent(&self) -> bool {
        matches!(self, QuerySource::Agent(_))
    }

    pub fn is_autonomous(&self) -> bool {
        matches!(
            self,
            QuerySource::ProactiveTick
                | QuerySource::ScheduledTask
                | QuerySource::WebhookEvent
                | QuerySource::ChannelNotification
        )
    }

    pub fn is_non_interactive(&self) -> bool {
        matches!(
            self,
            QuerySource::Sdk
                | QuerySource::Compact
                | QuerySource::SessionMemory
                | QuerySource::Agent(_)
                | QuerySource::ProactiveTick
                | QuerySource::ScheduledTask
                | QuerySource::WebhookEvent
                | QuerySource::ChannelNotification
        )
    }
}

/// 任务预算
#[derive(Debug, Clone)]
pub struct TaskBudget {
    pub total: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_gates_default_defer_tools() {
        let gates = QueryGates::default();

        assert!(!gates.streaming_tool_execution);
        assert!(!gates.emit_tool_use_summaries);
        assert!(!gates.fast_mode_enabled);
        assert!(gates.deferred_tool_loading);
    }

    #[test]
    fn query_gates_from_env_iter_reads_all_flags() {
        let gates = QueryGates::from_env_iter(
            true,
            [
                (
                    "ALLTHECODES_STREAMING_TOOL_EXECUTION".to_string(),
                    "yes".to_string(),
                ),
                (
                    "ALLTHECODES_EMIT_TOOL_USE_SUMMARIES".to_string(),
                    "ON".to_string(),
                ),
                (
                    "ALLTHECODES_DEFERRED_TOOL_LOADING".to_string(),
                    "true".to_string(),
                ),
            ],
        );

        assert!(gates.streaming_tool_execution);
        assert!(gates.emit_tool_use_summaries);
        assert!(gates.fast_mode_enabled);
        assert!(gates.deferred_tool_loading);
    }

    #[test]
    fn query_gates_from_env_iter_treats_unknown_values_as_off() {
        let gates = QueryGates::from_env_iter(
            false,
            [
                (
                    "ALLTHECODES_STREAMING_TOOL_EXECUTION".to_string(),
                    "enabled".to_string(),
                ),
                (
                    "ALLTHECODES_EMIT_TOOL_USE_SUMMARIES".to_string(),
                    "0".to_string(),
                ),
            ],
        );

        assert!(!gates.streaming_tool_execution);
        assert!(!gates.emit_tool_use_summaries);
        assert!(!gates.fast_mode_enabled);
        assert!(gates.deferred_tool_loading);
    }

    #[test]
    fn query_gates_from_env_iter_can_disable_deferred_loading() {
        for value in ["0", "false", "off", "no"] {
            let gates = QueryGates::from_env_iter(
                false,
                [(
                    "ALLTHECODES_DEFERRED_TOOL_LOADING".to_string(),
                    value.to_string(),
                )],
            );
            assert!(!gates.deferred_tool_loading, "{value} should disable");
        }

        let legacy = QueryGates::from_env_iter(
            false,
            [(
                "CC_RUST_DEFERRED_TOOL_LOADING".to_string(),
                "false".to_string(),
            )],
        );
        assert!(!legacy.deferred_tool_loading);
    }
}

/// QueryEngine 配置
///
/// 对应 TypeScript: QueryEngine.ts 的 QueryEngineConfig
#[derive(Clone)]
pub struct QueryEngineConfig {
    pub cwd: String,
    pub tools: Tools,
    pub custom_system_prompt: Option<String>,
    pub append_system_prompt: Option<String>,
    pub user_specified_model: Option<String>,
    pub fallback_model: Option<String>,
    pub max_turns: Option<usize>,
    pub max_budget_usd: Option<f64>,
    pub task_budget: Option<TaskBudget>,
    pub verbose: bool,
    pub initial_messages: Option<Vec<Message>>,

    // ── New fields (session lifecycle) ──────────────────────────────────
    /// Registered slash commands (placeholder: names only).
    pub commands: Vec<String>,

    /// Thinking / extended-thinking configuration.
    pub thinking_config: Option<ThinkingConfig>,

    /// JSON schema for structured output mode.
    pub json_schema: Option<serde_json::Value>,

    /// Whether to replay user messages back to SDK consumers.
    pub replay_user_messages: bool,

    /// Whether to persist the session to disk.
    pub persist_session: bool,

    /// Resolved model name (from CLI > config/env > provider default).
    /// Used to initialize AppState.main_loop_model.
    pub resolved_model: Option<String>,

    /// Automatically save session to disk after each assistant turn.
    /// Default: true.
    pub auto_save_session: bool,

    /// Sub-agent context — propagated from parent engine to child tools
    /// so that nested agents can enforce recursion depth limits.
    pub agent_context: Option<AgentContext>,
}

/// Context for sub-agent engines, propagated from parent QueryEngine.
///
/// When the Agent tool spawns a child engine, it sets this on the child's
/// `QueryEngineConfig` so that `execute_tool()` can propagate the correct
/// `agent_id` and `depth` into every `ToolUseContext`.
#[derive(Debug, Clone)]
pub struct AgentContext {
    /// Unique ID of this agent instance.
    pub agent_id: String,
    /// Chain tracking for recursion depth enforcement.
    pub query_tracking: QueryChainTracking,
    /// Root Langfuse session ID inherited from the parent agent chain.
    pub langfuse_session_id: String,
    /// Sub-agent role/type used for telemetry naming.
    pub agent_type: Option<String>,
    /// Active team context inherited from the parent engine, if any.
    ///
    /// Team-aware tools such as `SendMessage` read this from AppState; without
    /// carrying it into child engines, spawned subagents lose access to the
    /// current team even when the parent session is already in a team.
    pub team_context: Option<allthecodes_types::teams::TeamContext>,
    /// Permission context inherited from the parent engine.
    ///
    /// Child agents must inherit the parent allow/deny/session grants before
    /// applying any agent-specific permission mode so definitions cannot escape
    /// the current conversation's safety state.
    pub tool_permission_context: Option<ToolPermissionContext>,
}
