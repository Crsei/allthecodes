use super::tool::{PermissionMode, ToolPermissionContext};
use std::collections::{HashMap, HashSet};

/// Runtime settings projection — moved to `cc-config` in Phase 3 (issue #72).
///
/// Re-exported here so existing `crate::types::app_state::SettingsJson`
/// call sites keep compiling.
pub use allthecodes_config::runtime_settings::SettingsJson;

/// 应用全局状态 (简化版)
///
/// 对应 TypeScript: state/AppState.ts
/// 在 TypeScript 中通过 React context + DeepImmutable 管理
/// 在 Rust 中通过 Arc<RwLock<AppState>> 管理
#[derive(Debug, Clone)]
pub struct AppState {
    /// 当前设置
    pub settings: SettingsJson,
    /// 详细模式
    pub verbose: bool,
    /// 主循环模型
    pub main_loop_model: String,
    /// Active backend implementation ("native" or "codex").
    pub main_loop_backend: String,
    /// Optional advisor model (issue #33).
    ///
    /// When `Some`, and the current provider supports advisors (see
    /// `provider_supports_advisor` in `api::client`), the advisor model id
    /// is attached to every outbound [`crate::api::client::MessagesRequest`].
    /// Other providers log a warning and ignore the setting.
    pub advisor_model: Option<String>,
    /// 工具权限上下文
    pub tool_permission_context: ToolPermissionContext,
    /// thinking 是否启用
    pub thinking_enabled: Option<bool>,
    /// 快速模式
    pub fast_mode: bool,
    /// effort 值
    pub effort_value: Option<String>,
    /// Agent Teams 上下文 (feature-gated)
    pub team_context: Option<allthecodes_types::teams::TeamContext>,
    /// Hook configurations loaded from settings.json (merged config).
    /// Read by `allthecodes_tools::hooks::load_hook_configs()` and the hook execution pipeline.
    pub hooks: HashMap<String, serde_json::Value>,
    /// Durable plan-mode workflow state.
    ///
    /// This is distinct from `tool_permission_context.mode == Plan`: the
    /// permission mode gates tool execution, while this record tracks approval
    /// state, plan artifact path, and implementation evidence.
    pub plan_workflow: Option<allthecodes_types::plan_workflow::PlanWorkflowRecord>,
    /// Memory identities already surfaced by relevant-memory recall in this
    /// session. Stored as `<scope>:<key>` to avoid repeating the same recall.
    pub surfaced_memory_keys: HashSet<String>,
    /// Whether KAIROS daemon mode is running
    pub kairos_active: bool,
    /// Whether output is routed through BriefTool only
    pub is_brief_only: bool,
    /// Perpetual session mode
    pub is_assistant_mode: bool,
    /// Proactive tick interval (None = disabled)
    pub autonomous_tick_ms: Option<u64>,
    /// Whether user is looking at terminal (affects autonomy level)
    pub terminal_focus: bool,
    /// Shared keybinding registry (default + user, with hot reload).
    ///
    /// Populated at startup from `~/.allthecodes/keybindings.json` (issue #10).
    /// Multiple UI surfaces (Rust TUI, IPC-driven OpenTUI) share the same
    /// handle so reloads are observed everywhere.
    pub keybindings: allthecodes_keybindings::KeybindingRegistry,
    /// Shared scriptable status-line runner (issue #11). The TUI owns the
    /// renderer-facing side; `/statusline` and the IPC driver both reach
    /// into this handle to inspect / reset the subprocess.
    pub status_line_runner: crate::status_line::StatusLineRunner,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            settings: SettingsJson::default(),
            verbose: false,
            main_loop_model: allthecodes_types::models::default_model_id(),
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
            keybindings: allthecodes_keybindings::KeybindingRegistry::with_defaults(),
            status_line_runner: crate::status_line::StatusLineRunner::new(),
        }
    }
}

impl AppState {
    pub fn to_tool_app_state(&self) -> allthecodes_tools::tool::ToolAppState {
        allthecodes_tools::tool::ToolAppState {
            settings: self.settings.clone(),
            verbose: self.verbose,
            main_loop_model: self.main_loop_model.clone(),
            main_loop_backend: self.main_loop_backend.clone(),
            advisor_model: self.advisor_model.clone(),
            tool_permission_context: self.tool_permission_context.clone(),
            thinking_enabled: self.thinking_enabled,
            fast_mode: self.fast_mode,
            effort_value: self.effort_value.clone(),
            team_context: self.team_context.clone(),
            hooks: self.hooks.clone(),
            plan_workflow: self.plan_workflow.clone(),
            surfaced_memory_keys: self.surfaced_memory_keys.clone(),
            kairos_active: self.kairos_active,
            is_brief_only: self.is_brief_only,
            is_assistant_mode: self.is_assistant_mode,
            autonomous_tick_ms: self.autonomous_tick_ms,
            terminal_focus: self.terminal_focus,
        }
    }

    pub fn apply_tool_app_state(&mut self, state: allthecodes_tools::tool::ToolAppState) {
        self.settings = state.settings;
        self.verbose = state.verbose;
        self.main_loop_model = state.main_loop_model;
        self.main_loop_backend = state.main_loop_backend;
        self.advisor_model = state.advisor_model;
        self.tool_permission_context = state.tool_permission_context;
        self.thinking_enabled = state.thinking_enabled;
        self.fast_mode = state.fast_mode;
        self.effort_value = state.effort_value;
        self.team_context = state.team_context;
        self.hooks = state.hooks;
        self.plan_workflow = state.plan_workflow;
        self.surfaced_memory_keys = state.surfaced_memory_keys;
        self.kairos_active = state.kairos_active;
        self.is_brief_only = state.is_brief_only;
        self.is_assistant_mode = state.is_assistant_mode;
        self.autonomous_tick_ms = state.autonomous_tick_ms;
        self.terminal_focus = state.terminal_focus;
    }
}
