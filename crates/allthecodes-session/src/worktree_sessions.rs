use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::warn;

pub const WORKTREE_SESSION_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorktreeSessionCreator {
    Human,
    Agent,
}

impl WorktreeSessionCreator {
    fn db_value(self) -> &'static str {
        match self {
            Self::Human => "human",
            Self::Agent => "agent",
        }
    }

    fn from_db(value: &str) -> Result<Self> {
        match value {
            "human" => Ok(Self::Human),
            "agent" => Ok(Self::Agent),
            other => Err(anyhow!("unknown worktree session creator: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorktreeSessionStatus {
    Active,
    Kept,
    Removed,
    CleanupFailed,
    Orphaned,
}

impl WorktreeSessionStatus {
    fn db_value(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Kept => "kept",
            Self::Removed => "removed",
            Self::CleanupFailed => "cleanup_failed",
            Self::Orphaned => "orphaned",
        }
    }

    fn from_db(value: &str) -> Result<Self> {
        match value {
            "active" => Ok(Self::Active),
            "kept" => Ok(Self::Kept),
            "removed" => Ok(Self::Removed),
            "cleanup_failed" | "cleanupFailed" => Ok(Self::CleanupFailed),
            "orphaned" => Ok(Self::Orphaned),
            other => Err(anyhow!("unknown worktree session status: {other}")),
        }
    }

    fn is_reconcilable(self) -> bool {
        matches!(self, Self::Active | Self::Kept | Self::CleanupFailed)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorktreeSessionSource {
    EnterWorktree,
    AgentIsolation,
    WorktreeCreateHook,
    Imported,
}

impl WorktreeSessionSource {
    fn db_value(self) -> &'static str {
        match self {
            Self::EnterWorktree => "enter_worktree",
            Self::AgentIsolation => "agent_isolation",
            Self::WorktreeCreateHook => "worktree_create_hook",
            Self::Imported => "imported",
        }
    }

    fn from_db(value: &str) -> Result<Self> {
        match value {
            "enter_worktree" | "enterWorktree" => Ok(Self::EnterWorktree),
            "agent_isolation" | "agentIsolation" => Ok(Self::AgentIsolation),
            "worktree_create_hook" | "worktreeCreateHook" => Ok(Self::WorktreeCreateHook),
            "imported" => Ok(Self::Imported),
            other => Err(anyhow!("unknown worktree session source: {other}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeSessionRecord {
    pub schema_version: u32,
    pub session_id: String,
    pub repo_id: String,
    pub worktree_path: PathBuf,
    pub branch: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linked_goal_id: Option<String>,
    pub created_by: WorktreeSessionCreator,
    pub git_root: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_cwd: Option<PathBuf>,
    pub status: WorktreeSessionStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kept_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub removed_at: Option<DateTime<Utc>>,
    pub source: WorktreeSessionSource,
}

impl WorktreeSessionRecord {
    #[allow(clippy::too_many_arguments)]
    pub fn new_active(
        session_id: impl Into<String>,
        repo_id: impl Into<String>,
        worktree_path: impl Into<PathBuf>,
        branch: impl Into<String>,
        base_commit: Option<String>,
        linked_goal_id: Option<String>,
        created_by: WorktreeSessionCreator,
        git_root: impl Into<PathBuf>,
        original_cwd: Option<PathBuf>,
        source: WorktreeSessionSource,
    ) -> Self {
        let now = Utc::now();
        Self {
            schema_version: WORKTREE_SESSION_SCHEMA_VERSION,
            session_id: session_id.into(),
            repo_id: repo_id.into(),
            worktree_path: worktree_path.into(),
            branch: branch.into(),
            base_commit,
            linked_goal_id,
            created_by,
            git_root: git_root.into(),
            original_cwd,
            status: WorktreeSessionStatus::Active,
            created_at: now,
            updated_at: now,
            kept_at: None,
            removed_at: None,
            source,
        }
    }
}

pub fn reconcile_worktree_session_records(records: &[WorktreeSessionRecord]) -> Result<usize> {
    let mut orphaned = 0;
    for record in records {
        if !record.status.is_reconcilable() {
            continue;
        }
        if worktree_session_is_orphaned(record) {
            mark_worktree_session_orphaned(&record.session_id, &record.worktree_path)?;
            orphaned += 1;
        }
    }
    Ok(orphaned)
}

pub fn reconcile_all_worktree_sessions() -> Result<usize> {
    let records = list_reconcilable_worktree_sessions()?;
    reconcile_worktree_session_records(&records)
}

fn worktree_session_is_orphaned(record: &WorktreeSessionRecord) -> bool {
    if !record.worktree_path.exists() {
        return true;
    }

    match git_worktree_contains(record) {
        Ok(contains) => !contains,
        Err(error) => {
            warn!(
                session_id = %record.session_id,
                worktree_path = %record.worktree_path.display(),
                error = %error,
                "failed to reconcile worktree session"
            );
            false
        }
    }
}

fn git_worktree_contains(record: &WorktreeSessionRecord) -> Result<bool> {
    let repo = git2::Repository::discover(&record.git_root)
        .or_else(|_| git2::Repository::discover(&record.worktree_path))
        .context("failed to open git repository for worktree session")?;
    let target = fs::canonicalize(&record.worktree_path)
        .with_context(|| format!("failed to canonicalize {}", record.worktree_path.display()))?;

    if let Some(workdir) = repo.workdir() {
        if canonical_path_eq(workdir, &target)? {
            return Ok(true);
        }
    }

    let names = repo
        .worktrees()
        .context("failed to list git worktrees for worktree session")?;
    for name in names.iter().filter_map(|name| name.ok().flatten()) {
        let Ok(worktree) = repo.find_worktree(name) else {
            continue;
        };
        if canonical_path_eq(worktree.path(), &target)? {
            return Ok(true);
        }
    }

    Ok(false)
}

fn canonical_path_eq(path: &Path, canonical_target: &Path) -> Result<bool> {
    Ok(fs::canonicalize(path)
        .with_context(|| format!("failed to canonicalize {}", path.display()))?
        == canonical_target)
}

#[cfg(feature = "sqlite-storage")]
mod sqlite {
    use super::*;
    use allthecodes_db::{Migration, MigrationRunner};
    use sqlx::{Row, SqlitePool};

    const MIGRATIONS: &[Migration] = &[
        Migration::new(
            1,
            r#"
        CREATE TABLE IF NOT EXISTS worktree_sessions (
            session_id TEXT NOT NULL,
            worktree_path TEXT NOT NULL,
            schema_version INTEGER NOT NULL,
            repo_id TEXT NOT NULL,
            branch TEXT NOT NULL,
            base_commit TEXT,
            linked_goal_id TEXT,
            created_by TEXT NOT NULL,
            git_root TEXT NOT NULL,
            original_cwd TEXT,
            status TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            kept_at TEXT,
            removed_at TEXT,
            source TEXT NOT NULL,
            PRIMARY KEY (session_id, worktree_path)
        );

        CREATE INDEX IF NOT EXISTS idx_worktree_sessions_repo
            ON worktree_sessions(repo_id, updated_at DESC);

        CREATE INDEX IF NOT EXISTS idx_worktree_sessions_active
            ON worktree_sessions(session_id, status, updated_at DESC);
        "#,
        ),
        Migration::new(
            2,
            r#"
        CREATE INDEX IF NOT EXISTS idx_worktree_sessions_goal
            ON worktree_sessions(linked_goal_id, updated_at DESC);
        "#,
        ),
    ];

    pub fn upsert_worktree_session(record: &WorktreeSessionRecord) -> Result<()> {
        let record = record.clone();
        allthecodes_db::run_sqlite_sync("allthecodes-worktree-session-sqlite", async move {
            let pool = migrated_pool().await?;
            sqlx::query(
                r#"
                INSERT INTO worktree_sessions (
                    session_id,
                    worktree_path,
                    schema_version,
                    repo_id,
                    branch,
                    base_commit,
                    linked_goal_id,
                    created_by,
                    git_root,
                    original_cwd,
                    status,
                    created_at,
                    updated_at,
                    kept_at,
                    removed_at,
                    source
                )
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(session_id, worktree_path) DO UPDATE SET
                    schema_version = excluded.schema_version,
                    repo_id = excluded.repo_id,
                    branch = excluded.branch,
                    base_commit = excluded.base_commit,
                    linked_goal_id = excluded.linked_goal_id,
                    created_by = excluded.created_by,
                    git_root = excluded.git_root,
                    original_cwd = excluded.original_cwd,
                    status = excluded.status,
                    created_at = excluded.created_at,
                    updated_at = excluded.updated_at,
                    kept_at = excluded.kept_at,
                    removed_at = excluded.removed_at,
                    source = excluded.source
                "#,
            )
            .bind(&record.session_id)
            .bind(path_to_db(&record.worktree_path))
            .bind(u32_to_i64(record.schema_version))
            .bind(&record.repo_id)
            .bind(&record.branch)
            .bind(&record.base_commit)
            .bind(&record.linked_goal_id)
            .bind(record.created_by.db_value())
            .bind(path_to_db(&record.git_root))
            .bind(record.original_cwd.as_deref().map(path_to_db))
            .bind(record.status.db_value())
            .bind(record.created_at.to_rfc3339())
            .bind(record.updated_at.to_rfc3339())
            .bind(record.kept_at.map(|time| time.to_rfc3339()))
            .bind(record.removed_at.map(|time| time.to_rfc3339()))
            .bind(record.source.db_value())
            .execute(&pool)
            .await
            .context("failed to upsert worktree session")?;
            Ok(())
        })
    }

    pub fn mark_worktree_session_kept(session_id: &str, worktree_path: &Path) -> Result<()> {
        mark_status(
            session_id,
            worktree_path,
            WorktreeSessionStatus::Kept,
            Some("kept_at"),
        )
    }

    pub fn mark_worktree_session_removed(session_id: &str, worktree_path: &Path) -> Result<()> {
        mark_status(
            session_id,
            worktree_path,
            WorktreeSessionStatus::Removed,
            Some("removed_at"),
        )
    }

    pub fn mark_worktree_session_cleanup_failed(
        session_id: &str,
        worktree_path: &Path,
    ) -> Result<()> {
        mark_status(
            session_id,
            worktree_path,
            WorktreeSessionStatus::CleanupFailed,
            None,
        )
    }

    pub fn mark_worktree_session_orphaned(session_id: &str, worktree_path: &Path) -> Result<()> {
        mark_status(
            session_id,
            worktree_path,
            WorktreeSessionStatus::Orphaned,
            None,
        )
    }

    pub fn list_worktree_sessions_for_repo(repo_id: &str) -> Result<Vec<WorktreeSessionRecord>> {
        let repo_id = repo_id.to_string();
        allthecodes_db::run_sqlite_sync("allthecodes-worktree-session-sqlite", async move {
            let pool = migrated_pool().await?;
            let rows = sqlx::query(
                r#"
                SELECT *
                FROM worktree_sessions
                WHERE repo_id = ?
                ORDER BY updated_at DESC, created_at DESC, session_id ASC
                "#,
            )
            .bind(repo_id)
            .fetch_all(&pool)
            .await
            .context("failed to list worktree sessions")?;
            rows.into_iter().map(record_from_row).collect()
        })
    }

    pub fn list_worktree_sessions_for_session(
        session_id: &str,
    ) -> Result<Vec<WorktreeSessionRecord>> {
        let session_id = session_id.to_string();
        allthecodes_db::run_sqlite_sync("allthecodes-worktree-session-sqlite", async move {
            let pool = migrated_pool().await?;
            let rows = sqlx::query(
                r#"
                SELECT *
                FROM worktree_sessions
                WHERE session_id = ?
                ORDER BY updated_at DESC, created_at DESC, worktree_path ASC
                "#,
            )
            .bind(session_id)
            .fetch_all(&pool)
            .await
            .context("failed to list worktree sessions for session")?;
            rows.into_iter().map(record_from_row).collect()
        })
    }

    pub fn list_worktree_sessions_for_goal(goal_id: &str) -> Result<Vec<WorktreeSessionRecord>> {
        let goal_id = goal_id.to_string();
        allthecodes_db::run_sqlite_sync("allthecodes-worktree-session-sqlite", async move {
            let pool = migrated_pool().await?;
            let rows = sqlx::query(
                r#"
                SELECT *
                FROM worktree_sessions
                WHERE linked_goal_id = ?
                ORDER BY updated_at DESC, created_at DESC, session_id ASC, worktree_path ASC
                "#,
            )
            .bind(goal_id)
            .fetch_all(&pool)
            .await
            .context("failed to list worktree sessions for goal")?;
            rows.into_iter().map(record_from_row).collect()
        })
    }

    pub fn list_reconcilable_worktree_sessions() -> Result<Vec<WorktreeSessionRecord>> {
        allthecodes_db::run_sqlite_sync("allthecodes-worktree-session-sqlite", async move {
            let pool = migrated_pool().await?;
            let rows = sqlx::query(
                r#"
                SELECT *
                FROM worktree_sessions
                WHERE status IN ('active', 'kept', 'cleanup_failed')
                ORDER BY updated_at DESC, created_at DESC, session_id ASC, worktree_path ASC
                "#,
            )
            .fetch_all(&pool)
            .await
            .context("failed to list reconcilable worktree sessions")?;
            rows.into_iter().map(record_from_row).collect()
        })
    }

    pub fn get_active_worktree_session(session_id: &str) -> Result<Option<WorktreeSessionRecord>> {
        let session_id = session_id.to_string();
        allthecodes_db::run_sqlite_sync("allthecodes-worktree-session-sqlite", async move {
            let pool = migrated_pool().await?;
            let row = sqlx::query(
                r#"
                SELECT *
                FROM worktree_sessions
                WHERE session_id = ?
                  AND status IN ('active', 'cleanup_failed')
                ORDER BY updated_at DESC, created_at DESC, worktree_path ASC
                LIMIT 1
                "#,
            )
            .bind(session_id)
            .fetch_optional(&pool)
            .await
            .context("failed to get active worktree session")?;
            row.map(record_from_row).transpose()
        })
    }

    fn mark_status(
        session_id: &str,
        worktree_path: &Path,
        status: WorktreeSessionStatus,
        terminal_time_column: Option<&'static str>,
    ) -> Result<()> {
        let session_id = session_id.to_string();
        let worktree_path = path_to_db(worktree_path);
        allthecodes_db::run_sqlite_sync("allthecodes-worktree-session-sqlite", async move {
            let pool = migrated_pool().await?;
            let now = Utc::now().to_rfc3339();
            let mut query = format!(
                "UPDATE worktree_sessions SET status = ?, updated_at = ?{} WHERE session_id = ? AND worktree_path = ?",
                terminal_time_column
                    .map(|column| format!(", {column} = ?"))
                    .unwrap_or_default()
            );
            query.shrink_to_fit();

            let mut query = sqlx::query(&query).bind(status.db_value()).bind(&now);
            if terminal_time_column.is_some() {
                query = query.bind(&now);
            }
            query
                .bind(session_id)
                .bind(worktree_path)
                .execute(&pool)
                .await
                .context("failed to update worktree session status")?;
            Ok(())
        })
    }

    async fn migrated_pool() -> Result<SqlitePool> {
        let pool = allthecodes_db::DbPoolManager::new().state_pool()?;
        MigrationRunner::new("worktree_sessions", MIGRATIONS)
            .run(&pool)
            .await?;
        Ok(pool)
    }

    fn record_from_row(row: sqlx::sqlite::SqliteRow) -> Result<WorktreeSessionRecord> {
        let created_by: String = row.try_get("created_by")?;
        let status: String = row.try_get("status")?;
        let source: String = row.try_get("source")?;
        let schema_version: i64 = row.try_get("schema_version")?;

        Ok(WorktreeSessionRecord {
            schema_version: u32::try_from(schema_version)
                .context("invalid worktree session schema_version")?,
            session_id: row.try_get("session_id")?,
            repo_id: row.try_get("repo_id")?,
            worktree_path: PathBuf::from(row.try_get::<String, _>("worktree_path")?),
            branch: row.try_get("branch")?,
            base_commit: row.try_get("base_commit")?,
            linked_goal_id: row.try_get("linked_goal_id")?,
            created_by: WorktreeSessionCreator::from_db(&created_by)?,
            git_root: PathBuf::from(row.try_get::<String, _>("git_root")?),
            original_cwd: row
                .try_get::<Option<String>, _>("original_cwd")?
                .map(PathBuf::from),
            status: WorktreeSessionStatus::from_db(&status)?,
            created_at: parse_time(row.try_get::<String, _>("created_at")?, "created_at")?,
            updated_at: parse_time(row.try_get::<String, _>("updated_at")?, "updated_at")?,
            kept_at: parse_optional_time(row.try_get("kept_at")?, "kept_at")?,
            removed_at: parse_optional_time(row.try_get("removed_at")?, "removed_at")?,
            source: WorktreeSessionSource::from_db(&source)?,
        })
    }

    fn parse_optional_time(value: Option<String>, field: &str) -> Result<Option<DateTime<Utc>>> {
        value.map(|value| parse_time(value, field)).transpose()
    }

    fn parse_time(value: String, field: &str) -> Result<DateTime<Utc>> {
        Ok(DateTime::parse_from_rfc3339(&value)
            .with_context(|| format!("invalid worktree session {field} timestamp"))?
            .with_timezone(&Utc))
    }

    fn path_to_db(path: &Path) -> String {
        path.to_string_lossy().to_string()
    }

    fn u32_to_i64(value: u32) -> i64 {
        i64::from(value)
    }
}

#[cfg(feature = "sqlite-storage")]
pub use sqlite::{
    get_active_worktree_session, list_reconcilable_worktree_sessions,
    list_worktree_sessions_for_goal, list_worktree_sessions_for_repo,
    list_worktree_sessions_for_session, mark_worktree_session_cleanup_failed,
    mark_worktree_session_kept, mark_worktree_session_orphaned, mark_worktree_session_removed,
    upsert_worktree_session,
};

#[cfg(not(feature = "sqlite-storage"))]
pub fn upsert_worktree_session(_record: &WorktreeSessionRecord) -> Result<()> {
    Ok(())
}

#[cfg(not(feature = "sqlite-storage"))]
pub fn mark_worktree_session_kept(_session_id: &str, _worktree_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(not(feature = "sqlite-storage"))]
pub fn mark_worktree_session_removed(_session_id: &str, _worktree_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(not(feature = "sqlite-storage"))]
pub fn mark_worktree_session_cleanup_failed(
    _session_id: &str,
    _worktree_path: &Path,
) -> Result<()> {
    Ok(())
}

#[cfg(not(feature = "sqlite-storage"))]
pub fn mark_worktree_session_orphaned(_session_id: &str, _worktree_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(not(feature = "sqlite-storage"))]
pub fn list_worktree_sessions_for_repo(_repo_id: &str) -> Result<Vec<WorktreeSessionRecord>> {
    Ok(Vec::new())
}

#[cfg(not(feature = "sqlite-storage"))]
pub fn list_worktree_sessions_for_session(_session_id: &str) -> Result<Vec<WorktreeSessionRecord>> {
    Ok(Vec::new())
}

#[cfg(not(feature = "sqlite-storage"))]
pub fn list_worktree_sessions_for_goal(_goal_id: &str) -> Result<Vec<WorktreeSessionRecord>> {
    Ok(Vec::new())
}

#[cfg(not(feature = "sqlite-storage"))]
pub fn list_reconcilable_worktree_sessions() -> Result<Vec<WorktreeSessionRecord>> {
    Ok(Vec::new())
}

#[cfg(not(feature = "sqlite-storage"))]
pub fn get_active_worktree_session(_session_id: &str) -> Result<Option<WorktreeSessionRecord>> {
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_serializes_camel_case_shape() {
        let record = WorktreeSessionRecord::new_active(
            "session-1",
            "repo-1",
            "/tmp/wt",
            "branch-1",
            Some("abc123".to_string()),
            Some("goal-1".to_string()),
            WorktreeSessionCreator::Agent,
            "/tmp/repo",
            Some(PathBuf::from("/tmp/repo")),
            WorktreeSessionSource::AgentIsolation,
        );

        let value = serde_json::to_value(&record).unwrap();
        assert_eq!(value["sessionId"], "session-1");
        assert_eq!(value["repoId"], "repo-1");
        assert_eq!(value["createdBy"], "agent");
        assert_eq!(value["status"], "active");
        assert_eq!(value["source"], "agentIsolation");
        assert_eq!(value["linkedGoalId"], "goal-1");
    }

    #[cfg(feature = "sqlite-storage")]
    #[test]
    #[serial_test::serial]
    fn sqlite_store_roundtrips_and_tracks_status() {
        struct EnvGuard(Option<String>);
        impl EnvGuard {
            fn set(path: &Path) -> Self {
                let previous = std::env::var("ALLTHECODES_HOME").ok();
                std::env::set_var("ALLTHECODES_HOME", path);
                Self(previous)
            }
        }
        impl Drop for EnvGuard {
            fn drop(&mut self) {
                match &self.0 {
                    Some(value) => std::env::set_var("ALLTHECODES_HOME", value),
                    None => std::env::remove_var("ALLTHECODES_HOME"),
                }
            }
        }

        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set(temp.path());
        let record = WorktreeSessionRecord::new_active(
            "session-1",
            "repo-1",
            temp.path().join("wt"),
            "branch-1",
            Some("abc123".to_string()),
            Some("goal-1".to_string()),
            WorktreeSessionCreator::Human,
            temp.path().join("repo"),
            Some(temp.path().join("repo").join("subdir")),
            WorktreeSessionSource::EnterWorktree,
        );

        upsert_worktree_session(&record).unwrap();
        let active = get_active_worktree_session("session-1")
            .unwrap()
            .expect("active session");
        assert_eq!(active.session_id, "session-1");
        assert_eq!(active.repo_id, "repo-1");
        assert_eq!(active.linked_goal_id.as_deref(), Some("goal-1"));
        assert_eq!(active.status, WorktreeSessionStatus::Active);

        mark_worktree_session_cleanup_failed("session-1", &record.worktree_path).unwrap();
        let active = get_active_worktree_session("session-1")
            .unwrap()
            .expect("cleanup_failed remains active");
        assert_eq!(active.status, WorktreeSessionStatus::CleanupFailed);

        mark_worktree_session_kept("session-1", &record.worktree_path).unwrap();
        assert!(get_active_worktree_session("session-1").unwrap().is_none());

        let sessions = list_worktree_sessions_for_repo("repo-1").unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].status, WorktreeSessionStatus::Kept);
        assert!(sessions[0].kept_at.is_some());

        let sessions = list_worktree_sessions_for_goal("goal-1").unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "session-1");

        let sessions = list_worktree_sessions_for_session("session-1").unwrap();
        assert_eq!(sessions.len(), 1);

        mark_worktree_session_orphaned("session-1", &record.worktree_path).unwrap();
        let sessions = list_reconcilable_worktree_sessions().unwrap();
        assert!(sessions.is_empty());
    }
}
