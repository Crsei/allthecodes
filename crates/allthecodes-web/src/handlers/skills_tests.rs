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
