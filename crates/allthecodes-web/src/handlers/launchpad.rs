//! Launch Pad snapshot persistence handlers.

use std::fs;
use std::path::{Path, PathBuf};

use allthecodes_config::paths;
use allthecodes_protocol::v1::launchpad::{
    LaunchpadSnapshotCreateRequest, LaunchpadSnapshotCreateResponse,
};
use allthecodes_protocol::ApiError as ProtocolApiError;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::Utc;
use serde_json::Value;

pub async fn launchpad_snapshot_create_handler(
    Json(req): Json<LaunchpadSnapshotCreateRequest>,
) -> Response {
    match save_launchpad_snapshot(req.snapshot) {
        Ok(response) => Json(response).into_response(),
        Err(error) => protocol_error_response(error),
    }
}

fn snapshot_dir() -> PathBuf {
    paths::data_root().join("launchpad-snapshots")
}

fn save_launchpad_snapshot(
    snapshot: Value,
) -> Result<LaunchpadSnapshotCreateResponse, ProtocolApiError> {
    if !snapshot.is_object() {
        return Err(ProtocolApiError::BadRequest {
            code: "bad_request",
            message: "snapshot must be a JSON object".to_string(),
        });
    }

    let dir = snapshot_dir();
    fs::create_dir_all(&dir).map_err(|error| ProtocolApiError::Internal {
        message: format!("Failed to create {}: {error}", dir.display()),
    })?;

    let file_name = format!(
        "launchpad-snapshot-{}.json",
        Utc::now().format("%Y-%m-%dT%H-%M-%S-%3f")
    );
    let path = dir.join(&file_name);
    let bytes =
        serde_json::to_vec_pretty(&snapshot).map_err(|error| ProtocolApiError::Internal {
            message: format!("Failed to serialize launchpad snapshot: {error}"),
        })?;
    atomic_write(&path, &bytes).map_err(|error| ProtocolApiError::Internal {
        message: format!("Failed to write {}: {error}", path.display()),
    })?;

    Ok(LaunchpadSnapshotCreateResponse {
        path: path.to_string_lossy().to_string(),
        file_name,
    })
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), std::io::Error> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("launchpad-snapshot.json");
    let tmp_path = dir.join(format!(
        ".{file_name}.{}.{}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
    ));

    fs::write(&tmp_path, bytes)?;
    fs::rename(&tmp_path, path).map_err(|error| {
        let _ = fs::remove_file(&tmp_path);
        error
    })
}

fn protocol_error_response(error: ProtocolApiError) -> Response {
    let status =
        StatusCode::from_u16(error.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (status, Json(error.into_body())).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::http::StatusCode;
    use serde_json::json;
    use serial_test::serial;
    use tempfile::TempDir;

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set_path(key: &'static str, value: &std::path::Path) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(previous) = &self.previous {
                std::env::set_var(self.key, previous);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    async fn response_json(response: Response) -> Value {
        let body = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("response body");
        serde_json::from_slice(&body).expect("json body")
    }

    fn temp_home() -> (TempDir, EnvGuard) {
        let temp = tempfile::tempdir().expect("tempdir");
        let guard = EnvGuard::set_path("ALLTHECODES_HOME", temp.path());
        (temp, guard)
    }

    #[tokio::test]
    #[serial]
    async fn snapshot_create_writes_pretty_json_under_data_root() {
        let (home, _guard) = temp_home();
        let snapshot = json!({
            "version": 2,
            "recordedAt": "2026-06-09T00:00:00.000Z",
            "gridColumns": 8,
            "lanes": [],
            "pads": [],
            "runHistory": [],
        });

        let response = launchpad_snapshot_create_handler(Json(LaunchpadSnapshotCreateRequest {
            snapshot: snapshot.clone(),
        }))
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        let path = body["path"].as_str().expect("path");
        let file_name = body["file_name"].as_str().expect("file name");
        assert!(file_name.starts_with("launchpad-snapshot-"));
        assert!(file_name.ends_with(".json"));
        assert!(Path::new(path).starts_with(home.path().join("launchpad-snapshots")));

        let content = fs::read_to_string(path).expect("snapshot file");
        assert!(content.contains("\n  \"version\": 2"));
        let parsed: Value = serde_json::from_str(&content).expect("valid json");
        assert_eq!(parsed, snapshot);
    }

    #[tokio::test]
    #[serial]
    async fn snapshot_create_rejects_non_object_payload() {
        let (_home, _guard) = temp_home();

        let response = launchpad_snapshot_create_handler(Json(LaunchpadSnapshotCreateRequest {
            snapshot: json!(null),
        }))
        .await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = response_json(response).await;
        assert_eq!(body["code"], json!("bad_request"));
    }
}
