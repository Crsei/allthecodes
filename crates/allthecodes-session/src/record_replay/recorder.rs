use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio::sync::{mpsc, oneshot, Mutex};
use tracing::warn;

use super::config::RecordReplayConfig;
use super::paths;
use super::policy::should_persist;
use super::redaction::redact_record_item;
use super::types::{RecordItem, RecordLine, SessionMetaRecord, RECORD_SCHEMA_VERSION};

pub const DEFAULT_RECORDER_CHANNEL_CAPACITY: usize = 256;

pub enum RecorderOpenMode {
    Create {
        session_id: String,
        cwd: PathBuf,
        created_at: DateTime<Utc>,
        metadata: SessionMetaRecord,
    },
    Resume {
        session_id: String,
        rollout_path: PathBuf,
        next_seq: u64,
    },
}

#[derive(Debug, Clone)]
pub struct RecorderStats {
    pub session_id: String,
    pub rollout_path: PathBuf,
    pub schema_version: u32,
    pub next_seq: u64,
    pub last_seq: Option<u64>,
    pub written_events: u64,
    pub dropped_events: u64,
    pub flush_count: u64,
    pub shutdown: bool,
}

pub struct SessionRecorderHandle {
    session_id: String,
    rollout_path: PathBuf,
    tx: mpsc::Sender<RecorderCommand>,
    final_stats: Arc<Mutex<Option<RecorderStats>>>,
}

pub enum RecorderCommand {
    Add(Vec<RecordItem>),
    Persist,
    Flush(oneshot::Sender<Result<RecorderStats>>),
    Shutdown(oneshot::Sender<Result<RecorderStats>>),
}

impl Clone for SessionRecorderHandle {
    fn clone(&self) -> Self {
        Self {
            session_id: self.session_id.clone(),
            rollout_path: self.rollout_path.clone(),
            tx: self.tx.clone(),
            final_stats: Arc::clone(&self.final_stats),
        }
    }
}

impl SessionRecorderHandle {
    pub async fn open(mode: RecorderOpenMode, config: RecordReplayConfig) -> Result<Self> {
        let OpenedRecorder {
            session_id,
            rollout_path,
            writer,
            next_seq,
            written_events,
            last_seq,
        } = open_writer(mode).await?;

        let stats = RecorderStats {
            session_id: session_id.clone(),
            rollout_path: rollout_path.clone(),
            schema_version: RECORD_SCHEMA_VERSION,
            next_seq,
            last_seq,
            written_events,
            dropped_events: 0,
            flush_count: 0,
            shutdown: false,
        };

        let (tx, rx) = mpsc::channel(DEFAULT_RECORDER_CHANNEL_CAPACITY);
        tokio::spawn(writer_loop(writer, session_id.clone(), config, stats, rx));

        Ok(Self {
            session_id,
            rollout_path,
            tx,
            final_stats: Arc::new(Mutex::new(None)),
        })
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn rollout_path(&self) -> &PathBuf {
        &self.rollout_path
    }

    pub async fn add(&self, items: Vec<RecordItem>) -> Result<()> {
        if items.is_empty() {
            return Ok(());
        }
        self.tx
            .send(RecorderCommand::Add(items))
            .await
            .context("failed to enqueue record items")
    }

    pub async fn persist(&self) -> Result<()> {
        self.tx
            .send(RecorderCommand::Persist)
            .await
            .context("failed to enqueue record persist")
    }

    pub async fn flush(&self) -> Result<RecorderStats> {
        if let Some(stats) = self.final_stats.lock().await.clone() {
            return Ok(stats);
        }

        let (tx, rx) = oneshot::channel();
        self.tx
            .send(RecorderCommand::Flush(tx))
            .await
            .context("failed to enqueue record flush")?;
        rx.await.context("record flush response dropped")?
    }

    pub async fn shutdown(&self) -> Result<RecorderStats> {
        if let Some(stats) = self.final_stats.lock().await.clone() {
            return Ok(stats);
        }

        let (tx, rx) = oneshot::channel();
        self.tx
            .send(RecorderCommand::Shutdown(tx))
            .await
            .context("failed to enqueue record shutdown")?;
        let stats = rx.await.context("record shutdown response dropped")??;
        *self.final_stats.lock().await = Some(stats.clone());
        Ok(stats)
    }
}

struct OpenedRecorder {
    session_id: String,
    rollout_path: PathBuf,
    writer: BufWriter<File>,
    next_seq: u64,
    written_events: u64,
    last_seq: Option<u64>,
}

async fn open_writer(mode: RecorderOpenMode) -> Result<OpenedRecorder> {
    match mode {
        RecorderOpenMode::Create {
            session_id,
            cwd,
            created_at,
            mut metadata,
        } => {
            if metadata.cwd.is_empty() {
                metadata.cwd = cwd.to_string_lossy().to_string();
            }
            let rollout_path = paths::new_rollout_file(&session_id, created_at);
            if let Some(parent) = rollout_path.parent() {
                tokio::fs::create_dir_all(parent).await.with_context(|| {
                    format!("failed to create rollout directory {}", parent.display())
                })?;
            }
            let file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&rollout_path)
                .await
                .with_context(|| {
                    format!("failed to create rollout log {}", rollout_path.display())
                })?;
            let mut writer = BufWriter::new(file);
            let meta_line = RecordLine {
                schema_version: RECORD_SCHEMA_VERSION,
                seq: 0,
                timestamp: created_at,
                session_id: session_id.clone(),
                turn_id: None,
                item: RecordItem::SessionMeta(metadata),
            };
            write_record_line(&mut writer, &meta_line).await?;
            writer.flush().await.with_context(|| {
                format!("failed to flush rollout meta {}", rollout_path.display())
            })?;
            Ok(OpenedRecorder {
                session_id,
                rollout_path,
                writer,
                next_seq: 1,
                written_events: 1,
                last_seq: Some(0),
            })
        }
        RecorderOpenMode::Resume {
            session_id,
            rollout_path,
            next_seq,
        } => {
            if let Some(parent) = rollout_path.parent() {
                tokio::fs::create_dir_all(parent).await.with_context(|| {
                    format!("failed to create rollout directory {}", parent.display())
                })?;
            }
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&rollout_path)
                .await
                .with_context(|| {
                    format!("failed to resume rollout log {}", rollout_path.display())
                })?;
            Ok(OpenedRecorder {
                session_id,
                rollout_path,
                writer: BufWriter::new(file),
                next_seq,
                written_events: 0,
                last_seq: next_seq.checked_sub(1),
            })
        }
    }
}

async fn writer_loop(
    mut writer: BufWriter<File>,
    session_id: String,
    config: RecordReplayConfig,
    mut stats: RecorderStats,
    mut rx: mpsc::Receiver<RecorderCommand>,
) {
    let mut pending: VecDeque<RecordLine> = VecDeque::new();

    while let Some(command) = rx.recv().await {
        match command {
            RecorderCommand::Add(items) => {
                enqueue_items(items, &session_id, &config, &mut stats, &mut pending);
                if let Err(error) = drain_pending(&mut writer, &mut pending, &mut stats).await {
                    warn!(error = %error, session_id, "failed to persist record items");
                }
            }
            RecorderCommand::Persist => {
                if let Err(error) = drain_pending(&mut writer, &mut pending, &mut stats).await {
                    warn!(error = %error, session_id, "failed to persist pending record items");
                }
            }
            RecorderCommand::Flush(response_tx) => {
                let result = flush_writer(&mut writer, &mut pending, &mut stats, &config).await;
                let _ = response_tx.send(result);
            }
            RecorderCommand::Shutdown(response_tx) => {
                let result = flush_writer(&mut writer, &mut pending, &mut stats, &config)
                    .await
                    .map(|mut flushed| {
                        flushed.shutdown = true;
                        stats.shutdown = true;
                        flushed
                    });
                let _ = response_tx.send(result);
                break;
            }
        }
    }
}

fn enqueue_items(
    items: Vec<RecordItem>,
    session_id: &str,
    config: &RecordReplayConfig,
    stats: &mut RecorderStats,
    pending: &mut VecDeque<RecordLine>,
) {
    for item in items {
        if !should_persist(&item, config) {
            stats.dropped_events += 1;
            continue;
        }
        let redacted = redact_record_item(item, config);
        let line = RecordLine::new(session_id, stats.next_seq, redacted.item);
        stats.next_seq += 1;
        pending.push_back(line);
    }
}

async fn drain_pending(
    writer: &mut BufWriter<File>,
    pending: &mut VecDeque<RecordLine>,
    stats: &mut RecorderStats,
) -> Result<()> {
    while let Some(line) = pending.front() {
        write_record_line(writer, line).await?;
        let line = pending.pop_front().expect("front item just existed");
        stats.last_seq = Some(line.seq);
        stats.written_events += 1;
    }
    Ok(())
}

async fn flush_writer(
    writer: &mut BufWriter<File>,
    pending: &mut VecDeque<RecordLine>,
    stats: &mut RecorderStats,
    config: &RecordReplayConfig,
) -> Result<RecorderStats> {
    let flushed = !pending.is_empty();
    drain_pending(writer, pending, stats).await?;

    if flushed {
        // Best-effort SQLite index update
        if let Some(last_seq) = stats.last_seq {
            let first_seq_val = stats
                .last_seq
                .map(|ls| ls.saturating_sub(stats.written_events.saturating_sub(1)))
                .unwrap_or(0);
            let entry = crate::record_replay::index::SessionRolloutIndexEntry {
                session_id: stats.session_id.clone(),
                rollout_path: stats.rollout_path.clone(),
                schema_version: stats.schema_version,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
                first_seq: first_seq_val,
                last_seq,
                event_count: stats.written_events,
                status: "active".to_string(),
                parent_session_id: None,
                branch_from_seq: None,
                workspace_key: None,
                workspace_root: None,
                workspace_name: None,
            };
            if let Err(e) = crate::record_replay::index::upsert_rollout(&entry) {
                warn!(
                    session_id = %stats.session_id,
                    error = %e,
                    "failed to update session_rollouts index after flush"
                );
            }
        }
    }
    writer
        .flush()
        .await
        .context("failed to flush rollout writer")?;
    if config.fsync_on_flush {
        writer
            .get_ref()
            .sync_all()
            .await
            .context("failed to sync rollout writer")?;
    }
    stats.flush_count += 1;
    Ok(stats.clone())
}

async fn write_record_line(writer: &mut BufWriter<File>, line: &RecordLine) -> Result<()> {
    let json = serde_json::to_vec(line).context("failed to serialize record line")?;
    writer
        .write_all(&json)
        .await
        .context("failed to write record line")?;
    writer
        .write_all(b"\n")
        .await
        .context("failed to write record newline")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record_replay::reader::read_rollout_file;
    use crate::record_replay::types::{
        QueryEventRecord, ToolProgressRecord, TurnFinishStatus, TurnFinishedRecord,
        TurnStartedRecord,
    };
    use chrono::TimeZone;
    use serial_test::serial;

    struct EnvGuard {
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(path: &std::path::Path) -> Self {
            let previous = std::env::var("ALLTHECODES_HOME").ok();
            std::env::set_var("ALLTHECODES_HOME", path);
            Self { previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var("ALLTHECODES_HOME", value),
                None => std::env::remove_var("ALLTHECODES_HOME"),
            }
        }
    }

    fn create_mode(session_id: &str) -> RecorderOpenMode {
        let created_at = Utc.with_ymd_and_hms(2026, 7, 2, 13, 14, 55).unwrap();
        RecorderOpenMode::Create {
            session_id: session_id.into(),
            cwd: PathBuf::from("/repo"),
            created_at,
            metadata: SessionMetaRecord {
                created_at,
                cwd: "/repo".into(),
                workspace_key: None,
                workspace_root: None,
                workspace_name: None,
                model: None,
                config_summary: None,
                parent_session_id: None,
                branch_from_seq: None,
                migrated_from: None,
            },
        }
    }

    #[tokio::test]
    #[serial]
    async fn recorder_create_flush_shutdown_writes_valid_jsonl() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set(temp.path());
        let recorder = SessionRecorderHandle::open(
            create_mode("record-create"),
            RecordReplayConfig::default(),
        )
        .await
        .unwrap();

        recorder
            .add(vec![
                RecordItem::TurnStarted(TurnStartedRecord {
                    user_message_uuid: Some("u1".into()),
                    input_summary: Some("hello".into()),
                }),
                RecordItem::TurnFinished(TurnFinishedRecord {
                    status: TurnFinishStatus::Completed,
                    abort_reason: None,
                    error: None,
                    usage: None,
                }),
            ])
            .await
            .unwrap();

        let flushed = recorder.flush().await.unwrap();
        let shutdown = recorder.shutdown().await.unwrap();
        let shutdown_again = recorder.shutdown().await.unwrap();

        assert_eq!(flushed.written_events, 3);
        assert!(shutdown.shutdown);
        assert_eq!(shutdown_again.written_events, shutdown.written_events);

        let result = read_rollout_file(recorder.rollout_path()).unwrap();
        assert_eq!(result.lines.len(), 3);
        assert!(result.warnings.is_empty());
        assert_eq!(result.lines[0].seq, 0);
        assert_eq!(result.lines[2].seq, 2);
    }

    #[tokio::test]
    #[serial]
    async fn diagnostic_items_are_not_written_when_disabled() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set(temp.path());
        let recorder = SessionRecorderHandle::open(
            create_mode("record-diagnostic"),
            RecordReplayConfig::default(),
        )
        .await
        .unwrap();

        recorder
            .add(vec![
                RecordItem::ToolProgress(ToolProgressRecord {
                    tool_use_id: "toolu_1".into(),
                    data: serde_json::json!({ "chunk": 1 }),
                }),
                RecordItem::QueryEvent(QueryEventRecord::RawStream {
                    event: serde_json::json!({ "delta": "x" }),
                }),
            ])
            .await
            .unwrap();

        let stats = recorder.shutdown().await.unwrap();
        let result = read_rollout_file(recorder.rollout_path()).unwrap();

        assert_eq!(stats.dropped_events, 2);
        assert_eq!(result.lines.len(), 1);
        assert!(matches!(result.lines[0].item, RecordItem::SessionMeta(_)));
    }

    #[tokio::test]
    #[serial]
    async fn recorder_resume_appends_from_next_seq() {
        let temp = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set(home.path());
        let path = temp.path().join("resume.jsonl");
        let created_at = Utc.with_ymd_and_hms(2026, 7, 2, 13, 14, 55).unwrap();
        let first = SessionRecorderHandle::open(
            RecorderOpenMode::Create {
                session_id: "record-resume".into(),
                cwd: PathBuf::from("/repo"),
                created_at,
                metadata: SessionMetaRecord {
                    created_at,
                    cwd: "/repo".into(),
                    workspace_key: None,
                    workspace_root: None,
                    workspace_name: None,
                    model: None,
                    config_summary: None,
                    parent_session_id: None,
                    branch_from_seq: None,
                    migrated_from: None,
                },
            },
            RecordReplayConfig::default(),
        )
        .await
        .unwrap();
        let rollout_path = first.rollout_path().clone();
        first.shutdown().await.unwrap();
        std::fs::copy(&rollout_path, &path).unwrap();

        let resumed = SessionRecorderHandle::open(
            RecorderOpenMode::Resume {
                session_id: "record-resume".into(),
                rollout_path: path.clone(),
                next_seq: 1,
            },
            RecordReplayConfig::default(),
        )
        .await
        .unwrap();
        resumed
            .add(vec![RecordItem::TurnStarted(TurnStartedRecord::default())])
            .await
            .unwrap();
        resumed.shutdown().await.unwrap();

        let result = read_rollout_file(&path).unwrap();
        assert_eq!(result.lines.len(), 2);
        assert_eq!(result.lines[1].seq, 1);
    }
}
