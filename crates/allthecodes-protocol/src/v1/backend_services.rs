//! Typed Backend Services dashboard and action contracts.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ServiceHealth {
    Ready,
    Degraded,
    Unavailable,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ServiceIssueCode {
    RuntimeInventoryPartial,
    ProfileOwnershipUnavailable,
    SessionSourceError,
    SessionWriteFailed,
    ActiveTurn,
    RuntimeHistorySourceError,
    CompactionServiceUnavailable,
    RetryServiceUnavailable,
    MigrationServiceUnavailable,
    BackupServiceUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ServiceIssue {
    pub code: ServiceIssueCode,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct BackendServicesQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ServiceActionRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dry_run: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_budget: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct BackendServicesSessionSyncRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dry_run: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct BackendServicesCompressionRunParams {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_budget: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct BackendServicesAgentRetryParams {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct BackendServicesMigrationRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dry_run: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct BackendServicesBackupRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct BackendServicesResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    pub generated_at: String,
    pub database: ServiceDatabaseStatus,
    pub session_sync: ServiceSessionSyncStatus,
    pub context_compression: ServiceContextCompressionStatus,
    pub agent_bridge: ServiceAgentBridgeStatusResponse,
    pub local_state: ServiceLocalStateStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ServiceActionResponse {
    pub ok: bool,
    pub message: String,
    pub observed_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<ServiceTaskReference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reconciliation: Option<ServiceSessionReconciliation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup: Option<ServiceBackupSummary>,
    pub services: BackendServicesResponse,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ServiceTaskReference {
    pub id: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ServiceDatabaseStatus {
    pub status: ServiceHealth,
    pub observed_at: String,
    pub schema_version: u32,
    pub target_schema_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage_path: Option<String>,
    pub tables: Vec<ServiceTableStatus>,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub issues: Vec<ServiceIssue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ServiceTableStatus {
    pub id: String,
    pub name: String,
    pub category: String,
    pub status: ServiceHealth,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_version: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_schema_version: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_migrated_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ServiceInventoryCoverage {
    Full,
    Partial,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ServiceSessionRuntimeState {
    Active,
    Known,
    Missing,
    Stale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ServiceSessionDatabaseState {
    Synced,
    Missing,
    Stale,
    Conflict,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ServiceSessionSyncStatus {
    pub status: ServiceHealth,
    pub observed_at: String,
    pub coverage: ServiceInventoryCoverage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_synced_at: Option<String>,
    pub pending_count: u32,
    pub conflict_count: u32,
    pub sessions: Vec<ServiceSessionSyncItem>,
    #[serde(default)]
    pub issues: Vec<ServiceIssue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ServiceSessionSyncItem {
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    pub runtime_state: ServiceSessionRuntimeState,
    pub database_state: ServiceSessionDatabaseState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_runtime_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_database_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ServiceSessionReconciliation {
    pub reconciliation_id: String,
    pub observed_at: String,
    pub dry_run: bool,
    pub coverage: ServiceInventoryCoverage,
    pub examined: u32,
    pub created: u32,
    pub updated: u32,
    pub conflicts: u32,
    pub skipped: u32,
    pub truncated: bool,
    pub items: Vec<ServiceSessionSyncItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ServiceContextCompressionStatus {
    pub status: ServiceHealth,
    pub observed_at: String,
    pub default_token_budget: u32,
    pub jobs: Vec<ServiceContextCompressionJob>,
    #[serde(default)]
    pub issues: Vec<ServiceIssue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ServiceContextCompressionJob {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub room_id: Option<String>,
    pub status: String,
    pub token_budget: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audit_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ServiceAgentBridgeStatusResponse {
    pub status: ServiceHealth,
    pub observed_at: String,
    pub agents: Vec<ServiceAgentBridgeAgent>,
    pub events: Vec<ServiceAgentBridgeEvent>,
    #[serde(default)]
    pub issues: Vec<ServiceIssue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ServiceAgentBridgeAgent {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_task: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ServiceAgentBridgeEvent {
    pub id: String,
    pub kind: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    pub retryable: bool,
    pub timestamp: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ServiceLocalStateStatus {
    pub status: ServiceHealth,
    pub observed_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_path: Option<String>,
    pub schema_version: u32,
    pub target_schema_version: u32,
    pub migrations: Vec<ServiceMigrationStep>,
    pub backups: Vec<ServiceBackupSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_backup: Option<ServiceBackupSummary>,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub issues: Vec<ServiceIssue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ServiceMigrationStep {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_version: Option<u32>,
    pub to_version: u32,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applied_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ServiceBackupSummary {
    pub id: String,
    pub path: String,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_version: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default)]
    pub included_stores: Vec<String>,
    #[serde(default)]
    pub excluded_stores: Vec<String>,
    pub consistent: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restore_compatibility: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_health_has_no_unknown_success_state() {
        let states = [
            ServiceHealth::Ready,
            ServiceHealth::Degraded,
            ServiceHealth::Unavailable,
            ServiceHealth::Error,
        ];
        let encoded = serde_json::to_value(states).unwrap();

        assert_eq!(
            encoded,
            serde_json::json!(["ready", "degraded", "unavailable", "error"])
        );
    }

    #[test]
    fn action_requests_default_to_non_mutating_optional_fields() {
        let sync: BackendServicesSessionSyncRequest =
            serde_json::from_value(serde_json::json!({})).unwrap();
        let backup: BackendServicesBackupRequest =
            serde_json::from_value(serde_json::json!({})).unwrap();

        assert_eq!(sync.dry_run, None);
        assert_eq!(sync.profile_id, None);
        assert_eq!(backup.reason, None);
    }
}
