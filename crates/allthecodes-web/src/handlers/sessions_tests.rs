use super::*;
use crate::handlers::test_support::*;
use allthecodes_types::message::{
    AssistantMessage, ContentBlock, Message, MessageContent, UserMessage,
};
use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde_json::json;
use serial_test::serial;
use std::io::Write;
use std::path::PathBuf;
use uuid::Uuid;

fn user_message(uuid: &str, timestamp: i64, text: &str) -> Message {
    Message::User(UserMessage {
        uuid: Uuid::parse_str(uuid).unwrap(),
        timestamp,
        role: "user".into(),
        content: MessageContent::Text(text.into()),
        is_meta: false,
        tool_use_result: None,
        source_tool_assistant_uuid: None,
    })
}

fn assistant_message(uuid: &str, timestamp: i64, text: &str) -> Message {
    Message::Assistant(AssistantMessage {
        uuid: Uuid::parse_str(uuid).unwrap(),
        timestamp,
        role: "assistant".into(),
        content: vec![ContentBlock::Text { text: text.into() }],
        usage: None,
        stop_reason: Some("end_turn".into()),
        is_api_error_message: false,
        api_error: None,
        cost_usd: 0.0,
    })
}

fn seed_replay_session(session_id: &str, messages: &[Message]) -> PathBuf {
    allthecodes_session::storage::save_session(session_id, messages, ".")
        .expect("seed session snapshot");
    allthecodes_session::transcript::record_transcript(session_id, messages)
        .expect("seed transcript");
    allthecodes_session::record_replay::create_rollout_from_messages(
        session_id, messages, ".", None, None,
    )
    .expect("seed rollout")
}

#[tokio::test]
#[serial]
async fn session_archive_handler_returns_404_for_missing_session() {
    let (_home, _guard) = temp_home();
    let state = make_web_state();

    let response = session_archive_handler(AxumPath("missing-session".to_string()), State(state))
        .await
        .into_response();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = response_json(response).await;
    assert_eq!(body["code"], json!("not_found"));
    assert_eq!(body["details"]["entity"], json!("session"));
    assert_eq!(body["details"]["id"], json!("missing-session"));
}

#[tokio::test]
#[serial]
async fn session_report_handler_returns_typed_not_generated_state() {
    let (_home, _guard) = temp_home();
    let state = make_web_state();

    let response =
        session_report_handler(AxumPath("report-not-generated".to_string()), State(state))
            .await
            .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["session_id"], json!("report-not-generated"));
    assert_eq!(body["state"], json!("not_generated"));
    assert!(body.get("report").is_none());
    assert!(body.get("integrity_valid").is_none());
}

#[tokio::test]
#[serial]
async fn session_archive_handler_rejects_active_session_with_409() {
    let (_home, _guard) = temp_home();
    let state = make_web_state();
    let active_id = state.engine().current_session_id().to_string();

    let response = session_archive_handler(AxumPath(active_id), State(state))
        .await
        .into_response();

    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body = response_json(response).await;
    assert_eq!(body["code"], json!("conflict"));
    assert!(body["details"]["reason"].is_string());
}

#[tokio::test]
#[serial]
async fn session_archive_handler_archives_inactive_session() {
    let (_home, _guard) = temp_home();
    let state = make_web_state();
    let session_id = "inactive-web-archive";
    allthecodes_session::storage::save_session(session_id, &[], ".")
        .expect("seed inactive session");

    let response = session_archive_handler(AxumPath(session_id.to_string()), State(state))
        .await
        .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["ok"], json!(true));
    assert_eq!(body["message"], json!("Session archived"));
    assert!(!allthecodes_session::storage::get_session_file(session_id).exists());
    assert!(allthecodes_session::storage::get_archived_session_file(session_id).exists());
}

#[tokio::test]
#[serial]
async fn session_detail_handler_returns_current_storage_baseline() {
    let (_home, _guard) = temp_home();
    let state = make_web_state();
    let session_id = "phase0-web-detail";
    let messages = vec![
        Message::User(UserMessage {
            uuid: Uuid::parse_str("30000000-0000-0000-0000-000000000001").unwrap(),
            timestamp: 1,
            role: "user".into(),
            content: MessageContent::Text("phase0 web user".into()),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        }),
        Message::Assistant(AssistantMessage {
            uuid: Uuid::parse_str("30000000-0000-0000-0000-000000000002").unwrap(),
            timestamp: 2,
            role: "assistant".into(),
            content: vec![
                ContentBlock::Text {
                    text: "phase0 web assistant".into(),
                },
                ContentBlock::ToolUse {
                    id: "toolu_web_phase0".into(),
                    name: "Read".into(),
                    input: json!({ "file_path": "src/lib.rs" }),
                },
            ],
            usage: None,
            stop_reason: Some("tool_use".into()),
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        }),
    ];
    allthecodes_session::storage::save_session(session_id, &messages, ".")
        .expect("seed web detail session");

    let response = session_detail_handler(AxumPath(session_id.to_string()), State(state))
        .await
        .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["session_id"], json!(session_id));
    assert_eq!(body["messages"][0]["role"], json!("user"));
    assert_eq!(body["messages"][0]["content"], json!("phase0 web user"));
    assert_eq!(body["messages"][1]["role"], json!("assistant"));
    assert_eq!(
        body["messages"][1]["content"],
        json!("phase0 web assistant")
    );
    assert_eq!(
        body["messages"][1]["content_blocks"][1]["type"],
        json!("tool_use")
    );
}

#[tokio::test]
#[serial]
async fn session_search_handler_returns_matching_hits() {
    let (_home, _guard) = temp_home();
    let state = make_web_state();

    allthecodes_session::storage::save_session(
        "web-session-search-hit",
        &[user_message(
            "30000000-0000-0000-0000-000000000041",
            41,
            "hermes web search target",
        )],
        ".",
    )
    .expect("seed matching session");
    allthecodes_session::storage::save_session(
        "web-session-search-miss",
        &[user_message(
            "30000000-0000-0000-0000-000000000042",
            42,
            "unrelated retained context",
        )],
        ".",
    )
    .expect("seed non-matching session");

    let response = session_search_handler(
        State(state),
        axum::extract::Query(SessionSearchParams {
            query: "hermes web".to_string(),
            limit: Some(10),
            workspace_key: None,
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["query"], json!("hermes web"));
    let hits = body["hits"].as_array().expect("hits array");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0]["session_id"], json!("web-session-search-hit"));
    assert!(hits[0]["snippet"].as_str().unwrap().contains("hermes web"));
}

#[tokio::test]
#[serial]
async fn session_search_handler_filters_by_workspace_key() {
    let (home, _guard) = temp_home();
    let state = make_web_state();
    let project = home.path().join("project");
    let other = home.path().join("other");
    std::fs::create_dir_all(&project).expect("project dir");
    std::fs::create_dir_all(&other).expect("other dir");
    git2::Repository::init(&project).expect("project repo");
    git2::Repository::init(&other).expect("other repo");

    allthecodes_session::storage::save_session(
        "web-session-search-workspace-hit",
        &[user_message(
            "30000000-0000-0000-0000-000000000043",
            43,
            "shared gateway memory",
        )],
        project.to_str().unwrap(),
    )
    .expect("seed project session");
    allthecodes_session::storage::save_session(
        "web-session-search-workspace-miss",
        &[user_message(
            "30000000-0000-0000-0000-000000000044",
            44,
            "shared gateway memory",
        )],
        other.to_str().unwrap(),
    )
    .expect("seed other session");

    let response = session_search_handler(
        State(state),
        axum::extract::Query(SessionSearchParams {
            query: "gateway memory".to_string(),
            limit: Some(10),
            workspace_key: Some(allthecodes_session::storage::workspace_key(&project)),
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    let ids = body["hits"]
        .as_array()
        .expect("hits array")
        .iter()
        .map(|hit| hit["session_id"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(ids, vec!["web-session-search-workspace-hit"]);
}

#[tokio::test]
#[serial]
async fn session_detail_handler_returns_replay_status_fields() {
    let (_home, _guard) = temp_home();
    let state = make_web_state();
    let session_id = "phase5-web-replay-detail";
    let messages = vec![user_message(
        "30000000-0000-0000-0000-000000000011",
        11,
        "phase5 replay user",
    )];
    let rollout_path = seed_replay_session(session_id, &messages);
    allthecodes_session::record_replay::append_record_items(
        session_id,
        vec![
            allthecodes_session::record_replay::RecordItem::PermissionRequest(
                allthecodes_session::record_replay::types::PermissionRequestRecord {
                    request_id: "perm-phase5".into(),
                    tool_name: "Write".into(),
                    context: Some(json!({ "path": "src/lib.rs" })),
                },
            ),
        ],
    )
    .expect("append permission request");
    std::fs::OpenOptions::new()
        .append(true)
        .open(&rollout_path)
        .expect("open rollout")
        .write_all(b"{not valid json}\n")
        .expect("append bad rollout line");

    let response = session_detail_handler(AxumPath(session_id.to_string()), State(state))
        .await
        .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["session_id"], json!(session_id));
    assert_eq!(body["messages"].as_array().unwrap().len(), 1);
    assert_eq!(body["messages"][0]["content"], json!("phase5 replay user"));
    assert_eq!(body["last_seq"], json!(3));
    assert_eq!(
        body["record_schema_version"],
        json!(allthecodes_session::record_replay::RECORD_SCHEMA_VERSION)
    );
    assert!(body["rollout_path"].as_str().unwrap().contains(session_id));
    assert_eq!(body["pending_interactions"].as_array().unwrap().len(), 1);
    assert_eq!(body["pending_interactions"][0]["type"], json!("permission"));
    assert_eq!(
        body["pending_interactions"][0]["request_id"],
        json!("perm-phase5")
    );
    assert_eq!(body["pending_interactions"][0]["label"], json!("Write"));
    assert_eq!(body["pending_interactions"][0]["seq"], json!(3));
    assert_eq!(body["replay_warnings"].as_array().unwrap().len(), 1);
}

#[tokio::test]
#[serial]
async fn session_message_rollback_appends_event_and_preserves_history() {
    let (_home, _guard) = temp_home();
    let state = make_web_state();
    let session_id = "phase6-web-rollback";
    let messages = vec![
        user_message("30000000-0000-0000-0000-000000000021", 21, "keep this user"),
        assistant_message(
            "30000000-0000-0000-0000-000000000022",
            22,
            "remove this assistant",
        ),
        user_message(
            "30000000-0000-0000-0000-000000000023",
            23,
            "remove this user",
        ),
    ];
    let rollout_path = seed_replay_session(session_id, &messages);
    let target_id = messages[0].uuid().to_string();

    let preview = session_message_rollback_preview_handler(
        AxumPath((session_id.to_string(), target_id.clone())),
        State(state.clone()),
        None,
    )
    .await
    .into_response();
    assert_eq!(preview.status(), StatusCode::OK);
    let preview_body = response_json(preview).await;
    assert_eq!(preview_body["available"], json!(true));
    assert_eq!(preview_body["target_seq"], json!(1));
    assert_eq!(preview_body["last_seq"], json!(4));
    assert_eq!(preview_body["removed_message_count"], json!(2));

    let response = session_message_rollback_handler(
        AxumPath((session_id.to_string(), target_id)),
        State(state.clone()),
        None,
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["ok"], json!(true));
    assert_eq!(body["target_seq"], json!(1));
    assert_eq!(body["last_seq"], json!(5));
    assert_eq!(body["visible_message_count"], json!(1));

    let detail = session_detail_handler(AxumPath(session_id.to_string()), State(state))
        .await
        .into_response();
    assert_eq!(detail.status(), StatusCode::OK);
    let detail_body = response_json(detail).await;
    assert_eq!(detail_body["messages"].as_array().unwrap().len(), 1);
    assert_eq!(
        detail_body["messages"][0]["content"],
        json!("keep this user")
    );
    assert_eq!(detail_body["last_seq"], json!(5));

    let read =
        allthecodes_session::record_replay::read_rollout_file(&rollout_path).expect("read rollout");
    assert_eq!(
        read.lines
            .iter()
            .filter(|line| matches!(
                &line.item,
                allthecodes_session::record_replay::RecordItem::Message(_)
            ))
            .count(),
        3
    );
    assert!(read.lines.iter().any(|line| matches!(
        &line.item,
        allthecodes_session::record_replay::RecordItem::Rollback(_)
    )));

    let transcript = std::fs::read_to_string(allthecodes_session::transcript::get_transcript_file(
        session_id,
    ))
    .expect("read transcript");
    assert_eq!(transcript.lines().count(), 1);
}

#[tokio::test]
#[serial]
async fn session_message_branch_records_parent_seq_and_child_rollout() {
    let (_home, _guard) = temp_home();
    let state = make_web_state();
    let session_id = "phase6-web-branch";
    let messages = vec![
        user_message("30000000-0000-0000-0000-000000000031", 31, "branch user"),
        assistant_message(
            "30000000-0000-0000-0000-000000000032",
            32,
            "branch assistant",
        ),
        user_message(
            "30000000-0000-0000-0000-000000000033",
            33,
            "parent continues",
        ),
    ];
    let parent_rollout = seed_replay_session(session_id, &messages);
    let branch_target_id = messages[1].uuid().to_string();

    let response = session_message_branch_handler(
        AxumPath((session_id.to_string(), branch_target_id)),
        State(state),
        None,
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    let child_id = body["session_id"].as_str().unwrap();
    assert!(!child_id.is_empty());

    let parent_read = allthecodes_session::record_replay::read_rollout_file(&parent_rollout)
        .expect("read parent rollout");
    assert!(parent_read.lines.iter().any(|line| matches!(
        &line.item,
        allthecodes_session::record_replay::RecordItem::Branch(branch)
            if branch.parent_session_id == session_id
                && branch.new_session_id == child_id
                && branch.branch_from_seq == 2
    )));
    let parent_reconstructed =
        allthecodes_session::record_replay::reconstruct_recorded_messages(&parent_read.lines);
    assert_eq!(parent_reconstructed.messages.len(), 3);

    let child_rollout = allthecodes_session::record_replay::ensure_rollout_for_session(child_id)
        .expect("child rollout");
    let child_read = allthecodes_session::record_replay::read_rollout_file(&child_rollout)
        .expect("read child rollout");
    let child_reconstructed =
        allthecodes_session::record_replay::reconstruct_recorded_messages(&child_read.lines);
    let metadata = child_reconstructed.metadata.expect("child metadata");
    assert_eq!(metadata.parent_session_id.as_deref(), Some(session_id));
    assert_eq!(metadata.branch_from_seq, Some(2));
    assert_eq!(child_reconstructed.messages.len(), 2);
}

#[tokio::test]
#[serial]
async fn session_new_handler_can_target_known_workspace_cwd() {
    let (_home, _guard) = temp_home();
    let current = tempfile::tempdir().expect("current");
    let target = tempfile::tempdir().expect("target");
    let state = make_web_state_with_cwd(current.path());
    allthecodes_session::storage::save_session(
        "target-session",
        &[],
        target.path().to_str().unwrap(),
    )
    .expect("seed target workspace");
    let workspace_key = allthecodes_session::storage::workspace_key(target.path());

    let response = session_new_handler(
        State(state.clone()),
        Some(Json(NewSessionRequest {
            workspace_key: Some(workspace_key),
            cwd: Some(target.path().to_string_lossy().to_string()),
        })),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        allthecodes_session::storage::workspace_key(std::path::Path::new(state.engine().cwd())),
        allthecodes_session::storage::workspace_key(target.path())
    );
}

#[tokio::test]
#[serial]
async fn session_new_handler_can_target_existing_local_cwd() {
    let (_home, _guard) = temp_home();
    let current = tempfile::tempdir().expect("current");
    let target = tempfile::tempdir().expect("target");
    let state = make_web_state_with_cwd(current.path());

    let response = session_new_handler(
        State(state.clone()),
        Some(Json(NewSessionRequest {
            workspace_key: None,
            cwd: Some(target.path().to_string_lossy().to_string()),
        })),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        allthecodes_session::storage::workspace_key(std::path::Path::new(state.engine().cwd())),
        allthecodes_session::storage::workspace_key(target.path())
    );
}

#[tokio::test]
#[serial]
async fn session_resume_reuses_cached_engine_session_grants() {
    let (_home, _guard) = temp_home();
    let state = make_web_state();
    let session_id = state.engine().current_session_id().to_string();
    allthecodes_session::storage::save_session(&session_id, &[], ".").expect("seed cached session");
    state.engine().update_app_state(|app| {
        app.tool_permission_context
            .grant_session_allow("mcp__computer-use__screenshot");
    });

    let response = session_resume_handler(AxumPath(session_id.clone()), State(state.clone()))
        .await
        .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let app_state = state.engine().app_state();
    assert!(app_state
        .tool_permission_context
        .has_session_grant("mcp__computer-use__screenshot"));
    assert_eq!(state.engine().current_session_id().to_string(), session_id);
}

#[tokio::test]
#[serial]
async fn session_new_inherits_runtime_permissions_but_clears_session_grants() {
    let (_home, _guard) = temp_home();
    let state = make_web_state();
    state.engine().update_app_state(|app| {
        app.tool_permission_context.mode = allthecodes_types::permissions::PermissionMode::Auto;
        app.tool_permission_context
            .always_allow_rules
            .insert("user".into(), vec!["Read".into()]);
        app.tool_permission_context
            .always_deny_rules
            .insert("user".into(), vec!["Write".into()]);
        app.tool_permission_context
            .always_ask_rules
            .insert("user".into(), vec!["Bash".into()]);
        app.tool_permission_context
            .grant_session_allow("mcp__computer-use__screenshot");
    });

    let response = session_new_handler(State(state.clone()), None)
        .await
        .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let app_state = state.engine().app_state();
    assert_eq!(
        app_state.tool_permission_context.mode,
        allthecodes_types::permissions::PermissionMode::Auto
    );
    assert_eq!(
        app_state
            .tool_permission_context
            .always_allow_rules
            .get("user"),
        Some(&vec!["Read".to_string()])
    );
    assert_eq!(
        app_state
            .tool_permission_context
            .always_deny_rules
            .get("user"),
        Some(&vec!["Write".to_string()])
    );
    assert_eq!(
        app_state
            .tool_permission_context
            .always_ask_rules
            .get("user"),
        Some(&vec!["Bash".to_string()])
    );
    assert!(!app_state
        .tool_permission_context
        .has_session_grant("mcp__computer-use__screenshot"));
}

#[tokio::test]
#[serial]
async fn cold_session_engine_inherits_runtime_permissions_but_clears_session_grants() {
    let (_home, _guard) = temp_home();
    let state = make_web_state();
    let session_id = "cold-session-engine";
    allthecodes_session::storage::save_session(session_id, &[], ".").expect("seed cold session");
    state.engine().update_app_state(|app| {
        app.tool_permission_context.mode = allthecodes_types::permissions::PermissionMode::Plan;
        app.tool_permission_context
            .always_allow_rules
            .insert("user".into(), vec!["Read".into()]);
        app.tool_permission_context
            .grant_session_allow("mcp__computer-use__screenshot");
    });

    let engine =
        crate::handlers::sessions::build_engine_for_session(&state, session_id).expect("engine");

    let app_state = engine.app_state();
    assert_eq!(
        app_state.tool_permission_context.mode,
        allthecodes_types::permissions::PermissionMode::Plan
    );
    assert_eq!(
        app_state
            .tool_permission_context
            .always_allow_rules
            .get("user"),
        Some(&vec!["Read".to_string()])
    );
    assert!(!app_state
        .tool_permission_context
        .has_session_grant("mcp__computer-use__screenshot"));
}
