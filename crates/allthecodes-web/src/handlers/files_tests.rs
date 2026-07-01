use super::*;
use crate::handlers::test_support::*;
use axum::extract::{Query, State};
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde_json::json;
use serial_test::serial;

#[tokio::test]
#[serial]
async fn files_tree_lists_workspace_root() {
    let project = tempfile::tempdir().expect("project");
    std::fs::write(project.path().join("hello.txt"), b"hello").expect("seed file");
    std::fs::create_dir(project.path().join("sub")).expect("seed dir");
    let state = make_web_state_with_cwd(project.path());

    let response = files_tree_handler(
        State(state),
        Query(FileTreeQuery {
            path: None,
            profile_id: None,
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    let entries = body["entries"].as_array().expect("entries");
    assert!(entries.iter().any(|e| e["name"] == json!("hello.txt")));
    assert!(entries.iter().any(|e| e["name"] == json!("sub")));
    assert_eq!(body["truncated"], json!(false));
}

#[tokio::test]
#[serial]
async fn files_tree_rejects_path_traversal() {
    let project = tempfile::tempdir().expect("project");
    let state = make_web_state_with_cwd(project.path());

    let response = files_tree_handler(
        State(state),
        Query(FileTreeQuery {
            path: Some("../etc".to_string()),
            profile_id: None,
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let body = response_json(response).await;
    assert_eq!(body["code"], json!("path_traversal"));
}

#[tokio::test]
#[serial]
async fn files_stat_returns_file_metadata() {
    let project = tempfile::tempdir().expect("project");
    let file_path = project.path().join("test.txt");
    std::fs::write(&file_path, b"content").expect("seed file");
    let state = make_web_state_with_cwd(project.path());

    let response = files_stat_handler(
        State(state),
        Query(FileStatQuery {
            path: Some("test.txt".to_string()),
            profile_id: None,
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["exists"], json!(true));
    assert_eq!(body["is_file"], json!(true));
    assert_eq!(body["size"], json!(7));
    assert!(body["hash"].as_str().unwrap().len() >= 10);
}

#[tokio::test]
#[serial]
async fn files_stat_with_profile_id_echoes_it() {
    let project = tempfile::tempdir().expect("project");
    std::fs::write(project.path().join("a.txt"), b"data").expect("seed file");
    let state = make_web_state_with_cwd(project.path());

    let response = files_stat_handler(
        State(state),
        Query(FileStatQuery {
            path: Some("a.txt".to_string()),
            profile_id: Some("prof-99".to_string()),
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["profile_id"], json!("prof-99"));
}

#[tokio::test]
#[serial]
async fn files_read_returns_text_content() {
    let project = tempfile::tempdir().expect("project");
    std::fs::write(project.path().join("readme.md"), b"# Hello\n\nWorld!").expect("seed file");
    let state = make_web_state_with_cwd(project.path());

    let response = files_read_handler(
        State(state),
        Query(FileReadQuery {
            path: Some("readme.md".to_string()),
            profile_id: None,
            max_bytes: None,
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["content"], json!("# Hello\n\nWorld!"));
    assert_eq!(body["is_binary"], json!(false));
    assert_eq!(body["lines"], json!(3));
    assert_eq!(body["truncated"], json!(false));
}

#[tokio::test]
#[serial]
async fn files_read_detects_binary() {
    let project = tempfile::tempdir().expect("project");
    let binary = vec![0x00, 0x01, 0x02, 0x03];
    std::fs::write(project.path().join("binary.bin"), &binary).expect("seed file");
    let state = make_web_state_with_cwd(project.path());

    let response = files_read_handler(
        State(state),
        Query(FileReadQuery {
            path: Some("binary.bin".to_string()),
            profile_id: None,
            max_bytes: None,
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["is_binary"], json!(true));
    assert_eq!(body["content"], json!(""));
}

#[tokio::test]
#[serial]
async fn files_read_truncates_large_content() {
    let project = tempfile::tempdir().expect("project");
    let content = "x".repeat(2000);
    std::fs::write(project.path().join("large.txt"), &content).expect("seed file");
    let state = make_web_state_with_cwd(project.path());

    let response = files_read_handler(
        State(state),
        Query(FileReadQuery {
            path: Some("large.txt".to_string()),
            profile_id: None,
            max_bytes: Some(100),
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["content"].as_str().unwrap().len(), 100);
    assert_eq!(body["truncated"], json!(true));
}

#[tokio::test]
#[serial]
async fn files_write_creates_new_file() {
    let project = tempfile::tempdir().expect("project");
    let state = make_web_state_with_cwd(project.path());

    let response = files_write_handler(
        State(state),
        Json(FileWriteRequest {
            path: "new.txt".to_string(),
            content: "fresh content".to_string(),
            hash: None,
            overwrite: Some(false),
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["ok"], json!(true));
    assert!(project.path().join("new.txt").exists());
    assert_eq!(
        std::fs::read_to_string(project.path().join("new.txt")).expect("read"),
        "fresh content"
    );
}

#[tokio::test]
#[serial]
async fn files_write_rejects_overwrite_without_flag() {
    let project = tempfile::tempdir().expect("project");
    std::fs::write(project.path().join("existing.txt"), b"original").expect("seed file");
    let state = make_web_state_with_cwd(project.path());

    let response = files_write_handler(
        State(state),
        Json(FileWriteRequest {
            path: "existing.txt".to_string(),
            content: "overwritten".to_string(),
            hash: None,
            overwrite: Some(false),
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body = response_json(response).await;
    assert_eq!(body["code"], json!("conflict"));
    assert_eq!(
        std::fs::read_to_string(project.path().join("existing.txt")).expect("read"),
        "original"
    );
}

#[tokio::test]
#[serial]
async fn files_write_with_overwrite_flag_succeeds() {
    let project = tempfile::tempdir().expect("project");
    std::fs::write(project.path().join("replace.txt"), b"old").expect("seed file");
    let state = make_web_state_with_cwd(project.path());

    let response = files_write_handler(
        State(state),
        Json(FileWriteRequest {
            path: "replace.txt".to_string(),
            content: "new".to_string(),
            hash: None,
            overwrite: Some(true),
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        std::fs::read_to_string(project.path().join("replace.txt")).expect("read"),
        "new"
    );
}

#[tokio::test]
#[serial]
async fn files_write_enforces_hash() {
    let project = tempfile::tempdir().expect("project");
    std::fs::write(project.path().join("hash.txt"), b"original").expect("seed file");
    let state = make_web_state_with_cwd(project.path());

    // Get current hash.
    let hash = crate::handlers::files::file_hash(&project.path().join("hash.txt")).expect("hash");

    // Write with wrong hash.
    let response = files_write_handler(
        State(state.clone()),
        Json(FileWriteRequest {
            path: "hash.txt".to_string(),
            content: "modified".to_string(),
            hash: Some("badbadbad".to_string()),
            overwrite: Some(true),
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body = response_json(response).await;
    assert_eq!(body["code"], json!("conflict"));

    // Write with correct hash.
    let response = files_write_handler(
        State(state),
        Json(FileWriteRequest {
            path: "hash.txt".to_string(),
            content: "modified".to_string(),
            hash: Some(hash),
            overwrite: Some(true),
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
#[serial]
async fn files_read_with_profile_id_echoes_it() {
    let project = tempfile::tempdir().expect("project");
    std::fs::write(project.path().join("echo.txt"), b"data").expect("seed file");
    let state = make_web_state_with_cwd(project.path());

    let response = files_read_handler(
        State(state),
        Query(FileReadQuery {
            path: Some("echo.txt".to_string()),
            profile_id: Some("prof-read".to_string()),
            max_bytes: None,
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["profile_id"], json!("prof-read"));
}

#[tokio::test]
#[serial]
async fn files_preview_returns_text_metadata() {
    let project = tempfile::tempdir().expect("project");
    std::fs::write(project.path().join("readme.md"), b"# Hello\n\nWorld!").expect("seed file");
    let state = make_web_state_with_cwd(project.path());

    let response = files_preview_handler(
        State(state),
        Query(FilePreviewQuery {
            path: Some("readme.md".to_string()),
            profile_id: Some("prof-preview".to_string()),
            max_bytes: Some(7),
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(
        body["path"],
        json!(project.path().join("readme.md").to_string_lossy())
    );
    assert_eq!(body["profile_id"], json!("prof-preview"));
    assert_eq!(body["mime"], json!("text/markdown; charset=utf-8"));
    assert_eq!(body["media_kind"], json!("text"));
    assert_eq!(body["language"], json!("markdown"));
    assert_eq!(body["content"], json!("# Hello"));
    assert_eq!(body["encoding"], json!("utf-8"));
    assert_eq!(body["lines"], json!(1));
    assert_eq!(body["truncated"], json!(true));
    assert_eq!(body["is_binary"], json!(false));
    assert!(body["hash"].as_str().unwrap().len() >= 10);
    assert!(body["modified"].as_str().is_some());
}

#[tokio::test]
#[serial]
async fn files_media_serves_byte_range() {
    let project = tempfile::tempdir().expect("project");
    std::fs::write(project.path().join("media.txt"), b"hello world").expect("seed file");
    let state = make_web_state_with_cwd(project.path());
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        header::RANGE,
        axum::http::HeaderValue::from_static("bytes=0-4"),
    );

    let response = files_media_handler(
        State(state),
        headers,
        Query(FileMediaQuery {
            path: Some("media.txt".to_string()),
            profile_id: None,
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        response.headers().get(header::CONTENT_RANGE).unwrap(),
        "bytes 0-4/11"
    );
    assert_eq!(
        response.headers().get(header::ACCEPT_RANGES).unwrap(),
        "bytes"
    );
    assert_eq!(response.headers().get(header::CONTENT_LENGTH).unwrap(), "5");
    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("body");
    assert_eq!(&body[..], b"hello");
}

#[tokio::test]
#[serial]
async fn files_mkdir_creates_directory() {
    let project = tempfile::tempdir().expect("project");
    let state = make_web_state_with_cwd(project.path());

    let response = files_mkdir_handler(
        State(state),
        Json(FileMkdirRequest {
            path: "newdir".to_string(),
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    assert!(project.path().join("newdir").is_dir());
}

#[tokio::test]
#[serial]
async fn files_mkdir_creates_nested_directories() {
    let project = tempfile::tempdir().expect("project");
    let state = make_web_state_with_cwd(project.path());

    let response = files_mkdir_handler(
        State(state),
        Json(FileMkdirRequest {
            path: "a/b/c/d".to_string(),
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    assert!(project.path().join("a/b/c/d").is_dir());
}

#[tokio::test]
#[serial]
async fn files_mkdir_idempotent_on_existing() {
    let project = tempfile::tempdir().expect("project");
    std::fs::create_dir(project.path().join("exists")).expect("seed dir");
    let state = make_web_state_with_cwd(project.path());

    let response = files_mkdir_handler(
        State(state),
        Json(FileMkdirRequest {
            path: "exists".to_string(),
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
#[serial]
async fn files_delete_removes_file() {
    let project = tempfile::tempdir().expect("project");
    std::fs::write(project.path().join("delete_me.txt"), b"bye").expect("seed file");
    let state = make_web_state_with_cwd(project.path());

    let response = files_delete_handler(
        State(state),
        Json(FileDeleteRequest {
            path: "delete_me.txt".to_string(),
            recursive: Some(false),
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    assert!(!project.path().join("delete_me.txt").exists());
}

#[tokio::test]
#[serial]
async fn files_delete_rejects_non_empty_dir_without_recursive() {
    let project = tempfile::tempdir().expect("project");
    std::fs::create_dir(project.path().join("nonempty")).expect("seed dir");
    std::fs::write(project.path().join("nonempty/file.txt"), b"x").expect("seed file");
    let state = make_web_state_with_cwd(project.path());

    let response = files_delete_handler(
        State(state),
        Json(FileDeleteRequest {
            path: "nonempty".to_string(),
            recursive: Some(false),
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(project.path().join("nonempty/file.txt").exists());
}

#[tokio::test]
#[serial]
async fn files_delete_recursive_removes_directory() {
    let project = tempfile::tempdir().expect("project");
    std::fs::create_dir_all(project.path().join("nested/a/b")).expect("seed dirs");
    std::fs::write(project.path().join("nested/a/file.txt"), b"x").expect("seed file");
    let state = make_web_state_with_cwd(project.path());

    let response = files_delete_handler(
        State(state),
        Json(FileDeleteRequest {
            path: "nested".to_string(),
            recursive: Some(true),
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    assert!(!project.path().join("nested").exists());
}

#[tokio::test]
#[serial]
async fn files_copy_duplicates_file() {
    let project = tempfile::tempdir().expect("project");
    std::fs::write(project.path().join("src.txt"), b"copy me").expect("seed file");
    let state = make_web_state_with_cwd(project.path());

    let response = files_copy_handler(
        State(state),
        Json(FileCopyRequest {
            source: "src.txt".to_string(),
            destination: "dst.txt".to_string(),
            overwrite: Some(false),
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    assert!(project.path().join("dst.txt").exists());
    assert_eq!(
        std::fs::read_to_string(project.path().join("dst.txt")).expect("read"),
        "copy me"
    );
}

#[tokio::test]
#[serial]
async fn files_copy_rejects_overwrite_without_flag() {
    let project = tempfile::tempdir().expect("project");
    std::fs::write(project.path().join("src.txt"), b"source").expect("seed file");
    std::fs::write(project.path().join("dst.txt"), b"dest").expect("seed file");
    let state = make_web_state_with_cwd(project.path());

    let response = files_copy_handler(
        State(state),
        Json(FileCopyRequest {
            source: "src.txt".to_string(),
            destination: "dst.txt".to_string(),
            overwrite: Some(false),
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::CONFLICT);
}

#[tokio::test]
#[serial]
async fn files_rename_moves_file() {
    let project = tempfile::tempdir().expect("project");
    std::fs::write(project.path().join("old.txt"), b"rename me").expect("seed file");
    let state = make_web_state_with_cwd(project.path());

    let response = files_rename_handler(
        State(state),
        Json(FileRenameRequest {
            source: "old.txt".to_string(),
            destination: "new.txt".to_string(),
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    assert!(!project.path().join("old.txt").exists());
    assert!(project.path().join("new.txt").exists());
}

#[tokio::test]
#[serial]
async fn files_move_with_overwrite_replaces_destination() {
    let project = tempfile::tempdir().expect("project");
    std::fs::write(project.path().join("source.txt"), b"move me").expect("seed file");
    std::fs::write(project.path().join("dest.txt"), b"old dest").expect("seed file");
    let state = make_web_state_with_cwd(project.path());

    let response = files_move_handler(
        State(state),
        Json(FileMoveRequest {
            source: "source.txt".to_string(),
            destination: "dest.txt".to_string(),
            overwrite: Some(true),
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    assert!(!project.path().join("source.txt").exists());
    assert_eq!(
        std::fs::read_to_string(project.path().join("dest.txt")).expect("read"),
        "move me"
    );
}

#[tokio::test]
#[serial]
async fn files_upload_accepts_multiple_files() {
    let project = tempfile::tempdir().expect("project");
    let state = make_web_state_with_cwd(project.path());

    let response = files_upload_handler(
        State(state),
        Json(FileUploadRequest {
            path: ".".to_string(),
            files: vec![
                FileUploadItem {
                    name: "a.txt".to_string(),
                    content: "file a".to_string(),
                    encoding: None,
                },
                FileUploadItem {
                    name: "b.txt".to_string(),
                    content: "file b".to_string(),
                    encoding: None,
                },
            ],
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["ok"], json!(true));
    assert_eq!(body["files"].as_array().unwrap().len(), 2);
    assert!(project.path().join("a.txt").exists());
    assert!(project.path().join("b.txt").exists());
}

#[tokio::test]
#[serial]
async fn files_download_streams_bytes() {
    let project = tempfile::tempdir().expect("project");
    std::fs::write(project.path().join("dl.txt"), b"download content").expect("seed file");
    let state = make_web_state_with_cwd(project.path());

    let response = files_download_handler(
        State(state),
        Query(FileDownloadQuery {
            path: Some("dl.txt".to_string()),
            profile_id: None,
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "text/plain; charset=utf-8"
    );
    assert!(response
        .headers()
        .get(header::CONTENT_DISPOSITION)
        .unwrap()
        .to_str()
        .unwrap()
        .contains("dl.txt"));
    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("body");
    assert_eq!(&body[..], b"download content");
}

#[tokio::test]
#[serial]
async fn files_endpoints_reject_path_traversal() {
    let project = tempfile::tempdir().expect("project");
    let state = make_web_state_with_cwd(project.path());

    // stat
    let response = files_stat_handler(
        State(state.clone()),
        Query(FileStatQuery {
            path: Some("../../etc/passwd".to_string()),
            profile_id: None,
        }),
    )
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    // read
    let response = files_read_handler(
        State(state.clone()),
        Query(FileReadQuery {
            path: Some("../../etc/passwd".to_string()),
            profile_id: None,
            max_bytes: None,
        }),
    )
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    // write
    let response = files_write_handler(
        State(state.clone()),
        Json(FileWriteRequest {
            path: "../../etc/evil.txt".to_string(),
            content: "evil".to_string(),
            hash: None,
            overwrite: Some(true),
        }),
    )
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
#[serial]
async fn files_tree_with_profile_id_echoes_it() {
    let project = tempfile::tempdir().expect("project");
    let state = make_web_state_with_cwd(project.path());

    let response = files_tree_handler(
        State(state),
        Query(FileTreeQuery {
            path: None,
            profile_id: Some("prof-tree".to_string()),
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["profile_id"], json!("prof-tree"));
}
