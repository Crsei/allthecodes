//! Conversation forking — transcript-level branch.
//!
//! A conversation "fork" (issue #36) produces a copy of the current
//! conversation as a new session, preserving the original message UUIDs and
//! content while rewriting the envelope `session_id` so the fork is
//! self-consistent. The new transcript begins with a `session_header` record
//! carrying the fork provenance:
//!
//! ```json
//! { "msg_type": "session_header",
//!   "session_id": "<new>",
//!   "forked_from": "<old>",
//!   "forked_at_uuid": "<uuid-of-last-copied-message>",
//!   "title": "..." }
//! ```
//!
//! The fork lives alongside the parent — we do not modify the parent's
//! transcript, session file, or title. The caller is expected to display a
//! resume hint (`/resume <short_id>`); runtime "attach to new session"
//! behavior requires engine-state surgery that happens outside this crate.

use anyhow::{Context, Result};
use chrono::Utc;
use tracing::{debug, info};

use crate::storage;
use crate::transcript::{self, SessionHeader};
use allthecodes_types::message::Message;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Outcome of a successful transcript fork.
#[derive(Debug, Clone)]
pub struct ForkOutcome {
    /// The freshly allocated session ID for the new fork.
    pub new_session_id: String,
    /// The source session that was forked from.
    pub parent_session_id: String,
    /// UUID of the last message copied into the fork (the fork point), if
    /// one could be determined. `None` for a fork of an empty conversation.
    pub forked_at_uuid: Option<String>,
    /// Number of message entries copied from the parent transcript into the
    /// fork's transcript.
    pub copied_entry_count: usize,
    /// Title assigned to the fork.
    pub title: String,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Fork the parent session into a freshly allocated session ID.
///
/// The parent session is untouched. On success:
/// - A new transcript file is written with a `session_header` carrying the
///   fork provenance, followed by rewritten copies of the parent's messages
///   (through `cursor_uuid`, inclusive) with the `session_id` envelope
///   rewritten to the new ID and original message UUIDs preserved.
/// - A session file is persisted for the fork with the same `cwd` as the
///   parent and a recognizable custom title.
///
/// ## Parameters
/// - `parent_session_id`: the source session to copy from
/// - `new_session_id`: the freshly minted session ID for the fork
/// - `messages`: the current in-memory message list (used to persist a
///   session file alongside the transcript, and to derive a fork point when
///   `cursor_uuid` is `None`)
/// - `cwd`: working directory for the fork's session file
/// - `cursor_uuid`: copy messages up to and including this UUID. When `None`,
///   defaults to the last message's UUID in `messages` (or copies everything
///   from the parent transcript if `messages` is empty).
///
/// Returns a [`ForkOutcome`] with details that the caller can surface to the
/// user (IDs, title, resume hint).
pub fn fork_session(
    parent_session_id: &str,
    new_session_id: &str,
    messages: &[Message],
    cwd: &str,
    cursor_uuid: Option<&str>,
) -> Result<ForkOutcome> {
    if parent_session_id == new_session_id {
        anyhow::bail!("fork_session: parent and new session IDs must differ");
    }

    let parent_session_path = storage::get_session_file(parent_session_id);
    let new_session_path = storage::get_session_file(new_session_id);
    let parent_transcript_path = transcript::get_transcript_file(parent_session_id);
    let new_transcript_path = transcript::get_transcript_file(new_session_id);

    // Derive the fork point. If the caller passes an explicit UUID we honor
    // it; otherwise fall back to the last in-memory message, then to "copy
    // everything" if the buffer is empty.
    let derived_cursor = cursor_uuid
        .map(|s| s.to_string())
        .or_else(|| messages.last().map(|m| m.uuid().to_string()));

    // Build a recognizable title. Prefer the parent's current title
    // (custom or auto-derived); fall back to the short new ID when neither
    // is available.
    let parent_title = storage::load_session_info_from_file(&parent_session_path)
        .map(|info| info.title)
        .unwrap_or_default();
    let title = derive_fork_title(&parent_title, new_session_id);

    // Write the session_header line first so the transcript file exists
    // before we try to copy entries into it.
    let now_ms = Utc::now().timestamp_millis();
    let header = SessionHeader {
        timestamp: now_ms,
        session_id: new_session_id.to_string(),
        msg_type: "session_header".to_string(),
        forked_from: Some(parent_session_id.to_string()),
        forked_at_uuid: derived_cursor.clone(),
        title: Some(title.clone()),
    };
    transcript::write_session_header_to_file(&header, &new_transcript_path)
        .context("Failed to write session_header for fork")?;

    // Copy the parent transcript up through the fork point.
    let copied = transcript::copy_transcript_entries_between_files(
        &parent_transcript_path,
        &new_transcript_path,
        new_session_id,
        derived_cursor.as_deref(),
    )
    .context("Failed to copy parent transcript entries into fork")?;

    // Persist a session file so /resume and /session list can find the
    // fork. We truncate messages to the cursor (inclusive) so the saved
    // buffer matches the transcript's copied range.
    let saved_messages: Vec<Message> = match derived_cursor.as_deref() {
        Some(cursor) => messages_up_to_cursor(messages, cursor),
        None => Vec::new(),
    };

    storage::save_session_to_file(new_session_id, &saved_messages, cwd, &new_session_path)
        .context("Failed to save forked session file")?;

    // Pin the fork's title so it's distinguishable in /session list.
    storage::set_session_title_in_file(new_session_id, Some(&title), &new_session_path)
        .context("Failed to set forked session title")?;

    // Make durable before returning so a crash right after the fork
    // doesn't leave a half-written transcript header.
    let _ = transcript::flush_transcript_file(&new_transcript_path);

    info!(
        parent = parent_session_id,
        child = new_session_id,
        copied_entries = copied,
        "session forked"
    );
    debug!(forked_at_uuid = ?derived_cursor, "fork cursor recorded");

    Ok(ForkOutcome {
        new_session_id: new_session_id.to_string(),
        parent_session_id: parent_session_id.to_string(),
        forked_at_uuid: derived_cursor,
        copied_entry_count: copied,
        title,
    })
}

/// Build a recognizable fork title. Uses the parent title and the first 8
/// characters of the new session ID as a short fingerprint. Falls back to the
/// short ID alone when no parent title is available.
fn derive_fork_title(parent_title: &str, new_session_id: &str) -> String {
    let short: String = new_session_id.chars().take(8).collect();
    let parent = parent_title.trim();
    if parent.is_empty() {
        format!("Forked session ({})", short)
    } else {
        format!("{} (fork @ {})", parent, short)
    }
}

/// Copy messages from `messages` up to and including the first entry whose
/// UUID equals `cursor`. If no such entry is found, copies everything (this
/// mirrors the transcript-copy behavior, which also falls through to "copy
/// all" when the cursor can't be located).
fn messages_up_to_cursor(messages: &[Message], cursor: &str) -> Vec<Message> {
    let mut out = Vec::with_capacity(messages.len());
    for m in messages {
        out.push(m.clone());
        if m.uuid().to_string() == cursor {
            return out;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_types::message::{MessageContent, UserMessage};
    use std::path::Path;
    use tempfile::tempdir;
    use uuid::Uuid;

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

    fn user(text: &str, uuid: Uuid) -> Message {
        Message::User(UserMessage {
            uuid,
            timestamp: 0,
            role: "user".into(),
            content: MessageContent::Text(text.into()),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        })
    }

    fn seed_parent_transcript_and_session(
        session_id: &str,
        uuids: &[Uuid],
        cwd: &str,
    ) -> Vec<Message> {
        // Write matching transcript entries on disk so the copy has something
        // to read from. The transcript code expects NDJSON envelopes with
        // `session_id` + `msg_type` + `uuid` + `payload`.
        let dir = transcript::get_transcript_dir();
        std::fs::create_dir_all(&dir).unwrap();
        let path = transcript::get_transcript_file(session_id);
        let mut buf = String::new();
        for (i, uuid) in uuids.iter().enumerate() {
            let entry = serde_json::json!({
                "timestamp": 1_700_000_000_000_i64 + i as i64,
                "session_id": session_id,
                "msg_type": if i % 2 == 0 { "user" } else { "assistant" },
                "uuid": uuid.to_string(),
                "payload": { "text": format!("msg {}", i) }
            });
            buf.push_str(&serde_json::to_string(&entry).unwrap());
            buf.push('\n');
        }
        std::fs::write(&path, buf).unwrap();

        // Also persist a session file so /session list can find the parent.
        let messages: Vec<Message> = uuids
            .iter()
            .enumerate()
            .map(|(i, u)| user(&format!("msg {}", i), *u))
            .collect();
        storage::save_session(session_id, &messages, cwd).unwrap();
        messages
    }

    #[test]
    #[serial_test::serial]
    fn test_fork_session_copies_entries_and_writes_header() {
        let temp = tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        let uuids: Vec<Uuid> = (0..4).map(|_| Uuid::new_v4()).collect();
        let messages = seed_parent_transcript_and_session("parent-abc", &uuids, "/proj");

        let new_id = "child-xyz-12345678";
        let outcome = fork_session(
            "parent-abc",
            new_id,
            &messages,
            "/proj",
            Some(&uuids[2].to_string()),
        )
        .unwrap();

        assert_eq!(outcome.new_session_id, new_id);
        assert_eq!(outcome.parent_session_id, "parent-abc");
        assert_eq!(
            outcome.forked_at_uuid.as_deref(),
            Some(uuids[2].to_string().as_str())
        );
        assert_eq!(outcome.copied_entry_count, 3);

        // Verify transcript layout: header + 3 copied entries.
        let content = std::fs::read_to_string(transcript::get_transcript_file(new_id)).unwrap();
        let lines: Vec<&str> = content.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(lines.len(), 4);

        // First line is the header with correct forked_from / forked_at_uuid.
        let header: SessionHeader = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(header.msg_type, "session_header");
        assert_eq!(header.session_id, new_id);
        assert_eq!(header.forked_from.as_deref(), Some("parent-abc"));
        assert_eq!(header.forked_at_uuid, Some(uuids[2].to_string()));
        assert!(header.title.as_deref().unwrap_or("").contains("fork @"));

        // All copied entries must bear the new session_id.
        for line in &lines[1..] {
            let v: serde_json::Value = serde_json::from_str(line).unwrap();
            assert_eq!(v.get("session_id").and_then(|v| v.as_str()), Some(new_id));
        }

        // A session file was persisted for the fork so /resume can find it.
        let info = storage::load_session_info(new_id).unwrap();
        assert!(info.custom_title.is_some());
        assert_eq!(info.message_count, 3);
    }

    #[test]
    #[serial_test::serial]
    fn test_fork_session_defaults_cursor_to_last_message() {
        let temp = tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        let uuids: Vec<Uuid> = (0..2).map(|_| Uuid::new_v4()).collect();
        let messages = seed_parent_transcript_and_session("parent-def", &uuids, "/proj");

        let outcome = fork_session("parent-def", "child-def", &messages, "/proj", None).unwrap();
        assert_eq!(outcome.forked_at_uuid, Some(uuids[1].to_string()));
        assert_eq!(outcome.copied_entry_count, 2);
    }

    #[test]
    #[serial_test::serial]
    fn test_fork_session_rejects_self_fork() {
        let temp = tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        let err = fork_session("same", "same", &[], "/proj", None).unwrap_err();
        assert!(err.to_string().contains("must differ"));
    }

    #[test]
    fn test_derive_fork_title_with_parent() {
        let title = derive_fork_title(" Debug auth flow ", "abcd1234-etc");
        assert_eq!(title, "Debug auth flow (fork @ abcd1234)");
    }

    #[test]
    fn test_derive_fork_title_without_parent() {
        let title = derive_fork_title("", "abcd1234-etc");
        assert_eq!(title, "Forked session (abcd1234)");
    }

    #[test]
    fn test_messages_up_to_cursor_inclusive() {
        let u0 = Uuid::new_v4();
        let u1 = Uuid::new_v4();
        let u2 = Uuid::new_v4();
        let msgs = vec![user("a", u0), user("b", u1), user("c", u2)];
        let kept = messages_up_to_cursor(&msgs, &u1.to_string());
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[1].uuid().to_string(), u1.to_string());
    }

    #[test]
    fn test_messages_up_to_cursor_missing_keeps_all() {
        let u0 = Uuid::new_v4();
        let msgs = vec![user("a", u0)];
        let kept = messages_up_to_cursor(&msgs, "no-such-uuid");
        assert_eq!(kept.len(), 1);
    }
}
