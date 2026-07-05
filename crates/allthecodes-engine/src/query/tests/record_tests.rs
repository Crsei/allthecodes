use std::path::PathBuf;
use std::sync::Arc;

use allthecodes_types::agent_events::AgentEvent;
use allthecodes_types::agent_runtime_record::{compute_digest, AgentRuntimePermissionDecision};
use allthecodes_types::ShellExecutionOutput;
use futures::StreamExt;

use super::super::super::deps::{ModelResponse, QueryDeps, ToolExecResult};
use super::super::*;
use super::mocks::{MockDeps, MockStreamStep};
use crate::types::message::{AssistantMessage, ContentBlock, Usage};
use crate::types::tool::ToolResult;

fn shell_exec_result(
    stdout: &str,
    stderr: &str,
    exit_code: Option<i32>,
    is_error: bool,
    permission_decision: AgentRuntimePermissionDecision,
) -> ToolExecResult {
    ToolExecResult {
        tool_use_id: "toolu-shell".to_string(),
        tool_name: "Bash".to_string(),
        effective_input: serde_json::json!({"command": "display command"}),
        result: ToolResult {
            data: serde_json::json!({
                "stdout": "display stdout",
                "stderr": "display stderr",
            }),
            shell: Some(ShellExecutionOutput {
                command: Some("printf raw".to_string()),
                cwd: Some(PathBuf::from("/tmp/project")),
                stdout: stdout.to_string(),
                stderr: stderr.to_string(),
                exit_code,
                interrupted: false,
                termination: None,
                error: None,
            }),
            ..Default::default()
        },
        is_error,
        hook_stopped_continuation: false,
        duration_ms: Some(42),
        permission_decision: Some(permission_decision),
        brief_message: None,
    }
}

fn shell_tool_response(tool_use_id: &str, command: &str) -> ModelResponse {
    ModelResponse {
        assistant_message: AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            role: "assistant".to_string(),
            content: vec![
                ContentBlock::Text {
                    text: "Running command".to_string(),
                },
                ContentBlock::ToolUse {
                    id: tool_use_id.to_string(),
                    name: "Bash".to_string(),
                    input: serde_json::json!({ "command": command }),
                },
            ],
            usage: Some(Usage::default()),
            stop_reason: Some("tool_use".to_string()),
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        },
        stream_events: vec![],
        usage: Usage::default(),
    }
}

#[test]
fn execution_record_uses_shell_runtime_metadata() {
    let deps: Arc<dyn QueryDeps> = Arc::new(
        MockDeps::from_steps(Vec::<MockStreamStep>::new()).with_runtime_identity(
            "session-1",
            "agent-2",
            Some("agent-1"),
            Some("builder"),
        ),
    );
    let exec_result = shell_exec_result(
        "raw stdout\n",
        "",
        Some(0),
        false,
        AgentRuntimePermissionDecision::AllowedByUser,
    );
    let turn_context = RuntimeRecordTurnContext {
        model: Some("claude-test".to_string()),
        fallback_used: true,
        retry_count: 1,
    };

    let record = build_execution_record(&deps, &exec_result, &turn_context);

    assert_eq!(record.session_id, "session-1");
    assert_eq!(record.agent_id, "agent-2");
    assert_eq!(record.parent_agent_id.as_deref(), Some("agent-1"));
    assert_eq!(record.agent_role.as_deref(), Some("builder"));
    assert_eq!(record.tool, "shell");
    assert_eq!(record.tool_use_id.as_deref(), Some("toolu-shell"));
    assert_eq!(record.command.as_deref(), Some("printf raw"));
    assert_eq!(record.cwd, Some(PathBuf::from("/tmp/project")));
    assert_eq!(record.exit_code, Some(0));
    assert_eq!(record.stdout_digest, Some(compute_digest(b"raw stdout\n")));
    assert_eq!(record.stderr_digest, Some(compute_digest(b"")));
    assert_eq!(record.model.as_deref(), Some("claude-test"));
    assert!(record.fallback_used);
    assert_eq!(record.retry_count, 1);
    assert_eq!(
        record.permission_decision,
        Some(AgentRuntimePermissionDecision::AllowedByUser)
    );
    assert_eq!(record.duration_ms, Some(42));
    assert!(!record.had_error);
    assert_eq!(record.schema_version, 1);
}

#[test]
fn execution_record_shell_digests_use_raw_runtime_output() {
    let deps: Arc<dyn QueryDeps> = Arc::new(
        MockDeps::from_steps(Vec::<MockStreamStep>::new()).with_runtime_identity(
            "session-1",
            "agent-2",
            None,
            None,
        ),
    );
    let turn_context = RuntimeRecordTurnContext::default();
    let cases = vec![
        (
            "success",
            "raw stdout\n".to_string(),
            "raw stderr\n".to_string(),
            Some(0),
            false,
        ),
        (
            "failure",
            "partial stdout".to_string(),
            "fatal stderr".to_string(),
            Some(2),
            false,
        ),
        (
            "empty stdout",
            String::new(),
            "stderr only".to_string(),
            Some(0),
            false,
        ),
        (
            "empty stderr",
            "stdout only".to_string(),
            String::new(),
            Some(0),
            false,
        ),
        (
            "long output",
            "x".repeat(16_384),
            "e".repeat(8_192),
            Some(0),
            false,
        ),
    ];

    for (name, stdout, stderr, exit_code, is_error) in cases {
        let exec_result = shell_exec_result(
            &stdout,
            &stderr,
            exit_code,
            is_error,
            AgentRuntimePermissionDecision::NotRequired,
        );
        let record = build_execution_record(&deps, &exec_result, &turn_context);

        assert_eq!(
            record.stdout_digest,
            Some(compute_digest(stdout.as_bytes())),
            "{name} stdout digest"
        );
        assert_eq!(
            record.stderr_digest,
            Some(compute_digest(stderr.as_bytes())),
            "{name} stderr digest"
        );
        assert_eq!(
            record.had_error,
            is_error || exit_code.is_some_and(|code| code != 0),
            "{name} error flag"
        );
    }
}

#[tokio::test]
async fn failed_shell_command_emits_execution_record_event() {
    let exec_result = shell_exec_result(
        "raw stdout",
        "raw stderr",
        Some(2),
        false,
        AgentRuntimePermissionDecision::DeniedByPolicy,
    );
    let deps = Arc::new(
        MockDeps::new(vec![
            shell_tool_response("toolu-fail", "exit 2"),
            super::mocks::make_text_response("done"),
        ])
        .with_runtime_identity("session-runtime", "agent-runtime", None, Some("builder"))
        .with_tool_result_override(exec_result),
    );
    let params = super::mocks::make_query_params(vec![super::mocks::make_user_message_for_test(
        "run failing shell",
    )]);

    let _items: Vec<_> = query(params, deps.clone()).collect().await;
    let events = deps.recorded_agent_events();
    let record = events
        .iter()
        .find_map(|event| match event {
            AgentEvent::ExecutionRecord { record, .. } => Some(record.as_ref()),
            _ => None,
        })
        .expect("execution record event");

    assert_eq!(record.session_id, "session-runtime");
    assert_eq!(record.agent_id, "agent-runtime");
    assert_eq!(record.agent_role.as_deref(), Some("builder"));
    assert_eq!(record.tool, "shell");
    assert_eq!(record.tool_use_id.as_deref(), Some("toolu-fail"));
    assert_eq!(record.exit_code, Some(2));
    assert!(record.had_error);
    assert_eq!(record.stdout_digest, Some(compute_digest(b"raw stdout")));
    assert_eq!(record.stderr_digest, Some(compute_digest(b"raw stderr")));
    assert_eq!(
        record.permission_decision,
        Some(AgentRuntimePermissionDecision::DeniedByPolicy)
    );
}
