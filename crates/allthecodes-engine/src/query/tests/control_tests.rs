use std::sync::Arc;

use futures::StreamExt;

use super::super::super::deps::ModelResponse;
use super::super::*;
use super::mocks::{
    make_query_params, make_text_response, make_text_response_with_stop_and_output_tokens,
    make_user_message_for_test, request_start_count, MockDeps, MockStreamStep,
    StopContinuationHookRunner,
};
use crate::types::config::{QueryGates, QueryParams, QuerySource, TaskBudget};
use crate::types::message::{
    AssistantMessage, Attachment, AttachmentMessage, ContentBlock, Message, MessageContent,
    QueryYield, Usage, UserMessage,
};
use crate::types::tool::PermissionMode;
use allthecodes_tools::goals::{self, GoalStatus};
use serial_test::serial;
use std::path::Path;

struct EnvGuard {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvGuard {
    fn set_path(key: &'static str, value: &Path) -> Self {
        let previous = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.as_ref() {
            std::env::set_var(self.key, previous);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

#[tokio::test]
async fn test_stop_hook_continuation_injects_meta_user_message_once() {
    let deps = Arc::new(MockDeps::new(vec![
        make_text_response("Need final audit."),
        make_text_response("Final answer after stop hook."),
    ]));
    deps.set_hook_runner(Arc::new(StopContinuationHookRunner::new(
        "Run one more validation pass.",
    )));

    let stream = query(
        make_query_params(vec![make_user_message_for_test("Finish the task")]),
        deps.clone(),
    );
    let items: Vec<QueryYield> = stream.collect().await;

    assert_eq!(
        request_start_count(&items),
        2,
        "stop hook continuation should trigger one more model call"
    );
    let params = deps.recorded_params();
    assert_eq!(params.len(), 2, "expected continuation model call");

    let continuation = params[1].messages.iter().rev().find_map(|message| {
        if let Message::User(user) = message {
            if let MessageContent::Text(text) = &user.content {
                return Some((user.is_meta, text.as_str()));
            }
        }
        None
    });
    assert_eq!(
        continuation,
        Some((true, "Run one more validation pass.")),
        "stop hook continuation should be injected as a meta user message"
    );
}

#[tokio::test]
async fn test_token_budget_continuation_injects_nudge_message() {
    let deps = Arc::new(MockDeps::new(vec![
        make_text_response_with_stop_and_output_tokens("Still working.", "end_turn", 50),
        make_text_response_with_stop_and_output_tokens("Budget complete.", "end_turn", 50),
    ]));
    let mut params = make_query_params(vec![make_user_message_for_test("Spend the budget")]);
    params.task_budget = Some(TaskBudget { total: 100 });

    let stream = query(params, deps.clone());
    let items: Vec<QueryYield> = stream.collect().await;

    assert_eq!(
        request_start_count(&items),
        2,
        "token budget nudge should trigger one continuation"
    );
    let params = deps.recorded_params();
    assert_eq!(params.len(), 2, "expected continuation model call");

    let nudge = params[1].messages.iter().rev().find_map(|message| {
        if let Message::User(user) = message {
            if let MessageContent::Text(text) = &user.content {
                return Some((user.is_meta, text.as_str()));
            }
        }
        None
    });
    assert!(
        matches!(nudge, Some((true, text)) if text.contains("Token budget at 50%")),
        "token budget continuation should inject a meta nudge message"
    );
}

#[tokio::test]
#[serial]
async fn test_active_goal_continuation_injects_meta_user_message() {
    let tempdir = tempfile::tempdir().unwrap();
    let _home = EnvGuard::set_path("ALLTHECODES_HOME", tempdir.path());
    save_goal("goal-active", GoalStatus::Active);

    let deps = Arc::new(
        MockDeps::new(vec![
            make_text_response("Still working."),
            make_text_response("Paused by max turns."),
        ])
        .with_audit_session("goal-active"),
    );
    let mut params = make_query_params(vec![make_user_message_for_test("start")]);
    params.max_turns = Some(2);

    let stream = query(params, deps.clone());
    let items: Vec<QueryYield> = stream.collect().await;

    assert_eq!(
        request_start_count(&items),
        2,
        "active goal should trigger a continuation turn"
    );
    let params = deps.recorded_params();
    let continuation = params[1].messages.iter().rev().find_map(|message| {
        if let Message::User(user) = message {
            if let MessageContent::Text(text) = &user.content {
                return Some((user.is_meta, text.as_str()));
            }
        }
        None
    });
    assert!(
        matches!(continuation, Some((true, text)) if text.contains("Continue working toward the active session goal")),
        "active goal continuation should inject a meta user message"
    );
}

#[tokio::test]
#[serial]
async fn test_completed_goal_does_not_continue() {
    let tempdir = tempfile::tempdir().unwrap();
    let _home = EnvGuard::set_path("ALLTHECODES_HOME", tempdir.path());
    save_goal("goal-complete-query", GoalStatus::Complete);

    let deps = Arc::new(
        MockDeps::new(vec![make_text_response("Done.")]).with_audit_session("goal-complete-query"),
    );

    let stream = query(
        make_query_params(vec![make_user_message_for_test("start")]),
        deps.clone(),
    );
    let items: Vec<QueryYield> = stream.collect().await;

    assert_eq!(
        request_start_count(&items),
        1,
        "completed goal should not trigger continuation"
    );
}

#[tokio::test]
#[serial]
async fn test_non_active_goal_statuses_do_not_continue() {
    for (session_id, status) in [
        ("goal-paused-query", GoalStatus::Paused),
        ("goal-blocked-query", GoalStatus::Blocked),
        ("goal-usage-limited-query", GoalStatus::UsageLimited),
        ("goal-budget-limited-query", GoalStatus::BudgetLimited),
    ] {
        let tempdir = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set_path("ALLTHECODES_HOME", tempdir.path());
        save_goal(session_id, status);

        let deps = Arc::new(
            MockDeps::new(vec![make_text_response("Done.")]).with_audit_session(session_id),
        );

        let stream = query(
            make_query_params(vec![make_user_message_for_test("start")]),
            deps.clone(),
        );
        let items: Vec<QueryYield> = stream.collect().await;

        assert_eq!(
            request_start_count(&items),
            1,
            "non-active goal should not trigger continuation for {session_id}"
        );
    }
}

#[tokio::test]
#[serial]
async fn test_plan_mode_suppresses_active_goal_continuation() {
    let tempdir = tempfile::tempdir().unwrap();
    let _home = EnvGuard::set_path("ALLTHECODES_HOME", tempdir.path());
    save_goal("goal-plan-mode-query", GoalStatus::Active);

    let mut app_state = crate::types::app_state::AppState::default();
    app_state.tool_permission_context.mode = PermissionMode::Plan;
    let deps = Arc::new(
        MockDeps::new(vec![make_text_response("Planning only.")])
            .with_audit_session("goal-plan-mode-query")
            .with_app_state(app_state),
    );

    let stream = query(
        make_query_params(vec![make_user_message_for_test("start")]),
        deps.clone(),
    );
    let items: Vec<QueryYield> = stream.collect().await;

    assert_eq!(
        request_start_count(&items),
        1,
        "plan mode should suppress automatic goal continuation"
    );
}

#[tokio::test]
#[serial]
async fn test_empty_goal_continuation_marks_usage_limited() {
    let tempdir = tempfile::tempdir().unwrap();
    let _home = EnvGuard::set_path("ALLTHECODES_HOME", tempdir.path());
    save_goal("goal-empty-continuation", GoalStatus::Active);

    let deps = Arc::new(
        MockDeps::new(vec![
            make_text_response("Still working."),
            make_text_response(""),
            make_text_response("   "),
        ])
        .with_audit_session("goal-empty-continuation"),
    );

    let stream = query(
        make_query_params(vec![make_user_message_for_test("start")]),
        deps.clone(),
    );
    let items: Vec<QueryYield> = stream.collect().await;

    assert_eq!(
        request_start_count(&items),
        3,
        "two empty automatic continuations should stop recovery"
    );
    let goal = goals::load_goal_for_session("goal-empty-continuation")
        .unwrap()
        .unwrap();
    assert_eq!(goal.status, GoalStatus::UsageLimited);
    assert_eq!(
        goal.status_reason.as_deref(),
        Some("automatic goal continuation returned empty responses")
    );
}

#[tokio::test]
#[serial]
async fn test_prompt_overflow_marks_active_goal_usage_limited() {
    let tempdir = tempfile::tempdir().unwrap();
    let _home = EnvGuard::set_path("ALLTHECODES_HOME", tempdir.path());
    save_goal("goal-prompt-overflow", GoalStatus::Active);

    let deps = Arc::new(
        MockDeps::from_steps(vec![MockStreamStep::Error(
            "prompt_too_long: context overflow".to_string(),
        )])
        .with_audit_session("goal-prompt-overflow"),
    );

    let stream = query(
        make_query_params(vec![make_user_message_for_test("start")]),
        deps.clone(),
    );
    let _items: Vec<QueryYield> = stream.collect().await;

    let goal = goals::load_goal_for_session("goal-prompt-overflow")
        .unwrap()
        .unwrap();
    assert_eq!(goal.status, GoalStatus::UsageLimited);
    assert_eq!(
        goal.status_reason.as_deref(),
        Some("context overflow prevented goal continuation")
    );
}

#[tokio::test]
async fn test_max_turns_limit() {
    let tool_response = ModelResponse {
        assistant_message: AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            role: "assistant".to_string(),
            content: vec![ContentBlock::ToolUse {
                id: "tu_1".to_string(),
                name: "Bash".to_string(),
                input: serde_json::json!({"command": "ls"}),
            }],
            usage: Some(Usage::default()),
            stop_reason: Some("tool_use".to_string()),
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        },
        stream_events: vec![],
        usage: Usage::default(),
    };

    let deps = Arc::new(MockDeps::new(vec![tool_response]));

    let params = QueryParams {
        messages: vec![Message::User(UserMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 0,
            role: "user".to_string(),
            content: MessageContent::Text("list files".to_string()),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        })],
        system_prompt: vec![],
        user_context: Default::default(),
        system_context: Default::default(),
        fallback_model: None,
        query_source: QuerySource::ReplMainThread,
        max_output_tokens_override: None,
        max_turns: Some(1),
        skip_cache_write: None,
        task_budget: None,
        gates: QueryGates::default(),
    };

    let stream = query(params, deps);
    let items: Vec<QueryYield> = stream.collect().await;

    let has_max_turns = items.iter().any(|item| {
        matches!(
            item,
            QueryYield::Message(Message::Attachment(AttachmentMessage {
                attachment: Attachment::MaxTurnsReached { .. },
                ..
            }))
        )
    });
    assert!(has_max_turns, "expected MaxTurnsReached attachment");
}

fn save_goal(session_id: &str, status: GoalStatus) {
    let mut goal = goals::create_goal_record("ship the feature", None, chrono::Utc::now()).unwrap();
    goal.status = status;
    goals::save_goal_for_session(session_id, &goal).unwrap();
}

#[tokio::test]
async fn test_hook_stopped_tool_execution_yields_attachment_and_stops() {
    let tool_response = ModelResponse {
        assistant_message: AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            role: "assistant".to_string(),
            content: vec![ContentBlock::ToolUse {
                id: "tu_1".to_string(),
                name: "Bash".to_string(),
                input: serde_json::json!({"command": "ls"}),
            }],
            usage: Some(Usage::default()),
            stop_reason: Some("tool_use".to_string()),
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        },
        stream_events: vec![],
        usage: Usage::default(),
    };

    let deps = Arc::new(MockDeps::new(vec![
        tool_response,
        make_text_response("must not run"),
    ]));
    deps.stop_after_tool_execution();

    let stream = query(
        make_query_params(vec![make_user_message_for_test("run a tool")]),
        deps.clone(),
    );
    let items: Vec<QueryYield> = stream.collect().await;

    assert_eq!(
        request_start_count(&items),
        1,
        "hook stopped continuation should not start another model request"
    );
    assert!(
        items.iter().any(|item| {
            matches!(
                item,
                QueryYield::Message(Message::Attachment(AttachmentMessage {
                    attachment: Attachment::HookStoppedContinuation,
                    ..
                }))
            )
        }),
        "expected HookStoppedContinuation attachment"
    );
}
