use super::security::{find_tool, is_plan_mode_plan_file_write, security_validate};
use super::*;
use crate::types::app_state::AppState;
use crate::types::tool::{FileStateCache, PermissionMode, ToolUseOptions};
use allthecodes_tools::result::enforce_result_size;
use std::sync::Arc;
use std::time::Instant;

use serde_json::Value;

use crate::types::tool::{Tool, ToolProgress, ToolResult, ToolUseContext, Tools};

// -- Helper: minimal ToolUseContext for security_validate tests ----------

fn make_ctx_with_mode(mode: PermissionMode) -> ToolUseContext {
    let mut app = AppState::default();
    app.tool_permission_context.mode = mode;

    let (_tx, rx) = tokio::sync::watch::channel(false);
    ToolUseContext {
        options: ToolUseOptions {
            debug: false,
            main_loop_model: "test".into(),
            verbose: false,
            is_non_interactive_session: false,
            custom_system_prompt: None,
            append_system_prompt: None,
            max_budget_usd: None,
        },
        abort_signal: rx,
        read_file_state: FileStateCache::default(),
        get_app_state: Arc::new(move || app.to_tool_app_state()),
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
    }
}

struct OriginalCwdGuard(std::path::PathBuf);

impl OriginalCwdGuard {
    fn set(path: &std::path::Path) -> Self {
        let mut ps = crate::bootstrap::PROCESS_STATE.write();
        let previous = ps.original_cwd.clone();
        ps.original_cwd = path.to_path_buf();
        Self(previous)
    }
}

impl Drop for OriginalCwdGuard {
    fn drop(&mut self) {
        crate::bootstrap::PROCESS_STATE.write().original_cwd = self.0.clone();
    }
}

fn set_original_cwd_for_test(path: &std::path::Path) -> OriginalCwdGuard {
    OriginalCwdGuard::set(path)
}

// -- Stub tools for testing is_read_only behavior -----------------------

struct ReadOnlyStub;
#[async_trait::async_trait]
impl Tool for ReadOnlyStub {
    fn name(&self) -> &str {
        "Grep"
    }
    async fn description(&self, _: &Value) -> String {
        String::new()
    }
    fn input_json_schema(&self) -> Value {
        Value::Null
    }
    fn is_read_only(&self, _: &Value) -> bool {
        true
    }
    async fn call(
        &self,
        _: Value,
        _: &ToolUseContext,
        _: &crate::types::message::AssistantMessage,
        _: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> anyhow::Result<ToolResult> {
        Ok(ToolResult {
            data: Value::Null,
            new_messages: vec![],
            ..Default::default()
        })
    }
    async fn prompt(&self) -> String {
        String::new()
    }
}

struct WritableStub;
#[async_trait::async_trait]
impl Tool for WritableStub {
    fn name(&self) -> &str {
        "Bash"
    }
    async fn description(&self, _: &Value) -> String {
        String::new()
    }
    fn input_json_schema(&self) -> Value {
        Value::Null
    }
    fn is_read_only(&self, _: &Value) -> bool {
        false
    }
    async fn call(
        &self,
        _: Value,
        _: &ToolUseContext,
        _: &crate::types::message::AssistantMessage,
        _: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> anyhow::Result<ToolResult> {
        Ok(ToolResult {
            data: Value::Null,
            new_messages: vec![],
            ..Default::default()
        })
    }
    async fn prompt(&self) -> String {
        String::new()
    }
}

// -- Existing tests -----------------------------------------------------

#[test]
fn test_find_tool_missing() {
    let tools: Tools = vec![];
    assert!(find_tool("NonExistent", &tools).is_none());
}

#[test]
fn test_enforce_result_size_small() {
    let data = Value::String("hello".into());
    let result = enforce_result_size(data.clone(), 1000);
    assert_eq!(result, data);
}

#[test]
fn test_enforce_result_size_large() {
    let large = "x".repeat(10_000);
    let data = Value::String(large);
    let result = enforce_result_size(data, 1000);
    if let Value::String(s) = result {
        assert!(s.len() < 10_000);
        assert!(s.contains("characters omitted"));
    } else {
        panic!("expected string");
    }
}

// -- Stage 3c: security_validate tests ----------------------------------

#[test]
fn test_plan_mode_blocks_write_tools() {
    let ctx = make_ctx_with_mode(PermissionMode::Plan);
    let tool = WritableStub;
    let input = serde_json::json!({"command": "ls"});
    let now = Instant::now();

    let result = security_validate("id1", "Bash", &input, &tool, &ctx, now);
    assert!(
        result.is_some(),
        "Plan mode should block non-read-only tool"
    );
    let err = result.unwrap();
    assert!(err.is_error);
    assert!(
        err.result.data.as_str().unwrap().contains("Plan mode"),
        "Error message should mention Plan mode"
    );
}

#[test]
fn test_plan_mode_allows_read_tools() {
    let ctx = make_ctx_with_mode(PermissionMode::Plan);
    let tool = ReadOnlyStub;
    let input = serde_json::json!({"pattern": "foo"});
    let now = Instant::now();

    let result = security_validate("id2", "Grep", &input, &tool, &ctx, now);
    assert!(result.is_none(), "Plan mode should allow read-only tool");
}

#[test]
#[serial_test::serial]
fn test_plan_mode_allows_dedicated_plan_file_write() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(".allthecodes")).unwrap();
    let _cwd_guard = set_original_cwd_for_test(temp.path());

    let plan_path = crate::config::paths::current_plan_file_path(temp.path());
    let plan_path = plan_path.to_string_lossy().into_owned();
    let ctx = make_ctx_with_mode(PermissionMode::Plan);
    let tool = WritableStub;
    let input = serde_json::json!({
        "file_path": plan_path,
        "content": "## Plan\n- keep planning"
    });

    assert!(is_plan_mode_plan_file_write("Write", &input));
    let result = security_validate("id-plan", "Write", &input, &tool, &ctx, Instant::now());
    assert!(
        result.is_none(),
        "Plan mode should allow writes to its dedicated plan file"
    );
}

#[test]
#[serial_test::serial]
fn test_plan_mode_blocks_non_plan_file_write() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(".allthecodes")).unwrap();
    let _cwd_guard = set_original_cwd_for_test(temp.path());

    let other_path = temp.path().join("other.md").to_string_lossy().into_owned();
    let ctx = make_ctx_with_mode(PermissionMode::Plan);
    let tool = WritableStub;
    let input = serde_json::json!({
        "file_path": other_path,
        "content": "not the plan"
    });

    assert!(!is_plan_mode_plan_file_write("Write", &input));
    let result = security_validate("id-other", "Write", &input, &tool, &ctx, Instant::now());
    assert!(
        result.is_some(),
        "Plan mode should still block non-plan file writes"
    );
    let err = result.unwrap();
    assert!(err.is_error);
    assert!(err.result.data.as_str().unwrap().contains("Plan mode"));
}

#[test]
fn test_dangerous_command_blocked() {
    let ctx = make_ctx_with_mode(PermissionMode::Default);
    let tool = WritableStub;
    let input = serde_json::json!({"command": "rm -rf /"});
    let now = Instant::now();

    let result = security_validate("id3", "Bash", &input, &tool, &ctx, now);
    assert!(result.is_some(), "Dangerous command should be blocked");
    let err = result.unwrap();
    assert!(err
        .result
        .data
        .as_str()
        .unwrap()
        .contains("Dangerous command blocked"));
}

#[test]
fn test_safe_command_allowed() {
    let ctx = make_ctx_with_mode(PermissionMode::Default);
    let tool = WritableStub;
    let input = serde_json::json!({"command": "ls -la"});
    let now = Instant::now();

    let result = security_validate("id4", "Bash", &input, &tool, &ctx, now);
    assert!(result.is_none(), "Safe command should be allowed");
}

#[test]
fn test_path_traversal_blocked() {
    let ctx = make_ctx_with_mode(PermissionMode::Default);
    let tool = WritableStub; // is_read_only = false, but tool_name matters for path check
    let input = serde_json::json!({"file_path": "/../../../../../etc/passwd"});
    let now = Instant::now();

    let result = security_validate("id5", "Write", &input, &tool, &ctx, now);
    assert!(result.is_some(), "Path traversal should be blocked");
    let err = result.unwrap();
    assert!(err
        .result
        .data
        .as_str()
        .unwrap()
        .contains("Invalid file path"));
}

#[test]
#[serial_test::serial]
fn test_path_outside_cwd_blocked() {
    let cwd = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let _cwd_guard = OriginalCwdGuard::set(cwd.path());

    let ctx = make_ctx_with_mode(PermissionMode::Default);
    let tool = WritableStub;
    let outside_path = outside.path().join("evil.txt");
    let input = serde_json::json!({"file_path": outside_path.to_string_lossy()});
    let now = Instant::now();

    let result = security_validate("id6", "Write", &input, &tool, &ctx, now);
    assert!(
        result.is_some(),
        "Path outside cwd should be blocked, path={}",
        outside_path.display()
    );
    let err = result.unwrap();
    assert!(err
        .result
        .data
        .as_str()
        .unwrap()
        .contains("outside the allowed"));
}

#[test]
fn test_bypass_mode_skips_all() {
    let ctx = make_ctx_with_mode(PermissionMode::Bypass);
    let tool = WritableStub;

    // Dangerous command — would normally be blocked
    let input = serde_json::json!({"command": "rm -rf /"});
    let now = Instant::now();
    let result = security_validate("id7", "Bash", &input, &tool, &ctx, now);
    assert!(
        result.is_none(),
        "Bypass mode should skip all security checks"
    );

    // Path traversal — would normally be blocked
    let input2 = serde_json::json!({"file_path": "/../../../../../etc/passwd"});
    let result2 = security_validate("id8", "Write", &input2, &tool, &ctx, now);
    assert!(result2.is_none(), "Bypass mode should skip path check too");
}

#[test]
fn test_powershell_dangerous_command_blocked() {
    let ctx = make_ctx_with_mode(PermissionMode::Default);
    let tool = WritableStub;
    let now = Instant::now();

    // PowerShell should also be checked
    let input = serde_json::json!({"command": "rm -rf /"});
    let result = security_validate("id9", "PowerShell", &input, &tool, &ctx, now);
    assert!(
        result.is_some(),
        "PowerShell dangerous command should be blocked"
    );

    let input = serde_json::json!({"command": r"Remove-Item -Recurse -Force C:\tmp"});
    let result = security_validate("id10", "PowerShell", &input, &tool, &ctx, Instant::now());
    assert!(
        result.is_some(),
        "PowerShell Remove-Item destructive command should be blocked"
    );

    let input = serde_json::json!({"command": "Invoke-Expression $payload"});
    let result = security_validate("id11", "PowerShell", &input, &tool, &ctx, Instant::now());
    assert!(
        result.is_some(),
        "PowerShell Invoke-Expression security pattern should be blocked"
    );

    let input = serde_json::json!({"command": "Import-Module .\\payload.psm1"});
    let result = security_validate("id12", "PowerShell", &input, &tool, &ctx, Instant::now());
    assert!(
        result.is_some(),
        "PowerShell module-loading security pattern should be blocked"
    );

    let input = serde_json::json!({"command": "Write-Output 'unterminated"});
    let result = security_validate("id13", "PowerShell", &input, &tool, &ctx, Instant::now());
    assert!(
        result.is_some(),
        "PowerShell obvious parse errors should fail closed"
    );

    let input = serde_json::json!({"command": "Write-Output { Get-Date }"});
    let result = security_validate("id14", "PowerShell", &input, &tool, &ctx, Instant::now());
    assert!(
        result.is_some(),
        "PowerShell script blocks should fail closed unless used by safe consumers"
    );
}

// -- make_error_result tests (shared helper in mod.rs) --------------------

#[test]
fn test_make_error_result_fields() {
    let started = Instant::now();
    let result = make_error_result("use-id-42", "MyTool", "fatal error occurred", started);
    assert!(result.is_error);
    assert_eq!(result.tool_use_id, "use-id-42");
    assert_eq!(result.tool_name, "MyTool");
    assert_eq!(result.result.data.as_str().unwrap(), "fatal error occurred");
    assert!(result.new_messages.is_empty());
    assert!(result.result.new_messages.is_empty());
    assert!(!result.hook_stopped_continuation);
}

#[test]
fn test_make_error_result_empty_strings() {
    let started = Instant::now();
    let result = make_error_result("", "", "", started);
    assert!(result.is_error);
    assert_eq!(result.tool_use_id, "");
    assert_eq!(result.tool_name, "");
    assert_eq!(result.result.data.as_str().unwrap(), "");
}

#[test]
fn test_make_error_result_data_is_string_variant() {
    let started = Instant::now();
    let result = make_error_result("id", "Tool", "some message", started);
    // data must be a JSON string (not object/array/null)
    assert!(result.result.data.is_string());
}

#[test]
fn test_make_error_result_hook_stopped_continuation_always_false() {
    let started = Instant::now();
    let result = make_error_result("id", "Tool", "msg", started);
    assert!(
        !result.hook_stopped_continuation,
        "early-exit errors never stop continuation via hooks"
    );
}
