use super::*;
use crate::handlers::test_support::*;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde_json::json;
use serial_test::serial;

use allthecodes_engine::services::background_review::{
    self, BackgroundReviewProposal, BackgroundReviewProposalKind,
};
use allthecodes_session::memdir::{
    CuratedMemoryTarget, CuratedMemoryWrite, MemoryScope, MemoryType,
};

fn write_proposal(proposal: &BackgroundReviewProposal) {
    let path = background_review::proposal_path(&proposal.id);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, serde_json::to_vec_pretty(proposal).unwrap()).unwrap();
}

fn list_query() -> MemoryListQuery {
    MemoryListQuery::default()
}

#[tokio::test]
#[serial]
async fn memory_list_empty_returns_200_and_echoes_legacy_profile_id() {
    let (_home, _guard) = temp_home();
    let project = tempfile::tempdir().unwrap();
    let state = make_web_state_with_cwd(project.path());
    let response = memory_list_handler(
        State(state),
        Query(MemoryListQuery {
            profile_id: Some("prof-42".to_string()),
            ..list_query()
        }),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["profile_id"], json!("prof-42"));
    assert_eq!(body["entries"], json!([]));
}

#[tokio::test]
#[serial]
async fn memory_list_projects_the_same_four_memdir_scopes_used_by_runtime() {
    let (_home, _guard) = temp_home();
    let project = tempfile::tempdir().unwrap();
    let state = make_web_state_with_cwd(project.path());
    for (scope, key) in [
        (MemoryScope::Global, "global-memory"),
        (MemoryScope::Project, "project-memory"),
        (MemoryScope::Team, "team-memory"),
        (MemoryScope::Auto, "auto-memory"),
    ] {
        memdir::write_memory(key, key, "project", scope, project.path()).unwrap();
    }

    let response = memory_list_handler(State(state), Query(list_query())).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    let entries = body["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 4);
    for scope in ["global", "project", "team", "auto"] {
        assert!(entries.iter().any(|entry| entry["scope"] == scope));
    }
}

#[tokio::test]
#[serial]
async fn memory_update_preserves_runtime_provenance_and_omitted_fields() {
    let (_home, _guard) = temp_home();
    let project = tempfile::tempdir().unwrap();
    let state = make_web_state_with_cwd(project.path());
    let original = memdir::write_curated_memory(
        CuratedMemoryWrite {
            target: CuratedMemoryTarget::Project,
            key: "release-contract".to_string(),
            value: "original".to_string(),
            source_session_id: Some("session-1".to_string()),
            approval_id: Some("approval-1".to_string()),
        },
        project.path(),
    )
    .unwrap();

    let response = memory_update_handler(
        AxumPath(encode_memory_id(MemoryScope::Project, &original.key)),
        State(state),
        Json(MemoryUpdateRequest {
            value: Some("updated content".to_string()),
            tags: Some(vec!["  api ".to_string(), "api".to_string()]),
            pinned: Some(true),
            ..MemoryUpdateRequest::default()
        }),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["entry"]["content"], "updated content");
    assert_eq!(body["entry"]["source_session_id"], "session-1");
    assert_eq!(body["entry"]["approval_id"], "approval-1");
    assert_eq!(body["entry"]["type"], "project");
    assert_eq!(body["entry"]["created_at"], original.created_at);
    assert_eq!(body["entry"]["tags"], json!(["api"]));
    assert_eq!(body["entry"]["pinned"], true);

    let persisted =
        memdir::read_memory("release-contract", MemoryScope::Project, project.path()).unwrap();
    assert_eq!(persisted.source_session_id.as_deref(), Some("session-1"));
    assert_eq!(persisted.approval_id.as_deref(), Some("approval-1"));
    assert_eq!(persisted.memory_type, Some(MemoryType::Project));
}

#[tokio::test]
#[serial]
async fn memory_update_validates_opaque_id_and_content_bound() {
    let (_home, _guard) = temp_home();
    let project = tempfile::tempdir().unwrap();
    let state = make_web_state_with_cwd(project.path());

    let invalid = memory_update_handler(
        AxumPath("project:../escape".to_string()),
        State(state.clone()),
        Json(MemoryUpdateRequest::default()),
    )
    .await;
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);

    let too_large = memory_update_handler(
        AxumPath(encode_memory_id(MemoryScope::Project, "missing")),
        State(state),
        Json(MemoryUpdateRequest {
            value: Some("x".repeat(MAX_CONTENT_BYTES + 1)),
            ..MemoryUpdateRequest::default()
        }),
    )
    .await;
    assert_eq!(too_large.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
#[serial]
async fn legacy_migration_is_idempotent_and_stops_reading_the_old_store() {
    let (home, _guard) = temp_home();
    let project = tempfile::tempdir().unwrap();
    let state = make_web_state_with_cwd(project.path());
    let path = legacy_entries_path();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&json!({
            "entries": [{
                "id": "legacy-1",
                "timestamp": "2026-07-15T00:00:00Z",
                "session_id": "legacy-session",
                "workspace": "legacy-workspace",
                "content": "legacy content",
                "tags": ["legacy"],
                "pinned": true,
                "updated_at": "2026-07-16T00:00:00Z"
            }]
        }))
        .unwrap(),
    )
    .unwrap();

    let first = memory_list_handler(State(state.clone()), Query(list_query())).await;
    assert_eq!(first.status(), StatusCode::OK);
    let first = response_json(first).await;
    assert_eq!(first["migration"]["imported"], 1);
    assert_eq!(first["migration"]["completed"], true);
    assert!(!path.exists());
    assert!(home
        .path()
        .join("memory/entries.legacy-backup.json")
        .is_file());

    let second = memory_list_handler(State(state), Query(list_query())).await;
    let second = response_json(second).await;
    assert!(second.get("migration").is_none());
    assert_eq!(second["entries"].as_array().unwrap().len(), 1);
}

#[tokio::test]
#[serial]
async fn legacy_migration_preflights_conflicts_before_writing_any_row() {
    let (_home, _guard) = temp_home();
    let project = tempfile::tempdir().unwrap();
    let state = make_web_state_with_cwd(project.path());
    memdir::write_memory(
        &legacy_key("legacy-conflict"),
        "newer canonical value",
        "legacy",
        MemoryScope::Global,
        project.path(),
    )
    .unwrap();
    let path = legacy_entries_path();
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&json!({
            "entries": [{
                "id": "legacy-conflict",
                "timestamp": "2026-07-15T00:00:00Z",
                "content": "older legacy value",
                "updated_at": "2026-07-16T00:00:00Z"
            }]
        }))
        .unwrap(),
    )
    .unwrap();

    let response = memory_list_handler(State(state), Query(list_query())).await;
    let body = response_json(response).await;
    assert_eq!(body["migration"]["imported"], 0);
    assert_eq!(body["migration"]["completed"], false);
    assert_eq!(body["migration"]["conflicts"].as_array().unwrap().len(), 1);
    assert!(path.is_file());
}

#[tokio::test]
#[serial]
async fn dream_routes_sort_bound_and_reject_noncanonical_dates() {
    let (_home, _guard) = temp_home();
    for (date, body) in [
        ("2026-07-15", "# Older\n\nSource: `/private/old.log`\n"),
        ("2026-07-16", "# Newer\n\nSource: `/private/new.log`\n"),
    ] {
        let date = NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap();
        let path = allthecodes_services::dream::memory_path_for_date(date);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }

    let list = memory_dream_list_handler(Query(MemoryDreamListQuery::default())).await;
    assert_eq!(list.status(), StatusCode::OK);
    let list = response_json(list).await;
    assert_eq!(list["items"][0]["date"], "2026-07-16");
    assert!(!list["items"][0]["preview"]
        .as_str()
        .unwrap()
        .contains("/private/"));

    let detail = memory_dream_detail_handler(AxumPath("2026-07-16".to_string())).await;
    let detail = response_json(detail).await;
    assert!(!detail["markdown"]
        .as_str()
        .unwrap()
        .contains("/private/new.log"));

    let traversal = memory_dream_detail_handler(AxumPath("../../2026-07-16".to_string())).await;
    assert_eq!(traversal.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
#[serial]
async fn proposal_routes_exclude_skill_domain_and_consume_memory_approval_once() {
    let (_home, _guard) = temp_home();
    let project = tempfile::tempdir().unwrap();
    let state = make_web_state_with_cwd(project.path());
    let memory = BackgroundReviewProposal {
        id: "review-memory-web".to_string(),
        source_session_id: "session-memory".to_string(),
        kind: BackgroundReviewProposalKind::MemoryAdd,
        summary: "remember API gate".to_string(),
        payload: json!({
            "cwd": project.path(),
            "memory": {
                "target": "project",
                "key": "api-gate",
                "value": "run protocol freshness check"
            }
        }),
        created_at: chrono::Utc::now(),
    };
    let skill = BackgroundReviewProposal {
        id: "review-skill-web".to_string(),
        source_session_id: "session-skill".to_string(),
        kind: BackgroundReviewProposalKind::SkillCreate,
        summary: "create skill".to_string(),
        payload: json!({"cwd": project.path(), "name": "api-review"}),
        created_at: chrono::Utc::now(),
    };
    write_proposal(&memory);
    write_proposal(&skill);

    let list = memory_proposal_list_handler(
        State(state.clone()),
        Query(MemoryProposalListQuery::default()),
    )
    .await;
    let list = response_json(list).await;
    assert_eq!(list["proposals"].as_array().unwrap().len(), 1);
    assert_eq!(list["proposals"][0]["id"], memory.id);

    let approved = memory_proposal_approve_handler(
        AxumPath(memory.id.clone()),
        State(state.clone()),
    )
    .await;
    assert_eq!(approved.status(), StatusCode::OK);
    let approved = response_json(approved).await;
    assert_eq!(approved["disposition"], "approved");
    assert_eq!(approved["entry"]["approval_id"], memory.id);

    let replay = memory_proposal_approve_handler(
        AxumPath(memory.id.clone()),
        State(state),
    )
    .await;
    assert_eq!(replay.status(), StatusCode::NOT_FOUND);
    assert!(background_review::proposal_path(&skill.id).is_file());
    assert_eq!(
        memdir::list_memories(MemoryScope::Project, project.path())
            .unwrap()
            .len(),
        1
    );
}
