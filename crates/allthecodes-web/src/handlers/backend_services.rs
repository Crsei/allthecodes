//! Backend service dashboard and action handlers.
//!
//! This module only reports evidence obtained from canonical owners. Features
//! that do not yet have a shared action service remain explicitly unavailable;
//! the Web layer does not create substitute migration markers or backup files.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::sync::Arc;

use allthecodes_engine::lifecycle::QueryEngine;
use allthecodes_protocol::v1::backend_services::*;
use allthecodes_protocol::{ApiError as ProtocolApiError, ApiMethod, SerializationScope};
use allthecodes_services::agent_runtime_history::{
    load_dashboard_snapshot, AgentRuntimeAgentStatus,
};
use allthecodes_session::storage::{list_sessions, workspace_key, SessionInfo};
use allthecodes_tasks::{TaskEntry, TaskStatus};
use allthecodes_types::agent_runtime_dashboard::{
    AgentRuntimeDashboardQuery, AgentRuntimeDashboardResponse, AgentRuntimeEventItem,
};
use async_trait::async_trait;
use axum::extract::{Path as AxumPath, Query, State};
use axum::response::Response;
use axum::Json;
use chrono::{TimeZone, Utc};

use crate::api_dispatcher::rest_processor_response;
use crate::processors::Processor;
use crate::serialization::SerializationLayer;
use crate::state::WebState;

const DEFAULT_TOKEN_BUDGET: u32 = 24_000;
const MIN_TOKEN_BUDGET: u32 = 1_024;
const MAX_TOKEN_BUDGET: u32 = 2_000_000;
const DEFAULT_RESULT_LIMIT: usize = 100;
const MAX_RESULT_LIMIT: usize = 200;
const HISTORY_LIMIT: usize = 200;

#[derive(Clone)]
pub struct BackendServicesProcessor {
    state: WebState,
}

impl From<WebState> for BackendServicesProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for BackendServicesProcessor {
    type Request = BackendServicesQuery;
    type Response = BackendServicesResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "backend_services.dashboard"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    async fn handle(&self, query: Self::Request) -> Result<Self::Response, Self::Error> {
        Ok(services_response(
            &self.state,
            query.profile_id,
            bounded_limit(query.limit),
        ))
    }
}

#[derive(Clone)]
pub struct BackendServicesSessionSyncProcessor {
    state: WebState,
}

impl From<WebState> for BackendServicesSessionSyncProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for BackendServicesSessionSyncProcessor {
    type Request = BackendServicesSessionSyncRequest;
    type Response = ServiceActionResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "backend_services.sessions.sync"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    fn serialization_scope(_params: &Self::Request) -> SerializationScope {
        SerializationScope::PerProcess
    }

    async fn handle(&self, request: Self::Request) -> Result<Self::Response, Self::Error> {
        let profile_id = request.profile_id.clone();
        let limit = bounded_limit(request.limit);
        let reconciliation =
            reconcile_sessions(&self.state, request.dry_run.unwrap_or(false), limit)?;
        let observed_at = reconciliation.observed_at.clone();
        let message = if reconciliation.dry_run {
            "Session reconciliation dry run completed with partial runtime coverage."
        } else {
            "Session reconciliation completed with partial runtime coverage."
        };

        Ok(ServiceActionResponse {
            ok: true,
            message: message.to_string(),
            observed_at,
            task: None,
            reconciliation: Some(reconciliation),
            backup: None,
            services: services_response(&self.state, profile_id, limit),
        })
    }
}

#[derive(Clone)]
pub struct BackendServicesCompressionRunProcessor {
    state: WebState,
}

impl From<WebState> for BackendServicesCompressionRunProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for BackendServicesCompressionRunProcessor {
    type Request = BackendServicesCompressionRunParams;
    type Response = ServiceActionResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "backend_services.context_compression.run"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    fn serialization_scope(params: &Self::Request) -> SerializationScope {
        SerializationScope::per_key(params, "id")
    }

    async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error> {
        validate_target_alias(&params.id, params.target_id.as_deref())?;
        let token_budget = params.token_budget.unwrap_or(DEFAULT_TOKEN_BUDGET);
        if !(MIN_TOKEN_BUDGET..=MAX_TOKEN_BUDGET).contains(&token_budget) {
            return Err(ProtocolApiError::Validation {
                field: "token_budget".to_string(),
                message: format!("must be between {MIN_TOKEN_BUDGET} and {MAX_TOKEN_BUDGET}"),
            });
        }

        if !compression_target_exists(&self.state, &params.id)? {
            return Err(ProtocolApiError::NotFound {
                entity: "context_compression_target",
                id: bounded_text(&params.id, 128),
            });
        }

        Err(ProtocolApiError::ServiceUnavailable {
            code: "compaction_service_unavailable",
            message:
                "No shared compaction action service is connected; the target was not mutated."
                    .to_string(),
        })
    }
}

#[derive(Clone)]
pub struct BackendServicesAgentRetryProcessor {
    state: WebState,
}

impl From<WebState> for BackendServicesAgentRetryProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for BackendServicesAgentRetryProcessor {
    type Request = BackendServicesAgentRetryParams;
    type Response = ServiceActionResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "backend_services.agent_bridge.retry"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    fn serialization_scope(params: &Self::Request) -> SerializationScope {
        SerializationScope::per_key(params, "id")
    }

    async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error> {
        validate_target_alias(&params.id, params.target_id.as_deref())?;
        if !agent_event_exists(&self.state, &params.id)? {
            return Err(ProtocolApiError::NotFound {
                entity: "agent_bridge_event",
                id: bounded_text(&params.id, 128),
            });
        }

        Err(ProtocolApiError::Conflict {
            reason: "agent event is not retryable because no durable replay descriptor is stored"
                .to_string(),
        })
    }
}

#[derive(Clone)]
pub struct BackendServicesMigrationProcessor {
    state: WebState,
}

impl From<WebState> for BackendServicesMigrationProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for BackendServicesMigrationProcessor {
    type Request = BackendServicesMigrationRequest;
    type Response = ServiceActionResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "backend_services.migrations.run"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    fn serialization_scope(_params: &Self::Request) -> SerializationScope {
        SerializationScope::PerProcess
    }

    async fn handle(&self, _params: Self::Request) -> Result<Self::Response, Self::Error> {
        Err(ProtocolApiError::ServiceUnavailable {
            code: "migration_service_unavailable",
            message: "No aggregate migration registry covers the canonical local stores; no migration was run."
                .to_string(),
        })
    }
}

#[derive(Clone)]
pub struct BackendServicesBackupProcessor {
    state: WebState,
}

impl From<WebState> for BackendServicesBackupProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for BackendServicesBackupProcessor {
    type Request = BackendServicesBackupRequest;
    type Response = ServiceActionResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "backend_services.backups.create"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    fn serialization_scope(_params: &Self::Request) -> SerializationScope {
        SerializationScope::PerProcess
    }

    async fn handle(&self, _params: Self::Request) -> Result<Self::Response, Self::Error> {
        Err(ProtocolApiError::ServiceUnavailable {
            code: "backup_service_unavailable",
            message: "No consistent snapshot service covers the canonical local stores; no backup was created."
                .to_string(),
        })
    }
}

/// GET /api/backend-services
pub async fn backend_services_handler(
    State(state): State<WebState>,
    Query(query): Query<BackendServicesQuery>,
) -> Response {
    rest_processor_response::<BackendServicesProcessor>(state, ApiMethod::BackendServices, query)
        .await
}

/// POST /api/backend-services/sessions/sync
pub async fn backend_services_sessions_sync_handler(
    State(state): State<WebState>,
    body: Option<Json<BackendServicesSessionSyncRequest>>,
) -> Response {
    rest_processor_response::<BackendServicesSessionSyncProcessor>(
        state,
        ApiMethod::BackendServicesSessionsSync,
        body.map(|Json(body)| body).unwrap_or_default(),
    )
    .await
}

/// POST /api/backend-services/context-compression/{id}/run
pub async fn backend_services_context_compression_run_handler(
    AxumPath(id): AxumPath<String>,
    State(state): State<WebState>,
    body: Option<Json<ServiceActionRequest>>,
) -> Response {
    let body = body.map(|Json(body)| body).unwrap_or_default();
    rest_processor_response::<BackendServicesCompressionRunProcessor>(
        state,
        ApiMethod::BackendServicesContextCompressionRun,
        BackendServicesCompressionRunParams {
            id,
            profile_id: body.profile_id,
            target_id: body.target_id,
            token_budget: body.token_budget,
            idempotency_key: body.idempotency_key,
        },
    )
    .await
}

/// POST /api/backend-services/agent-bridge/events/{id}/retry
pub async fn backend_services_agent_bridge_retry_handler(
    AxumPath(id): AxumPath<String>,
    State(state): State<WebState>,
    body: Option<Json<ServiceActionRequest>>,
) -> Response {
    let body = body.map(|Json(body)| body).unwrap_or_default();
    rest_processor_response::<BackendServicesAgentRetryProcessor>(
        state,
        ApiMethod::BackendServicesAgentBridgeRetry,
        BackendServicesAgentRetryParams {
            id,
            profile_id: body.profile_id,
            target_id: body.target_id,
            idempotency_key: body.idempotency_key,
        },
    )
    .await
}

/// POST /api/backend-services/migrations/run
pub async fn backend_services_migrations_run_handler(
    State(state): State<WebState>,
    body: Option<Json<BackendServicesMigrationRequest>>,
) -> Response {
    rest_processor_response::<BackendServicesMigrationProcessor>(
        state,
        ApiMethod::BackendServicesMigrationsRun,
        body.map(|Json(body)| body).unwrap_or_default(),
    )
    .await
}

/// POST /api/backend-services/backups
pub async fn backend_services_backup_handler(
    State(state): State<WebState>,
    body: Option<Json<BackendServicesBackupRequest>>,
) -> Response {
    rest_processor_response::<BackendServicesBackupProcessor>(
        state,
        ApiMethod::BackendServicesBackups,
        body.map(|Json(body)| body).unwrap_or_default(),
    )
    .await
}

struct RuntimeSession {
    id: String,
    engine: Arc<QueryEngine>,
    current: bool,
    streaming: bool,
}

fn services_response(
    state: &WebState,
    profile_id: Option<String>,
    limit: usize,
) -> BackendServicesResponse {
    let observed_at = now_string();
    let runtime = runtime_sessions(state);
    let persisted = list_sessions();
    let tasks = allthecodes_tasks::global_store().list();
    let history = load_dashboard_snapshot(AgentRuntimeDashboardQuery {
        session_id: Some(state.engine().current_session_id().to_string()),
        limit: Some(HISTORY_LIMIT),
    });

    BackendServicesResponse {
        profile_id: profile_id.clone(),
        generated_at: observed_at.clone(),
        database: database_status(&persisted, &history, &tasks, &observed_at),
        session_sync: session_sync_status(
            &persisted,
            &runtime,
            profile_id.as_deref(),
            limit,
            &observed_at,
        ),
        context_compression: compression_status(&tasks, limit, &observed_at),
        agent_bridge: agent_bridge_status(&history, &tasks, limit, &observed_at),
        local_state: local_state_status(&observed_at),
    }
}

fn runtime_sessions(state: &WebState) -> Vec<RuntimeSession> {
    let current_id = state.engine().current_session_id().to_string();
    let mut engines = state
        .session_engines
        .read()
        .iter()
        .map(|(id, engine)| (id.clone(), engine.clone()))
        .collect::<BTreeMap<_, _>>();
    engines
        .entry(current_id.clone())
        .or_insert_with(|| state.engine());

    engines
        .into_iter()
        .map(|(id, engine)| RuntimeSession {
            streaming: state.is_session_streaming(&id),
            current: id == current_id,
            id,
            engine,
        })
        .collect()
}

fn database_status(
    sessions: &anyhow::Result<Vec<SessionInfo>>,
    history: &anyhow::Result<AgentRuntimeDashboardResponse>,
    tasks: &[TaskEntry],
    observed_at: &str,
) -> ServiceDatabaseStatus {
    let session_status = if sessions.is_ok() {
        ServiceHealth::Ready
    } else {
        ServiceHealth::Error
    };
    let history_status = if history.is_ok() {
        ServiceHealth::Ready
    } else {
        ServiceHealth::Error
    };
    let status = match (sessions.is_ok(), history.is_ok()) {
        (true, true) => ServiceHealth::Ready,
        (false, false) => ServiceHealth::Error,
        _ => ServiceHealth::Degraded,
    };
    let mut issues = Vec::new();
    let mut warnings = Vec::new();
    if sessions.is_err() {
        issues.push(issue(
            ServiceIssueCode::SessionSourceError,
            "The canonical session store could not be queried.",
        ));
        warnings.push("The canonical session store could not be queried.".to_string());
    }
    if history.is_err() {
        issues.push(issue(
            ServiceIssueCode::RuntimeHistorySourceError,
            "The agent runtime history store could not be queried.",
        ));
        warnings.push("The agent runtime history store could not be queried.".to_string());
    }

    let history_records = history.as_ref().ok().map(|snapshot| {
        saturating_u64(
            snapshot
                .events
                .len()
                .saturating_add(snapshot.execution_records.len()),
        )
    });
    ServiceDatabaseStatus {
        status,
        observed_at: observed_at.to_string(),
        schema_version: 0,
        target_schema_version: 0,
        storage_path: None,
        tables: vec![
            ServiceTableStatus {
                id: "sessions".to_string(),
                name: "Session store".to_string(),
                category: "web_sessions".to_string(),
                status: session_status,
                record_count: sessions
                    .as_ref()
                    .ok()
                    .map(|sessions| saturating_u64(sessions.len())),
                schema_version: None,
                target_schema_version: None,
                last_migrated_at: None,
                message: Some(if sessions.is_ok() {
                    "Canonical session metadata was queried.".to_string()
                } else {
                    "Canonical session metadata is unavailable.".to_string()
                }),
            },
            ServiceTableStatus {
                id: "task_store".to_string(),
                name: "Task store".to_string(),
                category: "jobs".to_string(),
                status: ServiceHealth::Ready,
                record_count: Some(saturating_u64(tasks.len())),
                schema_version: None,
                target_schema_version: None,
                last_migrated_at: None,
                message: Some("Canonical task records were queried.".to_string()),
            },
            ServiceTableStatus {
                id: "agent_runtime_history".to_string(),
                name: "Agent runtime history".to_string(),
                category: "profile_metadata".to_string(),
                status: history_status,
                record_count: history_records,
                schema_version: None,
                target_schema_version: None,
                last_migrated_at: None,
                message: Some(if history.is_ok() {
                    "Canonical runtime history was queried.".to_string()
                } else {
                    "Canonical runtime history is unavailable.".to_string()
                }),
            },
        ],
        warnings,
        issues,
    }
}

fn session_sync_status(
    sessions: &anyhow::Result<Vec<SessionInfo>>,
    runtime: &[RuntimeSession],
    profile_id: Option<&str>,
    limit: usize,
    observed_at: &str,
) -> ServiceSessionSyncStatus {
    let current_id = runtime
        .iter()
        .find(|session| session.current)
        .map(|session| session.id.clone());
    let mut issues = vec![issue(
        ServiceIssueCode::RuntimeInventoryPartial,
        "Only the Web process runtime inventory is available.",
    )];
    if profile_id.is_some() {
        issues.push(issue(
            ServiceIssueCode::ProfileOwnershipUnavailable,
            "Persisted sessions do not expose canonical profile ownership.",
        ));
    }

    let Ok(sessions) = sessions else {
        issues.push(issue(
            ServiceIssueCode::SessionSourceError,
            "The canonical session store could not be queried.",
        ));
        let mut items = runtime
            .iter()
            .map(|session| ServiceSessionSyncItem {
                session_id: bounded_text(&session.id, 128),
                title: None,
                profile_id: None,
                runtime_state: if session.current {
                    ServiceSessionRuntimeState::Active
                } else {
                    ServiceSessionRuntimeState::Known
                },
                database_state: ServiceSessionDatabaseState::Missing,
                last_runtime_at: None,
                last_database_at: None,
                message: Some("session_store_unavailable".to_string()),
            })
            .collect::<Vec<_>>();
        let pending_count = saturating_u32(items.len());
        items.truncate(limit);
        return ServiceSessionSyncStatus {
            status: ServiceHealth::Error,
            observed_at: observed_at.to_string(),
            coverage: ServiceInventoryCoverage::Unavailable,
            runtime_session_id: current_id,
            database_session_id: None,
            last_synced_at: None,
            pending_count,
            conflict_count: 0,
            sessions: items,
            issues,
        };
    };

    let persisted = sessions
        .iter()
        .map(|session| (session.session_id.as_str(), session))
        .collect::<HashMap<_, _>>();
    let runtime_by_id = runtime
        .iter()
        .map(|session| (session.id.as_str(), session))
        .collect::<HashMap<_, _>>();
    let mut ids = persisted.keys().copied().collect::<BTreeSet<_>>();
    ids.extend(runtime_by_id.keys().copied());
    let mut pending_count = 0_u32;
    let mut conflict_count = 0_u32;
    let mut items = Vec::with_capacity(ids.len().min(limit));

    for id in ids {
        let runtime_session = runtime_by_id.get(id).copied();
        let persisted_session = persisted.get(id).copied();
        let item = project_session_item(runtime_session, persisted_session);
        match item.database_state {
            ServiceSessionDatabaseState::Conflict => {
                conflict_count = conflict_count.saturating_add(1)
            }
            ServiceSessionDatabaseState::Missing | ServiceSessionDatabaseState::Stale => {
                pending_count = pending_count.saturating_add(1)
            }
            ServiceSessionDatabaseState::Synced if runtime_session.is_none() => {
                pending_count = pending_count.saturating_add(1)
            }
            ServiceSessionDatabaseState::Synced => {}
        }
        if items.len() < limit {
            items.push(item);
        }
    }

    let database_session_id = current_id
        .as_deref()
        .filter(|id| persisted.contains_key(*id))
        .map(ToOwned::to_owned);
    ServiceSessionSyncStatus {
        status: ServiceHealth::Degraded,
        observed_at: observed_at.to_string(),
        coverage: ServiceInventoryCoverage::Partial,
        runtime_session_id: current_id,
        database_session_id,
        last_synced_at: None,
        pending_count,
        conflict_count,
        sessions: items,
        issues,
    }
}

fn project_session_item(
    runtime: Option<&RuntimeSession>,
    persisted: Option<&SessionInfo>,
) -> ServiceSessionSyncItem {
    let id = runtime
        .map(|session| session.id.as_str())
        .or_else(|| persisted.map(|session| session.session_id.as_str()))
        .unwrap_or_default();
    let runtime_state = match runtime {
        Some(session) if session.current => ServiceSessionRuntimeState::Active,
        Some(_) => ServiceSessionRuntimeState::Known,
        None => ServiceSessionRuntimeState::Missing,
    };
    let (database_state, message) = match (runtime, persisted) {
        (Some(runtime), Some(persisted)) if !same_workspace(runtime, persisted) => {
            (ServiceSessionDatabaseState::Conflict, "workspace_conflict")
        }
        (Some(runtime), Some(persisted))
            if runtime.engine.messages().len() != persisted.message_count =>
        {
            (ServiceSessionDatabaseState::Stale, "message_count_changed")
        }
        (Some(_), Some(_)) => (ServiceSessionDatabaseState::Synced, "up_to_date"),
        (Some(_), None) => (ServiceSessionDatabaseState::Missing, "runtime_only"),
        (None, Some(_)) => (ServiceSessionDatabaseState::Synced, "persisted_only"),
        (None, None) => (ServiceSessionDatabaseState::Missing, "missing"),
    };

    ServiceSessionSyncItem {
        session_id: bounded_text(id, 128),
        title: persisted
            .and_then(|session| session.custom_title.as_deref())
            .map(|title| bounded_text(title, 120)),
        profile_id: None,
        runtime_state,
        database_state,
        last_runtime_at: None,
        last_database_at: persisted.and_then(|session| timestamp_seconds(session.last_modified)),
        message: Some(message.to_string()),
    }
}

fn same_workspace(runtime: &RuntimeSession, persisted: &SessionInfo) -> bool {
    let runtime_key = workspace_key(Path::new(runtime.engine.cwd()));
    let persisted_key = if persisted.workspace_key.is_empty() {
        workspace_key(Path::new(&persisted.cwd))
    } else {
        persisted.workspace_key.clone()
    };
    runtime_key == persisted_key
}

fn reconcile_sessions(
    state: &WebState,
    dry_run: bool,
    limit: usize,
) -> Result<ServiceSessionReconciliation, ProtocolApiError> {
    let persisted_sessions = list_sessions().map_err(|_| ProtocolApiError::Internal {
        message: "canonical session store could not be queried".to_string(),
    })?;
    let runtime = runtime_sessions(state);
    let mut persisted = persisted_sessions
        .iter()
        .map(|session| (session.session_id.as_str(), session))
        .collect::<HashMap<_, _>>();
    let mut examined = 0_u32;
    let mut created = 0_u32;
    let mut updated = 0_u32;
    let mut conflicts = 0_u32;
    let mut skipped = 0_u32;
    let mut items = Vec::new();
    let mut seen = BTreeSet::new();

    for runtime_session in &runtime {
        examined = examined.saturating_add(1);
        seen.insert(runtime_session.id.as_str());
        let existing = persisted.remove(runtime_session.id.as_str());
        let mut item = project_session_item(Some(runtime_session), existing);

        match existing {
            Some(existing) if !same_workspace(runtime_session, existing) => {
                conflicts = conflicts.saturating_add(1);
            }
            Some(existing) if existing.message_count != runtime_session.engine.messages().len() => {
                if dry_run {
                    updated = updated.saturating_add(1);
                    item.message = Some("would_refresh_from_runtime".to_string());
                } else if runtime_session.streaming {
                    skipped = skipped.saturating_add(1);
                    item.message = Some("active_turn_not_mutated".to_string());
                } else {
                    let messages = runtime_session.engine.messages();
                    match allthecodes_session::storage::save_session(
                        &runtime_session.id,
                        &messages,
                        runtime_session.engine.cwd(),
                    ) {
                        Ok(()) => {
                            updated = updated.saturating_add(1);
                            item.database_state = ServiceSessionDatabaseState::Synced;
                            item.message = Some("refreshed_from_runtime".to_string());
                        }
                        Err(_) => {
                            conflicts = conflicts.saturating_add(1);
                            item.database_state = ServiceSessionDatabaseState::Conflict;
                            item.message = Some("session_write_failed".to_string());
                        }
                    }
                }
            }
            Some(_) => {
                skipped = skipped.saturating_add(1);
            }
            None if dry_run => {
                created = created.saturating_add(1);
                item.message = Some("would_create_from_runtime".to_string());
            }
            None if runtime_session.streaming => {
                skipped = skipped.saturating_add(1);
                item.message = Some("active_turn_not_mutated".to_string());
            }
            None => {
                let messages = runtime_session.engine.messages();
                match allthecodes_session::storage::save_session(
                    &runtime_session.id,
                    &messages,
                    runtime_session.engine.cwd(),
                ) {
                    Ok(()) => {
                        created = created.saturating_add(1);
                        item.database_state = ServiceSessionDatabaseState::Synced;
                        item.message = Some("created_from_runtime".to_string());
                    }
                    Err(_) => {
                        conflicts = conflicts.saturating_add(1);
                        item.database_state = ServiceSessionDatabaseState::Conflict;
                        item.message = Some("session_write_failed".to_string());
                    }
                }
            }
        }
        items.push(item);
    }

    for persisted_session in persisted_sessions
        .iter()
        .filter(|session| !seen.contains(session.session_id.as_str()))
    {
        examined = examined.saturating_add(1);
        skipped = skipped.saturating_add(1);
        items.push(project_session_item(None, Some(persisted_session)));
    }

    let truncated = items.len() > limit;
    items.truncate(limit);
    Ok(ServiceSessionReconciliation {
        reconciliation_id: uuid::Uuid::new_v4().to_string(),
        observed_at: now_string(),
        dry_run,
        coverage: ServiceInventoryCoverage::Partial,
        examined,
        created,
        updated,
        conflicts,
        skipped,
        truncated,
        items,
    })
}

fn compression_status(
    tasks: &[TaskEntry],
    limit: usize,
    observed_at: &str,
) -> ServiceContextCompressionStatus {
    let mut jobs = tasks
        .iter()
        .filter(|task| is_compression_task(task))
        .map(project_compression_task)
        .collect::<Vec<_>>();
    jobs.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    jobs.truncate(limit);
    let status = if jobs.is_empty() {
        ServiceHealth::Unavailable
    } else {
        ServiceHealth::Degraded
    };

    ServiceContextCompressionStatus {
        status,
        observed_at: observed_at.to_string(),
        default_token_budget: DEFAULT_TOKEN_BUDGET,
        jobs,
        issues: vec![issue(
            ServiceIssueCode::CompactionServiceUnavailable,
            "Canonical task records can be projected, but no shared compaction action service is connected.",
        )],
    }
}

fn is_compression_task(task: &TaskEntry) -> bool {
    task.kind == "context_compression"
        || task
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("service_kind"))
            .and_then(serde_json::Value::as_str)
            == Some("context_compression")
}

fn project_compression_task(task: &TaskEntry) -> ServiceContextCompressionJob {
    let metadata = task.metadata.as_ref();
    ServiceContextCompressionJob {
        id: bounded_text(&task.id, 128),
        session_id: metadata
            .and_then(|value| value.get("session_id"))
            .and_then(serde_json::Value::as_str)
            .map(|value| bounded_text(value, 128)),
        room_id: metadata
            .and_then(|value| value.get("room_id"))
            .and_then(serde_json::Value::as_str)
            .map(|value| bounded_text(value, 128)),
        status: task_status_for_compression(task.status).to_string(),
        token_budget: metadata_u32(metadata, "token_budget").unwrap_or(DEFAULT_TOKEN_BUDGET),
        input_tokens: metadata_u32(metadata, "input_tokens"),
        output_tokens: metadata_u32(metadata, "output_tokens"),
        summary: None,
        audit_ref: None,
        updated_at: timestamp_seconds(task.updated_at),
        error: matches!(task.status, TaskStatus::Failed).then(|| "compression_failed".to_string()),
    }
}

fn task_status_for_compression(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Pending => "queued",
        TaskStatus::InProgress => "running",
        TaskStatus::Completed => "summarized",
        TaskStatus::Failed
        | TaskStatus::Cancelled
        | TaskStatus::Interrupted
        | TaskStatus::Stopped => "failed",
        TaskStatus::Recoverable => "queued",
    }
}

fn agent_bridge_status(
    history: &anyhow::Result<AgentRuntimeDashboardResponse>,
    tasks: &[TaskEntry],
    limit: usize,
    observed_at: &str,
) -> ServiceAgentBridgeStatusResponse {
    let mut agents = BTreeMap::<String, ServiceAgentBridgeAgent>::new();
    let mut events = Vec::new();
    let mut issues = Vec::new();

    if let Ok(snapshot) = history {
        for agent in &snapshot.agents {
            let current_task = tasks
                .iter()
                .find(|task| {
                    task.agent_id.as_deref() == Some(agent.agent_id.as_str())
                        && !task.status.is_terminal()
                })
                .map(|task| bounded_text(&task.id, 128));
            agents.insert(
                agent.agent_id.clone(),
                ServiceAgentBridgeAgent {
                    id: bounded_text(&agent.agent_id, 128),
                    name: bounded_text(&agent.agent_id, 128),
                    kind: "background_agent".to_string(),
                    status: agent_status(agent.status).to_string(),
                    profile_id: None,
                    model: agent.model.as_deref().map(|model| bounded_text(model, 80)),
                    current_task,
                    updated_at: agent.last_event_at_ms.and_then(timestamp_millis),
                },
            );
        }
        events.extend(snapshot.events.iter().map(project_runtime_event));
    } else {
        issues.push(issue(
            ServiceIssueCode::RuntimeHistorySourceError,
            "The canonical agent runtime history store could not be queried.",
        ));
    }

    for task in tasks.iter().filter(|task| task.agent_id.is_some()) {
        let agent_id = task.agent_id.as_deref().unwrap_or_default();
        agents
            .entry(agent_id.to_string())
            .or_insert_with(|| ServiceAgentBridgeAgent {
                id: bounded_text(agent_id, 128),
                name: bounded_text(agent_id, 128),
                kind: "background_agent".to_string(),
                status: task_status_for_agent(task.status).to_string(),
                profile_id: None,
                model: None,
                current_task: (!task.status.is_terminal()).then(|| bounded_text(&task.id, 128)),
                updated_at: timestamp_seconds(task.updated_at),
            });
        events.push(ServiceAgentBridgeEvent {
            id: format!("task:{}", bounded_text(&task.id, 120)),
            kind: "background_agent".to_string(),
            status: task_status_for_event(task.status).to_string(),
            source: task
                .remote_session_id
                .as_deref()
                .map(|id| bounded_text(id, 128)),
            summary: "Agent task lifecycle state".to_string(),
            trace_id: None,
            retryable: false,
            timestamp: timestamp_seconds(task.updated_at)
                .unwrap_or_else(|| observed_at.to_string()),
        });
    }

    let mut agents = agents.into_values().collect::<Vec<_>>();
    agents.sort_by(|left, right| left.id.cmp(&right.id));
    agents.truncate(limit);
    events.sort_by(|left, right| {
        right
            .timestamp
            .cmp(&left.timestamp)
            .then_with(|| right.id.cmp(&left.id))
    });
    events.truncate(limit);
    let status = if history.is_ok() {
        ServiceHealth::Ready
    } else if agents.is_empty() && events.is_empty() {
        ServiceHealth::Error
    } else {
        ServiceHealth::Degraded
    };

    ServiceAgentBridgeStatusResponse {
        status,
        observed_at: observed_at.to_string(),
        agents,
        events,
        issues,
    }
}

fn project_runtime_event(event: &AgentRuntimeEventItem) -> ServiceAgentBridgeEvent {
    ServiceAgentBridgeEvent {
        id: format!("runtime:{}", event.id),
        kind: "background_agent".to_string(),
        status: runtime_event_status(event).to_string(),
        source: event.session_id.as_deref().map(|id| bounded_text(id, 128)),
        summary: runtime_event_summary(&event.kind).to_string(),
        trace_id: None,
        retryable: false,
        timestamp: bounded_text(&event.timestamp, 64),
    }
}

fn runtime_event_status(event: &AgentRuntimeEventItem) -> &'static str {
    match event.kind.as_str() {
        "background_complete" | "completed" => {
            if event
                .payload
                .as_ref()
                .and_then(|payload| payload.get("had_error"))
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
            {
                "failed"
            } else {
                "delivered"
            }
        }
        "error" | "failed" | "cancelled" => "failed",
        "spawned" | "worktree_created" | "warning" => "running",
        _ => "queued",
    }
}

fn runtime_event_summary(kind: &str) -> &'static str {
    match kind {
        "spawned" => "Background agent started",
        "worktree_created" => "Background agent worktree created",
        "background_complete" | "completed" => "Background agent completed",
        "error" | "failed" => "Background agent failed",
        "cancelled" => "Background agent cancelled",
        "warning" => "Background agent warning recorded",
        _ => "Background agent lifecycle event",
    }
}

fn agent_status(status: AgentRuntimeAgentStatus) -> &'static str {
    match status {
        AgentRuntimeAgentStatus::Running => "running",
        AgentRuntimeAgentStatus::Completed => "delivered",
        AgentRuntimeAgentStatus::Failed | AgentRuntimeAgentStatus::Cancelled => "failed",
        AgentRuntimeAgentStatus::Unknown => "idle",
    }
}

fn task_status_for_agent(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Pending | TaskStatus::Recoverable => "queued",
        TaskStatus::InProgress => "running",
        TaskStatus::Completed => "delivered",
        TaskStatus::Failed
        | TaskStatus::Cancelled
        | TaskStatus::Interrupted
        | TaskStatus::Stopped => "failed",
    }
}

fn task_status_for_event(status: TaskStatus) -> &'static str {
    task_status_for_agent(status)
}

fn local_state_status(observed_at: &str) -> ServiceLocalStateStatus {
    ServiceLocalStateStatus {
        status: ServiceHealth::Unavailable,
        observed_at: observed_at.to_string(),
        state_path: None,
        schema_version: 0,
        target_schema_version: 0,
        migrations: Vec::new(),
        backups: Vec::new(),
        last_backup: None,
        warnings: vec![
            "Aggregate migration coverage is unavailable.".to_string(),
            "Consistent backup coverage is unavailable.".to_string(),
        ],
        issues: vec![
            issue(
                ServiceIssueCode::MigrationServiceUnavailable,
                "No aggregate migration registry covers the canonical local stores.",
            ),
            issue(
                ServiceIssueCode::BackupServiceUnavailable,
                "No consistent snapshot service covers the canonical local stores.",
            ),
        ],
    }
}

fn compression_target_exists(state: &WebState, id: &str) -> Result<bool, ProtocolApiError> {
    if state.engine_for_session(id).is_some()
        || allthecodes_tasks::global_store()
            .list()
            .iter()
            .any(|task| is_compression_task(task) && task.id == id)
    {
        return Ok(true);
    }
    list_sessions()
        .map(|sessions| sessions.iter().any(|session| session.session_id == id))
        .map_err(|_| ProtocolApiError::ServiceUnavailable {
            code: "session_catalog_unavailable",
            message: "The canonical session catalog could not be queried.".to_string(),
        })
}

fn agent_event_exists(state: &WebState, id: &str) -> Result<bool, ProtocolApiError> {
    if let Some(task_id) = id.strip_prefix("task:") {
        return Ok(allthecodes_tasks::global_store().get(task_id).is_some());
    }
    let Some(event_id) = id
        .strip_prefix("runtime:")
        .and_then(|value| value.parse::<i64>().ok())
    else {
        return Ok(false);
    };
    let snapshot = load_dashboard_snapshot(AgentRuntimeDashboardQuery {
        session_id: Some(state.engine().current_session_id().to_string()),
        limit: Some(1_000),
    })
    .map_err(|_| ProtocolApiError::ServiceUnavailable {
        code: "runtime_history_unavailable",
        message: "The canonical agent runtime history store could not be queried.".to_string(),
    })?;
    Ok(snapshot.events.iter().any(|event| event.id == event_id))
}

fn validate_target_alias(path_id: &str, target_id: Option<&str>) -> Result<(), ProtocolApiError> {
    if path_id.trim().is_empty() {
        return Err(ProtocolApiError::Validation {
            field: "id".to_string(),
            message: "must not be empty".to_string(),
        });
    }
    if target_id.is_some_and(|target| target != path_id) {
        return Err(ProtocolApiError::BadRequest {
            code: "target_id_mismatch",
            message: "target_id must match the route id".to_string(),
        });
    }
    Ok(())
}

fn metadata_u32(metadata: Option<&serde_json::Value>, key: &str) -> Option<u32> {
    metadata
        .and_then(|value| value.get(key))
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
}

fn issue(code: ServiceIssueCode, message: &str) -> ServiceIssue {
    ServiceIssue {
        code,
        message: message.to_string(),
    }
}

fn bounded_limit(limit: Option<usize>) -> usize {
    limit
        .unwrap_or(DEFAULT_RESULT_LIMIT)
        .clamp(1, MAX_RESULT_LIMIT)
}

fn bounded_text(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn saturating_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn saturating_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

fn timestamp_seconds(value: i64) -> Option<String> {
    Utc.timestamp_opt(value, 0)
        .single()
        .map(|value| value.to_rfc3339())
}

fn timestamp_millis(value: i64) -> Option<String> {
    Utc.timestamp_millis_opt(value)
        .single()
        .map(|value| value.to_rfc3339())
}

fn now_string() -> String {
    Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::test_support::*;
    use allthecodes_services::agent_runtime_history::{
        persist_subagent_event_for_session, RuntimeSubagentEvent,
    };

    #[tokio::test]
    #[serial_test::serial]
    async fn dashboard_projects_real_sessions_tasks_and_runtime_history() {
        let (_home, _guard) = temp_home();
        let project = tempfile::tempdir().expect("project");
        let state = make_web_state_with_cwd(project.path());
        let current_session_id = state.engine().current_session_id().to_string();
        allthecodes_session::storage::save_session(
            "persisted-session",
            &[],
            project.path().to_str().expect("project path"),
        )
        .expect("seed persisted session");
        persist_subagent_event_for_session(
            Some(&current_session_id),
            RuntimeSubagentEvent {
                kind: "spawned",
                agent_id: "agent-dashboard",
                parent_agent_id: None,
                description: Some("secret prompt that must not be exposed"),
                model: Some("test-model"),
                depth: 1,
                background: true,
                payload: None,
            },
        )
        .expect("seed runtime event");

        let dashboard = BackendServicesProcessor::from(state)
            .handle(BackendServicesQuery::default())
            .await
            .expect("dashboard");

        assert!(dashboard
            .session_sync
            .sessions
            .iter()
            .any(|session| session.session_id == "persisted-session"));
        assert!(dashboard
            .session_sync
            .sessions
            .iter()
            .any(|session| session.session_id == current_session_id));
        assert_eq!(dashboard.agent_bridge.agents.len(), 1);
        assert_eq!(dashboard.agent_bridge.events.len(), 1);
        assert!(!dashboard.agent_bridge.events[0]
            .summary
            .contains("secret prompt"));
        assert_eq!(dashboard.database.storage_path, None);
        assert_eq!(dashboard.local_state.state_path, None);
        assert_eq!(dashboard.local_state.status, ServiceHealth::Unavailable);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn session_sync_dry_run_reports_without_writing_then_real_sync_persists() {
        let (_home, _guard) = temp_home();
        let project = tempfile::tempdir().expect("project");
        let state = make_web_state_with_cwd(project.path());
        let session_id = state.engine().current_session_id().to_string();
        assert!(!list_sessions()
            .expect("initial list")
            .iter()
            .any(|session| session.session_id == session_id));

        let dry_run = BackendServicesSessionSyncProcessor::from(state.clone())
            .handle(BackendServicesSessionSyncRequest {
                dry_run: Some(true),
                ..BackendServicesSessionSyncRequest::default()
            })
            .await
            .expect("dry run");
        let dry_result = dry_run.reconciliation.expect("reconciliation");
        assert!(dry_result.dry_run);
        assert_eq!(dry_result.created, 1);
        assert!(!list_sessions()
            .expect("list after dry run")
            .iter()
            .any(|session| session.session_id == session_id));

        let applied = BackendServicesSessionSyncProcessor::from(state)
            .handle(BackendServicesSessionSyncRequest::default())
            .await
            .expect("sync");
        let applied_result = applied.reconciliation.expect("reconciliation");
        assert!(!applied_result.dry_run);
        assert_eq!(applied_result.created, 1);
        assert!(list_sessions()
            .expect("list after sync")
            .iter()
            .any(|session| session.session_id == session_id));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn session_sync_does_not_mutate_an_active_turn() {
        let (_home, _guard) = temp_home();
        let state = make_web_state();
        let session_id = state.engine().current_session_id().to_string();
        state.set_session_streaming(&session_id, true);

        let response = BackendServicesSessionSyncProcessor::from(state)
            .handle(BackendServicesSessionSyncRequest::default())
            .await
            .expect("sync");
        let result = response.reconciliation.expect("reconciliation");
        assert_eq!(result.created, 0);
        assert_eq!(result.skipped, 1);
        assert_eq!(
            result.items[0].message.as_deref(),
            Some("active_turn_not_mutated")
        );
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn valid_compression_target_is_truthfully_unavailable_and_unknown_is_not_found() {
        let (_home, _guard) = temp_home();
        let state = make_web_state();
        let session_id = state.engine().current_session_id().to_string();
        let processor = BackendServicesCompressionRunProcessor::from(state);

        let valid = processor
            .handle(BackendServicesCompressionRunParams {
                id: session_id,
                profile_id: None,
                target_id: None,
                token_budget: Some(DEFAULT_TOKEN_BUDGET),
                idempotency_key: None,
            })
            .await
            .expect_err("shared service is absent");
        assert!(matches!(
            valid,
            ProtocolApiError::ServiceUnavailable {
                code: "compaction_service_unavailable",
                ..
            }
        ));

        let unknown = processor
            .handle(BackendServicesCompressionRunParams {
                id: "missing-session".to_string(),
                profile_id: None,
                target_id: None,
                token_budget: None,
                idempotency_key: None,
            })
            .await
            .expect_err("unknown target");
        assert!(matches!(unknown, ProtocolApiError::NotFound { .. }));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn runtime_event_is_projected_but_rejected_without_retry_descriptor() {
        let (_home, _guard) = temp_home();
        let state = make_web_state();
        let session_id = state.engine().current_session_id().to_string();
        persist_subagent_event_for_session(
            Some(&session_id),
            RuntimeSubagentEvent {
                kind: "failed",
                agent_id: "agent-retry",
                parent_agent_id: None,
                description: Some("private execution input"),
                model: None,
                depth: 1,
                background: true,
                payload: None,
            },
        )
        .expect("seed runtime event");
        let dashboard = BackendServicesProcessor::from(state.clone())
            .handle(BackendServicesQuery::default())
            .await
            .expect("dashboard");
        let event_id = dashboard.agent_bridge.events[0].id.clone();

        let known = BackendServicesAgentRetryProcessor::from(state.clone())
            .handle(BackendServicesAgentRetryParams {
                id: event_id,
                profile_id: None,
                target_id: None,
                idempotency_key: None,
            })
            .await
            .expect_err("event has no durable descriptor");
        assert!(matches!(known, ProtocolApiError::Conflict { .. }));

        let unknown = BackendServicesAgentRetryProcessor::from(state)
            .handle(BackendServicesAgentRetryParams {
                id: "runtime:999999".to_string(),
                profile_id: None,
                target_id: None,
                idempotency_key: None,
            })
            .await
            .expect_err("unknown event");
        assert!(matches!(unknown, ProtocolApiError::NotFound { .. }));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn migration_and_backup_do_not_create_web_local_evidence() {
        let (home, _guard) = temp_home();
        let state = make_web_state();
        let migration = BackendServicesMigrationProcessor::from(state.clone())
            .handle(BackendServicesMigrationRequest {
                dry_run: Some(false),
                ..Default::default()
            })
            .await
            .expect_err("aggregate migration owner absent");
        let backup = BackendServicesBackupProcessor::from(state)
            .handle(BackendServicesBackupRequest {
                reason: Some("test".to_string()),
                ..Default::default()
            })
            .await
            .expect_err("backup owner absent");

        assert!(matches!(
            migration,
            ProtocolApiError::ServiceUnavailable { .. }
        ));
        assert!(matches!(
            backup,
            ProtocolApiError::ServiceUnavailable { .. }
        ));
        assert!(!home.path().join("web").join("schema-version.json").exists());
        assert!(!home.path().join("web").join("backups").exists());
    }
}
