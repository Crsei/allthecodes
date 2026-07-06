use std::collections::BTreeMap;

use allthecodes_db::{Migration, MigrationRunner};
use allthecodes_types::agent_runtime_dashboard::{
    AgentRuntimeAgentSummary, AgentRuntimeDashboardQuery, AgentRuntimeDashboardResponse,
    AgentRuntimeDashboardSummary, AgentRuntimeEventItem, AgentRuntimeExecutionRecordItem,
};
use allthecodes_types::agent_runtime_record::AgentRuntimeExecutionRecord;
use anyhow::{Context, Result};
use serde_json::Value;
use sqlx::{Row, SqlitePool};

pub use allthecodes_types::agent_runtime_dashboard::AgentRuntimeAgentStatus;

const DEFAULT_DASHBOARD_LIMIT: usize = 200;
const MAX_DASHBOARD_LIMIT: usize = 1000;

const MIGRATIONS: &[Migration] = &[
    Migration::new(
        1,
        r#"
        CREATE TABLE IF NOT EXISTS agent_runtime_events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp TEXT NOT NULL,
            ts_millis INTEGER NOT NULL,
            session_id TEXT,
            agent_id TEXT NOT NULL,
            parent_agent_id TEXT,
            kind TEXT NOT NULL,
            description TEXT,
            model TEXT,
            depth INTEGER,
            background INTEGER NOT NULL DEFAULT 0,
            payload_json TEXT,
            schema_version INTEGER NOT NULL DEFAULT 1
        );
        CREATE INDEX IF NOT EXISTS idx_agent_runtime_events_session
            ON agent_runtime_events(session_id, id);
        CREATE INDEX IF NOT EXISTS idx_agent_runtime_events_agent
            ON agent_runtime_events(agent_id, id);
        CREATE INDEX IF NOT EXISTS idx_agent_runtime_events_kind
            ON agent_runtime_events(kind, timestamp DESC, id DESC);
        "#,
    ),
    Migration::new(
        2,
        r#"
        CREATE TABLE IF NOT EXISTS agent_runtime_execution_records (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp TEXT NOT NULL,
            ts_millis INTEGER NOT NULL,
            session_id TEXT NOT NULL,
            agent_id TEXT NOT NULL,
            parent_agent_id TEXT,
            agent_role TEXT,
            tool TEXT NOT NULL,
            tool_use_id TEXT,
            command TEXT,
            cwd TEXT,
            exit_code INTEGER,
            stdout_digest TEXT,
            stderr_digest TEXT,
            retry_count INTEGER NOT NULL DEFAULT 0,
            model TEXT,
            fallback_used INTEGER NOT NULL DEFAULT 0,
            permission_decision TEXT,
            duration_ms INTEGER,
            had_error INTEGER NOT NULL DEFAULT 0,
            record_json TEXT NOT NULL,
            schema_version INTEGER NOT NULL DEFAULT 1
        );
        CREATE INDEX IF NOT EXISTS idx_agent_runtime_records_session
            ON agent_runtime_execution_records(session_id, id);
        CREATE INDEX IF NOT EXISTS idx_agent_runtime_records_agent
            ON agent_runtime_execution_records(agent_id, id);
        CREATE INDEX IF NOT EXISTS idx_agent_runtime_records_tool
            ON agent_runtime_execution_records(tool, timestamp DESC, id DESC);
        CREATE INDEX IF NOT EXISTS idx_agent_runtime_records_permission
            ON agent_runtime_execution_records(permission_decision, timestamp DESC, id DESC);
        CREATE INDEX IF NOT EXISTS idx_agent_runtime_records_model
            ON agent_runtime_execution_records(model, timestamp DESC, id DESC);
        "#,
    ),
];

pub struct RuntimeSubagentEvent<'a> {
    pub kind: &'a str,
    pub agent_id: &'a str,
    pub parent_agent_id: Option<&'a str>,
    pub description: Option<&'a str>,
    pub model: Option<&'a str>,
    pub depth: usize,
    pub background: bool,
    pub payload: Option<Value>,
}

#[derive(Clone, Debug)]
struct AgentAccumulator {
    agent_id: String,
    parent_agent_id: Option<String>,
    description: Option<String>,
    model: Option<String>,
    depth: Option<usize>,
    background: bool,
    status: AgentRuntimeAgentStatus,
    started_at_ms: Option<i64>,
    last_event_at_ms: Option<i64>,
    tool_call_count: usize,
    failed_tool_call_count: usize,
}

impl AgentAccumulator {
    fn new(agent_id: String) -> Self {
        Self {
            agent_id,
            parent_agent_id: None,
            description: None,
            model: None,
            depth: None,
            background: false,
            status: AgentRuntimeAgentStatus::Unknown,
            started_at_ms: None,
            last_event_at_ms: None,
            tool_call_count: 0,
            failed_tool_call_count: 0,
        }
    }

    fn observe_time(&mut self, ts_millis: i64) {
        self.started_at_ms = Some(
            self.started_at_ms
                .map_or(ts_millis, |value| value.min(ts_millis)),
        );
        self.last_event_at_ms = Some(
            self.last_event_at_ms
                .map_or(ts_millis, |value| value.max(ts_millis)),
        );
    }

    fn apply_status(&mut self, status: AgentRuntimeAgentStatus) {
        use AgentRuntimeAgentStatus::{Cancelled, Completed, Failed, Running, Unknown};

        self.status = match (self.status, status) {
            (Failed, _) | (_, Failed) => Failed,
            (Cancelled, _) | (_, Cancelled) => Cancelled,
            (Completed, _) | (_, Completed) => Completed,
            (Running, _) | (_, Running) => Running,
            (Unknown, Unknown) => Unknown,
        };
    }

    fn into_summary(self) -> AgentRuntimeAgentSummary {
        AgentRuntimeAgentSummary {
            agent_id: self.agent_id,
            parent_agent_id: self.parent_agent_id,
            description: self.description,
            model: self.model,
            depth: self.depth,
            background: self.background,
            status: self.status,
            started_at_ms: self.started_at_ms,
            last_event_at_ms: self.last_event_at_ms,
            tool_call_count: self.tool_call_count,
            failed_tool_call_count: self.failed_tool_call_count,
        }
    }
}

pub fn persist_subagent_event(event: RuntimeSubagentEvent<'_>) -> Result<()> {
    persist_subagent_event_for_session(None, event)
}

pub fn persist_subagent_event_for_session(
    session_id: Option<&str>,
    event: RuntimeSubagentEvent<'_>,
) -> Result<()> {
    let payload_json = event
        .payload
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .context("failed to serialize agent runtime event payload")?;
    let session_id = session_id.map(ToOwned::to_owned);
    let kind = event.kind.to_string();
    let agent_id = event.agent_id.to_string();
    let parent_agent_id = event.parent_agent_id.map(ToOwned::to_owned);
    let description = event.description.map(ToOwned::to_owned);
    let model = event.model.map(ToOwned::to_owned);
    let depth = event.depth as i64;
    let background = event.background;

    allthecodes_db::run_sqlite_sync("allthecodes-runtime-history-sqlite", async move {
        let pool = migrated_pool().await?;
        let now = chrono::Utc::now();
        sqlx::query(
            r#"
            INSERT INTO agent_runtime_events (
                timestamp, ts_millis, session_id, agent_id, parent_agent_id,
                kind, description, model, depth, background, payload_json,
                schema_version
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1)
            "#,
        )
        .bind(now.to_rfc3339())
        .bind(now.timestamp_millis())
        .bind(session_id)
        .bind(agent_id)
        .bind(parent_agent_id)
        .bind(kind)
        .bind(description)
        .bind(model)
        .bind(depth)
        .bind(background)
        .bind(payload_json)
        .execute(&pool)
        .await
        .context("failed to insert sqlite agent runtime event")?;
        Ok(())
    })
}

pub fn persist_execution_record(record: &AgentRuntimeExecutionRecord) -> Result<()> {
    let record = record.clone();
    let record_json =
        serde_json::to_string(&record).context("failed to serialize execution record")?;

    allthecodes_db::run_sqlite_sync("allthecodes-runtime-history-sqlite", async move {
        let pool = migrated_pool().await?;
        let now = chrono::Utc::now();
        sqlx::query(
            r#"
            INSERT INTO agent_runtime_execution_records (
                timestamp, ts_millis, session_id, agent_id, parent_agent_id,
                agent_role, tool, tool_use_id, command, cwd, exit_code,
                stdout_digest, stderr_digest, retry_count, model, fallback_used,
                permission_decision, duration_ms, had_error, record_json,
                schema_version
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(now.to_rfc3339())
        .bind(now.timestamp_millis())
        .bind(&record.session_id)
        .bind(&record.agent_id)
        .bind(&record.parent_agent_id)
        .bind(&record.agent_role)
        .bind(&record.tool)
        .bind(&record.tool_use_id)
        .bind(&record.command)
        .bind(record.cwd.as_ref().map(|p| p.to_string_lossy().to_string()))
        .bind(record.exit_code)
        .bind(&record.stdout_digest)
        .bind(&record.stderr_digest)
        .bind(record.retry_count as i64)
        .bind(&record.model)
        .bind(record.fallback_used)
        .bind(record.permission_decision.map(|d| d.as_str().to_string()))
        .bind(record.duration_ms.map(|value| value as i64))
        .bind(record.had_error)
        .bind(record_json)
        .bind(record.schema_version as i64)
        .execute(&pool)
        .await
        .context("failed to insert sqlite agent runtime execution record")?;
        Ok(())
    })
}

pub fn load_dashboard_snapshot(
    query: AgentRuntimeDashboardQuery,
) -> Result<AgentRuntimeDashboardResponse> {
    let session_id = query.session_id;
    let limit = query
        .limit
        .unwrap_or(DEFAULT_DASHBOARD_LIMIT)
        .min(MAX_DASHBOARD_LIMIT);

    allthecodes_db::run_sqlite_sync("allthecodes-runtime-history-dashboard", async move {
        let pool = migrated_pool().await?;
        let events = fetch_events(&pool, session_id.as_deref(), limit).await?;
        let execution_records =
            fetch_execution_records(&pool, session_id.as_deref(), limit).await?;
        let agents = derive_agent_summaries(&events, &execution_records);
        let summary = summarize_agents(&agents);

        Ok(AgentRuntimeDashboardResponse {
            session_id,
            updated_at_ms: chrono::Utc::now().timestamp_millis(),
            limit,
            summary,
            agents,
            events,
            execution_records,
        })
    })
}

async fn migrated_pool() -> Result<SqlitePool> {
    let pool = allthecodes_db::DbPoolManager::new().logs_pool()?;
    MigrationRunner::new("agent-runtime-history", MIGRATIONS)
        .run(&pool)
        .await?;
    Ok(pool)
}

async fn fetch_events(
    pool: &SqlitePool,
    session_id: Option<&str>,
    limit: usize,
) -> Result<Vec<AgentRuntimeEventItem>> {
    let limit = limit as i64;
    let rows = if let Some(session_id) = session_id {
        sqlx::query(
            r#"
            SELECT id, timestamp, ts_millis, session_id, agent_id,
                   parent_agent_id, kind, description, model, depth,
                   background, payload_json
            FROM agent_runtime_events
            WHERE session_id = ? OR session_id IS NULL
            ORDER BY id DESC
            LIMIT ?
            "#,
        )
        .bind(session_id)
        .bind(limit)
        .fetch_all(pool)
        .await
    } else {
        sqlx::query(
            r#"
            SELECT id, timestamp, ts_millis, session_id, agent_id,
                   parent_agent_id, kind, description, model, depth,
                   background, payload_json
            FROM agent_runtime_events
            ORDER BY id DESC
            LIMIT ?
            "#,
        )
        .bind(limit)
        .fetch_all(pool)
        .await
    }
    .context("failed to query sqlite agent runtime events")?;

    rows.into_iter()
        .map(|row| event_item_from_row(row))
        .collect()
}

async fn fetch_execution_records(
    pool: &SqlitePool,
    session_id: Option<&str>,
    limit: usize,
) -> Result<Vec<AgentRuntimeExecutionRecordItem>> {
    let limit = limit as i64;
    let rows = if let Some(session_id) = session_id {
        sqlx::query(
            r#"
            SELECT id, timestamp, ts_millis, record_json
            FROM agent_runtime_execution_records
            WHERE session_id = ?
            ORDER BY id DESC
            LIMIT ?
            "#,
        )
        .bind(session_id)
        .bind(limit)
        .fetch_all(pool)
        .await
    } else {
        sqlx::query(
            r#"
            SELECT id, timestamp, ts_millis, record_json
            FROM agent_runtime_execution_records
            ORDER BY id DESC
            LIMIT ?
            "#,
        )
        .bind(limit)
        .fetch_all(pool)
        .await
    }
    .context("failed to query sqlite agent runtime execution records")?;

    rows.into_iter()
        .map(|row| execution_record_item_from_row(row))
        .collect()
}

fn event_item_from_row(row: sqlx::sqlite::SqliteRow) -> Result<AgentRuntimeEventItem> {
    let payload_json: Option<String> = row.try_get("payload_json")?;
    let depth: Option<i64> = row.try_get("depth")?;
    Ok(AgentRuntimeEventItem {
        id: row.try_get("id")?,
        timestamp: row.try_get("timestamp")?,
        ts_millis: row.try_get("ts_millis")?,
        session_id: row.try_get("session_id")?,
        agent_id: row.try_get("agent_id")?,
        parent_agent_id: row.try_get("parent_agent_id")?,
        kind: row.try_get("kind")?,
        description: row.try_get("description")?,
        model: row.try_get("model")?,
        depth: depth.and_then(|value| usize::try_from(value).ok()),
        background: row.try_get::<i64, _>("background")? != 0,
        payload: payload_json.and_then(|json| serde_json::from_str(&json).ok()),
    })
}

fn execution_record_item_from_row(
    row: sqlx::sqlite::SqliteRow,
) -> Result<AgentRuntimeExecutionRecordItem> {
    let record_json: String = row.try_get("record_json")?;
    Ok(AgentRuntimeExecutionRecordItem {
        id: row.try_get("id")?,
        timestamp: row.try_get("timestamp")?,
        ts_millis: row.try_get("ts_millis")?,
        record: serde_json::from_str(&record_json)
            .context("failed to deserialize agent runtime execution record")?,
    })
}

fn derive_agent_summaries(
    events: &[AgentRuntimeEventItem],
    execution_records: &[AgentRuntimeExecutionRecordItem],
) -> Vec<AgentRuntimeAgentSummary> {
    let mut agents = BTreeMap::<String, AgentAccumulator>::new();

    for event in events {
        let agent = agents
            .entry(event.agent_id.clone())
            .or_insert_with(|| AgentAccumulator::new(event.agent_id.clone()));
        agent.observe_time(event.ts_millis);
        fill_if_empty(&mut agent.parent_agent_id, event.parent_agent_id.clone());
        fill_if_empty(&mut agent.description, event.description.clone());
        fill_if_empty(&mut agent.model, event.model.clone());
        if agent.depth.is_none() {
            agent.depth = event.depth;
        }
        agent.background |= event.background;
        agent.apply_status(status_for_event(event));
    }

    for item in execution_records {
        let record = &item.record;
        let agent = agents
            .entry(record.agent_id.clone())
            .or_insert_with(|| AgentAccumulator::new(record.agent_id.clone()));
        agent.observe_time(item.ts_millis);
        fill_if_empty(&mut agent.parent_agent_id, record.parent_agent_id.clone());
        fill_if_empty(&mut agent.description, record.agent_role.clone());
        fill_if_empty(&mut agent.model, record.model.clone());
        agent.tool_call_count += 1;
        if record.had_error {
            agent.failed_tool_call_count += 1;
            agent.apply_status(AgentRuntimeAgentStatus::Failed);
        } else if agent.status == AgentRuntimeAgentStatus::Unknown {
            agent.apply_status(AgentRuntimeAgentStatus::Running);
        }
    }

    let mut summaries = agents
        .into_values()
        .map(AgentAccumulator::into_summary)
        .collect::<Vec<_>>();
    summaries.sort_by(|left, right| {
        right
            .last_event_at_ms
            .cmp(&left.last_event_at_ms)
            .then_with(|| left.agent_id.cmp(&right.agent_id))
    });
    summaries
}

fn fill_if_empty(target: &mut Option<String>, value: Option<String>) {
    if target.is_none() {
        *target = value;
    }
}

fn status_for_event(event: &AgentRuntimeEventItem) -> AgentRuntimeAgentStatus {
    match event.kind.as_str() {
        "background_complete" | "completed" => {
            if payload_bool(event.payload.as_ref(), "had_error") {
                AgentRuntimeAgentStatus::Failed
            } else {
                AgentRuntimeAgentStatus::Completed
            }
        }
        "cancelled" => AgentRuntimeAgentStatus::Cancelled,
        "error" | "failed" => AgentRuntimeAgentStatus::Failed,
        "spawned" | "worktree_created" | "warning" => AgentRuntimeAgentStatus::Running,
        _ => AgentRuntimeAgentStatus::Unknown,
    }
}

fn payload_bool(payload: Option<&Value>, key: &str) -> bool {
    payload
        .and_then(|value| value.get(key))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn summarize_agents(agents: &[AgentRuntimeAgentSummary]) -> AgentRuntimeDashboardSummary {
    let mut summary = AgentRuntimeDashboardSummary {
        total_agents: agents.len(),
        ..AgentRuntimeDashboardSummary::default()
    };
    for agent in agents {
        match agent.status {
            AgentRuntimeAgentStatus::Running => summary.running_agents += 1,
            AgentRuntimeAgentStatus::Completed => summary.completed_agents += 1,
            AgentRuntimeAgentStatus::Failed => summary.failed_agents += 1,
            AgentRuntimeAgentStatus::Cancelled => summary.cancelled_agents += 1,
            AgentRuntimeAgentStatus::Unknown => {}
        }
        summary.tool_calls += agent.tool_call_count;
        summary.failed_tool_calls += agent.failed_tool_call_count;
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_types::agent_runtime_record::{
        AgentRuntimeExecutionRecord, AgentRuntimePermissionDecision,
    };

    struct HomeGuard {
        previous: Option<String>,
    }

    impl HomeGuard {
        fn set(path: &std::path::Path) -> Self {
            let previous = std::env::var("ALLTHECODES_HOME").ok();
            std::env::set_var("ALLTHECODES_HOME", path);
            Self { previous }
        }
    }

    impl Drop for HomeGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var("ALLTHECODES_HOME", value),
                None => std::env::remove_var("ALLTHECODES_HOME"),
            }
        }
    }

    fn execution_record(
        session_id: &str,
        agent_id: &str,
        had_error: bool,
    ) -> AgentRuntimeExecutionRecord {
        AgentRuntimeExecutionRecord {
            session_id: session_id.to_string(),
            agent_id: agent_id.to_string(),
            parent_agent_id: Some("main".to_string()),
            agent_role: Some("worker".to_string()),
            tool: "shell".to_string(),
            tool_use_id: Some(format!("tool-{agent_id}")),
            command: Some("cargo test".to_string()),
            exit_code: had_error.then_some(1).or(Some(0)),
            retry_count: 1,
            model: Some("gpt-5.5".to_string()),
            permission_decision: Some(AgentRuntimePermissionDecision::AllowedByPolicy),
            duration_ms: Some(42),
            had_error,
            ..AgentRuntimeExecutionRecord::default()
        }
    }

    #[test]
    #[serial_test::serial]
    fn dashboard_snapshot_groups_events_and_execution_records_by_agent() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _home = HomeGuard::set(temp.path());

        persist_subagent_event(RuntimeSubagentEvent {
            kind: "spawned",
            agent_id: "agent-a",
            parent_agent_id: Some("main"),
            description: Some("inspect runtime"),
            model: Some("gpt-5.5"),
            depth: 1,
            background: true,
            payload: Some(serde_json::json!({ "source": "test" })),
        })
        .expect("persist spawn");
        persist_subagent_event(RuntimeSubagentEvent {
            kind: "background_complete",
            agent_id: "agent-a",
            parent_agent_id: Some("main"),
            description: Some("inspect runtime"),
            model: Some("gpt-5.5"),
            depth: 1,
            background: true,
            payload: Some(serde_json::json!({ "had_error": false })),
        })
        .expect("persist complete");
        persist_execution_record(&execution_record("session-1", "agent-a", false))
            .expect("persist record");

        let snapshot = load_dashboard_snapshot(AgentRuntimeDashboardQuery {
            session_id: Some("session-1".to_string()),
            limit: Some(50),
        })
        .expect("snapshot");

        assert_eq!(snapshot.session_id.as_deref(), Some("session-1"));
        assert_eq!(snapshot.summary.total_agents, 1);
        assert_eq!(snapshot.summary.completed_agents, 1);
        assert_eq!(snapshot.summary.failed_agents, 0);
        assert_eq!(snapshot.summary.tool_calls, 1);
        assert_eq!(snapshot.summary.failed_tool_calls, 0);
        assert_eq!(snapshot.events.len(), 2);
        assert_eq!(snapshot.execution_records.len(), 1);
        assert_eq!(snapshot.agents[0].agent_id, "agent-a");
        assert_eq!(
            snapshot.agents[0].status,
            AgentRuntimeAgentStatus::Completed
        );
    }

    #[test]
    #[serial_test::serial]
    fn dashboard_snapshot_filters_sessions_and_caps_limit() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _home = HomeGuard::set(temp.path());

        persist_execution_record(&execution_record("session-a", "agent-a", false))
            .expect("persist a");
        persist_execution_record(&execution_record("session-b", "agent-b", true))
            .expect("persist b");

        let snapshot = load_dashboard_snapshot(AgentRuntimeDashboardQuery {
            session_id: Some("session-b".to_string()),
            limit: Some(10_000),
        })
        .expect("snapshot");

        assert_eq!(snapshot.summary.total_agents, 1);
        assert_eq!(snapshot.summary.failed_agents, 1);
        assert_eq!(snapshot.execution_records.len(), 1);
        assert_eq!(snapshot.execution_records[0].record.agent_id, "agent-b");
        assert!(snapshot.limit <= 1000);
    }
}
