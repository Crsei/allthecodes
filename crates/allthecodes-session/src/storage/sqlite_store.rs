use std::collections::HashSet;
use std::path::Path;

use allthecodes_db::{Migration, MigrationRunner};
use anyhow::{Context, Result};
use chrono::Utc;
use sqlx::{Row, SqlitePool};
use tracing::warn;

use super::file_store::{cursor_for_session, load_session_file_from_path};
use super::{
    derive_title, get_session_dir, workspace_key, workspace_name, workspace_root,
    SerializableMessage, SessionFile, SessionInfo, SessionListCursor, SessionListPage,
};

const MIGRATIONS: &[Migration] = &[
    Migration::new(
        1,
        r#"
            CREATE TABLE IF NOT EXISTS sessions (
                session_id TEXT PRIMARY KEY NOT NULL,
                created_at INTEGER NOT NULL,
                last_modified INTEGER NOT NULL,
                cwd TEXT NOT NULL,
                title TEXT NOT NULL DEFAULT '',
                custom_title TEXT,
                chat_mode_override TEXT,
                message_count INTEGER NOT NULL DEFAULT 0,
                workspace_key TEXT NOT NULL DEFAULT '',
                workspace_root TEXT NOT NULL DEFAULT '',
                workspace_name TEXT NOT NULL DEFAULT '',
                archived INTEGER NOT NULL DEFAULT 0,
                archived_at INTEGER
            )
            "#,
    ),
    Migration::new(
        2,
        r#"
            CREATE TABLE IF NOT EXISTS session_messages (
                session_id TEXT NOT NULL,
                position INTEGER NOT NULL,
                role TEXT,
                msg_type TEXT NOT NULL,
                uuid TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                content TEXT NOT NULL,
                PRIMARY KEY (session_id, position),
                FOREIGN KEY (session_id)
                    REFERENCES sessions(session_id)
                    ON DELETE CASCADE
            )
            "#,
    ),
    Migration::new(
        3,
        r#"
            CREATE INDEX IF NOT EXISTS idx_sessions_list
                ON sessions(archived, last_modified DESC, created_at DESC, session_id ASC)
            "#,
    ),
    Migration::new(
        4,
        r#"
            CREATE INDEX IF NOT EXISTS idx_session_messages_session
                ON session_messages(session_id, position)
            "#,
    ),
    Migration::new(
        5,
        r#"
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
            "#,
    ),
];

pub(super) struct SessionIndex {
    pub(super) sessions: Vec<SessionInfo>,
    pub(super) archived_ids: HashSet<String>,
}

pub(super) fn save_session_file(file: &SessionFile) -> Result<()> {
    let file = file.clone();
    allthecodes_db::run_sqlite_sync("allthecodes-session-sqlite", async move {
        let pool = migrated_pool().await?;
        let mut tx = pool
            .begin()
            .await
            .context("failed to begin sqlite session save transaction")?;

        let cwd_path = Path::new(&file.cwd);
        let ws_root = workspace_root(cwd_path);
        let ws_key = workspace_key(cwd_path);
        let ws_name = workspace_name(&ws_root);
        let title = display_title(&file);

        sqlx::query(
            r#"
                INSERT INTO sessions (
                    session_id,
                    created_at,
                    last_modified,
                    cwd,
                    title,
                    custom_title,
                    chat_mode_override,
                    message_count,
                    workspace_key,
                    workspace_root,
                    workspace_name,
                    archived,
                    archived_at
                )
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, NULL)
                ON CONFLICT(session_id) DO UPDATE SET
                    created_at = excluded.created_at,
                    last_modified = excluded.last_modified,
                    cwd = excluded.cwd,
                    title = excluded.title,
                    custom_title = excluded.custom_title,
                    chat_mode_override = excluded.chat_mode_override,
                    message_count = excluded.message_count,
                    workspace_key = excluded.workspace_key,
                    workspace_root = excluded.workspace_root,
                    workspace_name = excluded.workspace_name,
                    archived = 0,
                    archived_at = NULL
                "#,
        )
        .bind(&file.session_id)
        .bind(file.created_at)
        .bind(file.last_modified)
        .bind(&file.cwd)
        .bind(&title)
        .bind(&file.custom_title)
        .bind(&file.chat_mode_override)
        .bind(usize_to_i64(file.messages.len()))
        .bind(&ws_key)
        .bind(ws_root.to_string_lossy().to_string())
        .bind(&ws_name)
        .execute(&mut *tx)
        .await
        .context("failed to upsert sqlite session")?;

        sqlx::query("DELETE FROM session_messages WHERE session_id = ?")
            .bind(&file.session_id)
            .execute(&mut *tx)
            .await
            .context("failed to clear sqlite session messages")?;

        for (position, message) in file.messages.iter().enumerate() {
            sqlx::query(
                r#"
                    INSERT INTO session_messages (
                        session_id,
                        position,
                        role,
                        msg_type,
                        uuid,
                        timestamp,
                        content
                    )
                    VALUES (?, ?, ?, ?, ?, ?, ?)
                    "#,
            )
            .bind(&file.session_id)
            .bind(usize_to_i64(position))
            .bind(role_for_message(message))
            .bind(&message.msg_type)
            .bind(&message.uuid)
            .bind(message.timestamp)
            .bind(
                serde_json::to_string(&message.data)
                    .context("failed to serialize sqlite session message JSON")?,
            )
            .execute(&mut *tx)
            .await
            .context("failed to insert sqlite session message")?;
        }

        tx.commit()
            .await
            .context("failed to commit sqlite session save transaction")?;
        Ok(())
    })
}

pub(super) enum SessionLookup {
    Active(SessionFile),
    Archived,
    Missing,
}

pub(super) fn lookup_session_file(session_id: &str) -> Result<SessionLookup> {
    let session_id = session_id.to_string();
    allthecodes_db::run_sqlite_sync("allthecodes-session-sqlite", async move {
        let pool = migrated_pool().await?;
        let session_row = sqlx::query(
            r#"
                SELECT
                    session_id,
                    created_at,
                    last_modified,
                    cwd,
                    custom_title,
                    chat_mode_override,
                    archived
                FROM sessions
                WHERE session_id = ?
                "#,
        )
        .bind(&session_id)
        .fetch_optional(&pool)
        .await
        .context("failed to query sqlite session")?;

        let Some(session_row) = session_row else {
            return Ok(SessionLookup::Missing);
        };

        if session_row.try_get::<i64, _>("archived")? != 0 {
            return Ok(SessionLookup::Archived);
        }

        let message_rows = sqlx::query(
            r#"
                SELECT msg_type, uuid, timestamp, content
                FROM session_messages
                WHERE session_id = ?
                ORDER BY position ASC
                "#,
        )
        .bind(&session_id)
        .fetch_all(&pool)
        .await
        .context("failed to query sqlite session messages")?;

        let mut messages = Vec::with_capacity(message_rows.len());
        for row in message_rows {
            let content: String = row.try_get("content")?;
            messages.push(SerializableMessage {
                msg_type: row.try_get("msg_type")?,
                uuid: row.try_get("uuid")?,
                timestamp: row.try_get("timestamp")?,
                data: serde_json::from_str(&content)
                    .context("failed to parse sqlite session message JSON")?,
            });
        }

        Ok(SessionLookup::Active(SessionFile {
            session_id: session_row.try_get("session_id")?,
            created_at: session_row.try_get("created_at")?,
            last_modified: session_row.try_get("last_modified")?,
            cwd: session_row.try_get("cwd")?,
            custom_title: session_row.try_get("custom_title")?,
            chat_mode_override: session_row.try_get("chat_mode_override")?,
            messages,
        }))
    })
}

pub(super) fn list_session_index() -> Result<SessionIndex> {
    allthecodes_db::run_sqlite_sync("allthecodes-session-sqlite", async move {
        let pool = migrated_pool().await?;
        import_legacy_json_sessions(&pool).await?;
        let rows = sqlx::query(
            r#"
                SELECT
                    session_id,
                    created_at,
                    last_modified,
                    message_count,
                    cwd,
                    title,
                    custom_title,
                    chat_mode_override,
                    workspace_key,
                    workspace_root,
                    workspace_name
                FROM sessions
                WHERE archived = 0
                ORDER BY last_modified DESC, created_at DESC, session_id ASC
                "#,
        )
        .fetch_all(&pool)
        .await
        .context("failed to list sqlite sessions")?;

        let archived_rows = sqlx::query(
            r#"
                SELECT session_id
                FROM sessions
                WHERE archived != 0
                "#,
        )
        .fetch_all(&pool)
        .await
        .context("failed to list archived sqlite sessions")?;

        let sessions = rows
            .into_iter()
            .map(session_info_from_row)
            .collect::<Result<Vec<_>>>()?;
        let archived_ids = archived_rows
            .into_iter()
            .map(|row| row.try_get::<String, _>("session_id"))
            .collect::<std::result::Result<HashSet<_>, _>>()
            .context("failed to decode archived sqlite session ids")?;

        Ok(SessionIndex {
            sessions,
            archived_ids,
        })
    })
}

pub(super) fn list_session_page(
    limit: usize,
    cursor: Option<SessionListCursor>,
) -> Result<SessionListPage> {
    allthecodes_db::run_sqlite_sync("allthecodes-session-sqlite", async move {
        let pool = migrated_pool().await?;
        import_legacy_json_sessions(&pool).await?;
        list_session_page_from_pool(&pool, limit, cursor, None).await
    })
}

pub(super) fn list_workspace_session_page(
    cwd: &Path,
    limit: usize,
    cursor: Option<SessionListCursor>,
) -> Result<SessionListPage> {
    let workspace_key = workspace_key(cwd);
    allthecodes_db::run_sqlite_sync("allthecodes-session-sqlite", async move {
        let pool = migrated_pool().await?;
        import_legacy_json_sessions(&pool).await?;
        list_session_page_from_pool(&pool, limit, cursor, Some(workspace_key)).await
    })
}

pub(super) fn archive_session(session_id: &str) -> Result<bool> {
    let session_id = session_id.to_string();
    allthecodes_db::run_sqlite_sync("allthecodes-session-sqlite", async move {
        let pool = migrated_pool().await?;
        let now = Utc::now().timestamp();
        let result = sqlx::query(
            r#"
                UPDATE sessions
                SET archived = 1,
                    archived_at = ?,
                    last_modified = ?
                WHERE session_id = ? AND archived = 0
                "#,
        )
        .bind(now)
        .bind(now)
        .bind(&session_id)
        .execute(&pool)
        .await
        .context("failed to archive sqlite session")?;

        if result.rows_affected() > 0 {
            return Ok(true);
        }

        let archived_row = sqlx::query(
            r#"
                SELECT 1
                FROM sessions
                WHERE session_id = ? AND archived != 0
                "#,
        )
        .bind(&session_id)
        .fetch_optional(&pool)
        .await
        .context("failed to query archived sqlite session")?;

        Ok(archived_row.is_some())
    })
}

async fn migrated_pool() -> Result<SqlitePool> {
    let pool = allthecodes_db::DbPoolManager::new().state_pool()?;
    MigrationRunner::new("sessions", MIGRATIONS)
        .run(&pool)
        .await?;
    Ok(pool)
}

async fn import_legacy_json_sessions(pool: &SqlitePool) -> Result<()> {
    let dir = get_session_dir();
    if !dir.exists() {
        return Ok(());
    }

    for entry in std::fs::read_dir(&dir)
        .with_context(|| format!("Failed to read session directory {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }

        let file = match load_session_file_from_path(&path) {
            Ok(file) => file,
            Err(err) => {
                warn!(
                    path = %path.display(),
                    error = %err,
                    "skipping invalid legacy session JSON during sqlite import"
                );
                continue;
            }
        };

        let exists = sqlx::query("SELECT 1 FROM sessions WHERE session_id = ?")
            .bind(&file.session_id)
            .fetch_optional(pool)
            .await
            .context("failed to check sqlite session before legacy import")?
            .is_some();
        if exists {
            continue;
        }

        save_session_file_to_pool(pool, &file)
            .await
            .with_context(|| format!("failed to import legacy session {}", file.session_id))?;
    }

    Ok(())
}

async fn save_session_file_to_pool(pool: &SqlitePool, file: &SessionFile) -> Result<()> {
    let mut tx = pool
        .begin()
        .await
        .context("failed to begin sqlite session save transaction")?;

    let cwd_path = Path::new(&file.cwd);
    let ws_root = workspace_root(cwd_path);
    let ws_key = workspace_key(cwd_path);
    let ws_name = workspace_name(&ws_root);
    let title = display_title(file);

    sqlx::query(
        r#"
            INSERT INTO sessions (
                session_id,
                created_at,
                last_modified,
                cwd,
                title,
                custom_title,
                chat_mode_override,
                message_count,
                workspace_key,
                workspace_root,
                workspace_name,
                archived,
                archived_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, NULL)
            ON CONFLICT(session_id) DO NOTHING
            "#,
    )
    .bind(&file.session_id)
    .bind(file.created_at)
    .bind(file.last_modified)
    .bind(&file.cwd)
    .bind(&title)
    .bind(&file.custom_title)
    .bind(&file.chat_mode_override)
    .bind(usize_to_i64(file.messages.len()))
    .bind(&ws_key)
    .bind(ws_root.to_string_lossy().to_string())
    .bind(&ws_name)
    .execute(&mut *tx)
    .await
    .context("failed to insert sqlite legacy session")?;

    for (position, message) in file.messages.iter().enumerate() {
        sqlx::query(
            r#"
                INSERT OR IGNORE INTO session_messages (
                    session_id,
                    position,
                    role,
                    msg_type,
                    uuid,
                    timestamp,
                    content
                )
                VALUES (?, ?, ?, ?, ?, ?, ?)
                "#,
        )
        .bind(&file.session_id)
        .bind(usize_to_i64(position))
        .bind(role_for_message(message))
        .bind(&message.msg_type)
        .bind(&message.uuid)
        .bind(message.timestamp)
        .bind(
            serde_json::to_string(&message.data)
                .context("failed to serialize sqlite session message JSON")?,
        )
        .execute(&mut *tx)
        .await
        .context("failed to insert sqlite legacy session message")?;
    }

    tx.commit()
        .await
        .context("failed to commit sqlite legacy session import")?;
    Ok(())
}

async fn list_session_page_from_pool(
    pool: &SqlitePool,
    limit: usize,
    cursor: Option<SessionListCursor>,
    workspace_key_filter: Option<String>,
) -> Result<SessionListPage> {
    let fetch_limit = usize_to_i64(limit.saturating_add(1));
    let rows = match (cursor, workspace_key_filter) {
        (Some(cursor), Some(workspace_key)) => {
            sqlx::query(
                r#"
                    SELECT
                        session_id,
                        created_at,
                        last_modified,
                        message_count,
                        cwd,
                        title,
                        custom_title,
                        chat_mode_override,
                        workspace_key,
                        workspace_root,
                        workspace_name
                    FROM sessions
                    WHERE archived = 0
                      AND workspace_key = ?
                      AND (
                          last_modified < ?
                          OR (last_modified = ? AND created_at < ?)
                          OR (last_modified = ? AND created_at = ? AND session_id > ?)
                      )
                    ORDER BY last_modified DESC, created_at DESC, session_id ASC
                    LIMIT ?
                    "#,
            )
            .bind(workspace_key)
            .bind(cursor.last_modified)
            .bind(cursor.last_modified)
            .bind(cursor.created_at)
            .bind(cursor.last_modified)
            .bind(cursor.created_at)
            .bind(cursor.session_id)
            .bind(fetch_limit)
            .fetch_all(pool)
            .await
        }
        (Some(cursor), None) => {
            sqlx::query(
                r#"
                    SELECT
                        session_id,
                        created_at,
                        last_modified,
                        message_count,
                        cwd,
                        title,
                        custom_title,
                        chat_mode_override,
                        workspace_key,
                        workspace_root,
                        workspace_name
                    FROM sessions
                    WHERE archived = 0
                      AND (
                          last_modified < ?
                          OR (last_modified = ? AND created_at < ?)
                          OR (last_modified = ? AND created_at = ? AND session_id > ?)
                      )
                    ORDER BY last_modified DESC, created_at DESC, session_id ASC
                    LIMIT ?
                    "#,
            )
            .bind(cursor.last_modified)
            .bind(cursor.last_modified)
            .bind(cursor.created_at)
            .bind(cursor.last_modified)
            .bind(cursor.created_at)
            .bind(cursor.session_id)
            .bind(fetch_limit)
            .fetch_all(pool)
            .await
        }
        (None, Some(workspace_key)) => {
            sqlx::query(
                r#"
                    SELECT
                        session_id,
                        created_at,
                        last_modified,
                        message_count,
                        cwd,
                        title,
                        custom_title,
                        chat_mode_override,
                        workspace_key,
                        workspace_root,
                        workspace_name
                    FROM sessions
                    WHERE archived = 0 AND workspace_key = ?
                    ORDER BY last_modified DESC, created_at DESC, session_id ASC
                    LIMIT ?
                    "#,
            )
            .bind(workspace_key)
            .bind(fetch_limit)
            .fetch_all(pool)
            .await
        }
        (None, None) => {
            sqlx::query(
                r#"
                    SELECT
                        session_id,
                        created_at,
                        last_modified,
                        message_count,
                        cwd,
                        title,
                        custom_title,
                        chat_mode_override,
                        workspace_key,
                        workspace_root,
                        workspace_name
                    FROM sessions
                    WHERE archived = 0
                    ORDER BY last_modified DESC, created_at DESC, session_id ASC
                    LIMIT ?
                    "#,
            )
            .bind(fetch_limit)
            .fetch_all(pool)
            .await
        }
    }
    .context("failed to page sqlite sessions")?;

    let mut sessions = rows
        .into_iter()
        .map(session_info_from_row)
        .collect::<Result<Vec<_>>>()?;
    let next_cursor = if sessions.len() > limit {
        Some(cursor_for_session(&sessions[limit - 1]))
    } else {
        None
    };
    sessions.truncate(limit);
    Ok(SessionListPage {
        sessions,
        next_cursor,
    })
}

fn session_info_from_row(row: sqlx::sqlite::SqliteRow) -> Result<SessionInfo> {
    let cwd: String = row.try_get("cwd")?;
    let workspace_root_value: String = row.try_get("workspace_root")?;
    let workspace_name_value: String = row.try_get("workspace_name")?;
    let cwd_path = Path::new(&cwd);
    let computed_root = workspace_root(cwd_path);
    let computed_key = workspace_key(cwd_path);
    let computed_name = workspace_name(&computed_root);
    Ok(SessionInfo {
        session_id: row.try_get("session_id")?,
        created_at: row.try_get("created_at")?,
        last_modified: row.try_get("last_modified")?,
        message_count: i64_to_usize(row.try_get("message_count")?),
        cwd,
        title: row.try_get("title")?,
        custom_title: row.try_get("custom_title")?,
        chat_mode_override: row.try_get("chat_mode_override")?,
        workspace_key: row
            .try_get::<String, _>("workspace_key")
            .ok()
            .filter(|value| !value.is_empty())
            .unwrap_or(computed_key),
        workspace_root: if workspace_root_value.is_empty() {
            computed_root.to_string_lossy().to_string()
        } else {
            workspace_root_value
        },
        workspace_name: if workspace_name_value.is_empty() {
            computed_name
        } else {
            workspace_name_value
        },
    })
}

fn display_title(file: &SessionFile) -> String {
    file.custom_title
        .as_ref()
        .filter(|title| !title.is_empty())
        .cloned()
        .unwrap_or_else(|| derive_title(&file.messages))
}

fn role_for_message(message: &SerializableMessage) -> Option<&str> {
    match message.msg_type.as_str() {
        "user" => Some("user"),
        "assistant" => Some("assistant"),
        "system" => Some("system"),
        _ => None,
    }
}

fn i64_to_usize(value: i64) -> usize {
    usize::try_from(value.max(0)).unwrap_or(usize::MAX)
}

fn usize_to_i64(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}
