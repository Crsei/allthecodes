use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::warn;

use super::paths;
use super::types::{RecordItem, RecordLine};

/// SQLite `session_rollouts` table schema SQL.
pub const SESSION_ROLLOUTS_SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS session_rollouts (
  session_id TEXT NOT NULL,
  rollout_path TEXT NOT NULL,
  schema_version INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  first_seq INTEGER NOT NULL DEFAULT 0,
  last_seq INTEGER NOT NULL DEFAULT 0,
  event_count INTEGER NOT NULL DEFAULT 0,
  status TEXT NOT NULL DEFAULT 'active',
  parent_session_id TEXT,
  branch_from_seq INTEGER,
  workspace_key TEXT,
  workspace_root TEXT,
  workspace_name TEXT,
  PRIMARY KEY (session_id, rollout_path)
);

CREATE INDEX IF NOT EXISTS idx_session_rollouts_session_id
  ON session_rollouts(session_id, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_session_rollouts_workspace
  ON session_rollouts(workspace_key, updated_at DESC);
"#;

pub struct SessionRolloutsMigration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRolloutIndexEntry {
    pub session_id: String,
    pub rollout_path: PathBuf,
    pub schema_version: u32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub first_seq: u64,
    pub last_seq: u64,
    pub event_count: u64,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch_from_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_name: Option<String>,
}

/// Look up the latest active rollout file path for a session.
///
/// Checks the SQLite index first; if SQLite is unavailable or no entry
/// exists, falls back to scanning the rollouts directory tree.
pub fn lookup_rollout(session_id: &str) -> Result<Option<PathBuf>> {
    // Try SQLite index first
    #[cfg(feature = "sqlite-storage")]
    match lookup_rollout_sqlite(session_id) {
        Ok(Some(path)) if path.exists() => return Ok(Some(path)),
        Ok(Some(path)) => {
            warn!(
                session_id,
                path = %path.display(),
                "SQLite rollout path is missing; falling back to filesystem scan"
            );
        }
        Ok(None) => {}
        Err(e) => {
            warn!(session_id, error = %e, "SQLite rollout lookup failed");
        }
    }

    // Fallback: scan filesystem
    scan_rollout_file_filesystem(session_id)
}

/// Upsert a rollout entry in the SQLite index.
pub fn upsert_rollout(index_entry: &SessionRolloutIndexEntry) -> Result<()> {
    let entry = index_entry.clone();
    allthecodes_db::run_sqlite_sync("allthecodes-session-rollouts-sqlite", async move {
        let pool = allthecodes_db::DbPoolManager::new().state_pool()?;
        let pool = pool;

        ensure_session_rollouts_table(&pool).await?;

        sqlx::query(
            r#"
            INSERT INTO session_rollouts (
                session_id, rollout_path, schema_version,
                created_at, updated_at,
                first_seq, last_seq, event_count,
                status, parent_session_id, branch_from_seq,
                workspace_key, workspace_root, workspace_name
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(session_id, rollout_path) DO UPDATE SET
                schema_version = excluded.schema_version,
                created_at = excluded.created_at,
                updated_at = excluded.updated_at,
                first_seq = excluded.first_seq,
                last_seq = excluded.last_seq,
                event_count = excluded.event_count,
                status = excluded.status,
                parent_session_id = excluded.parent_session_id,
                branch_from_seq = excluded.branch_from_seq,
                workspace_key = excluded.workspace_key,
                workspace_root = excluded.workspace_root,
                workspace_name = excluded.workspace_name
            "#,
        )
        .bind(&entry.session_id)
        .bind(entry.rollout_path.to_string_lossy().to_string())
        .bind(i64::from(entry.schema_version))
        .bind(entry.created_at.to_rfc3339())
        .bind(entry.updated_at.to_rfc3339())
        .bind(i64::try_from(entry.first_seq).unwrap_or(0))
        .bind(i64::try_from(entry.last_seq).unwrap_or(0))
        .bind(i64::try_from(entry.event_count).unwrap_or(0))
        .bind(&entry.status)
        .bind(&entry.parent_session_id)
        .bind(entry.branch_from_seq.map(|v| i64::try_from(v).unwrap_or(0)))
        .bind(&entry.workspace_key)
        .bind(&entry.workspace_root)
        .bind(&entry.workspace_name)
        .execute(&pool)
        .await
        .context("failed to upsert session_rollouts index entry")?;

        Ok(())
    })
}

/// Ensure the session has a rollout log, lazily migrating legacy JSON if needed.
pub fn ensure_rollout_for_session(session_id: &str) -> Result<PathBuf> {
    if let Some(path) = lookup_rollout(session_id)? {
        return Ok(path);
    }
    super::migration::migrate_legacy_session(session_id)
}

/// Append canonical record items to a session rollout and rebuild derived views.
///
/// This is intended for non-streaming mutations such as rollback and branch
/// metadata. Live turns should keep using `SessionRecorderHandle`.
pub fn append_record_items(
    session_id: &str,
    items: Vec<RecordItem>,
) -> Result<SessionRolloutIndexEntry> {
    let rollout_path = ensure_rollout_for_session(session_id)?;
    append_record_items_to_rollout(session_id, &rollout_path, items)
}

pub fn append_record_items_to_rollout(
    session_id: &str,
    rollout_path: &Path,
    items: Vec<RecordItem>,
) -> Result<SessionRolloutIndexEntry> {
    if items.is_empty() {
        return index_rollout_file(rollout_path);
    }

    let read = super::reader::read_rollout_file(rollout_path)
        .with_context(|| format!("failed to read rollout file {}", rollout_path.display()))?;
    let mut next_seq = read
        .lines
        .iter()
        .map(|line| line.seq)
        .max()
        .unwrap_or(0)
        .saturating_add(1);

    let mut file = OpenOptions::new()
        .read(true)
        .append(true)
        .open(rollout_path)
        .with_context(|| {
            format!(
                "failed to open rollout {} for append",
                rollout_path.display()
            )
        })?;
    ensure_append_starts_on_new_line(&mut file)
        .with_context(|| format!("failed to normalize rollout {}", rollout_path.display()))?;

    for item in items {
        let line = RecordLine::new(session_id, next_seq, item);
        next_seq = next_seq.saturating_add(1);
        let json = serde_json::to_string(&line).context("failed to serialize record line")?;
        writeln!(file, "{json}")
            .with_context(|| format!("failed to append rollout {}", rollout_path.display()))?;
    }
    file.sync_all()
        .with_context(|| format!("failed to sync rollout {}", rollout_path.display()))?;

    index_rollout_file(rollout_path)
}

fn ensure_append_starts_on_new_line(file: &mut std::fs::File) -> Result<()> {
    let len = file.metadata()?.len();
    if len == 0 {
        return Ok(());
    }
    file.seek(SeekFrom::End(-1))?;
    let mut byte = [0_u8; 1];
    file.read_exact(&mut byte)?;
    if byte[0] != b'\n' {
        file.write_all(b"\n")?;
    }
    file.seek(SeekFrom::End(0))?;
    Ok(())
}

/// Rebuild the session_rollouts index by scanning the rollouts directory.
pub fn reindex_rollouts() -> Result<u64> {
    let rollout_dir = paths::rollouts_dir();
    if !rollout_dir.exists() {
        return Ok(0);
    }

    let mut count = 0u64;

    let walker = walkdir::WalkDir::new(&rollout_dir)
        .max_depth(5)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| paths::is_rollout_path(e.path()));

    for entry in walker {
        let path = entry.path().to_path_buf();
        match index_rollout_file(&path) {
            Ok(_) => count += 1,
            Err(e) => {
                warn!(
                    path = %path.display(),
                    error = %e,
                    "failed to reindex rollout file"
                );
            }
        }
    }

    Ok(count)
}

/// Index a single rollout file and rebuild the derived session projection.
pub(crate) fn index_rollout_file(path: &Path) -> Result<SessionRolloutIndexEntry> {
    let read = super::reader::read_rollout_file(path)
        .with_context(|| format!("failed to read rollout file {}", path.display()))?;
    if read.lines.is_empty() {
        bail!(
            "rollout file {} has no readable record lines",
            path.display()
        );
    }

    let metadata_line = read
        .lines
        .iter()
        .find(|line| matches!(line.item, RecordItem::SessionMeta(_)));
    let metadata = metadata_line.and_then(|line| match &line.item {
        RecordItem::SessionMeta(metadata) => Some(metadata.clone()),
        _ => None,
    });
    let session_id = metadata_line
        .map(|line| line.session_id.clone())
        .unwrap_or_else(|| read.lines[0].session_id.clone());
    if session_id.is_empty() {
        bail!("rollout file {} has an empty session id", path.display());
    }

    let first_seq = read.lines.iter().map(|line| line.seq).min().unwrap_or(0);
    let last_seq = read.lines.iter().map(|line| line.seq).max().unwrap_or(0);
    let schema_version = read
        .lines
        .iter()
        .map(|line| line.schema_version)
        .max()
        .unwrap_or(0);
    let created_at = metadata
        .as_ref()
        .map(|metadata| metadata.created_at)
        .unwrap_or_else(|| {
            read.lines
                .iter()
                .map(|line| line.timestamp)
                .min()
                .unwrap_or_else(Utc::now)
        });
    let updated_at = read
        .lines
        .iter()
        .map(|line| line.timestamp)
        .max()
        .unwrap_or_else(Utc::now);
    let cwd = metadata
        .as_ref()
        .map(|metadata| metadata.cwd.as_str())
        .filter(|cwd| !cwd.is_empty())
        .unwrap_or(".");
    let messages = super::reconstruct::reconstruct_messages(&read.lines);

    crate::storage::save_session(&session_id, &messages, cwd)
        .with_context(|| format!("failed to rebuild session projection for {session_id}"))?;

    let entry = SessionRolloutIndexEntry {
        session_id,
        rollout_path: path.to_path_buf(),
        schema_version,
        created_at,
        updated_at,
        first_seq,
        last_seq,
        event_count: u64::try_from(read.lines.len()).unwrap_or(u64::MAX),
        status: "active".to_string(),
        parent_session_id: metadata
            .as_ref()
            .and_then(|metadata| metadata.parent_session_id.clone()),
        branch_from_seq: metadata
            .as_ref()
            .and_then(|metadata| metadata.branch_from_seq),
        workspace_key: metadata
            .as_ref()
            .and_then(|metadata| metadata.workspace_key.clone()),
        workspace_root: metadata
            .as_ref()
            .and_then(|metadata| metadata.workspace_root.clone()),
        workspace_name: metadata
            .as_ref()
            .and_then(|metadata| metadata.workspace_name.clone()),
    };

    upsert_rollout(&entry)?;
    Ok(entry)
}

// -- SQLite helpers --

#[cfg(feature = "sqlite-storage")]
fn lookup_rollout_sqlite(session_id: &str) -> Result<Option<PathBuf>> {
    let sid = session_id.to_string();
    allthecodes_db::run_sqlite_sync("allthecodes-session-rollouts-lookup", async move {
        let pool = allthecodes_db::DbPoolManager::new().state_pool()?;
        ensure_session_rollouts_table(&pool).await?;

        let row: Option<(String,)> = sqlx::query_as(
            r#"
            SELECT rollout_path
            FROM session_rollouts
            WHERE session_id = ?
              AND status = 'active'
            ORDER BY updated_at DESC
            LIMIT 1
            "#,
        )
        .bind(&sid)
        .fetch_optional(&pool)
        .await
        .context("failed to query session_rollouts index")?;

        Ok(row.map(|(path,)| PathBuf::from(path)))
    })
}

#[cfg(feature = "sqlite-storage")]
async fn ensure_session_rollouts_table(pool: &sqlx::SqlitePool) -> Result<()> {
    sqlx::raw_sql(SESSION_ROLLOUTS_SCHEMA_SQL)
        .execute(pool)
        .await
        .context("failed to create session_rollouts table")?;
    Ok(())
}

fn scan_rollout_file_filesystem(session_id: &str) -> Result<Option<PathBuf>> {
    let rollout_dir = paths::rollouts_dir();
    if !rollout_dir.exists() {
        return Ok(None);
    }

    for entry in walkdir::WalkDir::new(&rollout_dir)
        .max_depth(5)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let name = entry.file_name().to_string_lossy();
        if name.contains(session_id) && paths::is_rollout_path(entry.path()) {
            return Ok(Some(entry.path().to_path_buf()));
        }
    }

    Ok(None)
}

#[cfg(not(feature = "sqlite-storage"))]
fn lookup_rollout_sqlite(_session_id: &str) -> Result<Option<PathBuf>> {
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_types::message::{Message, MessageContent};
    use chrono::TimeZone;
    use serial_test::serial;

    use crate::record_replay::types::{
        MessageRecord, RecordLine, RecordedMessage, RecordedMessageContent, SessionMetaRecord,
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

    #[test]
    #[serial]
    fn index_rollout_file_rebuilds_session_projection() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = HomeGuard::set(temp.path());
        let session_id = "indexed-rollout-session";
        let created_at = Utc.with_ymd_and_hms(2026, 7, 2, 10, 0, 0).unwrap();
        let rollout_path = paths::new_rollout_file(session_id, created_at);
        std::fs::create_dir_all(rollout_path.parent().unwrap()).unwrap();

        let lines = vec![
            RecordLine::new(
                session_id,
                0,
                RecordItem::SessionMeta(SessionMetaRecord {
                    created_at,
                    cwd: "/repo/from-rollout".into(),
                    workspace_key: Some("workspace-key".into()),
                    workspace_root: Some("/repo".into()),
                    workspace_name: Some("repo".into()),
                    model: Some("model-from-rollout".into()),
                    config_summary: None,
                    parent_session_id: None,
                    branch_from_seq: None,
                    migrated_from: None,
                }),
            ),
            RecordLine::new(
                session_id,
                1,
                RecordItem::Message(MessageRecord {
                    message: RecordedMessage::User {
                        uuid: uuid::Uuid::parse_str("10000000-0000-0000-0000-000000000020")
                            .unwrap(),
                        timestamp: 77,
                        role: "user".into(),
                        content: RecordedMessageContent::Text("from rollout".into()),
                        is_meta: false,
                        tool_use_result: None,
                        source_tool_assistant_uuid: None,
                    },
                }),
            ),
        ];
        let content = format!(
            "{}\n",
            lines
                .iter()
                .map(|line| serde_json::to_string(line).unwrap())
                .collect::<Vec<_>>()
                .join("\n")
        );
        std::fs::write(&rollout_path, content).unwrap();

        let entry = index_rollout_file(&rollout_path).unwrap();

        assert_eq!(entry.session_id, session_id);
        assert_eq!(entry.last_seq, 1);
        let loaded = crate::storage::load_session(session_id).unwrap();
        assert_eq!(loaded.len(), 1);
        assert!(matches!(
            &loaded[0],
            Message::User(user)
                if matches!(&user.content, MessageContent::Text(text) if text == "from rollout")
        ));
        let info = crate::storage::load_session_info(session_id).unwrap();
        assert_eq!(info.cwd, "/repo/from-rollout");
    }

    #[test]
    #[serial]
    fn lookup_rollout_skips_missing_sqlite_path_and_scans_filesystem() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = HomeGuard::set(temp.path());
        let session_id = "stale-index-rollout-session";
        let created_at = Utc.with_ymd_and_hms(2026, 7, 2, 12, 0, 0).unwrap();
        let valid_path = paths::new_rollout_file(session_id, created_at);
        std::fs::create_dir_all(valid_path.parent().unwrap()).unwrap();
        std::fs::write(&valid_path, "").unwrap();

        upsert_rollout(&SessionRolloutIndexEntry {
            session_id: session_id.to_string(),
            rollout_path: temp.path().join("missing-rollout.jsonl"),
            schema_version: crate::record_replay::RECORD_SCHEMA_VERSION,
            created_at,
            updated_at: created_at,
            first_seq: 0,
            last_seq: 0,
            event_count: 0,
            status: "active".to_string(),
            parent_session_id: None,
            branch_from_seq: None,
            workspace_key: None,
            workspace_root: None,
            workspace_name: None,
        })
        .unwrap();

        assert_eq!(lookup_rollout(session_id).unwrap(), Some(valid_path));
    }
}
