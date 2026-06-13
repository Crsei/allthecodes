//! Backend service dashboard and action handlers.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::extract::{Path as AxumPath, Query};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;

use allthecodes_config::paths;

use allthecodes_protocol::ApiError as ProtocolApiError;

const TARGET_SCHEMA_VERSION: u32 = 1;
const DEFAULT_TOKEN_BUDGET: u32 = 24_000;

static ID_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Deserialize)]
pub struct BackendServicesQuery {
    pub profile_id: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct ServiceActionRequest {
    pub profile_id: Option<String>,
    pub target_id: Option<String>,
    pub dry_run: Option<bool>,
    pub token_budget: Option<u32>,
    pub reason: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BackendServicesResponse {
    pub profile_id: Option<String>,
    pub generated_at: String,
    pub database: ServiceDatabaseStatus,
    pub session_sync: ServiceSessionSyncStatus,
    pub context_compression: ServiceContextCompressionStatus,
    pub agent_bridge: ServiceAgentBridgeStatusResponse,
    pub local_state: ServiceLocalStateStatus,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ServiceActionResponse {
    pub ok: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<serde_json::Value>,
    pub services: BackendServicesResponse,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ServiceDatabaseStatus {
    pub status: String,
    pub schema_version: u32,
    pub target_schema_version: u32,
    pub storage_path: Option<String>,
    pub tables: Vec<ServiceTableStatus>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ServiceTableStatus {
    pub id: String,
    pub name: String,
    pub category: String,
    pub status: String,
    pub record_count: Option<u64>,
    pub schema_version: Option<u32>,
    pub target_schema_version: Option<u32>,
    pub last_migrated_at: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ServiceSessionSyncStatus {
    pub status: String,
    pub runtime_session_id: Option<String>,
    pub database_session_id: Option<String>,
    pub last_synced_at: Option<String>,
    pub pending_count: u32,
    pub conflict_count: u32,
    pub sessions: Vec<ServiceSessionSyncItem>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ServiceSessionSyncItem {
    pub session_id: String,
    pub title: Option<String>,
    pub profile_id: Option<String>,
    pub runtime_state: String,
    pub database_state: String,
    pub last_runtime_at: Option<String>,
    pub last_database_at: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ServiceContextCompressionStatus {
    pub status: String,
    pub default_token_budget: u32,
    pub jobs: Vec<ServiceContextCompressionJob>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ServiceContextCompressionJob {
    pub id: String,
    pub session_id: Option<String>,
    pub room_id: Option<String>,
    pub status: String,
    pub token_budget: u32,
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
    pub summary: Option<String>,
    pub audit_ref: Option<String>,
    pub updated_at: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ServiceAgentBridgeStatusResponse {
    pub status: String,
    pub agents: Vec<ServiceAgentBridgeAgent>,
    pub events: Vec<ServiceAgentBridgeEvent>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ServiceAgentBridgeAgent {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub status: String,
    pub profile_id: Option<String>,
    pub model: Option<String>,
    pub current_task: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ServiceAgentBridgeEvent {
    pub id: String,
    pub kind: String,
    pub status: String,
    pub source: Option<String>,
    pub summary: String,
    pub trace_id: Option<String>,
    pub retryable: Option<bool>,
    pub timestamp: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ServiceLocalStateStatus {
    pub status: String,
    pub state_path: Option<String>,
    pub schema_version: u32,
    pub target_schema_version: u32,
    pub migrations: Vec<ServiceMigrationStep>,
    pub backups: Vec<ServiceBackupSummary>,
    pub last_backup: Option<ServiceBackupSummary>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceMigrationStep {
    pub id: String,
    pub from_version: Option<u32>,
    pub to_version: u32,
    pub status: String,
    pub description: Option<String>,
    pub applied_at: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceBackupSummary {
    pub id: String,
    pub path: String,
    pub created_at: String,
    pub size_bytes: Option<u64>,
    pub schema_version: Option<u32>,
    pub reason: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct BackupEnvelope {
    id: String,
    created_at: String,
    schema_version: u32,
    target_schema_version: u32,
    profile_id: Option<String>,
    reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SchemaVersionFile {
    schema_version: u32,
}

/// GET /api/backend-services
pub async fn backend_services_handler(Query(query): Query<BackendServicesQuery>) -> Response {
    Json(services_response(query.profile_id)).into_response()
}

/// POST /api/backend-services/sessions/sync
pub async fn backend_services_sessions_sync_handler(
    body: Option<Json<ServiceActionRequest>>,
) -> Response {
    let profile_id = body.and_then(|Json(req)| req.profile_id);
    action_response(
        profile_id,
        "Session sync completed; no runtime session inventory is available in the local MVP.",
    )
}

/// POST /api/backend-services/context-compression/{id}/run
pub async fn backend_services_context_compression_run_handler(
    AxumPath(id): AxumPath<String>,
    body: Option<Json<ServiceActionRequest>>,
) -> Response {
    let req = body.map(|Json(req)| req).unwrap_or_default();
    let target_id = req.target_id.as_deref().unwrap_or(&id);
    let token_budget = req.token_budget.unwrap_or(DEFAULT_TOKEN_BUDGET);
    not_found(format!(
        "Context compression job `{target_id}` was not found for token budget {token_budget}."
    ))
}

/// POST /api/backend-services/agent-bridge/events/{id}/retry
pub async fn backend_services_agent_bridge_retry_handler(
    AxumPath(id): AxumPath<String>,
    body: Option<Json<ServiceActionRequest>>,
) -> Response {
    let req = body.map(|Json(req)| req).unwrap_or_default();
    let target_id = req.target_id.as_deref().unwrap_or(&id);
    not_found(format!("Agent bridge event `{target_id}` was not found."))
}

/// POST /api/backend-services/migrations/run
pub async fn backend_services_migrations_run_handler(
    body: Option<Json<ServiceActionRequest>>,
) -> Response {
    let req = body.map(|Json(req)| req).unwrap_or_default();
    let profile_id = req.profile_id.clone();
    let dry_run = req.dry_run.unwrap_or(false);
    let schema_version = read_schema_version().unwrap_or(0);

    if schema_version >= TARGET_SCHEMA_VERSION {
        return action_response(profile_id, "No backend service migrations are pending.");
    }

    if dry_run {
        return action_response(
            profile_id,
            "Dry run completed; 1 backend service migration would be applied.",
        );
    }

    match apply_local_state_migration() {
        Ok(()) => action_response(profile_id, "Applied 1 backend service migration."),
        Err(error) => internal_error(error),
    }
}

/// POST /api/backend-services/backups
pub async fn backend_services_backup_handler(body: Option<Json<ServiceActionRequest>>) -> Response {
    let req = body.map(|Json(req)| req).unwrap_or_default();
    match create_backup(req.profile_id.clone(), req.reason.clone()) {
        Ok(backup) => Json(ServiceActionResponse {
            ok: true,
            message: format!("Created backend service backup `{}`.", backup.id),
            task: None,
            services: services_response(req.profile_id),
        })
        .into_response(),
        Err(error) => internal_error(error),
    }
}

fn action_response(profile_id: Option<String>, message: impl Into<String>) -> Response {
    Json(ServiceActionResponse {
        ok: true,
        message: message.into(),
        task: None,
        services: services_response(profile_id),
    })
    .into_response()
}

fn services_response(profile_id: Option<String>) -> BackendServicesResponse {
    let local_state = local_state_status();
    BackendServicesResponse {
        profile_id,
        generated_at: now_string(),
        database: database_status(&local_state),
        session_sync: ServiceSessionSyncStatus {
            status: "unknown".to_string(),
            runtime_session_id: None,
            database_session_id: None,
            last_synced_at: None,
            pending_count: 0,
            conflict_count: 0,
            sessions: Vec::new(),
        },
        context_compression: ServiceContextCompressionStatus {
            status: "unknown".to_string(),
            default_token_budget: DEFAULT_TOKEN_BUDGET,
            jobs: Vec::new(),
        },
        agent_bridge: ServiceAgentBridgeStatusResponse {
            status: "unknown".to_string(),
            agents: Vec::new(),
            events: Vec::new(),
        },
        local_state,
    }
}

fn database_status(local_state: &ServiceLocalStateStatus) -> ServiceDatabaseStatus {
    let mut warnings = Vec::new();
    if local_state.schema_version < local_state.target_schema_version {
        warnings.push("Backend service local state has pending migrations.".to_string());
    }
    if !state_root().exists() {
        warnings
            .push("Backend service local state directory has not been initialized.".to_string());
    }

    ServiceDatabaseStatus {
        status: local_state.status.clone(),
        schema_version: local_state.schema_version,
        target_schema_version: local_state.target_schema_version,
        storage_path: Some(state_root().display().to_string()),
        tables: Vec::new(),
        warnings,
    }
}

fn local_state_status() -> ServiceLocalStateStatus {
    let state_path = state_root();
    let schema_version = read_schema_version().unwrap_or(0);
    let backups = list_backups();
    let last_backup = backups.first().cloned();
    let mut warnings = Vec::new();

    if !state_path.exists() {
        warnings
            .push("Backend service local state directory has not been initialized.".to_string());
    }

    let status = if schema_version >= TARGET_SCHEMA_VERSION {
        "ready"
    } else if state_path.exists() {
        "degraded"
    } else {
        "unknown"
    };

    ServiceLocalStateStatus {
        status: status.to_string(),
        state_path: Some(state_path.display().to_string()),
        schema_version,
        target_schema_version: TARGET_SCHEMA_VERSION,
        migrations: migration_steps(schema_version, None),
        backups,
        last_backup,
        warnings,
    }
}

fn migration_steps(schema_version: u32, applied_at: Option<String>) -> Vec<ServiceMigrationStep> {
    if schema_version >= TARGET_SCHEMA_VERSION {
        return Vec::new();
    }

    vec![ServiceMigrationStep {
        id: "web-local-state-v1".to_string(),
        from_version: Some(schema_version),
        to_version: TARGET_SCHEMA_VERSION,
        status: applied_at
            .as_ref()
            .map(|_| "applied")
            .unwrap_or("pending")
            .to_string(),
        description: Some("Initialize backend service local state metadata.".to_string()),
        applied_at,
        error: None,
    }]
}

fn apply_local_state_migration() -> Result<(), String> {
    let root = state_root();
    fs::create_dir_all(&root)
        .map_err(|error| format!("failed to create {}: {error}", root.display()))?;
    let payload = json!({
        "schema_version": TARGET_SCHEMA_VERSION,
        "updated_at": now_string(),
    });
    let path = schema_version_path();
    let bytes = serde_json::to_vec_pretty(&payload)
        .map_err(|error| format!("failed to serialize schema metadata: {error}"))?;
    fs::write(&path, bytes).map_err(|error| format!("failed to write {}: {error}", path.display()))
}

fn create_backup(
    profile_id: Option<String>,
    reason: Option<String>,
) -> Result<ServiceBackupSummary, String> {
    let backup_dir = backups_dir();
    fs::create_dir_all(&backup_dir)
        .map_err(|error| format!("failed to create {}: {error}", backup_dir.display()))?;

    let id = generated_id("backup");
    let path = backup_dir.join(format!("{id}.json"));
    let created_at = now_string();
    let schema_version = read_schema_version().unwrap_or(0);
    let envelope = BackupEnvelope {
        id: id.clone(),
        created_at: created_at.clone(),
        schema_version,
        target_schema_version: TARGET_SCHEMA_VERSION,
        profile_id,
        reason,
    };
    let bytes = serde_json::to_vec_pretty(&envelope)
        .map_err(|error| format!("failed to serialize backup `{id}`: {error}"))?;
    fs::write(&path, bytes)
        .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
    let metadata = fs::metadata(&path)
        .map_err(|error| format!("failed to inspect {}: {error}", path.display()))?;

    Ok(ServiceBackupSummary {
        id,
        path: path.display().to_string(),
        created_at,
        size_bytes: Some(metadata.len()),
        schema_version: Some(schema_version),
        reason: envelope.reason,
    })
}

fn list_backups() -> Vec<ServiceBackupSummary> {
    let dir = backups_dir();
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };

    let mut backups = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if let Some(summary) = backup_summary(path) {
            backups.push(summary);
        }
    }
    backups.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| b.id.cmp(&a.id))
    });
    backups
}

fn backup_summary(path: PathBuf) -> Option<ServiceBackupSummary> {
    let metadata = fs::metadata(&path).ok()?;
    let envelope = fs::read(&path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<BackupEnvelope>(&bytes).ok());
    let id = envelope
        .as_ref()
        .map(|backup| backup.id.clone())
        .or_else(|| {
            path.file_stem()
                .map(|name| name.to_string_lossy().to_string())
        })?;
    let created_at = envelope
        .as_ref()
        .map(|backup| backup.created_at.clone())
        .or_else(|| metadata_time(metadata.created().or_else(|_| metadata.modified()).ok()));

    Some(ServiceBackupSummary {
        id,
        path: path.display().to_string(),
        created_at: created_at.unwrap_or_else(now_string),
        size_bytes: Some(metadata.len()),
        schema_version: envelope.as_ref().map(|backup| backup.schema_version),
        reason: envelope.and_then(|backup| backup.reason),
    })
}

fn read_schema_version() -> Option<u32> {
    let path = schema_version_path();
    let bytes = fs::read(&path).ok()?;
    serde_json::from_slice::<SchemaVersionFile>(&bytes)
        .map(|file| file.schema_version)
        .ok()
        .or_else(|| {
            std::str::from_utf8(&bytes)
                .ok()
                .and_then(|text| text.trim().parse::<u32>().ok())
        })
}

fn state_root() -> PathBuf {
    paths::data_root().join("web")
}

fn backups_dir() -> PathBuf {
    state_root().join("backups")
}

fn schema_version_path() -> PathBuf {
    state_root().join("schema-version.json")
}

fn metadata_time(time: Option<std::time::SystemTime>) -> Option<String> {
    time.map(|time| DateTime::<Utc>::from(time).to_rfc3339())
}

fn now_string() -> String {
    Utc::now().to_rfc3339()
}

fn generated_id(prefix: &str) -> String {
    let count = ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!(
        "{}-{}-{}-{}",
        prefix,
        Utc::now().timestamp_millis(),
        std::process::id(),
        count
    )
}

fn not_found(error: String) -> Response {
    let body = ProtocolApiError::NotFound {
        entity: "backend_service",
        id: error,
    }
    .into_body();
    (StatusCode::NOT_FOUND, Json(body)).into_response()
}

fn internal_error(error: String) -> Response {
    let body = ProtocolApiError::Internal { message: error }.into_body();
    (StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::test_support::*;
    use std::path::Path;

    #[tokio::test]
    #[serial_test::serial]
    async fn backend_services_empty_response_matches_frontend_shape() {
        let (_home, _guard) = temp_home();

        let response = backend_services_handler(Query(BackendServicesQuery {
            profile_id: Some("profile-a".to_string()),
        }))
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["profile_id"], json!("profile-a"));
        assert!(body.get("database").is_some());
        assert!(body.get("session_sync").is_some());
        assert!(body.get("context_compression").is_some());
        assert!(body.get("agent_bridge").is_some());
        assert!(body.get("local_state").is_some());
        assert_eq!(body["database"]["tables"], json!([]));
        assert_eq!(body["session_sync"]["sessions"], json!([]));
        assert_eq!(body["context_compression"]["jobs"], json!([]));
        assert_eq!(body["agent_bridge"]["events"], json!([]));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn session_sync_returns_action_response() {
        let (_home, _guard) = temp_home();

        let response = backend_services_sessions_sync_handler(Some(Json(ServiceActionRequest {
            profile_id: Some("profile-a".to_string()),
            ..ServiceActionRequest::default()
        })))
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["ok"], json!(true));
        assert_eq!(body["services"]["profile_id"], json!("profile-a"));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn migrations_dry_run_does_not_mutate_state() {
        let (home, _guard) = temp_home();

        let response = backend_services_migrations_run_handler(Some(Json(ServiceActionRequest {
            dry_run: Some(true),
            ..ServiceActionRequest::default()
        })))
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(!home.path().join("web").join("schema-version.json").exists());
        let body = response_json(response).await;
        assert_eq!(body["ok"], json!(true));
        assert_eq!(body["services"]["local_state"]["schema_version"], json!(0));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn backup_creates_file_under_web_backups() {
        let (home, _guard) = temp_home();

        let response = backend_services_backup_handler(Some(Json(ServiceActionRequest {
            profile_id: Some("profile-a".to_string()),
            reason: Some("test backup".to_string()),
            ..ServiceActionRequest::default()
        })))
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        let backup_path = body["services"]["local_state"]["last_backup"]["path"]
            .as_str()
            .expect("backup path");
        assert!(Path::new(backup_path).starts_with(home.path().join("web").join("backups")));
        assert!(Path::new(backup_path).is_file());
        assert_eq!(
            body["services"]["local_state"]["last_backup"]["reason"],
            json!("test backup")
        );
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn unknown_compression_id_returns_not_found() {
        let (_home, _guard) = temp_home();

        let response =
            backend_services_context_compression_run_handler(AxumPath("missing".to_string()), None)
                .await
                .into_response();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = response_json(response).await;
        assert_eq!(body["code"], json!("not_found"));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn unknown_agent_event_id_returns_not_found() {
        let (_home, _guard) = temp_home();

        let response =
            backend_services_agent_bridge_retry_handler(AxumPath("missing".to_string()), None)
                .await
                .into_response();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = response_json(response).await;
        assert_eq!(body["code"], json!("not_found"));
    }
}
