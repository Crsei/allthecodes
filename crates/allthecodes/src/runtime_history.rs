use allthecodes_db::{Migration, MigrationRunner};
use allthecodes_types::agent_runtime_record::AgentRuntimeExecutionRecord;
use anyhow::{Context, Result};
use serde_json::Value;
use sqlx::SqlitePool;

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

pub(crate) struct RuntimeSubagentEvent<'a> {
    pub kind: &'a str,
    pub agent_id: &'a str,
    pub parent_agent_id: Option<&'a str>,
    pub description: Option<&'a str>,
    pub model: Option<&'a str>,
    pub depth: usize,
    pub background: bool,
    pub payload: Option<Value>,
}

pub(crate) fn persist_subagent_event(event: RuntimeSubagentEvent<'_>) -> Result<()> {
    let payload_json = event
        .payload
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .context("failed to serialize agent runtime event payload")?;
    let session_id = crate::dashboard::current_session_id().map(ToOwned::to_owned);
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

pub(crate) fn persist_execution_record(record: &AgentRuntimeExecutionRecord) -> Result<()> {
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
        .bind(record.duration_ms.map(|v| v as i64))
        .bind(record.had_error)
        .bind(record_json)
        .bind(record.schema_version as i64)
        .execute(&pool)
        .await
        .context("failed to insert sqlite agent runtime execution record")?;
        Ok(())
    })
}

async fn migrated_pool() -> Result<SqlitePool> {
    let pool = allthecodes_db::DbPoolManager::new().logs_pool()?;
    MigrationRunner::new("agent-runtime-history", MIGRATIONS)
        .run(&pool)
        .await?;
    Ok(pool)
}

#[cfg(test)]
mod tests {
    use allthecodes_types::agent_runtime_record::{
        AgentRuntimeExecutionRecord, AgentRuntimePermissionDecision,
    };
    use sqlx::Row;

    use super::*;

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, previous }
        }

        fn remove(key: &'static str) -> Self {
            let previous = std::env::var(key).ok();
            std::env::remove_var(key);
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

    #[derive(Debug)]
    struct TestExecutionRecordRow {
        session_id: String,
        tool: String,
        command: Option<String>,
        exit_code: Option<i64>,
        stdout_digest: Option<String>,
        retry_count: i64,
        fallback_used: bool,
        permission_decision: Option<String>,
        record_json: String,
    }

    #[derive(Debug)]
    struct TestRuntimeEventRow {
        kind: String,
        agent_id: String,
        parent_agent_id: Option<String>,
        payload_json: Option<String>,
    }

    fn query_execution_records_for_test() -> anyhow::Result<Vec<TestExecutionRecordRow>> {
        allthecodes_db::run_sqlite_sync("allthecodes-runtime-history-test", async {
            let pool = migrated_pool().await?;
            let rows = sqlx::query(
                r#"
                SELECT session_id, tool, command, exit_code, stdout_digest,
                       retry_count, fallback_used, permission_decision, record_json
                FROM agent_runtime_execution_records
                ORDER BY id ASC
                "#,
            )
            .fetch_all(&pool)
            .await?;

            rows.into_iter()
                .map(|row| {
                    Ok(TestExecutionRecordRow {
                        session_id: row.try_get("session_id")?,
                        tool: row.try_get("tool")?,
                        command: row.try_get("command")?,
                        exit_code: row.try_get("exit_code")?,
                        stdout_digest: row.try_get("stdout_digest")?,
                        retry_count: row.try_get("retry_count")?,
                        fallback_used: row.try_get::<i64, _>("fallback_used")? != 0,
                        permission_decision: row.try_get("permission_decision")?,
                        record_json: row.try_get("record_json")?,
                    })
                })
                .collect()
        })
    }

    fn query_events_for_test() -> anyhow::Result<Vec<TestRuntimeEventRow>> {
        allthecodes_db::run_sqlite_sync("allthecodes-runtime-history-test", async {
            let pool = migrated_pool().await?;
            let rows = sqlx::query(
                r#"
                SELECT kind, agent_id, parent_agent_id, payload_json
                FROM agent_runtime_events
                ORDER BY id ASC
                "#,
            )
            .fetch_all(&pool)
            .await?;

            rows.into_iter()
                .map(|row| {
                    Ok(TestRuntimeEventRow {
                        kind: row.try_get("kind")?,
                        agent_id: row.try_get("agent_id")?,
                        parent_agent_id: row.try_get("parent_agent_id")?,
                        payload_json: row.try_get("payload_json")?,
                    })
                })
                .collect()
        })
    }

    fn sample_execution_record() -> AgentRuntimeExecutionRecord {
        AgentRuntimeExecutionRecord {
            session_id: "session-1".to_string(),
            agent_id: "agent-1".to_string(),
            parent_agent_id: Some("parent-1".to_string()),
            agent_role: Some("build-agent".to_string()),
            tool: "shell".to_string(),
            tool_use_id: Some("toolu-1".to_string()),
            command: Some("npm test".to_string()),
            cwd: Some(std::path::PathBuf::from("/repo")),
            exit_code: Some(1),
            stdout_digest: Some("a".repeat(64)),
            stderr_digest: Some("b".repeat(64)),
            retry_count: 2,
            model: Some("claude-sonnet-5".to_string()),
            fallback_used: true,
            permission_decision: Some(AgentRuntimePermissionDecision::AllowedByPolicy),
            duration_ms: Some(123),
            had_error: true,
            schema_version: 1,
        }
    }

    #[test]
    #[serial_test::serial]
    fn execution_record_persists_queryable_columns() {
        let temp = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", temp.path().to_str().unwrap());

        let record = sample_execution_record();

        persist_execution_record(&record).unwrap();

        let rows = query_execution_records_for_test().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].session_id, "session-1");
        assert_eq!(rows[0].tool, "shell");
        assert_eq!(rows[0].command.as_deref(), Some("npm test"));
        assert_eq!(rows[0].exit_code, Some(1));
        assert_eq!(rows[0].stdout_digest.as_ref(), Some(&"a".repeat(64)));
        assert_eq!(rows[0].retry_count, 2);
        assert!(rows[0].fallback_used);
        assert_eq!(
            rows[0].permission_decision.as_deref(),
            Some("allowed_by_policy")
        );
        assert!(rows[0].record_json.contains("\"stdout_digest\""));
    }

    #[test]
    #[serial_test::serial]
    fn subagent_event_persists_without_dashboard_feature() {
        let temp = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", temp.path().to_str().unwrap());
        let _dashboard = EnvGuard::remove("FEATURE_SUBAGENT_DASHBOARD");

        persist_subagent_event(RuntimeSubagentEvent {
            kind: "spawned",
            agent_id: "agent-1",
            parent_agent_id: Some("main"),
            description: Some("build checks"),
            model: Some("claude-sonnet-5"),
            depth: 1,
            background: false,
            payload: Some(serde_json::json!({"source": "test"})),
        })
        .unwrap();

        let rows = query_events_for_test().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, "spawned");
        assert_eq!(rows[0].agent_id, "agent-1");
        assert_eq!(rows[0].parent_agent_id.as_deref(), Some("main"));
        assert!(rows[0]
            .payload_json
            .as_ref()
            .unwrap()
            .contains("\"source\""));
    }

    #[test]
    #[serial_test::serial]
    fn non_shell_record_persists_null_shell_columns() {
        let temp = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", temp.path().to_str().unwrap());

        let record = AgentRuntimeExecutionRecord {
            session_id: "session-2".to_string(),
            agent_id: "main".to_string(),
            tool: "read".to_string(),
            tool_use_id: Some("toolu-read".to_string()),
            permission_decision: Some(AgentRuntimePermissionDecision::NotRequired),
            model: Some("claude-sonnet-5".to_string()),
            ..Default::default()
        };

        persist_execution_record(&record).unwrap();

        let rows = query_execution_records_for_test().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].tool, "read");
        assert!(rows[0].command.is_none());
        assert!(rows[0].exit_code.is_none());
        assert!(rows[0].stdout_digest.is_none());
        assert_eq!(rows[0].permission_decision.as_deref(), Some("not_required"));
    }

    #[test]
    #[serial_test::serial]
    fn root_dashboard_emitter_persists_execution_record_when_ndjson_disabled() {
        use allthecodes_engine::agent_runtime::DashboardEmitter;

        let temp = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", temp.path().to_str().unwrap());
        let _dashboard = EnvGuard::remove("FEATURE_SUBAGENT_DASHBOARD");

        let emitter = crate::startup_traits::RootDashboardEmitter;
        emitter
            .emit_execution_record(&sample_execution_record())
            .unwrap();

        let rows = query_execution_records_for_test().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].session_id, "session-1");
    }

    #[test]
    #[serial_test::serial]
    fn root_dashboard_emitter_persists_subagent_event_when_ndjson_disabled() {
        use allthecodes_engine::agent_runtime::DashboardEmitter;

        let temp = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", temp.path().to_str().unwrap());
        let _dashboard = EnvGuard::remove("FEATURE_SUBAGENT_DASHBOARD");

        let emitter = crate::startup_traits::RootDashboardEmitter;
        emitter
            .emit_subagent_event(
                "completed",
                "agent-1",
                Some("main"),
                Some("build checks"),
                Some("claude-sonnet-5"),
                1,
                false,
                Some(serde_json::json!({"source": "adapter"})),
            )
            .unwrap();

        let rows = query_events_for_test().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, "completed");
        assert_eq!(rows[0].agent_id, "agent-1");
        assert_eq!(rows[0].parent_agent_id.as_deref(), Some("main"));
    }
}
