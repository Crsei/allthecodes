use std::sync::Arc;

use allthecodes_tools::tasks::DelegateTaskTool;
use allthecodes_tools::tool::{
    DeferredToolExecutionResult, FileStateCache, Tool, ToolAppState, ToolResult, ToolUseContext,
    ToolUseOptions, Tools,
};
use allthecodes_types::commands::NoopCommandDispatcher;
use allthecodes_types::hooks::NoopHookRunner;
use allthecodes_types::message::AssistantMessage;
use serde_json::json;

struct EnvGuard {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &std::path::Path) -> Self {
        let previous = std::env::var_os(key);
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

fn context(session_id: &str, cwd: &std::path::Path) -> ToolUseContext {
    let (_, abort_signal) = tokio::sync::watch::channel(false);
    ToolUseContext {
        cwd: cwd.display().to_string(),
        options: ToolUseOptions {
            debug: false,
            main_loop_model: "mock-model".to_string(),
            verbose: false,
            is_non_interactive_session: true,
            custom_system_prompt: None,
            append_system_prompt: None,
            max_budget_usd: None,
        },
        abort_signal,
        read_file_state: FileStateCache::default(),
        get_app_state: Arc::new(ToolAppState::default),
        set_app_state: Arc::new(|_| {}),
        session_id: session_id.to_string(),
        langfuse_session_id: session_id.to_string(),
        messages: Vec::new(),
        agent_id: None,
        agent_type: None,
        query_tracking: None,
        permission_callback: None,
        ask_user_callback: None,
        permission_event_callback: None,
        bg_agent_tx: None,
        hook_runner: Arc::new(NoopHookRunner::new()),
        command_dispatcher: Arc::new(NoopCommandDispatcher::new()),
        available_tools: Tools::new(),
        execute_deferred_tool: None,
        taint_context: Default::default(),
    }
}

fn parent() -> AssistantMessage {
    AssistantMessage {
        uuid: uuid::Uuid::new_v4(),
        timestamp: 0,
        role: "assistant".to_string(),
        content: Vec::new(),
        usage: None,
        stop_reason: None,
        is_api_error_message: false,
        api_error: None,
        cost_usd: 0.0,
    }
}

#[tokio::test]
#[serial_test::serial]
async fn delegated_launch_adopts_only_the_parent_scoped_task() {
    let home = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
    let mut ctx = context("delegate-e2e-parent", cwd.path());
    ctx.execute_deferred_tool = Some(Arc::new(|request| {
        Box::pin(async move {
            let task_id = request.input["_delegate_task_id"].as_str().unwrap();
            let task_list_id = request.input["_delegate_task_list_id"].as_str().unwrap();
            let session_id = request.input["_delegate_session_id"].as_str().unwrap();
            allthecodes_tasks::store_for_task_list_id(task_list_id)
                .adopt_pending_agent_task(task_id, session_id, session_id, None, None)?;
            Ok(DeferredToolExecutionResult {
                tool_use_id: request.tool_use_id,
                tool_name: request.tool_name,
                result: ToolResult {
                    data: json!({
                        "status": "running",
                        "agent_id": session_id,
                        "task_id": task_id,
                        "child_session_id": session_id,
                        "worktree_path": null,
                        "worktree_branch": null,
                        "message": "mock runtime launched"
                    }),
                    ..ToolResult::default()
                },
                is_error: false,
            })
        })
    }));

    let result = DelegateTaskTool
        .call(
            json!({
                "role": "explorer",
                "prompt": "inspect scoped delegation",
                "max_turns": 3,
                "verification_policy": "targeted_tests"
            }),
            &ctx,
            &parent(),
            None,
        )
        .await
        .unwrap();

    assert_eq!(result.data["status"], "running");
    assert_eq!(result.data["agent_id"], result.data["child_session_id"]);
    let scoped = allthecodes_tasks::store_for_task_list_id("delegate-e2e-parent").list();
    assert_eq!(scoped.len(), 1);
    assert_eq!(scoped[0].status, allthecodes_tasks::TaskStatus::InProgress);
    assert_eq!(
        scoped[0]
            .runtime_activity
            .as_ref()
            .map(|activity| activity.phase),
        Some(allthecodes_tasks::AgentRuntimePhase::Running)
    );
    assert!(allthecodes_tasks::global_store().list().is_empty());
}

#[tokio::test]
#[serial_test::serial]
async fn malformed_launch_fails_task_but_keeps_bootstrap_session() {
    let home = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
    let mut ctx = context("delegate-e2e-failure", cwd.path());
    ctx.execute_deferred_tool = Some(Arc::new(|request| {
        Box::pin(async move {
            Ok(DeferredToolExecutionResult {
                tool_use_id: request.tool_use_id,
                tool_name: request.tool_name,
                result: ToolResult {
                    data: json!({ "status": "running" }),
                    ..ToolResult::default()
                },
                is_error: false,
            })
        })
    }));

    let error = DelegateTaskTool
        .call(
            json!({ "role": "worker", "prompt": "fail deterministically" }),
            &ctx,
            &parent(),
            None,
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("identity mismatch"));

    let tasks = allthecodes_tasks::store_for_task_list_id("delegate-e2e-failure").list();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].status, allthecodes_tasks::TaskStatus::Failed);
    assert!(tasks[0].output.contains("launch failed"));
    let session_id = tasks[0].remote_session_id.as_deref().unwrap();
    assert!(!allthecodes_session::storage::load_session(session_id)
        .unwrap()
        .is_empty());
}
