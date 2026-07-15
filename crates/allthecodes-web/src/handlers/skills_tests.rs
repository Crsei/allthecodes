use super::*;
use crate::handlers::test_support::*;
use allthecodes_skills::{SkillDefinition, SkillFrontmatter, SkillSource};
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde_json::json;
use serial_test::serial;

fn make_test_skill(name: &str) -> SkillDefinition {
    SkillDefinition {
        name: name.to_string(),
        source: SkillSource::Bundled,
        base_dir: None,
        frontmatter: SkillFrontmatter {
            description: format!("Skill {} description", name),
            version: Some("1.0.0".to_string()),
            user_invocable: true,
            ..Default::default()
        },
        prompt_body: format!("Do the {} thing.", name),
    }
}

#[tokio::test]
#[serial]
async fn skills_list_returns_all_registered_skills() {
    let (_home, _guard) = temp_home();
    allthecodes_skills::clear_skills();
    allthecodes_skills::register_skill(make_test_skill("alpha"));
    allthecodes_skills::register_skill(make_test_skill("beta"));

    let response = skills_list_handler(
        State(make_web_state()),
        Query(SkillsListQuery { profile_id: None }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    let names: Vec<&str> = body["skills"]
        .as_array()
        .expect("skills")
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"alpha"));
    assert!(names.contains(&"beta"));
    assert!(body["skills"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["id"] == json!("alpha")));
    assert!(body["revision"].as_u64().unwrap() > 0);
    allthecodes_skills::clear_skills();
}

#[tokio::test]
#[serial]
async fn skills_list_with_profile_id_echoes_it() {
    let (_home, _guard) = temp_home();
    allthecodes_skills::clear_skills();

    let response = skills_list_handler(
        State(make_web_state()),
        Query(SkillsListQuery {
            profile_id: Some("prof-skills".to_string()),
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["skills"], json!([]));
    allthecodes_skills::clear_skills();
}

#[tokio::test]
#[serial]
async fn skills_detail_returns_skill_info() {
    let (_home, _guard) = temp_home();
    allthecodes_skills::clear_skills();
    allthecodes_skills::register_skill(make_test_skill("detail-test"));

    let response = skills_detail_handler(AxumPath("detail-test".to_string()))
        .await
        .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["skill"]["id"], json!("detail-test"));
    assert_eq!(body["skill"]["name"], json!("detail-test"));
    assert!(body["prompt_body"]
        .as_str()
        .unwrap()
        .contains("Do the detail-test thing"));
    assert_eq!(body["skill"]["source"], json!("Bundled"));
    assert_eq!(body["skill"]["enabled"], json!(false));
    assert_eq!(body["skill"]["pinned"], json!(false));
    allthecodes_skills::clear_skills();
}

#[tokio::test]
#[serial]
async fn skills_detail_missing_returns_404() {
    let (_home, _guard) = temp_home();
    allthecodes_skills::clear_skills();

    let response = skills_detail_handler(AxumPath("nonexistent".to_string()))
        .await
        .into_response();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = response_json(response).await;
    assert_eq!(body["code"], json!("not_found"));
}

#[tokio::test]
#[serial]
async fn skills_files_missing_skill_returns_404() {
    let (_home, _guard) = temp_home();
    allthecodes_skills::clear_skills();

    let response = skills_files_handler(
        AxumPath("nosuch".to_string()),
        Query(SkillFileQuery {
            path: "SKILL.md".to_string(),
            max_bytes: None,
            profile_id: None,
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = response_json(response).await;
    assert_eq!(body["code"], json!("not_found"));
}

#[tokio::test]
#[serial]
async fn skills_files_bundled_skill_has_no_files() {
    let (_home, _guard) = temp_home();
    allthecodes_skills::clear_skills();
    allthecodes_skills::register_skill(make_test_skill("bundled-only"));

    let response = skills_files_handler(
        AxumPath("bundled-only".to_string()),
        Query(SkillFileQuery {
            path: "SKILL.md".to_string(),
            max_bytes: None,
            profile_id: None,
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response_json(response).await;
    assert_eq!(body["code"], json!("bad_request"));
    allthecodes_skills::clear_skills();
}

#[tokio::test]
#[serial]
async fn skills_files_reads_file_from_user_skill() {
    let (_home, _guard) = temp_home();
    allthecodes_skills::clear_skills();

    let skill_dir = tempfile::tempdir().expect("skill dir");
    std::fs::write(skill_dir.path().join("hello.txt"), b"Hello, World!").expect("seed file");

    let skill = SkillDefinition {
        name: "file-skill".to_string(),
        source: SkillSource::User,
        base_dir: Some(skill_dir.path().to_path_buf()),
        frontmatter: SkillFrontmatter {
            description: "File skill".to_string(),
            ..Default::default()
        },
        prompt_body: String::new(),
    };
    allthecodes_skills::register_skill(skill);

    let response = skills_files_handler(
        AxumPath("file-skill".to_string()),
        Query(SkillFileQuery {
            path: "hello.txt".to_string(),
            max_bytes: None,
            profile_id: None,
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["skill_id"], json!("file-skill"));
    assert_eq!(body["path"], json!("hello.txt"));
    assert_eq!(body["content"], json!("Hello, World!"));
    assert_eq!(body["truncated"], json!(false));
    assert_eq!(body["is_binary"], json!(false));
    assert_eq!(body["size"], json!(13));
    assert_eq!(body["media_type"], json!("text/plain"));
    allthecodes_skills::clear_skills();
}

#[tokio::test]
#[serial]
async fn skills_files_rejects_path_traversal() {
    let (_home, _guard) = temp_home();
    allthecodes_skills::clear_skills();

    let skill_dir = tempfile::tempdir().expect("skill dir");
    let skill = SkillDefinition {
        name: "traverse-test".to_string(),
        source: SkillSource::User,
        base_dir: Some(skill_dir.path().to_path_buf()),
        frontmatter: SkillFrontmatter {
            description: "test".to_string(),
            ..Default::default()
        },
        prompt_body: String::new(),
    };
    allthecodes_skills::register_skill(skill);

    let response = skills_files_handler(
        AxumPath("traverse-test".to_string()),
        Query(SkillFileQuery {
            path: "../../etc/passwd".to_string(),
            max_bytes: None,
            profile_id: None,
        }),
    )
    .await
    .into_response();

    // Either forbidden or not_found (the canonicalize might resolve then
    // the starts_with check catches it, or the existence check catches it first)
    let status = response.status();
    assert!(
        status == StatusCode::FORBIDDEN || status == StatusCode::NOT_FOUND,
        "expected 403 or 404, got {}",
        status
    );
    allthecodes_skills::clear_skills();
}

#[tokio::test]
#[serial]
async fn skills_files_rejects_hidden_files() {
    let (_home, _guard) = temp_home();
    allthecodes_skills::clear_skills();

    let skill_dir = tempfile::tempdir().expect("skill dir");
    std::fs::write(skill_dir.path().join(".secret"), b"secret data").expect("seed hidden file");

    let skill = SkillDefinition {
        name: "hidden-test".to_string(),
        source: SkillSource::User,
        base_dir: Some(skill_dir.path().to_path_buf()),
        frontmatter: SkillFrontmatter {
            description: "test".to_string(),
            ..Default::default()
        },
        prompt_body: String::new(),
    };
    allthecodes_skills::register_skill(skill);

    let response = skills_files_handler(
        AxumPath("hidden-test".to_string()),
        Query(SkillFileQuery {
            path: ".secret".to_string(),
            max_bytes: None,
            profile_id: None,
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let body = response_json(response).await;
    assert_eq!(body["code"], json!("path_traversal"));
    allthecodes_skills::clear_skills();
}

fn stage_project_proposal(cwd: &std::path::Path, name: &str) -> allthecodes_skills::SkillProposal {
    allthecodes_skills::stage_skill_proposal(
        allthecodes_skills::SkillProposalDraft {
            action: allthecodes_skills::SkillProposalAction::Create,
            scope: allthecodes_skills::SkillProposalScope::Project,
            skill_name: name.to_string(),
            source_session_id: Some("web-skills-session".to_string()),
            markdown: format!("---\nname: {name}\ndescription: Review {name}.\n---\n\nUse {name}."),
        },
        cwd,
    )
    .unwrap()
}

#[tokio::test]
#[serial]
async fn skill_proposal_handlers_list_detail_and_diff_without_host_paths() {
    let (_home, _guard) = temp_home();
    let project = tempfile::tempdir().unwrap();
    let proposal = stage_project_proposal(project.path(), "web-proposal");
    let proposal_id = format!("native:project:{}", proposal.id);
    let state = make_web_state_with_cwd(project.path());

    let response = skill_proposals_list_handler(
        State(state.clone()),
        Query(SkillProposalListQuery::default()),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["proposals"][0]["proposal_id"], proposal_id);
    assert_eq!(body["proposals"][0]["source"], "native");
    assert_eq!(
        body["proposals"][0]["relative_target"],
        ".allthecodes/skills/web-proposal/SKILL.md"
    );
    assert!(!body
        .to_string()
        .contains(&project.path().display().to_string()));

    let response =
        skill_proposal_detail_handler(AxumPath(proposal_id.clone()), State(state.clone())).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["proposal"]["proposal_id"], proposal_id);
    assert!(body["markdown"].as_str().unwrap().contains("web-proposal"));
    assert!(!body
        .to_string()
        .contains(&project.path().display().to_string()));

    let response = skill_proposal_diff_handler(AxumPath(proposal_id), State(state)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert!(body["diff"].as_str().unwrap().contains("web-proposal"));
    assert!(!body
        .to_string()
        .contains(&project.path().display().to_string()));
}

#[tokio::test]
#[serial]
async fn skill_proposal_approve_is_atomic_and_idempotent() {
    let (_home, _guard) = temp_home();
    let project = tempfile::tempdir().unwrap();
    let proposal = stage_project_proposal(project.path(), "approved-web-proposal");
    let proposal_id = format!("native:project:{}", proposal.id);
    let state = make_web_state_with_cwd(project.path());
    let detail =
        allthecodes_engine::services::skill_proposals::SkillProposalService::trusted_local(
            project.path(),
        )
        .detail(&proposal_id)
        .unwrap();
    let request = SkillProposalMutationRequest {
        request_id: "web-approve-request".to_string(),
        expected_proposal_digest: detail.summary.proposal_digest,
        expected_target_digest: SkillProposalExpectedTarget::Absent,
    };

    let response = skill_proposal_approve_handler(
        AxumPath(proposal_id.clone()),
        State(state.clone()),
        Json(request.clone()),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["disposition"], "approved");
    assert_eq!(body["idempotent_replay"], false);
    assert!(project
        .path()
        .join(".allthecodes/skills/approved-web-proposal/SKILL.md")
        .is_file());

    let response =
        skill_proposal_approve_handler(AxumPath(proposal_id), State(state), Json(request)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["idempotent_replay"], true);
}

#[tokio::test]
#[serial]
async fn skill_proposal_api_excludes_workflow_warnings_without_consuming_them() {
    let (_home, _guard) = temp_home();
    let project = tempfile::tempdir().unwrap();
    let warning = allthecodes_engine::services::background_review::stage_background_review_if_due(
        allthecodes_engine::services::background_review::BackgroundReviewInput {
            source_session_id: "web-workflow-warning".to_string(),
            cwd: project.path().display().to_string(),
            turn_count: 1,
            replay_seq_start: None,
            replay_seq_end: None,
            recent_summary: "warning must stay in the memory domain".to_string(),
            tool_errors: Vec::new(),
            similar_session_hits: Vec::new(),
        },
        &allthecodes_engine::services::background_review::BackgroundReviewConfig {
            enabled: true,
            turn_threshold: 1,
        },
    )
    .unwrap()
    .unwrap();
    let state = make_web_state_with_cwd(project.path());

    let response = skill_proposals_list_handler(
        State(state.clone()),
        Query(SkillProposalListQuery::default()),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response_json(response).await["proposals"], json!([]));

    let response = skill_proposal_detail_handler(
        AxumPath(format!("background_review:{}", warning.id)),
        State(state),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(
        allthecodes_engine::services::background_review::load_background_review_proposal(
            &warning.id
        )
        .is_ok()
    );
}

#[tokio::test]
#[serial]
async fn skill_proposal_handlers_preserve_limit_size_and_store_error_statuses() {
    let (_home, _guard) = temp_home();
    let project = tempfile::tempdir().unwrap();
    let state = make_web_state_with_cwd(project.path());

    let response = skill_proposals_list_handler(
        State(state.clone()),
        Query(SkillProposalListQuery {
            limit: Some(101),
            ..Default::default()
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(response_json(response).await["code"], "invalid_limit");

    let proposal_id = "oversized-proposal";
    let proposal = allthecodes_skills::SkillProposal {
        id: proposal_id.to_string(),
        action: allthecodes_skills::SkillProposalAction::Create,
        scope: allthecodes_skills::SkillProposalScope::Project,
        skill_name: "oversized-skill".to_string(),
        source_session_id: None,
        proposed_path: project
            .path()
            .join(".allthecodes/skills/oversized-skill/SKILL.md"),
        markdown: "x".repeat(allthecodes_skills::proposals::MAX_PROPOSAL_MARKDOWN_BYTES + 1),
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    let owner = project.path().join(".allthecodes/skill_proposals");
    std::fs::create_dir_all(&owner).unwrap();
    std::fs::write(
        owner.join(format!("{proposal_id}.json")),
        serde_json::to_vec(&proposal).unwrap(),
    )
    .unwrap();
    let response = skill_proposal_detail_handler(
        AxumPath(format!("native:project:{proposal_id}")),
        State(state.clone()),
    )
    .await;
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(response_json(response).await["code"], "proposal_too_large");

    std::fs::remove_dir_all(&owner).unwrap();
    std::fs::write(&owner, b"not a directory").unwrap();
    let response =
        skill_proposals_list_handler(State(state), Query(SkillProposalListQuery::default())).await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        response_json(response).await["code"],
        "proposal_store_unavailable"
    );
}

#[tokio::test]
#[serial]
async fn skills_files_rejects_directory() {
    let (_home, _guard) = temp_home();
    allthecodes_skills::clear_skills();

    let skill_dir = tempfile::tempdir().expect("skill dir");

    let skill = SkillDefinition {
        name: "dir-test".to_string(),
        source: SkillSource::User,
        base_dir: Some(skill_dir.path().to_path_buf()),
        frontmatter: SkillFrontmatter {
            description: "test".to_string(),
            ..Default::default()
        },
        prompt_body: String::new(),
    };
    allthecodes_skills::register_skill(skill);

    // Passing the skill dir itself as a path should be rejected because
    // it's a directory.
    let response = skills_files_handler(
        AxumPath("dir-test".to_string()),
        Query(SkillFileQuery {
            path: ".".to_string(),
            max_bytes: None,
            profile_id: None,
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response_json(response).await;
    assert_eq!(body["code"], json!("bad_request"));
    allthecodes_skills::clear_skills();
}

#[tokio::test]
#[serial]
async fn skills_files_echoes_profile_id() {
    let (_home, _guard) = temp_home();
    allthecodes_skills::clear_skills();

    let skill_dir = tempfile::tempdir().expect("skill dir");
    std::fs::write(skill_dir.path().join("readme.md"), b"content").expect("seed file");

    let skill = SkillDefinition {
        name: "profile-echo".to_string(),
        source: SkillSource::User,
        base_dir: Some(skill_dir.path().to_path_buf()),
        frontmatter: SkillFrontmatter {
            description: "test".to_string(),
            ..Default::default()
        },
        prompt_body: String::new(),
    };
    allthecodes_skills::register_skill(skill);

    let response = skills_files_handler(
        AxumPath("profile-echo".to_string()),
        Query(SkillFileQuery {
            path: "readme.md".to_string(),
            max_bytes: None,
            profile_id: Some("prof-file".to_string()),
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["profile_id"], json!("prof-file"));
    allthecodes_skills::clear_skills();
}

#[tokio::test]
#[serial]
async fn skills_patch_persists_enabled_and_pinned() {
    let (_home, _guard) = temp_home();
    allthecodes_skills::clear_skills();
    allthecodes_skills::register_skill(make_test_skill("patchable"));

    let response = skills_patch_handler(
        AxumPath("patchable".to_string()),
        Json(SkillPatchRequest {
            enabled: Some(true),
            pinned: Some(true),
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["ok"], json!(true));
    assert_eq!(body["skill"]["id"], json!("patchable"));
    assert_eq!(body["skill"]["enabled"], json!(true));
    assert_eq!(body["skill"]["pinned"], json!(true));

    // Verify it persisted by checking detail
    let response = skills_detail_handler(AxumPath("patchable".to_string()))
        .await
        .into_response();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["skill"]["enabled"], json!(true));
    assert_eq!(body["skill"]["pinned"], json!(true));
    allthecodes_skills::clear_skills();
}

#[tokio::test]
#[serial]
async fn skills_patch_missing_skill_returns_404() {
    let (_home, _guard) = temp_home();
    allthecodes_skills::clear_skills();

    let response = skills_patch_handler(
        AxumPath("does-not-exist".to_string()),
        Json(SkillPatchRequest {
            enabled: Some(true),
            pinned: None,
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = response_json(response).await;
    assert_eq!(body["code"], json!("not_found"));
}

#[tokio::test]
#[serial]
async fn skills_patch_partial_update_only_changes_provided_fields() {
    let (_home, _guard) = temp_home();
    allthecodes_skills::clear_skills();
    allthecodes_skills::register_skill(make_test_skill("partial"));

    // First enable the skill
    let response = skills_patch_handler(
        AxumPath("partial".to_string()),
        Json(SkillPatchRequest {
            enabled: Some(true),
            pinned: Some(true),
        }),
    )
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::OK);

    // Now only change pinned to false, enabled should remain true
    let response = skills_patch_handler(
        AxumPath("partial".to_string()),
        Json(SkillPatchRequest {
            enabled: None,
            pinned: Some(false),
        }),
    )
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["skill"]["enabled"], json!(true));
    assert_eq!(body["skill"]["pinned"], json!(false));
    allthecodes_skills::clear_skills();
}
