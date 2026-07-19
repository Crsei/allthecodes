//! Session resume — finding and restoring the most recent session.
//!
//! Provides helpers to locate the last session for a given working directory
//! and to reload its message history so the conversation can continue.
//!
//! Replay-first: when a record log (rollout JSONL) exists for the session,
//! the message history is reconstructed from the log instead of from the
//! legacy JSON/SQLite projection. Falls back to legacy storage when no
//! rollout is found.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use tracing::{debug, info, warn};

use super::record_replay;
use super::record_replay::PendingInteraction;
use super::storage::{self, SessionInfo};
use allthecodes_types::message::{
    ContentBlock, Message, MessageContent, ToolResultContent, UserMessage,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeSource {
    Replay,
    Legacy,
}

#[derive(Debug, Clone)]
pub struct ResumedSession {
    pub session_id: String,
    pub messages: Vec<Message>,
    pub source: ResumeSource,
    pub rollout_path: Option<std::path::PathBuf>,
    pub record_schema_version: Option<u32>,
    pub last_seq: Option<u64>,
    pub replay_warnings: Vec<record_replay::ReplayReadWarning>,
    pub pending_interactions: Vec<PendingInteraction>,
}

#[derive(Debug, Clone)]
struct MissingToolResult {
    assistant_index: usize,
    assistant_uuid: uuid::Uuid,
    assistant_timestamp: i64,
    tool_use_id: String,
}

#[derive(Debug, Clone, Default)]
struct ToolHistoryAnalysis {
    missing: Vec<MissingToolResult>,
    errors: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct ToolHistoryRepair {
    messages: Vec<Message>,
    repaired: bool,
    recovered_from_projection: usize,
    synthetic_results: usize,
}

#[derive(Debug, Clone)]
struct LegacyToolResultCandidate {
    user: UserMessage,
    block: ContentBlock,
    tool_result_blocks_in_message: usize,
}

/// Find the most recently modified session in the same workspace/repository as `cwd`.
///
/// Returns `None` if no matching session exists.
pub fn get_last_session(cwd: &Path) -> Result<Option<SessionInfo>> {
    let session = storage::list_workspace_sessions(cwd)?.into_iter().next();
    if let Some(ref s) = session {
        debug!(session_id = %s.session_id, messages = s.message_count, "found last session");
    }
    Ok(session)
}

/// Resume a session by loading its messages from disk.
///
/// Replay-first: tries to reconstruct from the rollout JSONL log first.
/// Falls back to `storage::load_session` when no rollout is found.
/// If the fallback succeeds and a rollout does not exist, a lazy migration
/// is triggered to generate a synthetic rollout.
pub fn resume_session(session_id: &str) -> Result<Vec<Message>> {
    Ok(resume_session_detail(session_id)?.messages)
}

/// Resume a session and include replay metadata when a rollout log is used.
pub fn resume_session_detail(session_id: &str) -> Result<ResumedSession> {
    info!(session_id = session_id, "resuming session");
    let record_config = record_replay::RecordReplayConfig::from_env();

    // Try replay-first: reconstruct from rollout JSONL
    if record_config.enabled && record_config.read_prefer_replay {
        match record_replay::index::lookup_rollout(session_id) {
            Ok(Some(rollout_path)) => {
                debug!(
                    session_id,
                    path = %rollout_path.display(),
                    "replaying session from rollout log"
                );
                match replay_session_from_path(session_id, rollout_path.clone()) {
                    Ok(resumed) => {
                        info!(
                            session_id,
                            count = resumed.messages.len(),
                            seq = resumed.last_seq.unwrap_or(0),
                            "session reconstructed from rollout log"
                        );
                        return Ok(resumed);
                    }
                    Err(e) => {
                        warn!(
                            session_id,
                            path = %rollout_path.display(),
                            error = %e,
                            "failed to read rollout log, falling back to legacy session load"
                        );
                    }
                }
            }
            Ok(None) => {
                debug!(
                    session_id,
                    "no rollout log found, falling back to legacy session load"
                );
            }
            Err(e) => {
                warn!(
                    session_id,
                    error = %e,
                    "rollout lookup failed, falling back to legacy session load"
                );
            }
        }
    } else {
        debug!(
            session_id,
            "record replay read disabled; using legacy session load"
        );
    }

    // Fallback: load from legacy JSON/SQLite and repair protocol gaps before
    // any caller can send the history back to a provider.
    let messages = storage::load_session(session_id)?;
    let repair = repair_tool_history(session_id, &messages, &messages)?;
    let messages = repair.messages;
    if repair.repaired {
        let session_info = storage::load_session_info(session_id).with_context(|| {
            format!("failed to load metadata for repaired session {session_id}")
        })?;
        storage::save_session(session_id, &messages, &session_info.cwd).with_context(|| {
            format!("failed to save repaired legacy session projection {session_id}")
        })?;
        info!(
            session_id,
            recovered = repair.recovered_from_projection,
            synthetic = repair.synthetic_results,
            "repaired resumable tool history from legacy session"
        );
    }

    // Trigger lazy migration if the session was successfully loaded
    // (best effort — migration failure should not block resume)
    if record_config.enabled && !messages.is_empty() {
        match record_replay::migration::find_rollout_for_session(session_id) {
            Ok(Some(rollout_path)) => {
                if repair.repaired {
                    append_repair_snapshot(session_id, &rollout_path, &messages)?;
                }
            }
            Ok(None) => {
                debug!(session_id, "triggering lazy migration to rollout log");
                if let Err(e) = record_replay::migration::migrate_legacy_session(session_id) {
                    warn!(
                        session_id,
                        error = %e,
                        "lazy migration failed; session will remain legacy-only"
                    );
                }
            }
            Err(e) => {
                warn!(
                    session_id,
                    error = %e,
                    "rollout search failed; skipping lazy migration"
                );
            }
        }
    }

    info!(
        session_id,
        count = messages.len(),
        "session loaded from legacy storage"
    );
    Ok(ResumedSession {
        session_id: session_id.to_string(),
        messages,
        source: ResumeSource::Legacy,
        rollout_path: None,
        record_schema_version: None,
        last_seq: None,
        replay_warnings: Vec::new(),
        pending_interactions: Vec::new(),
    })
}

fn replay_session_from_path(
    session_id: &str,
    rollout_path: std::path::PathBuf,
) -> Result<ResumedSession> {
    let result = record_replay::reader::read_rollout_file(&rollout_path)?;
    if !result.warnings.is_empty() {
        warn!(
            session_id,
            warnings = result.warnings.len(),
            "replay read produced warnings"
        );
        for warning in &result.warnings {
            debug!(
                session_id,
                line = warning.line_number,
                message = %warning.message,
                "replay warning"
            );
        }
    }

    let reconstructed = record_replay::reconstruct::reconstruct_recorded_messages(&result.lines);
    let replay_messages =
        record_replay::reconstruct::recorded_messages_to_typed(&reconstructed.messages);
    let legacy_messages = match storage::load_session(session_id) {
        Ok(messages) => messages,
        Err(error) => {
            debug!(
                session_id,
                %error,
                "legacy projection unavailable while checking replay tool history"
            );
            Vec::new()
        }
    };
    let repair = repair_tool_history(session_id, &replay_messages, &legacy_messages)?;
    let messages = repair.messages;
    let record_schema_version = result.lines.iter().map(|line| line.schema_version).max();
    let mut last_seq = result.lines.iter().map(|line| line.seq).max();

    if repair.repaired {
        last_seq = Some(append_repair_snapshot(
            session_id,
            &rollout_path,
            &messages,
        )?);
        if let Ok(session_info) = storage::load_session_info(session_id) {
            if let Err(error) = storage::save_session(session_id, &messages, &session_info.cwd) {
                warn!(
                    session_id,
                    %error,
                    "canonical repair succeeded but legacy session projection update failed"
                );
            }
        }
        info!(
            session_id,
            recovered = repair.recovered_from_projection,
            synthetic = repair.synthetic_results,
            "repaired resumable tool history and appended canonical snapshot"
        );
    }

    Ok(ResumedSession {
        session_id: session_id.to_string(),
        messages,
        source: ResumeSource::Replay,
        rollout_path: Some(rollout_path),
        record_schema_version,
        last_seq,
        replay_warnings: result.warnings,
        pending_interactions: reconstructed.pending_interactions,
    })
}

fn repair_tool_history(
    session_id: &str,
    replay_messages: &[Message],
    legacy_messages: &[Message],
) -> Result<ToolHistoryRepair> {
    let analysis = analyze_tool_history(replay_messages);
    if !analysis.errors.is_empty() {
        bail!(
            "resumable tool history is ambiguous: {}",
            analysis.errors.join("; ")
        );
    }
    if analysis.missing.is_empty() {
        return Ok(ToolHistoryRepair {
            messages: replay_messages.to_vec(),
            ..ToolHistoryRepair::default()
        });
    }

    let legacy_candidates = collect_legacy_tool_results(legacy_messages);
    let mut insertions: HashMap<usize, Vec<Message>> = HashMap::new();
    let mut recovered_from_projection = 0usize;
    let mut synthetic_results = 0usize;

    for (position, missing) in analysis.missing.iter().enumerate() {
        let repaired_message = match legacy_candidates.get(&missing.tool_use_id) {
            Some(candidates) if candidates.len() == 1 => {
                recovered_from_projection = recovered_from_projection.saturating_add(1);
                Message::User(project_legacy_tool_result(
                    session_id,
                    missing,
                    &candidates[0],
                ))
            }
            _ => {
                synthetic_results = synthetic_results.saturating_add(1);
                Message::User(synthetic_interrupted_tool_result(
                    session_id, missing, position,
                ))
            }
        };
        insertions
            .entry(missing.assistant_index)
            .or_default()
            .push(repaired_message);
    }

    let mut repaired_messages =
        Vec::with_capacity(replay_messages.len().saturating_add(analysis.missing.len()));
    for (index, message) in replay_messages.iter().enumerate() {
        repaired_messages.push(message.clone());
        if let Some(messages) = insertions.remove(&index) {
            repaired_messages.extend(messages);
        }
    }

    let repaired_analysis = analyze_tool_history(&repaired_messages);
    if !repaired_analysis.errors.is_empty() || !repaired_analysis.missing.is_empty() {
        bail!("resumable tool history remained invalid after deterministic repair");
    }

    Ok(ToolHistoryRepair {
        messages: repaired_messages,
        repaired: true,
        recovered_from_projection,
        synthetic_results,
    })
}

fn analyze_tool_history(messages: &[Message]) -> ToolHistoryAnalysis {
    #[derive(Debug)]
    struct PendingToolCalls {
        assistant_index: usize,
        assistant_uuid: uuid::Uuid,
        assistant_timestamp: i64,
        ordered_ids: Vec<String>,
        result_counts: HashMap<String, usize>,
    }

    fn finish_pending(pending: Option<PendingToolCalls>, analysis: &mut ToolHistoryAnalysis) {
        let Some(pending) = pending else {
            return;
        };
        for tool_use_id in pending.ordered_ids {
            match pending
                .result_counts
                .get(&tool_use_id)
                .copied()
                .unwrap_or(0)
            {
                0 => analysis.missing.push(MissingToolResult {
                    assistant_index: pending.assistant_index,
                    assistant_uuid: pending.assistant_uuid,
                    assistant_timestamp: pending.assistant_timestamp,
                    tool_use_id,
                }),
                1 => {}
                count => analysis
                    .errors
                    .push(format!("tool call {tool_use_id} has {count} results")),
            }
        }
    }

    let mut analysis = ToolHistoryAnalysis::default();
    let mut pending: Option<PendingToolCalls> = None;
    for (index, message) in messages.iter().enumerate() {
        match message {
            Message::Assistant(assistant) => {
                finish_pending(pending.take(), &mut analysis);
                let ordered_ids = assistant
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        ContentBlock::ToolUse { id, .. } => Some(id.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                if ordered_ids.is_empty() {
                    continue;
                }
                let mut unique_ids = HashSet::new();
                for id in &ordered_ids {
                    if !unique_ids.insert(id.clone()) {
                        analysis
                            .errors
                            .push(format!("assistant repeats tool call id {id}"));
                    }
                }
                pending = Some(PendingToolCalls {
                    assistant_index: index,
                    assistant_uuid: assistant.uuid,
                    assistant_timestamp: assistant.timestamp,
                    ordered_ids,
                    result_counts: HashMap::new(),
                });
            }
            Message::User(user) => {
                for tool_use_id in tool_result_ids(user) {
                    match pending.as_mut() {
                        Some(pending) if pending.ordered_ids.contains(&tool_use_id) => {
                            let count = pending.result_counts.entry(tool_use_id).or_default();
                            *count = count.saturating_add(1);
                        }
                        _ => analysis
                            .errors
                            .push(format!("orphan tool result {tool_use_id}")),
                    }
                }
            }
            Message::System(_) | Message::Progress(_) | Message::Attachment(_) => {}
        }
    }
    finish_pending(pending, &mut analysis);
    analysis
}

fn tool_result_ids(user: &UserMessage) -> Vec<String> {
    match &user.content {
        MessageContent::Blocks(blocks) => blocks
            .iter()
            .filter_map(|block| match block {
                ContentBlock::ToolResult { tool_use_id, .. } => Some(tool_use_id.clone()),
                _ => None,
            })
            .collect(),
        MessageContent::Text(_) => Vec::new(),
    }
}

fn collect_legacy_tool_results(
    messages: &[Message],
) -> HashMap<String, Vec<LegacyToolResultCandidate>> {
    let mut candidates: HashMap<String, Vec<LegacyToolResultCandidate>> = HashMap::new();
    for message in messages {
        let Message::User(user) = message else {
            continue;
        };
        let MessageContent::Blocks(blocks) = &user.content else {
            continue;
        };
        let tool_result_blocks_in_message = blocks
            .iter()
            .filter(|block| matches!(block, ContentBlock::ToolResult { .. }))
            .count();
        for block in blocks {
            let ContentBlock::ToolResult { tool_use_id, .. } = block else {
                continue;
            };
            candidates
                .entry(tool_use_id.clone())
                .or_default()
                .push(LegacyToolResultCandidate {
                    user: user.clone(),
                    block: block.clone(),
                    tool_result_blocks_in_message,
                });
        }
    }
    candidates
}

fn project_legacy_tool_result(
    session_id: &str,
    missing: &MissingToolResult,
    candidate: &LegacyToolResultCandidate,
) -> UserMessage {
    let mut user = candidate.user.clone();
    if candidate.tool_result_blocks_in_message != 1 {
        user.uuid = deterministic_repair_uuid(
            session_id,
            missing.assistant_uuid,
            &missing.tool_use_id,
            "projection",
        );
    }
    user.content = MessageContent::Blocks(vec![candidate.block.clone()]);
    user.is_meta = true;
    user.source_tool_assistant_uuid = Some(missing.assistant_uuid);
    user
}

fn synthetic_interrupted_tool_result(
    session_id: &str,
    missing: &MissingToolResult,
    position: usize,
) -> UserMessage {
    let text = format!(
        "The previous execution result for tool call `{}` is unavailable because the session was interrupted. The tool was not re-run.",
        missing.tool_use_id
    );
    UserMessage {
        uuid: deterministic_repair_uuid(
            session_id,
            missing.assistant_uuid,
            &missing.tool_use_id,
            "synthetic",
        ),
        timestamp: missing.assistant_timestamp.saturating_add(
            i64::try_from(position)
                .unwrap_or(i64::MAX)
                .saturating_add(1),
        ),
        role: "user".to_string(),
        content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
            tool_use_id: missing.tool_use_id.clone(),
            content: ToolResultContent::Text(text.clone()),
            is_error: true,
        }]),
        is_meta: true,
        tool_use_result: Some(text),
        source_tool_assistant_uuid: Some(missing.assistant_uuid),
    }
}

fn deterministic_repair_uuid(
    session_id: &str,
    assistant_uuid: uuid::Uuid,
    tool_use_id: &str,
    kind: &str,
) -> uuid::Uuid {
    let mut hasher = Sha256::new();
    hasher.update(b"allthecodes-resume-tool-result-v1\0");
    hasher.update(session_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(assistant_uuid.as_bytes());
    hasher.update(b"\0");
    hasher.update(tool_use_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(kind.as_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    uuid::Uuid::from_bytes(bytes)
}

fn append_repair_snapshot(
    session_id: &str,
    rollout_path: &Path,
    messages: &[Message],
) -> Result<u64> {
    let recorded_messages = messages
        .iter()
        .map(record_replay::types::RecordedMessage::from_message)
        .collect::<Vec<_>>();
    let encoded = serde_json::to_vec(&recorded_messages)
        .context("failed to encode repaired session snapshot")?;
    let hash = Some(hex::encode(Sha256::digest(&encoded)));
    let entry = record_replay::index::append_record_items_to_rollout(
        session_id,
        rollout_path,
        vec![record_replay::RecordItem::Snapshot(
            record_replay::types::SessionSnapshotRecord {
                message_count: recorded_messages.len(),
                messages: recorded_messages,
                hash,
            },
        )],
    )?;
    Ok(entry.last_seq)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_types::message::AssistantMessage;

    struct EnvGuard {
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set_home(path: &Path) -> Self {
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

    fn tool_call(tool_use_id: &str) -> Message {
        Message::Assistant(AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 10,
            role: "assistant".to_string(),
            content: vec![ContentBlock::ToolUse {
                id: tool_use_id.to_string(),
                name: "Read".to_string(),
                input: serde_json::json!({"file_path": "/tmp/input.txt"}),
            }],
            usage: None,
            stop_reason: Some("tool_use".to_string()),
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        })
    }

    fn tool_result(tool_use_id: &str, source_uuid: uuid::Uuid, text: &str) -> Message {
        Message::User(UserMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 11,
            role: "user".to_string(),
            content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                tool_use_id: tool_use_id.to_string(),
                content: ToolResultContent::Text(text.to_string()),
                is_error: false,
            }]),
            is_meta: true,
            tool_use_result: Some(text.to_string()),
            source_tool_assistant_uuid: Some(source_uuid),
        })
    }

    #[test]
    fn test_get_last_session_no_sessions() {
        // When no sessions directory exists, should return None, not an error.
        let result = get_last_session(Path::new("/nonexistent/path"));
        assert!(result.is_ok());
        // The result may or may not be None depending on whether there are
        // sessions on this machine, but it should not error.
    }

    #[test]
    fn complete_rollout_tool_history_is_left_unchanged() {
        let call = tool_call("toolu_complete");
        let assistant_uuid = call.uuid();
        let result = tool_result("toolu_complete", assistant_uuid, "exact output");
        let messages = vec![call, result];

        let repaired = repair_tool_history("complete-session", &messages, &[]).unwrap();

        assert!(!repaired.repaired);
        assert_eq!(repaired.messages.len(), 2);
        assert_eq!(repaired.messages[0].uuid(), messages[0].uuid());
        assert_eq!(repaired.messages[1].uuid(), messages[1].uuid());
    }

    #[test]
    fn missing_rollout_result_is_recovered_from_legacy_projection() {
        let call = tool_call("toolu_projection");
        let assistant_uuid = call.uuid();
        let exact_result = tool_result("toolu_projection", assistant_uuid, "projection output");
        let replay = vec![call.clone()];
        let legacy = vec![call, exact_result.clone()];

        let repaired = repair_tool_history("projection-session", &replay, &legacy).unwrap();

        assert!(repaired.repaired);
        assert_eq!(repaired.recovered_from_projection, 1);
        assert_eq!(repaired.synthetic_results, 0);
        assert_eq!(repaired.messages.len(), 2);
        assert_eq!(repaired.messages[1].uuid(), exact_result.uuid());
        assert!(matches!(
            &repaired.messages[1],
            Message::User(UserMessage {
                content: MessageContent::Blocks(blocks),
                ..
            }) if matches!(blocks.as_slice(), [ContentBlock::ToolResult {
                content: ToolResultContent::Text(text),
                is_error: false,
                ..
            }] if text == "projection output")
        ));
    }

    #[test]
    fn missing_result_in_both_sources_gets_deterministic_non_replaying_error() {
        let replay = vec![tool_call("toolu_interrupted")];

        let first = repair_tool_history("synthetic-session", &replay, &[]).unwrap();
        let second = repair_tool_history("synthetic-session", &replay, &[]).unwrap();

        assert!(first.repaired);
        assert_eq!(first.recovered_from_projection, 0);
        assert_eq!(first.synthetic_results, 1);
        assert_eq!(first.messages[1].uuid(), second.messages[1].uuid());
        assert!(matches!(
            &first.messages[1],
            Message::User(UserMessage {
                content: MessageContent::Blocks(blocks),
                ..
            }) if matches!(blocks.as_slice(), [ContentBlock::ToolResult {
                tool_use_id,
                content: ToolResultContent::Text(text),
                is_error: true,
            }] if tool_use_id == "toolu_interrupted"
                && text.contains("was not re-run"))
        ));
    }

    #[test]
    #[serial_test::serial]
    fn replay_repair_appends_full_snapshot_before_returning() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set_home(home.path());
        let workspace = home.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = "resume-projection-snapshot";
        let call = tool_call("toolu_snapshot");
        let result = tool_result("toolu_snapshot", call.uuid(), "snapshot output");
        let rollout_path = record_replay::create_rollout_from_messages(
            session_id,
            std::slice::from_ref(&call),
            workspace.to_str().unwrap(),
            None,
            None,
        )
        .unwrap();
        storage::save_session(session_id, &[call, result], workspace.to_str().unwrap()).unwrap();

        let resumed = resume_session_detail(session_id).unwrap();

        assert_eq!(resumed.source, ResumeSource::Replay);
        assert_eq!(resumed.messages.len(), 2);
        let read = record_replay::read_rollout_file(&rollout_path).unwrap();
        assert!(matches!(
            read.lines.last().map(|line| &line.item),
            Some(record_replay::RecordItem::Snapshot(snapshot))
                if snapshot.message_count == 2
        ));
        let reconstructed = record_replay::reconstruct_messages(&read.lines);
        assert_eq!(reconstructed.len(), 2);
        assert!(analyze_tool_history(&reconstructed).missing.is_empty());
        assert!(analyze_tool_history(&reconstructed).errors.is_empty());
    }
}
