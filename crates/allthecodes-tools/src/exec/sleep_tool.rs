//! SleepTool -- signals the proactive tick loop to pause for N seconds.
//!
//! This is a KAIROS tool. When the model calls Sleep, it signals the daemon
//! tick loop to set a `sleep_until` marker and stop ticking for the requested
//! duration. The tool itself does NOT actually block -- it only returns a
//! JSON result describing the requested sleep.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::exec::sleep as sleep_spec;
use crate::tool::{Tool, ToolProgress, ToolResult, ToolUseContext, ValidationResult};
use allthecodes_config::features::{self, Feature};
use allthecodes_config::proactive_sleep::write_sleep_state;
use allthecodes_types::message::AssistantMessage;

/// SleepTool -- signal the proactive tick loop to pause.
pub struct SleepTool;

#[async_trait]
impl Tool for SleepTool {
    fn name(&self) -> &str {
        sleep_spec::NAME
    }

    async fn description(&self, _input: &Value) -> String {
        sleep_spec::description().to_string()
    }

    fn input_json_schema(&self) -> Value {
        sleep_spec::input_schema()
    }

    fn is_enabled(&self) -> bool {
        features::enabled(Feature::Proactive)
            || features::enabled(Feature::Kairos)
            || allthecodes_types::proactive_context::is_proactive_active()
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        let duration = input.get("duration_seconds").and_then(|v| v.as_i64());

        match sleep_spec::validate_duration_seconds(duration) {
            Ok(()) => ValidationResult::Ok,
            Err(message) => ValidationResult::Error {
                message,
                error_code: 1,
            },
        }
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
        _parent_message: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let duration_seconds = input
            .get("duration_seconds")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let reason = input
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let sleep_state = write_sleep_state(duration_seconds as u64, &reason)?;

        Ok(ToolResult {
            data: json!({
                "status": "sleeping",
                "duration_seconds": duration_seconds,
                "reason": reason,
                "sleep_until": sleep_state.sleeping_until.to_rfc3339(),
            }),
            new_messages: vec![],
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        sleep_spec::prompt()
    }

    fn user_facing_name(&self, _input: Option<&Value>) -> String {
        sleep_spec::NAME.to_string()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
            match &self.previous {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    struct ProactiveActiveGuard;

    impl Drop for ProactiveActiveGuard {
        fn drop(&mut self) {
            allthecodes_types::proactive_context::set_proactive_active(false);
        }
    }

    struct FeatureOverrideGuard(Option<allthecodes_config::features::FeatureFlags>);

    impl FeatureOverrideGuard {
        fn set(flags: allthecodes_config::features::FeatureFlags) -> Self {
            let previous = allthecodes_config::features::runtime_override();
            allthecodes_config::features::set_runtime_override(flags);
            Self(previous)
        }
    }

    impl Drop for FeatureOverrideGuard {
        fn drop(&mut self) {
            match self.0.take() {
                Some(flags) => allthecodes_config::features::set_runtime_override(flags),
                None => allthecodes_config::features::clear_runtime_override(),
            }
        }
    }

    #[test]
    fn test_sleep_tool_name() {
        let tool = SleepTool;
        assert_eq!(tool.name(), "Sleep");
    }

    #[test]
    fn test_sleep_tool_schema() {
        let tool = SleepTool;
        let schema = tool.input_json_schema();
        let props = schema["properties"].as_object().unwrap();
        assert!(props.contains_key("duration_seconds"));
        assert!(props.contains_key("reason"));

        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("duration_seconds")));
    }

    #[test]
    fn test_sleep_tool_is_read_only() {
        let tool = SleepTool;
        assert!(tool.is_read_only(&json!({})));
    }

    #[test]
    fn test_sleep_tool_is_enabled_by_active_proactive_controller() {
        let _guard = ProactiveActiveGuard;
        let _features =
            FeatureOverrideGuard::set(allthecodes_config::features::FeatureFlags::all_disabled());
        allthecodes_types::proactive_context::set_proactive_active(true);

        let tool = SleepTool;

        assert!(tool.is_enabled());
    }

    #[test]
    fn test_sleep_tool_validates_missing_duration() {
        let tool = SleepTool;
        let ctx = make_test_ctx();
        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(tool.validate_input(&json!({}), &ctx));
        match result {
            ValidationResult::Error { message, .. } => {
                assert!(
                    message.contains("required"),
                    "expected 'required' in error: {}",
                    message
                );
            }
            ValidationResult::Ok => panic!("expected error for missing duration_seconds"),
        }
    }

    #[test]
    fn test_sleep_tool_validates_too_high() {
        let tool = SleepTool;
        let ctx = make_test_ctx();
        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(tool.validate_input(&json!({"duration_seconds": 7200}), &ctx));
        match result {
            ValidationResult::Error { message, .. } => {
                assert!(
                    message.contains("3600"),
                    "expected '3600' in error: {}",
                    message
                );
            }
            ValidationResult::Ok => panic!("expected error for duration_seconds > 3600"),
        }
    }

    #[test]
    fn test_sleep_tool_validates_too_low() {
        let tool = SleepTool;
        let ctx = make_test_ctx();
        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(tool.validate_input(&json!({"duration_seconds": 0}), &ctx));
        match result {
            ValidationResult::Error { .. } => {}
            ValidationResult::Ok => panic!("expected error for duration_seconds < 1"),
        }
    }

    #[test]
    fn test_sleep_tool_validates_valid_input() {
        let tool = SleepTool;
        let ctx = make_test_ctx();
        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(tool.validate_input(&json!({"duration_seconds": 60}), &ctx));
        assert!(
            matches!(result, ValidationResult::Ok),
            "expected Ok for valid input"
        );
    }

    #[test]
    #[serial_test::serial]
    fn sleep_tool_writes_shared_sleep_contract() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());

        let state = write_sleep_state(60, " waiting ").unwrap();
        let shared = allthecodes_config::proactive_sleep::active_sleep_state()
            .unwrap()
            .expect("shared sleep state");

        assert_eq!(
            state.schema_version,
            allthecodes_config::proactive_sleep::SLEEP_STATE_SCHEMA_VERSION
        );
        assert_eq!(shared.schema_version, state.schema_version);
        assert_eq!(shared.reason.as_deref(), Some("waiting"));
    }

    #[test]
    #[serial_test::serial]
    fn write_sleep_state_uses_config_daemon_contract() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());

        let state = write_sleep_state(60, " waiting ").unwrap();

        let path = allthecodes_config::paths::daemon_dir().join("sleep-state.json");
        let legacy_path = home
            .path()
            .join(".allthecodes")
            .join("daemon")
            .join("sleep-state.json");
        let body: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert!(path.exists());
        assert!(!legacy_path.exists());
        assert_eq!(state.schema_version, 2);
        assert_eq!(body["schema_version"], 2);
        assert_eq!(body["reason"], "waiting");
    }

    // -----------------------------------------------------------------------
    // Test helpers
    // -----------------------------------------------------------------------

    fn make_test_ctx() -> ToolUseContext {
        use crate::tool::ToolAppState as AppState;
        use crate::tool::{FileStateCache, ToolUseOptions};
        use std::sync::Arc;

        ToolUseContext {
            cwd: ".".to_string(),
            options: ToolUseOptions {
                debug: false,
                main_loop_model: "test".to_string(),
                verbose: false,
                is_non_interactive_session: true,
                custom_system_prompt: None,
                append_system_prompt: None,
                max_budget_usd: None,
            },
            abort_signal: tokio::sync::watch::channel(false).1,
            read_file_state: FileStateCache::default(),
            get_app_state: Arc::new(AppState::default),
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
            taint_context: Default::default(),
        }
    }
}
