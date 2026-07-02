//! Transcript recording.
//!
//! Provides an append-friendly write mechanism for recording the full
//! conversation transcript. Unlike session storage (which overwrites the
//! whole file), the transcript log appends newline-delimited JSON entries
//! so that partial conversations are preserved even on crash.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use allthecodes_types::message::Message;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Session-level metadata written as the first line of a transcript file.
///
/// Unlike regular message entries, `session_header` entries carry metadata
/// about the session as a whole — e.g. fork provenance. A transcript may have
/// at most one `session_header` entry, written at creation time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionHeader {
    /// Unix timestamp (milliseconds) when the header was written.
    pub timestamp: i64,
    /// Session this header describes.
    pub session_id: String,
    /// Always `"session_header"` — mirrors the `msg_type` tag on regular
    /// entries so readers can dispatch on a single field.
    pub msg_type: String,
    /// Parent session this one was forked from, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forked_from: Option<String>,
    /// UUID of the last message copied from the parent (the fork point).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forked_at_uuid: Option<String>,
    /// Optional display title captured at fork time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// A single transcript entry written as one line of NDJSON.
#[derive(Debug, Serialize)]
struct TranscriptEntry {
    /// Unix timestamp (milliseconds).
    timestamp: i64,
    /// Session this entry belongs to.
    session_id: String,
    /// Message type tag (user, assistant, system, ...).
    msg_type: String,
    /// Message UUID.
    uuid: String,
    /// Condensed payload (text or tool use summary).
    payload: serde_json::Value,
}

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

/// Return the directory for transcript files. Resolves through
/// [`allthecodes_config::paths::transcripts_dir`].
pub fn get_transcript_dir() -> PathBuf {
    allthecodes_config::paths::transcripts_dir()
}

/// Return the transcript file path for a specific session.
pub fn get_transcript_file(session_id: &str) -> PathBuf {
    get_transcript_dir().join(format!("{}.ndjson", session_id))
}

// ---------------------------------------------------------------------------
// Fork helpers
// ---------------------------------------------------------------------------

/// Write the `session_header` record as the first (or only) line of a new
/// transcript file for `session_id`.
pub fn write_session_header(header: &SessionHeader) -> Result<()> {
    let path = get_transcript_file(&header.session_id);
    write_session_header_to_file(header, &path)
}

pub(crate) fn write_session_header_to_file(header: &SessionHeader, path: &Path) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("Failed to create transcript directory {}", dir.display()))?;
    }

    if path.exists() {
        anyhow::bail!(
            "Transcript for session {} already exists; refusing to overwrite header",
            header.session_id
        );
    }

    let line = serde_json::to_string(header).context("Failed to serialize session header")?;
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .with_context(|| format!("Failed to create transcript {}", path.display()))?;
    writeln!(file, "{}", line)
        .with_context(|| format!("Failed to write header to transcript {}", path.display()))?;
    Ok(())
}

/// Load the session_header (first line) from a transcript file if present.
pub fn read_session_header(session_id: &str) -> Result<Option<SessionHeader>> {
    let path = get_transcript_file(session_id);
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read transcript {}", path.display()))?;
    let Some(first_line) = content.lines().next() else {
        return Ok(None);
    };
    let value: serde_json::Value = match serde_json::from_str(first_line) {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };
    if value.get("msg_type").and_then(|v| v.as_str()) != Some("session_header") {
        return Ok(None);
    }
    let header: SessionHeader = serde_json::from_value(value)
        .with_context(|| format!("Failed to parse session header in {}", path.display()))?;
    Ok(Some(header))
}

/// Copy transcript entries from `source_session_id` into the transcript file
/// for `target_session_id`, rewriting the `session_id` field on each envelope
/// to point at the target. Stops after writing the entry matching
/// `stop_at_uuid` (inclusive) if provided. Source session_header entries are
/// skipped.
pub fn copy_transcript_entries(
    source_session_id: &str,
    target_session_id: &str,
    stop_at_uuid: Option<&str>,
) -> Result<usize> {
    let source_path = get_transcript_file(source_session_id);
    let target_path = get_transcript_file(target_session_id);
    copy_transcript_entries_between_files(
        &source_path,
        &target_path,
        target_session_id,
        stop_at_uuid,
    )
}

pub(crate) fn copy_transcript_entries_between_files(
    source_path: &Path,
    target_path: &Path,
    target_session_id: &str,
    stop_at_uuid: Option<&str>,
) -> Result<usize> {
    if !source_path.exists() {
        return Ok(0);
    }

    let content = std::fs::read_to_string(source_path)
        .with_context(|| format!("Failed to read source transcript {}", source_path.display()))?;

    let mut target_file = std::fs::OpenOptions::new()
        .append(true)
        .open(target_path)
        .with_context(|| {
            format!(
                "Target transcript {} does not exist — call write_session_header first",
                target_path.display()
            )
        })?;

    let mut copied = 0usize;
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let mut value: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        if value.get("msg_type").and_then(|v| v.as_str()) == Some("session_header") {
            continue;
        }

        if let Some(obj) = value.as_object_mut() {
            obj.insert(
                "session_id".to_string(),
                serde_json::Value::String(target_session_id.to_string()),
            );
        }

        let rewritten =
            serde_json::to_string(&value).context("Failed to re-serialize transcript entry")?;
        writeln!(target_file, "{}", rewritten).with_context(|| {
            format!(
                "Failed to append to target transcript {}",
                target_path.display()
            )
        })?;
        copied += 1;

        if let Some(stop) = stop_at_uuid {
            if value.get("uuid").and_then(|v| v.as_str()) == Some(stop) {
                break;
            }
        }
    }

    Ok(copied)
}

// ---------------------------------------------------------------------------
// Recording
// ---------------------------------------------------------------------------

/// Append the given messages to the transcript log for `session_id`.
///
/// Each message is written as a single line of JSON (NDJSON format) so that
/// the file can be tailed in real time and is resilient to partial writes.
pub fn record_transcript(session_id: &str, messages: &[Message]) -> Result<()> {
    let dir = get_transcript_dir();
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create transcript directory {}", dir.display()))?;

    let path = get_transcript_file(session_id);

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("Failed to open transcript file {}", path.display()))?;

    let now = Utc::now().timestamp_millis();

    for msg in messages {
        let (msg_type, payload) = message_to_payload(msg);
        let entry = TranscriptEntry {
            timestamp: now,
            session_id: session_id.to_string(),
            msg_type,
            uuid: msg.uuid().to_string(),
            payload,
        };

        let line = serde_json::to_string(&entry).context("Failed to serialize transcript entry")?;

        writeln!(file, "{}", line)
            .with_context(|| format!("Failed to write to transcript {}", path.display()))?;
    }

    Ok(())
}

/// Rebuild the transcript file from a visible message list.
///
/// This treats the transcript as a derived view. The canonical record log is
/// not modified; callers should only use this after replay has produced the
/// visible history they want mirrored for legacy transcript consumers.
pub fn rebuild_transcript_from_messages(session_id: &str, messages: &[Message]) -> Result<PathBuf> {
    let dir = get_transcript_dir();
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create transcript directory {}", dir.display()))?;

    let path = get_transcript_file(session_id);
    let tmp_path = path.with_extension("ndjson.tmp");
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&tmp_path)
        .with_context(|| format!("Failed to open transcript temp file {}", tmp_path.display()))?;

    for msg in messages {
        let (msg_type, payload) = message_to_payload(msg);
        let entry = TranscriptEntry {
            timestamp: message_timestamp(msg),
            session_id: session_id.to_string(),
            msg_type,
            uuid: msg.uuid().to_string(),
            payload,
        };
        let line = serde_json::to_string(&entry).context("Failed to serialize transcript entry")?;
        writeln!(file, "{}", line).with_context(|| {
            format!(
                "Failed to write transcript temp file {}",
                tmp_path.display()
            )
        })?;
    }
    file.sync_all()
        .with_context(|| format!("Failed to sync transcript temp file {}", tmp_path.display()))?;
    drop(file);

    std::fs::rename(&tmp_path, &path).with_context(|| {
        format!(
            "Failed to replace transcript {} with {}",
            path.display(),
            tmp_path.display()
        )
    })?;
    Ok(path)
}

/// Rebuild the transcript from the session's replay log.
pub fn rebuild_transcript_from_replay(session_id: &str) -> Result<PathBuf> {
    let rollout_path = crate::record_replay::index::ensure_rollout_for_session(session_id)?;
    let read = crate::record_replay::reader::read_rollout_file(&rollout_path)
        .with_context(|| format!("Failed to read rollout {}", rollout_path.display()))?;
    let messages = crate::record_replay::reconstruct::reconstruct_messages(&read.lines);
    rebuild_transcript_from_messages(session_id, &messages)
}

/// Flush the transcript file for `session_id` by syncing to disk.
///
/// This is a no-op on most systems (the OS flushes on close), but provides
/// an explicit sync point for durability guarantees.
pub fn flush_transcript(session_id: &str) -> Result<()> {
    let path = get_transcript_file(session_id);
    flush_transcript_file(&path)
}

pub(crate) fn flush_transcript_file(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }

    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .with_context(|| format!("Failed to open transcript for flush {}", path.display()))?;

    file.sync_all()
        .with_context(|| format!("Failed to sync transcript {}", path.display()))?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract a compact payload from a `Message` for transcript storage.
fn message_to_payload(msg: &Message) -> (String, serde_json::Value) {
    match msg {
        Message::User(u) => {
            let text = match &u.content {
                allthecodes_types::message::MessageContent::Text(t) => t.clone(),
                allthecodes_types::message::MessageContent::Blocks(blocks) => {
                    format!("[{} content blocks]", blocks.len())
                }
            };
            ("user".into(), serde_json::json!({ "text": text }))
        }
        Message::Assistant(a) => {
            let text_blocks: Vec<String> = a
                .content
                .iter()
                .filter_map(|block| match block {
                    allthecodes_types::message::ContentBlock::Text { text } => Some(text.clone()),
                    allthecodes_types::message::ContentBlock::ToolUse { name, .. } => {
                        Some(format!("[tool_use: {}]", name))
                    }
                    _ => None,
                })
                .collect();
            (
                "assistant".into(),
                serde_json::json!({
                    "content_summary": text_blocks,
                    "stop_reason": a.stop_reason,
                }),
            )
        }
        Message::System(s) => ("system".into(), serde_json::json!({ "content": s.content })),
        Message::Progress(p) => (
            "progress".into(),
            serde_json::json!({ "tool_use_id": p.tool_use_id }),
        ),
        Message::Attachment(a) => (
            "attachment".into(),
            serde_json::json!({ "attachment": a.attachment }),
        ),
    }
}

fn message_timestamp(msg: &Message) -> i64 {
    match msg {
        Message::User(m) => m.timestamp,
        Message::Assistant(m) => m.timestamp,
        Message::System(m) => m.timestamp,
        Message::Progress(m) => m.timestamp,
        Message::Attachment(m) => m.timestamp,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transcript_dir() {
        let dir = get_transcript_dir();
        assert!(dir.to_string_lossy().contains("transcripts"));
    }

    #[test]
    fn test_transcript_file_extension() {
        let path = get_transcript_file("test-session");
        assert!(path.to_string_lossy().ends_with(".ndjson"));
    }

    use std::path::Path;
    use tempfile::tempdir;

    struct HomeGuard {
        previous: Option<String>,
    }

    impl HomeGuard {
        fn set(path: &Path) -> Self {
            let previous = std::env::var("ALLTHECODES_HOME").ok();
            std::env::set_var("ALLTHECODES_HOME", path);
            Self { previous }
        }
    }

    impl Drop for HomeGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(v) => std::env::set_var("ALLTHECODES_HOME", v),
                None => std::env::remove_var("ALLTHECODES_HOME"),
            }
        }
    }

    fn seed_parent_transcript(session_id: &str, uuids: &[&str]) {
        let dir = get_transcript_dir();
        std::fs::create_dir_all(&dir).unwrap();
        let path = get_transcript_file(session_id);
        let mut buf = String::new();
        for (i, uuid) in uuids.iter().enumerate() {
            let msg_type = if i % 2 == 0 { "user" } else { "assistant" };
            let entry = serde_json::json!({
                "timestamp": 1_700_000_000_000_i64 + i as i64,
                "session_id": session_id,
                "msg_type": msg_type,
                "uuid": uuid,
                "payload": { "text": format!("msg {}", i) }
            });
            buf.push_str(&serde_json::to_string(&entry).unwrap());
            buf.push('\n');
        }
        std::fs::write(&path, buf).unwrap();
    }

    #[test]
    #[serial_test::serial]
    fn test_write_and_read_session_header() {
        let temp = tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        let header = SessionHeader {
            timestamp: 42,
            session_id: "child".into(),
            msg_type: "session_header".into(),
            forked_from: Some("parent".into()),
            forked_at_uuid: Some("00000000-0000-0000-0000-000000000003".into()),
            title: Some("Debug auth (fork @ 00000000)".into()),
        };

        write_session_header(&header).unwrap();

        let loaded = read_session_header("child").unwrap().unwrap();
        assert_eq!(loaded.session_id, "child");
        assert_eq!(loaded.forked_from.as_deref(), Some("parent"));
        assert_eq!(
            loaded.forked_at_uuid.as_deref(),
            Some("00000000-0000-0000-0000-000000000003")
        );
        assert!(write_session_header(&header).is_err());
    }

    #[test]
    #[serial_test::serial]
    fn test_copy_transcript_entries_stops_at_uuid_and_rewrites_session_id() {
        let temp = tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        let uuids = [
            "11111111-1111-1111-1111-111111111111",
            "22222222-2222-2222-2222-222222222222",
            "33333333-3333-3333-3333-333333333333",
            "44444444-4444-4444-4444-444444444444",
        ];
        seed_parent_transcript("parent", &uuids);

        write_session_header(&SessionHeader {
            timestamp: 1,
            session_id: "child".into(),
            msg_type: "session_header".into(),
            forked_from: Some("parent".into()),
            forked_at_uuid: Some(uuids[2].into()),
            title: None,
        })
        .unwrap();

        let copied = copy_transcript_entries("parent", "child", Some(uuids[2])).unwrap();
        assert_eq!(copied, 3);

        let child_content = std::fs::read_to_string(get_transcript_file("child")).unwrap();
        let lines: Vec<&str> = child_content.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(lines.len(), 4);

        for line in &lines[1..] {
            let v: serde_json::Value = serde_json::from_str(line).unwrap();
            assert_eq!(v.get("session_id").and_then(|v| v.as_str()), Some("child"));
        }
    }

    #[test]
    #[serial_test::serial]
    fn test_copy_transcript_entries_skips_source_header() {
        let temp = tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        let parent_path = get_transcript_file("parent2");
        std::fs::create_dir_all(get_transcript_dir()).unwrap();
        let header = serde_json::json!({
            "timestamp": 0, "session_id": "parent2", "msg_type": "session_header",
            "forked_from": null, "forked_at_uuid": null,
        });
        let msg = serde_json::json!({
            "timestamp": 1, "session_id": "parent2", "msg_type": "user",
            "uuid": "55555555-5555-5555-5555-555555555555",
            "payload": { "text": "hello" }
        });
        std::fs::write(&parent_path, format!("{}\n{}\n", header, msg)).unwrap();

        write_session_header(&SessionHeader {
            timestamp: 1,
            session_id: "child2".into(),
            msg_type: "session_header".into(),
            forked_from: Some("parent2".into()),
            forked_at_uuid: None,
            title: None,
        })
        .unwrap();

        let copied = copy_transcript_entries("parent2", "child2", None).unwrap();
        assert_eq!(copied, 1);
    }

    #[test]
    #[serial_test::serial]
    fn test_rebuild_transcript_from_messages_replaces_visible_view() {
        let temp = tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());
        let first = Message::User(allthecodes_types::message::UserMessage {
            uuid: uuid::Uuid::parse_str("10000000-0000-0000-0000-000000000031").unwrap(),
            timestamp: 10,
            role: "user".into(),
            content: allthecodes_types::message::MessageContent::Text("first".into()),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        });
        let second = Message::User(allthecodes_types::message::UserMessage {
            uuid: uuid::Uuid::parse_str("10000000-0000-0000-0000-000000000032").unwrap(),
            timestamp: 11,
            role: "user".into(),
            content: allthecodes_types::message::MessageContent::Text("second".into()),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        });

        record_transcript("rebuilt", &[first.clone(), second]).unwrap();
        rebuild_transcript_from_messages("rebuilt", &[first]).unwrap();

        let content = std::fs::read_to_string(get_transcript_file("rebuilt")).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 1);
        let value: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(value["payload"]["text"], "first");
    }
}
