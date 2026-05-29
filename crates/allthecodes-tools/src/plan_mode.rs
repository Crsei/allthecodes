//! PlanMode tools — EnterPlanMode / ExitPlanMode.
//!
//! Corresponds to TypeScript:
//!   src/tools/EnterPlanModeTool/EnterPlanModeTool.ts
//!   src/tools/ExitPlanModeTool/ExitPlanModeV2Tool.ts
//!
//! Plan mode switches the permission context to read-only, preventing
//! any write operations.  The model explores the codebase, designs an
//! approach, and then calls ExitPlanMode to present the plan for approval.
//!
//! State lifecycle:
//!   1. EnterPlanMode saves `pre_plan_mode` and sets mode → Plan
//!   2. Model explores (read-only tools only)
//!   3. ExitPlanMode restores mode from `pre_plan_mode`

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use allthecodes_types::message::AssistantMessage;

use crate::tool::*;

// ---------------------------------------------------------------------------
// EnterPlanMode
// ---------------------------------------------------------------------------

/// EnterPlanMode — switch the session to read-only planning mode.
pub struct EnterPlanModeTool;

#[async_trait]
impl Tool for EnterPlanModeTool {
    fn name(&self) -> &str {
        "EnterPlanMode"
    }

    async fn description(&self, _input: &Value) -> String {
        "Enter plan mode for complex tasks requiring exploration and design before coding."
            .to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    async fn validate_input(&self, _input: &Value, ctx: &ToolUseContext) -> ValidationResult {
        // Cannot enter plan mode from agent context
        if ctx.agent_id.is_some() {
            return ValidationResult::Error {
                message: "EnterPlanMode cannot be used in agent contexts.".to_string(),
                error_code: 1,
            };
        }

        // Already in plan mode?
        let state = (ctx.get_app_state)();
        if state.tool_permission_context.mode == PermissionMode::Plan {
            return ValidationResult::Error {
                message: "Already in plan mode.".to_string(),
                error_code: 2,
            };
        }

        ValidationResult::Ok
    }

    async fn call(
        &self,
        _input: Value,
        ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let record = mutate_plan_workflow(ctx, plan_cwd(), |state, cwd, existing| {
            crate::plan_workflow::enter_plan_mode_state(
                state, cwd, existing, "main", "tool", None, None,
            )
        })?;

        let instructions = concat!(
            "Entered plan mode. You should now focus on exploring the codebase ",
            "and designing an implementation approach.\n\n",
            "In plan mode, you should:\n",
            "1. Thoroughly explore the codebase to understand existing patterns\n",
            "2. Identify similar features and architectural approaches\n",
            "3. Consider multiple approaches and their trade-offs\n",
            "4. Use AskUserQuestion if you need to clarify the approach\n",
            "5. Design a concrete implementation strategy\n",
            "6. When ready, use ExitPlanMode to present your plan for approval\n\n",
            "Remember: DO NOT write or edit any files yet. ",
            "This is a read-only exploration and planning phase.",
        );

        Ok(ToolResult {
            data: json!({
                "message": instructions,
                "plan_workflow": record,
            }),
            new_messages: vec![],
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        concat!(
            "Enter plan mode for complex tasks requiring exploration and design. ",
            "In plan mode, only read-only tools (Read, Glob, Grep, LSP) are allowed. ",
            "Use this before implementing complex features to ensure a thorough ",
            "understanding of the codebase and a well-designed approach.",
        )
        .to_string()
    }

    fn user_facing_name(&self, _input: Option<&Value>) -> String {
        String::new() // hidden from tool-use display
    }
}

// ---------------------------------------------------------------------------
// ExitPlanMode
// ---------------------------------------------------------------------------

/// ExitPlanMode — present the plan for approval and exit plan mode.
pub struct ExitPlanModeTool;

#[async_trait]
impl Tool for ExitPlanModeTool {
    fn name(&self) -> &str {
        "ExitPlanMode"
    }

    async fn description(&self, _input: &Value) -> String {
        "Exit plan mode and begin implementation.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "plan": {
                    "type": "string",
                    "description": "The implementation plan (optional — can also be written to a plan file)"
                },
                "allowedPrompts": {
                    "type": "array",
                    "description": "Prompt-based permissions requested by the plan. allthecodes maps explicit Bash patterns and common verification prompts conservatively to transient session allow rules.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "tool": {
                                "type": "string",
                                "enum": ["Bash"],
                                "description": "The tool this prompt applies to"
                            },
                            "prompt": {
                                "type": "string",
                                "description": "Bash permission pattern such as 'cargo test*' or 'prefix:cargo', or a common verification prompt such as 'run tests'"
                            }
                        },
                        "required": ["tool", "prompt"],
                        "additionalProperties": false
                    }
                }
            },
            "additionalProperties": false
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    async fn validate_input(&self, input: &Value, ctx: &ToolUseContext) -> ValidationResult {
        let state = (ctx.get_app_state)();
        if state.tool_permission_context.mode != PermissionMode::Plan {
            return ValidationResult::Error {
                message: concat!(
                    "You are not in plan mode. This tool is only for exiting plan mode ",
                    "after writing a plan. If your plan was already approved, continue ",
                    "with implementation.",
                )
                .to_string(),
                error_code: 1,
            };
        }
        if let Err(message) = allowed_prompt_rules(input) {
            return ValidationResult::Error {
                message,
                error_code: 2,
            };
        }
        ValidationResult::Ok
    }

    async fn check_permissions(&self, input: &Value, ctx: &ToolUseContext) -> PermissionResult {
        let plan = input
            .get("plan")
            .and_then(|v| v.as_str())
            .map(str::to_string);

        let derived_rules = match allowed_prompt_rules(input) {
            Ok(rules) => rules,
            Err(message) => {
                return PermissionResult::Deny { message };
            }
        };

        match mutate_plan_workflow(ctx, plan_cwd(), move |state, cwd, existing| {
            crate::plan_workflow::request_approval_state(state, cwd, existing, "main", "tool", plan)
        }) {
            Ok(record) => PermissionResult::Ask {
                message: if derived_rules.is_empty() {
                    format!(
                        "Approve plan {} and exit plan mode to begin implementation?",
                        record.id
                    )
                } else {
                    let rules = derived_rules
                        .into_iter()
                        .map(|rule| format!("- {rule}"))
                        .collect::<Vec<_>>()
                        .join("\n");
                    format!(
                        "Approve plan {} and exit plan mode to begin implementation?\n\nTransient allowed prompt rules:\n{}",
                        record.id, rules
                    )
                },
            },
            Err(err) => PermissionResult::Deny {
                message: format!("Unable to persist plan approval request: {err}"),
            },
        }
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let plan = input
            .get("plan")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let allowed_prompt_rules =
            allowed_prompt_rules(&input).map_err(|message| anyhow::anyhow!(message))?;
        let auto_mode_stripped_allowed_prompt_rules = Arc::new(Mutex::new(0usize));

        let record = mutate_plan_workflow(ctx, plan_cwd(), {
            let plan_for_record = (!plan.is_empty()).then(|| plan.clone());
            let rules_for_state = allowed_prompt_rules.clone();
            let stripped_count = Arc::clone(&auto_mode_stripped_allowed_prompt_rules);
            move |state, cwd, existing| {
                let record = crate::plan_workflow::approve_and_exit_state(
                    state,
                    cwd,
                    existing,
                    "main",
                    "tool",
                    plan_for_record,
                );
                if !rules_for_state.is_empty() {
                    state
                        .tool_permission_context
                        .session_allow_rules
                        .entry("plan_allowed_prompts".into())
                        .or_default()
                        .extend(rules_for_state);
                    let transition =
                        allthecodes_permissions::dangerous::strip_dangerous_permissions_for_active_auto_mode(
                            &mut state.tool_permission_context,
                        );
                    *stripped_count
                        .lock()
                        .expect("auto mode stripped allowed prompt count poisoned") =
                        transition.stripped_session_allow_count;
                }
                record
            }
        })?;

        let mut result = json!({
            "message": "Exited plan mode. Normal operations restored. You may now implement the plan.",
            "plan_workflow": record,
        });

        if !plan.is_empty() {
            result["plan"] = json!(plan);
        }
        if !allowed_prompt_rules.is_empty() {
            result["allowed_prompt_rules"] = json!(allowed_prompt_rules);
            let stripped_count = *auto_mode_stripped_allowed_prompt_rules
                .lock()
                .expect("auto mode stripped allowed prompt count poisoned");
            if stripped_count > 0 {
                result["auto_mode_stripped_allowed_prompt_rules"] = json!(stripped_count);
            }
        }

        Ok(ToolResult {
            data: result,
            new_messages: vec![],
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        concat!(
            "Exit plan mode after designing your implementation approach. ",
            "Optionally include the plan text. The user will be asked to ",
            "approve before normal operations resume.",
        )
        .to_string()
    }

    fn user_facing_name(&self, _input: Option<&Value>) -> String {
        String::new() // hidden from tool-use display
    }
}

fn allowed_prompt_rules(input: &Value) -> std::result::Result<Vec<String>, String> {
    let Some(prompts) = input.get("allowedPrompts") else {
        return Ok(Vec::new());
    };
    let prompts = prompts
        .as_array()
        .ok_or_else(|| "allowedPrompts must be an array.".to_string())?;

    let mut rules = Vec::new();
    for (idx, prompt) in prompts.iter().enumerate() {
        let tool = prompt
            .get("tool")
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("allowedPrompts[{idx}].tool must be a string."))?;
        if tool != "Bash" {
            return Err(format!(
                "allowedPrompts[{idx}].tool '{tool}' is not supported; only Bash is supported."
            ));
        }
        let pattern = prompt
            .get("prompt")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| format!("allowedPrompts[{idx}].prompt must be a non-empty string."))?;
        if pattern.contains('\n') || pattern.contains('\r') {
            return Err(format!(
                "allowedPrompts[{idx}].prompt contains unsupported line breaks."
            ));
        }
        let normalized_pattern = pattern.to_ascii_lowercase();
        if has_negative_intent(&normalized_pattern) && !looks_like_bash_permission_pattern(pattern)
        {
            return Err(format!(
                "allowedPrompts[{idx}].prompt describes a negative permission intent and cannot be turned into an allow rule."
            ));
        }
        let derived_rules = classify_allowed_prompt(pattern);
        if derived_rules.is_empty() {
            if pattern.contains(')') {
                return Err(format!(
                    "allowedPrompts[{idx}].prompt contains unsupported characters for a Bash permission pattern."
                ));
            }
            push_unique_rule(&mut rules, format!("Bash({pattern})"));
        } else {
            for rule in derived_rules {
                push_unique_rule(&mut rules, rule);
            }
        }
    }
    Ok(rules)
}

fn classify_allowed_prompt(prompt: &str) -> Vec<String> {
    let trimmed = prompt.trim();
    if looks_like_bash_permission_pattern(trimmed) {
        return vec![format!("Bash({trimmed})")];
    }

    let normalized = trimmed.to_ascii_lowercase();
    if has_negative_intent(&normalized) {
        return Vec::new();
    }

    let mut rules = Vec::new();
    if contains_any(
        &normalized,
        &["test", "tests", "unit test", "integration test"],
    ) {
        push_unique_rule(&mut rules, "Bash(cargo test*)".to_string());
    }
    if contains_any(&normalized, &["lint", "clippy"]) {
        push_unique_rule(&mut rules, "Bash(cargo clippy*)".to_string());
    }
    if contains_any(
        &normalized,
        &["typecheck", "type check", "cargo check", "check build"],
    ) {
        push_unique_rule(&mut rules, "Bash(cargo check*)".to_string());
    }
    if contains_any(&normalized, &["build", "compile"]) {
        push_unique_rule(&mut rules, "Bash(cargo build*)".to_string());
    }
    if contains_any(&normalized, &["fmt", "format", "formatting"]) {
        if contains_any(&normalized, &["check", "verify", "ci"]) {
            push_unique_rule(&mut rules, "Bash(cargo fmt --check*)".to_string());
        } else {
            push_unique_rule(&mut rules, "Bash(cargo fmt*)".to_string());
        }
    }
    if contains_any(&normalized, &["all checks", "ci checks", "verify changes"]) {
        push_unique_rule(&mut rules, "Bash(cargo test*)".to_string());
        push_unique_rule(&mut rules, "Bash(cargo clippy*)".to_string());
        push_unique_rule(&mut rules, "Bash(cargo fmt --check*)".to_string());
    }

    rules
}

fn looks_like_bash_permission_pattern(prompt: &str) -> bool {
    let normalized = prompt.trim_start().to_ascii_lowercase();
    normalized.starts_with("prefix:")
        || prompt.contains('*')
        || [
            "cargo", "npm", "pnpm", "yarn", "bun", "uv", "python", "pytest",
        ]
        .iter()
        .any(|command| normalized == *command || normalized.starts_with(&format!("{command} ")))
}

fn has_negative_intent(normalized: &str) -> bool {
    contains_any(normalized, &["do not", "don't", "dont", "never", "without"])
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn push_unique_rule(rules: &mut Vec<String>, rule: String) {
    if !rules.iter().any(|existing| existing == &rule) {
        rules.push(rule);
    }
}

fn plan_cwd() -> PathBuf {
    if let Ok(cwd) = std::env::current_dir() {
        if !cwd.as_os_str().is_empty() {
            return cwd;
        }
    }

    #[cfg(test)]
    let fallback = std::env::temp_dir()
        .join("allthecodes-plan-workflow")
        .join(plan_test_thread_dir());
    #[cfg(not(test))]
    let fallback = std::env::temp_dir().join("allthecodes-plan-workflow");
    let _ = std::fs::create_dir_all(fallback.join(".allthecodes"));
    fallback
}

#[cfg(test)]
fn plan_test_thread_dir() -> String {
    std::thread::current()
        .name()
        .unwrap_or("unnamed")
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn mutate_plan_workflow<F>(
    ctx: &ToolUseContext,
    cwd: PathBuf,
    f: F,
) -> Result<allthecodes_types::plan_workflow::PlanWorkflowRecord>
where
    F: FnOnce(
            &mut crate::tool::ToolAppState,
            &Path,
            Option<allthecodes_types::plan_workflow::PlanWorkflowRecord>,
        ) -> allthecodes_types::plan_workflow::PlanWorkflowRecord
        + 'static,
{
    let existing = crate::plan_workflow::load(&cwd)?;
    let slot: Arc<Mutex<Option<allthecodes_types::plan_workflow::PlanWorkflowRecord>>> =
        Arc::new(Mutex::new(None));
    let slot_for_update = Arc::clone(&slot);
    let persist_cwd = cwd.clone();

    (ctx.set_app_state)(Box::new(move |mut state| {
        let record = f(&mut state, &cwd, existing);
        *slot_for_update.lock().expect("plan workflow slot poisoned") = Some(record);
        state
    }));

    let record = slot
        .lock()
        .expect("plan workflow slot poisoned")
        .clone()
        .expect("plan workflow update should set record");
    crate::plan_workflow::persist(&persist_cwd, &record)?;
    Ok(record)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::ToolAppState as AppState;
    use parking_lot::RwLock;
    use std::sync::Arc;

    fn make_ctx(state: Arc<RwLock<AppState>>) -> ToolUseContext {
        let state_r = Arc::clone(&state);
        let state_w = Arc::clone(&state);

        ToolUseContext {
            options: ToolUseOptions {
                debug: false,
                main_loop_model: "test".to_string(),
                verbose: false,
                is_non_interactive_session: false,
                custom_system_prompt: None,
                append_system_prompt: None,
                max_budget_usd: None,
            },
            abort_signal: tokio::sync::watch::channel(false).1,
            read_file_state: FileStateCache::default(),
            get_app_state: Arc::new(move || state_r.read().clone()),
            set_app_state: Arc::new(move |f: Box<dyn FnOnce(AppState) -> AppState>| {
                let mut s = state_w.write();
                let old = s.clone();
                *s = f(old);
            }),
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

    struct PlanCwdGuard {
        previous: std::path::PathBuf,
        _dir: tempfile::TempDir,
    }

    impl PlanCwdGuard {
        fn new() -> Self {
            let dir = tempfile::tempdir().expect("tempdir");
            std::fs::create_dir_all(dir.path().join(".allthecodes"))
                .expect("create project config dir");
            let previous = std::env::current_dir().expect("read current dir");
            std::env::set_current_dir(dir.path()).expect("set current dir");
            Self {
                previous,
                _dir: dir,
            }
        }
    }

    impl Drop for PlanCwdGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.previous);
        }
    }

    #[test]
    fn test_enter_plan_mode_name() {
        let tool = EnterPlanModeTool;
        assert_eq!(tool.name(), "EnterPlanMode");
        assert!(tool.is_read_only(&json!({})));
        assert!(tool.is_concurrency_safe(&json!({})));
    }

    #[test]
    fn test_exit_plan_mode_name() {
        let tool = ExitPlanModeTool;
        assert_eq!(tool.name(), "ExitPlanMode");
    }

    #[tokio::test]
    async fn test_enter_plan_mode_blocks_agent() {
        let tool = EnterPlanModeTool;
        let state = Arc::new(RwLock::new(AppState::default()));
        let mut ctx = make_ctx(state);
        ctx.agent_id = Some("agent-1".to_string());

        let result = tool.validate_input(&json!({}), &ctx).await;
        assert!(matches!(
            result,
            ValidationResult::Error { error_code: 1, .. }
        ));
    }

    #[tokio::test]
    async fn test_enter_plan_mode_blocks_if_already_plan() {
        let tool = EnterPlanModeTool;
        let state = Arc::new(RwLock::new(AppState::default()));
        {
            let mut s = state.write();
            s.tool_permission_context.mode = PermissionMode::Plan;
        }
        let ctx = make_ctx(state);

        let result = tool.validate_input(&json!({}), &ctx).await;
        assert!(matches!(
            result,
            ValidationResult::Error { error_code: 2, .. }
        ));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_enter_exit_plan_mode_roundtrip() {
        let _cwd_guard = PlanCwdGuard::new();
        let state = Arc::new(RwLock::new(AppState::default()));
        let dummy_msg = AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 0,
            role: "assistant".to_string(),
            content: vec![],
            usage: None,
            stop_reason: None,
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        };

        // Enter plan mode
        let enter_tool = EnterPlanModeTool;
        let ctx = make_ctx(Arc::clone(&state));
        let validation = enter_tool.validate_input(&json!({}), &ctx).await;
        assert!(matches!(validation, ValidationResult::Ok));

        let result = enter_tool
            .call(json!({}), &ctx, &dummy_msg, None)
            .await
            .unwrap();
        assert!(result.data["message"]
            .as_str()
            .unwrap()
            .contains("plan mode"));

        // Verify state changed
        {
            let s = state.read();
            assert_eq!(s.tool_permission_context.mode, PermissionMode::Plan);
            assert_eq!(
                s.tool_permission_context.pre_plan_mode,
                Some(PermissionMode::Default)
            );
        }

        // Exit plan mode
        let exit_tool = ExitPlanModeTool;
        let ctx2 = make_ctx(Arc::clone(&state));
        let validation = exit_tool.validate_input(&json!({}), &ctx2).await;
        assert!(matches!(validation, ValidationResult::Ok));

        let result = exit_tool
            .call(json!({"plan": "My plan here"}), &ctx2, &dummy_msg, None)
            .await
            .unwrap();
        assert!(result.data["message"].as_str().unwrap().contains("Exited"));
        assert_eq!(result.data["plan"].as_str().unwrap(), "My plan here");

        // Verify state restored
        {
            let s = state.read();
            assert_eq!(s.tool_permission_context.mode, PermissionMode::Default);
            assert!(s.tool_permission_context.pre_plan_mode.is_none());
        }
    }

    #[tokio::test]
    async fn test_exit_plan_mode_rejects_outside_plan() {
        let tool = ExitPlanModeTool;
        let state = Arc::new(RwLock::new(AppState::default()));
        let ctx = make_ctx(state);

        let result = tool.validate_input(&json!({}), &ctx).await;
        assert!(matches!(
            result,
            ValidationResult::Error { error_code: 1, .. }
        ));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_exit_plan_mode_restores_auto() {
        let _cwd_guard = PlanCwdGuard::new();
        let state = Arc::new(RwLock::new(AppState::default()));
        {
            let mut s = state.write();
            s.tool_permission_context.mode = PermissionMode::Plan;
            s.tool_permission_context.pre_plan_mode = Some(PermissionMode::Auto);
            s.tool_permission_context.always_allow_rules.insert(
                "user".into(),
                vec!["Bash".into(), "Bash(cargo test*)".into()],
            );
        }

        let exit_tool = ExitPlanModeTool;
        let dummy_msg = AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 0,
            role: "assistant".to_string(),
            content: vec![],
            usage: None,
            stop_reason: None,
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        };
        let ctx = make_ctx(Arc::clone(&state));

        let _ = exit_tool
            .call(json!({}), &ctx, &dummy_msg, None)
            .await
            .unwrap();

        let s = state.read();
        assert_eq!(s.tool_permission_context.mode, PermissionMode::Auto);
        assert_eq!(
            s.tool_permission_context
                .always_allow_rules
                .get("user")
                .unwrap(),
            &vec!["Bash(cargo test*)".to_string()]
        );
        assert_eq!(
            s.tool_permission_context
                .auto_mode_stripped_always_allow_rules
                .len(),
            1
        );
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_exit_plan_mode_adds_allowed_prompt_session_rules() {
        let _cwd_guard = PlanCwdGuard::new();
        let state = Arc::new(RwLock::new(AppState::default()));
        {
            let mut s = state.write();
            s.tool_permission_context.mode = PermissionMode::Plan;
            s.tool_permission_context.pre_plan_mode = Some(PermissionMode::Default);
        }

        let exit_tool = ExitPlanModeTool;
        let dummy_msg = AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 0,
            role: "assistant".to_string(),
            content: vec![],
            usage: None,
            stop_reason: None,
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        };
        let ctx = make_ctx(Arc::clone(&state));
        let input = json!({
            "plan": "Run tests after implementation.",
            "allowedPrompts": [
                {"tool": "Bash", "prompt": "cargo test*"}
            ]
        });

        let result = exit_tool.call(input, &ctx, &dummy_msg, None).await.unwrap();
        assert_eq!(
            result.data["allowed_prompt_rules"][0].as_str().unwrap(),
            "Bash(cargo test*)"
        );

        let s = state.read();
        let plan_rules = s
            .tool_permission_context
            .session_allow_rules
            .get("plan_allowed_prompts")
            .expect("plan allowed prompt rules should be recorded");
        assert!(plan_rules.iter().any(|rule| rule == "Bash(cargo test*)"));

        let decision = allthecodes_permissions::decision::has_permissions_to_use_tool(
            "Bash",
            &json!({"command": "cargo test --all"}),
            &s.tool_permission_context,
            None,
        );
        assert_eq!(
            decision.behavior,
            allthecodes_permissions::decision::PermissionBehavior::Allow
        );
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_exit_plan_mode_strips_dangerous_allowed_prompts_in_auto_mode() {
        let _cwd_guard = PlanCwdGuard::new();
        let state = Arc::new(RwLock::new(AppState::default()));
        {
            let mut s = state.write();
            s.tool_permission_context.mode = PermissionMode::Plan;
            s.tool_permission_context.pre_plan_mode = Some(PermissionMode::Auto);
        }

        let exit_tool = ExitPlanModeTool;
        let dummy_msg = AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 0,
            role: "assistant".to_string(),
            content: vec![],
            usage: None,
            stop_reason: None,
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        };
        let ctx = make_ctx(Arc::clone(&state));
        let input = json!({
            "plan": "Run requested checks after implementation.",
            "allowedPrompts": [
                {"tool": "Bash", "prompt": "*"},
                {"tool": "Bash", "prompt": "prefix:python"},
                {"tool": "Bash", "prompt": "prefix:npm"},
                {"tool": "Bash", "prompt": "cargo test*"}
            ]
        });

        let result = exit_tool.call(input, &ctx, &dummy_msg, None).await.unwrap();
        assert_eq!(
            result.data["auto_mode_stripped_allowed_prompt_rules"]
                .as_u64()
                .unwrap(),
            3
        );

        let s = state.read();
        assert_eq!(s.tool_permission_context.mode, PermissionMode::Auto);
        let active_rules = s
            .tool_permission_context
            .session_allow_rules
            .get("plan_allowed_prompts")
            .expect("safe plan allowed prompt rule should remain active");
        assert_eq!(active_rules, &vec!["Bash(cargo test*)".to_string()]);
        for dangerous_rule in ["Bash(*)", "Bash(prefix:python)", "Bash(prefix:npm)"] {
            assert!(
                s.tool_permission_context
                    .auto_mode_stripped_session_allow_rules
                    .iter()
                    .any(|stripped| stripped.rule == dangerous_rule),
                "expected {dangerous_rule} to be withheld in Auto mode"
            );
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_exit_plan_mode_approval_message_lists_allowed_prompt_rules() {
        let _cwd_guard = PlanCwdGuard::new();
        let state = Arc::new(RwLock::new(AppState::default()));
        {
            let mut s = state.write();
            s.tool_permission_context.mode = PermissionMode::Plan;
            s.tool_permission_context.pre_plan_mode = Some(PermissionMode::Default);
        }

        let exit_tool = ExitPlanModeTool;
        let ctx = make_ctx(state);
        let result = exit_tool
            .check_permissions(
                &json!({
                    "plan": "Run tests.",
                    "allowedPrompts": [
                        {"tool": "Bash", "prompt": "cargo test*"},
                        {"tool": "Bash", "prompt": "cargo test*"},
                        {"tool": "Bash", "prompt": "run lint"}
                    ]
                }),
                &ctx,
            )
            .await;

        let PermissionResult::Ask { message } = result else {
            panic!("expected ask permission result");
        };
        assert!(message.contains("Transient allowed prompt rules:"));
        assert!(message.contains("- Bash(cargo test*)"));
        assert!(message.contains("- Bash(cargo clippy*)"));
        assert_eq!(message.matches("- Bash(cargo test*)").count(), 1);
        assert!(!message.contains("transient allowed prompt rule(s)"));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_exit_plan_mode_classifies_common_allowed_prompts() {
        let _cwd_guard = PlanCwdGuard::new();
        let state = Arc::new(RwLock::new(AppState::default()));
        {
            let mut s = state.write();
            s.tool_permission_context.mode = PermissionMode::Plan;
            s.tool_permission_context.pre_plan_mode = Some(PermissionMode::Default);
        }

        let exit_tool = ExitPlanModeTool;
        let dummy_msg = AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 0,
            role: "assistant".to_string(),
            content: vec![],
            usage: None,
            stop_reason: None,
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        };
        let ctx = make_ctx(Arc::clone(&state));
        let input = json!({
            "plan": "Run verification after implementation.",
            "allowedPrompts": [
                {"tool": "Bash", "prompt": "run tests and lint"}
            ]
        });

        let result = exit_tool.call(input, &ctx, &dummy_msg, None).await.unwrap();
        let rules = result.data["allowed_prompt_rules"]
            .as_array()
            .expect("allowed prompt rules should be an array")
            .iter()
            .map(|value| value.as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        assert!(rules.iter().any(|rule| rule == "Bash(cargo test*)"));
        assert!(rules.iter().any(|rule| rule == "Bash(cargo clippy*)"));

        let s = state.read();
        let decision = allthecodes_permissions::decision::has_permissions_to_use_tool(
            "Bash",
            &json!({"command": "cargo clippy --all-targets"}),
            &s.tool_permission_context,
            None,
        );
        assert_eq!(
            decision.behavior,
            allthecodes_permissions::decision::PermissionBehavior::Allow
        );
    }

    #[test]
    fn test_allowed_prompt_classifier_keeps_explicit_patterns() {
        let rules = allowed_prompt_rules(
            &json!({"allowedPrompts": [{"tool": "Bash", "prompt": "prefix:cargo"}]}),
        )
        .unwrap();
        assert_eq!(rules, vec!["Bash(prefix:cargo)"]);
    }

    #[tokio::test]
    async fn test_exit_plan_mode_rejects_invalid_allowed_prompts() {
        let tool = ExitPlanModeTool;
        let state = Arc::new(RwLock::new(AppState::default()));
        {
            let mut s = state.write();
            s.tool_permission_context.mode = PermissionMode::Plan;
        }
        let ctx = make_ctx(state);

        let result = tool
            .validate_input(
                &json!({"allowedPrompts": [{"tool": "Read", "prompt": "src/*"}]}),
                &ctx,
            )
            .await;
        assert!(matches!(
            result,
            ValidationResult::Error { error_code: 2, .. }
        ));
    }

    #[tokio::test]
    async fn test_exit_plan_mode_rejects_negative_allowed_prompt_intent() {
        let tool = ExitPlanModeTool;
        let state = Arc::new(RwLock::new(AppState::default()));
        {
            let mut s = state.write();
            s.tool_permission_context.mode = PermissionMode::Plan;
        }
        let ctx = make_ctx(state);

        let result = tool
            .validate_input(
                &json!({"allowedPrompts": [{"tool": "Bash", "prompt": "do not run tests"}]}),
                &ctx,
            )
            .await;
        assert!(matches!(
            result,
            ValidationResult::Error { error_code: 2, .. }
        ));
    }

    #[test]
    fn test_plan_mode_schema() {
        let enter = EnterPlanModeTool;
        let schema = enter.input_json_schema();
        assert!(schema.get("properties").is_some());

        let exit = ExitPlanModeTool;
        let schema = exit.input_json_schema();
        assert!(schema["properties"].get("plan").is_some());
        assert!(schema["properties"].get("allowedPrompts").is_some());
    }
}
