use super::tool::{PermissionMode, ToolPermissionContext};
use std::collections::{HashMap, HashSet};
use std::convert::TryFrom;

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

impl TryFrom<&allthecodes_config::settings::EffectiveSettings> for AppState {
    type Error = anyhow::Error;

    fn try_from(
        settings: &allthecodes_config::settings::EffectiveSettings,
    ) -> Result<Self, Self::Error> {
        let mode = PermissionMode::parse_configured(settings.permission_mode.as_deref())?;
        let mut tool_permission_context = ToolPermissionContext {
            mode,
            additional_working_directories: settings
                .permissions
                .additional_directories
                .iter()
                .map(|dir| {
                    (
                        dir.clone(),
                        allthecodes_tools::tool::AdditionalWorkingDirectory {
                            path: dir.clone(),
                            read_only: false,
                        },
                    )
                })
                .collect(),
            always_allow_rules: rules_by_source("settings", &settings.permissions.allow),
            always_deny_rules: rules_by_source("settings", &settings.permissions.deny),
            always_ask_rules: rules_by_source("settings", &settings.permissions.ask),
            session_allow_rules: HashMap::new(),
            auto_mode_stripped_always_allow_rules: Vec::new(),
            auto_mode_stripped_session_allow_rules: Vec::new(),
            is_bypass_permissions_mode_available: settings
                .permissions
                .enable_bypass_mode
                .unwrap_or(true),
            is_auto_mode_available: Some(settings.permissions.enable_auto_mode.unwrap_or(true)),
            pre_plan_mode: None,
        };
        let mode = tool_permission_context.mode.clone();
        allthecodes_permissions::dangerous::set_permission_mode_with_auto_mode_safety(
            &mut tool_permission_context,
            mode,
        );

        let mut app_state = Self {
            settings: SettingsJson::from_effective(settings, Default::default()),
            verbose: settings.verbose,
            main_loop_model: settings
                .model
                .clone()
                .unwrap_or_else(allthecodes_types::models::default_model_id),
            main_loop_backend: settings
                .backend
                .clone()
                .unwrap_or_else(|| "native".to_string()),
            advisor_model: settings.advisor_model.clone(),
            tool_permission_context,
            thinking_enabled: settings_thinking_enabled(settings),
            fast_mode: settings.fast_mode.unwrap_or(false),
            effort_value: settings_effort_value(settings),
            hooks: settings.hooks.clone(),
            ..Default::default()
        };
        app_state.settings.model = Some(app_state.main_loop_model.clone());
        app_state.settings.backend = Some(app_state.main_loop_backend.clone());
        Ok(app_state)
    }
}

fn rules_by_source(source: &str, rules: &[String]) -> HashMap<String, Vec<String>> {
    if rules.is_empty() {
        HashMap::new()
    } else {
        HashMap::from([(source.to_string(), rules.to_vec())])
    }
}

fn settings_thinking_enabled(
    settings: &allthecodes_config::settings::EffectiveSettings,
) -> Option<bool> {
    let thinking = settings.thinking.as_ref()?;
    if let Some(enabled) = thinking.as_bool() {
        return Some(enabled);
    }
    let kind = thinking
        .get("type")
        .and_then(serde_json::Value::as_str)
        .or_else(|| thinking.as_str())?
        .trim()
        .to_ascii_lowercase();
    match kind.as_str() {
        "enabled" | "adaptive" => Some(true),
        "disabled" => Some(false),
        _ => None,
    }
}

fn settings_effort_value(
    settings: &allthecodes_config::settings::EffectiveSettings,
) -> Option<String> {
    settings
        .output_config
        .as_ref()
        .and_then(|value| {
            value
                .get("effort")
                .and_then(crate::effort::normalize_output_effort_json)
        })
        .or_else(|| settings.effort_level.clone())
}

impl From<&AppState> for allthecodes_tools::tool::ToolAppState {
    fn from(state: &AppState) -> Self {
        Self {
            settings: state.settings.clone(),
            verbose: state.verbose,
            main_loop_model: state.main_loop_model.clone(),
            main_loop_backend: state.main_loop_backend.clone(),
            advisor_model: state.advisor_model.clone(),
            tool_permission_context: state.tool_permission_context.clone(),
            thinking_enabled: state.thinking_enabled,
            fast_mode: state.fast_mode,
            effort_value: state.effort_value.clone(),
            team_context: state.team_context.clone(),
            hooks: state.hooks.clone(),
            plan_workflow: state.plan_workflow.clone(),
            surfaced_memory_keys: state.surfaced_memory_keys.clone(),
            kairos_active: state.kairos_active,
            is_brief_only: state.is_brief_only,
            is_assistant_mode: state.is_assistant_mode,
            autonomous_tick_ms: state.autonomous_tick_ms,
            terminal_focus: state.terminal_focus,
        }
    }
}

impl AppState {
    pub fn to_tool_app_state(&self) -> allthecodes_tools::tool::ToolAppState {
        allthecodes_tools::tool::ToolAppState::from(self)
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

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_config::settings::{EffectiveSettings, PermissionsSettings};
    use allthecodes_tools::tool::ToolAppState;
    use serde_json::json;

    #[test]
    fn app_state_projection_effective_settings_project_to_app_and_tool_state() {
        let effective = EffectiveSettings {
            model: Some("claude-opus-4-20250514".to_string()),
            backend: Some("codex".to_string()),
            permission_mode: Some("auto".to_string()),
            permissions: PermissionsSettings {
                default_mode: Some("auto".to_string()),
                allow: vec!["Bash(cargo test*)".to_string()],
                ..Default::default()
            },
            env: HashMap::from([(
                "ANTHROPIC_MODEL".to_string(),
                "claude-opus-4-20250514".to_string(),
            )]),
            fast_mode: Some(true),
            advisor_model: Some("advisor-pro".to_string()),
            thinking: Some(json!({ "type": "enabled" })),
            output_config: Some(json!({ "effort": "max" })),
            hooks: HashMap::from([("PreToolUse".to_string(), json!([]))]),
            ..Default::default()
        };

        let app = AppState::try_from(&effective).expect("effective settings project to app state");
        assert_eq!(app.main_loop_model, "claude-opus-4-20250514");
        assert_eq!(app.main_loop_backend, "codex");
        assert_eq!(
            app.settings.env.get("ANTHROPIC_MODEL").map(String::as_str),
            Some("claude-opus-4-20250514")
        );
        assert_eq!(app.settings.permission_mode.as_deref(), Some("auto"));
        assert!(app.fast_mode);
        assert_eq!(app.advisor_model.as_deref(), Some("advisor-pro"));
        assert!(app.hooks.contains_key("PreToolUse"));

        let tool = ToolAppState::from(&app);
        assert_eq!(tool.main_loop_model, app.main_loop_model);
        assert_eq!(tool.main_loop_backend, app.main_loop_backend);
        assert_eq!(
            tool.settings.env.get("ANTHROPIC_MODEL").map(String::as_str),
            Some("claude-opus-4-20250514")
        );
        assert_eq!(tool.settings.permission_mode.as_deref(), Some("auto"));
        assert!(tool.fast_mode);
        assert_eq!(tool.advisor_model.as_deref(), Some("advisor-pro"));
    }

    #[test]
    fn app_state_projection_tool_app_state_roundtrip_preserves_mutable_fields() {
        let mut app = AppState {
            main_loop_model: "initial-model".to_string(),
            settings: SettingsJson {
                language: Some("en-US".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };

        let mut tool = ToolAppState::from(&app);
        tool.settings.language = Some("zh-CN".to_string());
        tool.main_loop_model = "mutated-model".to_string();
        tool.fast_mode = true;
        tool.is_brief_only = true;
        tool.is_assistant_mode = true;
        tool.autonomous_tick_ms = Some(2500);
        tool.terminal_focus = false;
        tool.tool_permission_context.mode = PermissionMode::Plan;
        tool.surfaced_memory_keys
            .insert("project:memory-key".to_string());

        app.apply_tool_app_state(tool);
        let roundtrip = ToolAppState::from(&app);

        assert_eq!(roundtrip.settings.language.as_deref(), Some("zh-CN"));
        assert_eq!(roundtrip.main_loop_model, "mutated-model");
        assert!(roundtrip.fast_mode);
        assert!(roundtrip.is_brief_only);
        assert!(roundtrip.is_assistant_mode);
        assert_eq!(roundtrip.autonomous_tick_ms, Some(2500));
        assert!(!roundtrip.terminal_focus);
        assert_eq!(roundtrip.tool_permission_context.mode, PermissionMode::Plan);
        assert!(roundtrip
            .surfaced_memory_keys
            .contains("project:memory-key"));
    }
}
