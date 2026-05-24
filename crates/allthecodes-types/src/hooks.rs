//! Hook runner trait and plain data types for the tool-execution hook system.
//!
//! The engine uses hooks at several lifecycle points (PreToolUse, PostToolUse,
//! PostToolUseFailure, SubagentStart, SubagentStop, UserPromptSubmit,
//! InstructionsLoaded, PermissionRequest, PermissionDenied, …). The concrete
//! runner that spawns shell commands lives in `cc-tools::hooks`; the engine
//! depends only on this trait so it has no direct edge to `cc-tools`.
//!
//! This file also holds type definitions for the complete hook subsystem:
//! session hooks, async hook registry, hook events, SSRF guard, etc.

use std::collections::HashMap;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

// ---------------------------------------------------------------------------
// Hook event enum — canonical list of all hook event names
// ---------------------------------------------------------------------------

/// All known hook events, matching the TypeScript `HookEvent` union.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HookEvent {
    #[serde(rename = "PreToolUse")]
    PreToolUse,
    #[serde(rename = "PostToolUse")]
    PostToolUse,
    #[serde(rename = "PostToolUseFailure")]
    PostToolUseFailure,
    #[serde(rename = "PermissionDenied")]
    PermissionDenied,
    #[serde(rename = "Notification")]
    Notification,
    #[serde(rename = "UserPromptSubmit")]
    UserPromptSubmit,
    #[serde(rename = "SessionStart")]
    SessionStart,
    #[serde(rename = "Stop")]
    Stop,
    #[serde(rename = "StopFailure")]
    StopFailure,
    #[serde(rename = "SubagentStart")]
    SubagentStart,
    #[serde(rename = "SubagentStop")]
    SubagentStop,
    #[serde(rename = "PreCompact")]
    PreCompact,
    #[serde(rename = "PostCompact")]
    PostCompact,
    #[serde(rename = "SessionEnd")]
    SessionEnd,
    #[serde(rename = "PermissionRequest")]
    PermissionRequest,
    #[serde(rename = "Setup")]
    Setup,
    #[serde(rename = "TeammateIdle")]
    TeammateIdle,
    #[serde(rename = "TaskCreated")]
    TaskCreated,
    #[serde(rename = "TaskCompleted")]
    TaskCompleted,
    #[serde(rename = "Elicitation")]
    Elicitation,
    #[serde(rename = "ElicitationResult")]
    ElicitationResult,
    #[serde(rename = "ConfigChange")]
    ConfigChange,
    #[serde(rename = "InstructionsLoaded")]
    InstructionsLoaded,
    #[serde(rename = "WorktreeCreate")]
    WorktreeCreate,
    #[serde(rename = "WorktreeRemove")]
    WorktreeRemove,
    #[serde(rename = "CwdChanged")]
    CwdChanged,
    #[serde(rename = "FileChanged")]
    FileChanged,
}

/// All hook event name strings.
pub const HOOK_EVENTS: &[HookEvent] = &[
    HookEvent::PreToolUse,
    HookEvent::PostToolUse,
    HookEvent::PostToolUseFailure,
    HookEvent::PermissionDenied,
    HookEvent::Notification,
    HookEvent::UserPromptSubmit,
    HookEvent::SessionStart,
    HookEvent::Stop,
    HookEvent::StopFailure,
    HookEvent::SubagentStart,
    HookEvent::SubagentStop,
    HookEvent::PreCompact,
    HookEvent::PostCompact,
    HookEvent::SessionEnd,
    HookEvent::PermissionRequest,
    HookEvent::Setup,
    HookEvent::TeammateIdle,
    HookEvent::TaskCreated,
    HookEvent::TaskCompleted,
    HookEvent::Elicitation,
    HookEvent::ElicitationResult,
    HookEvent::ConfigChange,
    HookEvent::InstructionsLoaded,
    HookEvent::WorktreeCreate,
    HookEvent::WorktreeRemove,
    HookEvent::CwdChanged,
    HookEvent::FileChanged,
];

impl std::fmt::Display for HookEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PreToolUse => write!(f, "PreToolUse"),
            Self::PostToolUse => write!(f, "PostToolUse"),
            Self::PostToolUseFailure => write!(f, "PostToolUseFailure"),
            Self::PermissionDenied => write!(f, "PermissionDenied"),
            Self::Notification => write!(f, "Notification"),
            Self::UserPromptSubmit => write!(f, "UserPromptSubmit"),
            Self::SessionStart => write!(f, "SessionStart"),
            Self::Stop => write!(f, "Stop"),
            Self::StopFailure => write!(f, "StopFailure"),
            Self::SubagentStart => write!(f, "SubagentStart"),
            Self::SubagentStop => write!(f, "SubagentStop"),
            Self::PreCompact => write!(f, "PreCompact"),
            Self::PostCompact => write!(f, "PostCompact"),
            Self::SessionEnd => write!(f, "SessionEnd"),
            Self::PermissionRequest => write!(f, "PermissionRequest"),
            Self::Setup => write!(f, "Setup"),
            Self::TeammateIdle => write!(f, "TeammateIdle"),
            Self::TaskCreated => write!(f, "TaskCreated"),
            Self::TaskCompleted => write!(f, "TaskCompleted"),
            Self::Elicitation => write!(f, "Elicitation"),
            Self::ElicitationResult => write!(f, "ElicitationResult"),
            Self::ConfigChange => write!(f, "ConfigChange"),
            Self::InstructionsLoaded => write!(f, "InstructionsLoaded"),
            Self::WorktreeCreate => write!(f, "WorktreeCreate"),
            Self::WorktreeRemove => write!(f, "WorktreeRemove"),
            Self::CwdChanged => write!(f, "CwdChanged"),
            Self::FileChanged => write!(f, "FileChanged"),
        }
    }
}

// ---------------------------------------------------------------------------
// Hook result types
// ---------------------------------------------------------------------------

/// Result of running pre-tool hooks.
#[derive(Debug, Clone)]
pub enum PreToolHookResult {
    /// Continue with execution (possibly with modified input).
    Continue {
        /// Modified input (None = use original).
        updated_input: Option<Value>,
        /// Permission override from hook.
        permission_override: Option<PermissionOverride>,
    },
    /// Stop tool execution (hook explicitly blocked it).
    Stop {
        /// Message explaining why the hook stopped execution.
        message: String,
    },
}

/// Permission override from a hook.
#[derive(Debug, Clone)]
pub enum PermissionOverride {
    /// Force allow.
    Allow,
    /// Force deny.
    Deny { reason: String },
}

/// Result of running post-tool hooks.
#[derive(Debug, Clone)]
pub enum PostToolHookResult {
    /// Continue normally.
    Continue,
    /// Hook wants to stop the continuation chain.
    StopContinuation { message: String },
}

// ---------------------------------------------------------------------------
// Hook configuration types (deserialized from settings.json)
// ---------------------------------------------------------------------------

/// Hook configuration from settings.json.
///
/// Each event (e.g. "PreToolUse") contains a list of these, each optionally
/// matching a tool name and containing a list of hook entries to run.
#[derive(Debug, Clone, Deserialize)]
pub struct HookEventConfig {
    /// Tool name matcher (e.g., "Bash", "Read", "*").
    /// None or "*" matches all tools.
    pub matcher: Option<String>,
    /// Whether this hook config is policy-critical and should fail closed.
    /// Existing configs omit this field and remain optional/best-effort.
    #[serde(default)]
    pub critical: bool,
    /// List of hook entries to run when this config matches.
    pub hooks: Vec<HookEntry>,
}

/// A single hook entry — supports `command`, `prompt`, `agent`, `http` types.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum HookEntry {
    #[serde(rename = "command")]
    Command {
        command: String,
        #[serde(default = "default_timeout")]
        timeout: u64, // seconds
        #[serde(skip_serializing_if = "Option::is_none")]
        shell: Option<String>,
        #[serde(rename = "if", skip_serializing_if = "Option::is_none")]
        if_condition: Option<String>,
    },
    /// Prompt-type hook: fires an LLM call with a prompt template.
    #[serde(rename = "prompt")]
    Prompt {
        prompt: String,
        #[serde(default = "default_prompt_timeout")]
        timeout: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        #[serde(rename = "if", skip_serializing_if = "Option::is_none")]
        if_condition: Option<String>,
    },
    /// Agent-type hook: spawns a sub-agent with tool access.
    #[serde(rename = "agent")]
    Agent {
        prompt: String,
        #[serde(default = "default_agent_timeout")]
        timeout: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        #[serde(rename = "if", skip_serializing_if = "Option::is_none")]
        if_condition: Option<String>,
    },
    /// HTTP-type hook: POSTs to a URL.
    #[serde(rename = "http")]
    Http {
        url: String,
        #[serde(default = "default_http_timeout")]
        timeout: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        headers: Option<HashMap<String, String>>,
        #[serde(rename = "if", skip_serializing_if = "Option::is_none")]
        if_condition: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        allowed_env_vars: Option<Vec<String>>,
    },
}

fn default_timeout() -> u64 {
    60
}

fn default_prompt_timeout() -> u64 {
    30
}

fn default_agent_timeout() -> u64 {
    60
}

fn default_http_timeout() -> u64 {
    600 // 10 minutes
}

/// JSON output from a hook subprocess.
///
/// The subprocess writes a single JSON line to stdout. All fields are
/// optional; the default is to continue execution without changes.
#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct HookOutput {
    /// If false, stop tool execution.
    #[serde(rename = "continue")]
    pub should_continue: bool,
    /// Reason for stopping (post-tool hooks).
    pub stop_reason: Option<String>,
    /// Decision string (e.g., "allow", "deny", "block").
    pub decision: Option<String>,
    /// Reason for the decision.
    pub reason: Option<String>,
    /// Permission decision for pre-tool hooks ("allow" or "deny").
    pub permission_decision: Option<String>,
    /// Modified tool input (pre-tool hooks).
    pub updated_input: Option<Value>,
    /// Additional context to include in messages.
    pub additional_context: Option<String>,
    /// Hook-specific output (arbitrary JSON).
    pub hook_specific_output: Option<Value>,
    /// Watch paths for file watcher.
    #[serde(default)]
    pub watch_paths: Vec<String>,
}

impl Default for HookOutput {
    fn default() -> Self {
        Self {
            should_continue: true,
            stop_reason: None,
            decision: None,
            reason: None,
            permission_decision: None,
            updated_input: None,
            additional_context: None,
            hook_specific_output: None,
            watch_paths: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Hook source and individual config
// ---------------------------------------------------------------------------

/// Where a hook configuration originates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HookSource {
    UserSettings,
    ProjectSettings,
    LocalSettings,
    PolicySettings,
    PluginHook,
    SessionHook,
    BuiltinHook,
}

impl std::fmt::Display for HookSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UserSettings => write!(f, "userSettings"),
            Self::ProjectSettings => write!(f, "projectSettings"),
            Self::LocalSettings => write!(f, "localSettings"),
            Self::PolicySettings => write!(f, "policySettings"),
            Self::PluginHook => write!(f, "pluginHook"),
            Self::SessionHook => write!(f, "sessionHook"),
            Self::BuiltinHook => write!(f, "builtinHook"),
        }
    }
}

/// A single hook config with its event, matcher, and source metadata.
#[derive(Debug, Clone)]
pub struct IndividualHookConfig {
    pub event: HookEvent,
    pub config: HookEntry,
    pub matcher: String,
    pub source: HookSource,
    pub plugin_name: Option<String>,
}

// ---------------------------------------------------------------------------
// Matcher metadata for hook event UI display
// ---------------------------------------------------------------------------

/// Metadata about what field a hook event matches on.
#[derive(Debug, Clone)]
pub struct MatcherMetadata {
    pub field_to_match: String,
    pub values: Vec<String>,
}

/// UI display metadata for a hook event.
#[derive(Debug, Clone)]
pub struct HookEventMetadata {
    pub summary: String,
    pub description: String,
    pub matcher_metadata: Option<MatcherMetadata>,
}

// ---------------------------------------------------------------------------
// Hook event system types
// ---------------------------------------------------------------------------

/// Emitted when a hook starts executing.
#[derive(Debug, Clone)]
pub struct HookStartedEvent {
    pub hook_id: String,
    pub hook_name: String,
    pub hook_event: String,
}

/// Emitted periodically while a hook is running.
#[derive(Debug, Clone)]
pub struct HookProgressEvent {
    pub hook_id: String,
    pub hook_name: String,
    pub hook_event: String,
    pub stdout: String,
    pub stderr: String,
    pub output: String,
}

/// Emitted when a hook completes.
#[derive(Debug, Clone)]
pub struct HookResponseEvent {
    pub hook_id: String,
    pub hook_name: String,
    pub hook_event: String,
    pub output: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub outcome: HookOutcome,
}

/// Outcome of a hook execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HookOutcome {
    Success,
    Error,
    Cancelled,
}

/// Any hook execution event.
#[derive(Debug, Clone)]
pub enum HookExecutionEvent {
    Started(HookStartedEvent),
    Progress(HookProgressEvent),
    Response(HookResponseEvent),
}

/// Callback for hook events.
pub type HookEventHandler = Box<dyn Fn(HookExecutionEvent) + Send + Sync>;

// ---------------------------------------------------------------------------
// Session hook types
// ---------------------------------------------------------------------------

/// Callback for function hooks — returns true if check passes, false to block.
pub type FunctionHookCallback =
    Box<dyn Fn(&[Value], Option<tokio::sync::watch::Receiver<bool>>) -> bool + Send + Sync>;

/// A function hook runs an in-memory callback instead of a subprocess.
#[derive(Debug, Clone)]
pub struct FunctionHook {
    pub id: String,
    pub timeout: u64,
    pub error_message: String,
    pub status_message: Option<String>,
}

/// A session-scoped hook entry combining a hook config with optional callback.
#[derive(Debug, Clone)]
pub struct SessionHookEntry {
    pub hook: HookEntry,
    pub function_hook: Option<FunctionHook>,
    pub on_hook_success: Option<String>, // callback identifier
}

/// A matcher group within session hooks for a given event.
#[derive(Debug, Clone)]
pub struct SessionHookMatcher {
    pub matcher: String,
    pub skill_root: Option<String>,
    pub hooks: Vec<SessionHookEntry>,
}

/// Per-session hook store (ephemeral, in-memory).
#[derive(Debug, Clone)]
pub struct SessionStore {
    pub hooks: HashMap<HookEvent, Vec<SessionHookMatcher>>,
}

/// A derived hook matcher for display/execution (function hooks filtered out).
#[derive(Debug, Clone)]
pub struct SessionDerivedHookMatcher {
    pub matcher: String,
    pub hooks: Vec<HookEntry>,
    pub skill_root: Option<String>,
}

// ---------------------------------------------------------------------------
// Async hook registry types
// ---------------------------------------------------------------------------

/// A pending async hook that has been spawned but not yet completed.
#[derive(Debug, Clone)]
pub struct PendingAsyncHook {
    pub process_id: String,
    pub hook_id: String,
    pub hook_name: String,
    pub hook_event: String, // HookEvent or "StatusLine" or "FileSuggestion"
    pub tool_name: Option<String>,
    pub plugin_id: Option<String>,
    pub start_time: DateTime<Utc>,
    pub timeout: u64,
    pub command: String,
    pub response_attachment_sent: bool,
}

/// Collected response from a completed async hook.
#[derive(Debug, Clone)]
pub struct AsyncHookResponse {
    pub process_id: String,
    pub response: Value,
    pub hook_name: String,
    pub hook_event: String,
    pub tool_name: Option<String>,
    pub plugin_id: Option<String>,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
}

// ---------------------------------------------------------------------------
// Agent hook result
// ---------------------------------------------------------------------------

/// Result of running an agent or prompt hook.
#[derive(Debug, Clone)]
pub struct HookResult {
    pub outcome: HookResultOutcome,
    pub message: Option<Value>,
    pub blocking_error: Option<HookBlockingError>,
    pub prevent_continuation: bool,
    pub stop_reason: Option<String>,
}

/// Outcome classification for agent/prompt hooks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookResultOutcome {
    Success,
    Blocking,
    Cancelled,
    NonBlockingError,
}

impl HookResult {
    pub fn success() -> Self {
        Self {
            outcome: HookResultOutcome::Success,
            message: None,
            blocking_error: None,
            prevent_continuation: false,
            stop_reason: None,
        }
    }

    pub fn cancelled() -> Self {
        Self {
            outcome: HookResultOutcome::Cancelled,
            message: None,
            blocking_error: None,
            prevent_continuation: false,
            stop_reason: None,
        }
    }

    pub fn blocking(error: String, command: String) -> Self {
        Self {
            outcome: HookResultOutcome::Blocking,
            message: None,
            blocking_error: Some(HookBlockingError {
                blocking_error: error,
                command,
            }),
            prevent_continuation: true,
            stop_reason: None,
        }
    }

    pub fn non_blocking_error(message: Value) -> Self {
        Self {
            outcome: HookResultOutcome::NonBlockingError,
            message: Some(message),
            blocking_error: None,
            prevent_continuation: false,
            stop_reason: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct HookBlockingError {
    pub blocking_error: String,
    pub command: String,
}

// ---------------------------------------------------------------------------
// Post-sampling hook types
// ---------------------------------------------------------------------------

/// Context passed to post-sampling hooks.
#[derive(Debug, Clone)]
pub struct ReplHookContext {
    pub messages: Vec<Value>,
    pub system_prompt: String,
    pub user_context: HashMap<String, String>,
    pub system_context: HashMap<String, String>,
    pub query_source: Option<String>,
}

/// A post-sampling hook is called after model sampling completes.
pub type PostSamplingHook = Box<dyn Fn(ReplHookContext) + Send + Sync>;

// ---------------------------------------------------------------------------
// API query hook types
// ---------------------------------------------------------------------------

/// Extended context for API query hooks.
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
pub struct ApiQueryResult<T> {
    pub query_name: String,
    pub result: T,
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

// ---------------------------------------------------------------------------
// Skill improvement types
// ---------------------------------------------------------------------------

/// A single suggested improvement to a skill.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillUpdate {
    pub section: String,
    pub change: String,
    pub reason: String,
}

// ---------------------------------------------------------------------------
// SSRF guard types
// ---------------------------------------------------------------------------

/// SSRF guard configuration for HTTP hooks.
#[derive(Debug, Clone)]
pub struct SsrfConfig {
    pub blocked_v4_ranges: Vec<&'static str>,
    pub blocked_v6_ranges: Vec<&'static str>,
    pub allow_loopback: bool,
}

impl Default for SsrfConfig {
    fn default() -> Self {
        Self {
            blocked_v4_ranges: vec![
                "0.0.0.0/8",
                "10.0.0.0/8",
                "100.64.0.0/10",
                "169.254.0.0/16",
                "172.16.0.0/12",
                "192.168.0.0/16",
            ],
            blocked_v6_ranges: vec!["::/128", "fc00::/7", "fe80::/10"],
            allow_loopback: true,
        }
    }
}

// ---------------------------------------------------------------------------
// HookRunner trait
// ---------------------------------------------------------------------------

/// Type alias for the hooks map loaded from `settings.json`.
pub type HooksMap = HashMap<String, Value>;

/// Check if a hook matcher pattern applies to a tool name.
///
/// `None` and `"*"` match all tools. Other values match either an exact tool
/// name or a prefix, which covers MCP tool families such as `mcp__`.
pub fn matches_tool(matcher: Option<&str>, tool_name: &str) -> bool {
    match matcher {
        None => true,
        Some("*") => true,
        Some(pattern) => tool_name == pattern || tool_name.starts_with(pattern),
    }
}

/// Load hook configurations for a specific event from the hooks settings map.
///
/// Invalid event payloads preserve the existing best-effort behavior by
/// returning an empty config set.
pub fn load_hook_configs(hooks_value: &HooksMap, event_name: &str) -> Vec<HookEventConfig> {
    hooks_value
        .get(event_name)
        .and_then(|event_value| serde_json::from_value(event_value.clone()).ok())
        .unwrap_or_default()
}

/// Trait for running hook subprocess commands.
///
/// Decouples the engine from the concrete shell-execution implementation that
/// lives in `tools::hooks`. Object-safe: callers store this as
/// `Arc<dyn HookRunner>`.
#[async_trait]
pub trait HookRunner: Send + Sync {
    /// Load hook configurations for a specific event from the hooks settings.
    fn load_hook_configs(&self, hooks_value: &HooksMap, event_name: &str) -> Vec<HookEventConfig>;

    /// Run pre-tool hooks for a tool invocation.
    async fn run_pre_tool_hooks(
        &self,
        tool_name: &str,
        input: &Value,
        hook_configs: &[HookEventConfig],
    ) -> anyhow::Result<PreToolHookResult>;

    /// Run post-tool hooks after a successful tool call.
    async fn run_post_tool_hooks(
        &self,
        tool_name: &str,
        input: &Value,
        tool_result_data: &Value,
        hook_configs: &[HookEventConfig],
    ) -> anyhow::Result<PostToolHookResult>;

    /// Run post-tool failure hooks after a failed tool call.
    async fn run_post_tool_failure_hooks(
        &self,
        tool_name: &str,
        input: &Value,
        error: &str,
        hook_configs: &[HookEventConfig],
    ) -> anyhow::Result<()>;

    /// Generic event hook runner for non-tool lifecycle events.
    async fn run_event_hooks(
        &self,
        event_name: &str,
        payload: &Value,
        hook_configs: &[HookEventConfig],
    ) -> anyhow::Result<HookOutput>;

    /// Run Stop lifecycle hooks.
    async fn run_stop_hooks(
        &self,
        hook_configs: &[HookEventConfig],
    ) -> anyhow::Result<PostToolHookResult>;
}

// ---------------------------------------------------------------------------
// NoopHookRunner — a safe default that never fires any hooks
// ---------------------------------------------------------------------------

/// A `HookRunner` that runs no hooks, regardless of settings.
pub struct NoopHookRunner;

impl NoopHookRunner {
    pub fn new() -> Self {
        Self
    }
}

impl Default for NoopHookRunner {
    fn default() -> Self {
        Self
    }
}

#[async_trait]
impl HookRunner for NoopHookRunner {
    fn load_hook_configs(
        &self,
        _hooks_value: &HooksMap,
        _event_name: &str,
    ) -> Vec<HookEventConfig> {
        Vec::new()
    }

    async fn run_pre_tool_hooks(
        &self,
        _tool_name: &str,
        _input: &Value,
        _hook_configs: &[HookEventConfig],
    ) -> anyhow::Result<PreToolHookResult> {
        Ok(PreToolHookResult::Continue {
            updated_input: None,
            permission_override: None,
        })
    }

    async fn run_post_tool_hooks(
        &self,
        _tool_name: &str,
        _input: &Value,
        _tool_result_data: &Value,
        _hook_configs: &[HookEventConfig],
    ) -> anyhow::Result<PostToolHookResult> {
        Ok(PostToolHookResult::Continue)
    }

    async fn run_post_tool_failure_hooks(
        &self,
        _tool_name: &str,
        _input: &Value,
        _error: &str,
        _hook_configs: &[HookEventConfig],
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn run_event_hooks(
        &self,
        _event_name: &str,
        _payload: &Value,
        _hook_configs: &[HookEventConfig],
    ) -> anyhow::Result<HookOutput> {
        Ok(HookOutput::default())
    }

    async fn run_stop_hooks(
        &self,
        _hook_configs: &[HookEventConfig],
    ) -> anyhow::Result<PostToolHookResult> {
        Ok(PostToolHookResult::Continue)
    }
}

// ---------------------------------------------------------------------------
// Helper: hook event metadata
// ---------------------------------------------------------------------------

/// Returns UI metadata for all hook events.
pub fn get_hook_event_metadata() -> HashMap<HookEvent, HookEventMetadata> {
    let mut m = HashMap::new();

    m.insert(
        HookEvent::PreToolUse,
        HookEventMetadata {
            summary: "Before tool execution".into(),
            description: "Input to command is JSON of tool call arguments.\nExit code 0 - stdout/stderr not shown\nExit code 2 - show stderr to model and block tool call\nOther exit codes - show stderr to user only but continue with tool call".into(),
            matcher_metadata: Some(MatcherMetadata {
                field_to_match: "tool_name".into(),
                values: Vec::new(),
            }),
        },
    );

    m.insert(
        HookEvent::PostToolUse,
        HookEventMetadata {
            summary: "After tool execution".into(),
            description: "Input to command is JSON with fields \"inputs\" (tool call arguments) and \"response\" (tool call response).\nExit code 0 - stdout shown in transcript mode (ctrl+o)\nExit code 2 - show stderr to model immediately\nOther exit codes - show stderr to user only".into(),
            matcher_metadata: Some(MatcherMetadata {
                field_to_match: "tool_name".into(),
                values: Vec::new(),
            }),
        },
    );

    m.insert(
        HookEvent::PostToolUseFailure,
        HookEventMetadata {
            summary: "After tool execution fails".into(),
            description: "Input to command is JSON with tool_name, tool_input, tool_use_id, error, error_type, is_interrupt, and is_timeout.\nExit code 0 - stdout shown in transcript mode (ctrl+o)\nExit code 2 - show stderr to model immediately\nOther exit codes - show stderr to user only".into(),
            matcher_metadata: Some(MatcherMetadata {
                field_to_match: "tool_name".into(),
                values: Vec::new(),
            }),
        },
    );

    m.insert(
        HookEvent::Notification,
        HookEventMetadata {
            summary: "When notifications are sent".into(),
            description: "Input to command is JSON with notification message and type.\nExit code 0 - stdout/stderr not shown\nOther exit codes - show stderr to user only".into(),
            matcher_metadata: Some(MatcherMetadata {
                field_to_match: "notification_type".into(),
                values: vec![
                    "permission_prompt".into(),
                    "idle_prompt".into(),
                    "auth_success".into(),
                    "elicitation_dialog".into(),
                    "elicitation_complete".into(),
                    "elicitation_response".into(),
                ],
            }),
        },
    );

    m.insert(
        HookEvent::UserPromptSubmit,
        HookEventMetadata {
            summary: "When the user submits a prompt".into(),
            description: "Input to command is JSON with original user prompt text.\nExit code 0 - stdout shown to Claude\nExit code 2 - block processing, erase original prompt, and show stderr to user only\nOther exit codes - show stderr to user only".into(),
            matcher_metadata: None,
        },
    );

    m.insert(
        HookEvent::SessionStart,
        HookEventMetadata {
            summary: "When a new session is started".into(),
            description: "Input to command is JSON with session start source.\nExit code 0 - stdout shown to Claude\nBlocking errors are ignored\nOther exit codes - show stderr to user only".into(),
            matcher_metadata: Some(MatcherMetadata {
                field_to_match: "source".into(),
                values: vec!["startup".into(), "resume".into(), "clear".into(), "compact".into()],
            }),
        },
    );

    m.insert(
        HookEvent::Stop,
        HookEventMetadata {
            summary: "Right before Claude concludes its response".into(),
            description: "Exit code 0 - stdout/stderr not shown\nExit code 2 - show stderr to model and continue conversation\nOther exit codes - show stderr to user only".into(),
            matcher_metadata: None,
        },
    );

    m.insert(
        HookEvent::StopFailure,
        HookEventMetadata {
            summary: "When the turn ends due to an API error".into(),
            description: "Fires instead of Stop when an API error (rate limit, auth failure, etc.) ended the turn. Fire-and-forget — hook output and exit codes are ignored.".into(),
            matcher_metadata: Some(MatcherMetadata {
                field_to_match: "error".into(),
                values: vec![
                    "rate_limit".into(),
                    "authentication_failed".into(),
                    "billing_error".into(),
                    "invalid_request".into(),
                    "server_error".into(),
                    "max_output_tokens".into(),
                    "unknown".into(),
                ],
            }),
        },
    );

    m.insert(
        HookEvent::SubagentStart,
        HookEventMetadata {
            summary: "When a subagent (Agent tool call) is started".into(),
            description: "Input to command is JSON with agent_id and agent_type.\nExit code 0 - stdout shown to subagent\nBlocking errors are ignored\nOther exit codes - show stderr to user only".into(),
            matcher_metadata: Some(MatcherMetadata {
                field_to_match: "agent_type".into(),
                values: Vec::new(),
            }),
        },
    );

    m.insert(
        HookEvent::SubagentStop,
        HookEventMetadata {
            summary: "Right before a subagent concludes its response".into(),
            description: "Input to command is JSON with agent_id, agent_type, and agent_transcript_path.\nExit code 0 - stdout/stderr not shown\nExit code 2 - show stderr to subagent and continue having it run\nOther exit codes - show stderr to user only".into(),
            matcher_metadata: Some(MatcherMetadata {
                field_to_match: "agent_type".into(),
                values: Vec::new(),
            }),
        },
    );

    m.insert(
        HookEvent::PreCompact,
        HookEventMetadata {
            summary: "Before conversation compaction".into(),
            description: "Input to command is JSON with compaction details.\nExit code 0 - stdout appended as custom compact instructions\nExit code 2 - block compaction\nOther exit codes - show stderr to user only but continue with compaction".into(),
            matcher_metadata: Some(MatcherMetadata {
                field_to_match: "trigger".into(),
                values: vec!["manual".into(), "auto".into()],
            }),
        },
    );

    m.insert(
        HookEvent::PostCompact,
        HookEventMetadata {
            summary: "After conversation compaction".into(),
            description: "Input to command is JSON with compaction details and the summary.\nExit code 0 - stdout shown to user\nOther exit codes - show stderr to user only".into(),
            matcher_metadata: Some(MatcherMetadata {
                field_to_match: "trigger".into(),
                values: vec!["manual".into(), "auto".into()],
            }),
        },
    );

    m.insert(
        HookEvent::SessionEnd,
        HookEventMetadata {
            summary: "When a session is ending".into(),
            description: "Input to command is JSON with session end reason.\nExit code 0 - command completes successfully\nOther exit codes - show stderr to user only".into(),
            matcher_metadata: Some(MatcherMetadata {
                field_to_match: "reason".into(),
                values: vec![
                    "clear".into(),
                    "logout".into(),
                    "prompt_input_exit".into(),
                    "other".into(),
                ],
            }),
        },
    );

    m.insert(
        HookEvent::PermissionRequest,
        HookEventMetadata {
            summary: "When a permission dialog is displayed".into(),
            description: "Input to command is JSON with tool_name, tool_input, and tool_use_id.\nOutput JSON with hookSpecificOutput containing decision to allow or deny.\nExit code 0 - use hook decision if provided\nOther exit codes - show stderr to user only".into(),
            matcher_metadata: Some(MatcherMetadata {
                field_to_match: "tool_name".into(),
                values: Vec::new(),
            }),
        },
    );

    m.insert(
        HookEvent::PermissionDenied,
        HookEventMetadata {
            summary: "When a permission request is denied".into(),
            description: "Input to command is JSON with tool_name, reason, and tool_use_id.\nExit code 0 - command completes successfully\nOther exit codes - show stderr to user only".into(),
            matcher_metadata: Some(MatcherMetadata {
                field_to_match: "tool_name".into(),
                values: Vec::new(),
            }),
        },
    );

    m.insert(
        HookEvent::InstructionsLoaded,
        HookEventMetadata {
            summary: "When AGENTS.md (or CLAUDE.md fallback) instructions are loaded".into(),
            description: "Input to command is JSON with file_path and instruction_count.\nExit code 0 - command completes successfully\nOther exit codes - show stderr to user only".into(),
            matcher_metadata: Some(MatcherMetadata {
                field_to_match: "file_path".into(),
                values: Vec::new(),
            }),
        },
    );

    m.insert(
        HookEvent::TeammateIdle,
        HookEventMetadata {
            summary: "When teammate is detected as idle".into(),
            description: "Input to command is JSON with teammate_id and idle_duration.\nExit code 0 - command completes successfully\nOther exit codes - show stderr to user only".into(),
            matcher_metadata: Some(MatcherMetadata {
                field_to_match: "teammate_id".into(),
                values: Vec::new(),
            }),
        },
    );

    m.insert(
        HookEvent::TaskCreated,
        HookEventMetadata {
            summary: "When a task is created".into(),
            description: "Input to command is JSON with task_id, title, and description.\nExit code 0 - command completes successfully\nOther exit codes - show stderr to user only".into(),
            matcher_metadata: Some(MatcherMetadata {
                field_to_match: "task_id".into(),
                values: Vec::new(),
            }),
        },
    );

    m.insert(
        HookEvent::TaskCompleted,
        HookEventMetadata {
            summary: "When a task is completed".into(),
            description: "Input to command is JSON with task_id and result.\nExit code 0 - command completes successfully\nOther exit codes - show stderr to user only".into(),
            matcher_metadata: Some(MatcherMetadata {
                field_to_match: "task_id".into(),
                values: Vec::new(),
            }),
        },
    );

    m.insert(
        HookEvent::Elicitation,
        HookEventMetadata {
            summary: "When user elicitation is triggered".into(),
            description: "Input to command is JSON with elicitation_type and prompt.\nExit code 0 - command completes successfully\nOther exit codes - show stderr to user only".into(),
            matcher_metadata: Some(MatcherMetadata {
                field_to_match: "elicitation_type".into(),
                values: Vec::new(),
            }),
        },
    );

    m.insert(
        HookEvent::ElicitationResult,
        HookEventMetadata {
            summary: "When elicitation result is received".into(),
            description: "Input to command is JSON with elicitation_type and response.\nExit code 0 - command completes successfully\nOther exit codes - show stderr to user only".into(),
            matcher_metadata: Some(MatcherMetadata {
                field_to_match: "elicitation_type".into(),
                values: Vec::new(),
            }),
        },
    );

    m.insert(
        HookEvent::ConfigChange,
        HookEventMetadata {
            summary: "When configuration changes".into(),
            description: "Input to command is JSON with changed_keys and source.\nExit code 0 - command completes successfully\nOther exit codes - show stderr to user only".into(),
            matcher_metadata: Some(MatcherMetadata {
                field_to_match: "source".into(),
                values: vec!["user".into(), "managed".into(), "project".into()],
            }),
        },
    );

    m.insert(
        HookEvent::Setup,
        HookEventMetadata {
            summary: "Repo setup hooks for init and maintenance".into(),
            description: "Input to command is JSON with trigger (init or maintenance).\nExit code 0 - stdout shown to Claude\nBlocking errors are ignored\nOther exit codes - show stderr to user only".into(),
            matcher_metadata: Some(MatcherMetadata {
                field_to_match: "trigger".into(),
                values: vec!["init".into(), "maintenance".into()],
            }),
        },
    );

    m.insert(
        HookEvent::WorktreeCreate,
        HookEventMetadata {
            summary: "Create an isolated worktree".into(),
            description: "Input to command is JSON with name (suggested worktree slug).\nStdout should contain the absolute path to the created worktree directory.\nExit code 0 - worktree created successfully".into(),
            matcher_metadata: None,
        },
    );

    m.insert(
        HookEvent::WorktreeRemove,
        HookEventMetadata {
            summary: "Remove a previously created worktree".into(),
            description: "Input to command is JSON with worktree_path (absolute path to worktree).\nExit code 0 - worktree removed successfully".into(),
            matcher_metadata: None,
        },
    );

    m.insert(
        HookEvent::CwdChanged,
        HookEventMetadata {
            summary: "After the working directory changes".into(),
            description: "Input to command is JSON with old_cwd and new_cwd.\nCLAUDE_ENV_FILE is set — write bash exports there to apply env.\nHook output can include hookSpecificOutput.watchPaths to register with the FileChanged watcher.".into(),
            matcher_metadata: None,
        },
    );

    m.insert(
        HookEvent::FileChanged,
        HookEventMetadata {
            summary: "When a watched file changes".into(),
            description: "Input to command is JSON with file_path and event (change, add, unlink).\nCLAUDE_ENV_FILE is set — write bash exports there to apply env.".into(),
            matcher_metadata: None,
        },
    );

    m
}

// ---------------------------------------------------------------------------
// Helper: compare hook entries by identity (command/prompt/url content, not timeout)
// ---------------------------------------------------------------------------

/// Check if two HookEntry values are equal in identity (comparing only
/// command/prompt/url content, not timeout or if-condition).
pub fn hook_entries_equal(a: &HookEntry, b: &HookEntry) -> bool {
    match (a, b) {
        (HookEntry::Command { command: ca, .. }, HookEntry::Command { command: cb, .. }) => {
            ca == cb
        }
        (HookEntry::Prompt { prompt: pa, .. }, HookEntry::Prompt { prompt: pb, .. }) => pa == pb,
        (HookEntry::Agent { prompt: pa, .. }, HookEntry::Agent { prompt: pb, .. }) => pa == pb,
        (HookEntry::Http { url: ua, .. }, HookEntry::Http { url: ub, .. }) => ua == ub,
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Display helpers for hook sources
// ---------------------------------------------------------------------------

pub fn hook_source_description(source: HookSource) -> &'static str {
    match source {
        HookSource::UserSettings => "User settings (~/.cc-rust/settings.json)",
        HookSource::ProjectSettings => "Project settings (.claude/settings.json)",
        HookSource::LocalSettings => "Local settings (.claude/settings.local.json)",
        HookSource::PolicySettings => "Policy settings (managed)",
        HookSource::PluginHook => "Plugin hooks (~/.cc-rust/plugins/*/hooks/hooks.json)",
        HookSource::SessionHook => "Session hooks (in-memory, temporary)",
        HookSource::BuiltinHook => "Built-in hooks (registered internally by cc-rust)",
    }
}

pub fn hook_source_header(source: HookSource) -> &'static str {
    match source {
        HookSource::UserSettings => "User Settings",
        HookSource::ProjectSettings => "Project Settings",
        HookSource::LocalSettings => "Local Settings",
        HookSource::PolicySettings => "Policy Settings",
        HookSource::PluginHook => "Plugin Hooks",
        HookSource::SessionHook => "Session Hooks",
        HookSource::BuiltinHook => "Built-in Hooks",
    }
}

pub fn hook_source_inline(source: HookSource) -> &'static str {
    match source {
        HookSource::UserSettings => "User",
        HookSource::ProjectSettings => "Project",
        HookSource::LocalSettings => "Local",
        HookSource::PolicySettings => "Policy",
        HookSource::PluginHook => "Plugin",
        HookSource::SessionHook => "Session",
        HookSource::BuiltinHook => "Built-in",
    }
}

// ---------------------------------------------------------------------------
// HTTP hook policy
// ---------------------------------------------------------------------------

/// Policy for HTTP hook URL allowlisting.
#[derive(Debug, Clone, Default)]
pub struct HttpHookPolicy {
    pub allowed_urls: Option<Vec<String>>,
    pub allowed_env_vars: Option<Vec<String>>,
}

// ---------------------------------------------------------------------------
// Hook result wrapper for file-watcher results
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct HookOutsideReplResult {
    pub succeeded: bool,
    pub output: String,
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use super::*;

    #[test]
    fn matcher_supports_exact_wildcard_and_prefix() {
        assert!(matches_tool(Some("Bash"), "Bash"));
        assert!(!matches_tool(Some("Bash"), "Read"));
        assert!(matches_tool(None, "Read"));
        assert!(matches_tool(Some("*"), "anything_at_all"));
        assert!(matches_tool(Some("mcp__"), "mcp__server__tool"));
        assert!(!matches_tool(Some("mcp__"), "mcp_single_underscore"));
    }

    #[test]
    fn load_hook_configs_preserves_critical_and_default_timeout() {
        let hooks_value = HooksMap::from([(
            "PreToolUse".to_string(),
            json!([
                {
                    "matcher": "Bash",
                    "hooks": [
                        {
                            "type": "command",
                            "command": "echo ok",
                            "timeout": 30
                        }
                    ]
                },
                {
                    "matcher": "*",
                    "critical": true,
                    "hooks": [
                        {
                            "type": "command",
                            "command": "echo audit"
                        }
                    ]
                }
            ]),
        )]);

        let configs = load_hook_configs(&hooks_value, "PreToolUse");
        assert_eq!(configs.len(), 2);
        assert_eq!(configs[0].matcher.as_deref(), Some("Bash"));
        assert!(!configs[0].critical);
        assert_eq!(configs[1].matcher.as_deref(), Some("*"));
        assert!(configs[1].critical);

        match &configs[1].hooks[0] {
            HookEntry::Command {
                command, timeout, ..
            } => {
                assert_eq!(command, "echo audit");
                assert_eq!(*timeout, 60);
            }
            _ => panic!("expected Command variant"),
        }
    }

    #[test]
    fn load_hook_configs_ignores_missing_or_invalid_event() {
        let hooks_value = HooksMap::from([("PreToolUse".to_string(), Value::String("bad".into()))]);

        assert!(load_hook_configs(&hooks_value, "PreToolUse").is_empty());
        assert!(load_hook_configs(&hooks_value, "PostToolUse").is_empty());
    }

    #[test]
    fn hook_event_display() {
        assert_eq!(HookEvent::PreToolUse.to_string(), "PreToolUse");
        assert_eq!(HookEvent::Stop.to_string(), "Stop");
        assert_eq!(HookEvent::FileChanged.to_string(), "FileChanged");
    }

    #[test]
    fn hook_source_display() {
        assert_eq!(HookSource::UserSettings.to_string(), "userSettings");
        assert_eq!(HookSource::SessionHook.to_string(), "sessionHook");
    }

    #[test]
    fn hook_event_metadata_contains_all_events() {
        let metadata = get_hook_event_metadata();
        for event in HOOK_EVENTS {
            assert!(metadata.contains_key(event), "Missing metadata for {event}");
        }
    }
}
