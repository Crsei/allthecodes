//! Session resume — finding and restoring the most recent session.
//!
//! Provides helpers to locate the last session for a given working directory
//! and to reload its message history so the conversation can continue.
//!
//! Replay-first: when a record log (rollout JSONL) exists for the session,
//! the message history is reconstructed from the log instead of from the
//! legacy JSON/SQLite projection. Falls back to legacy storage when no
//! rollout is found.

use std::path::Path;

use anyhow::Result;
use tracing::{debug, info, warn};

use super::record_replay;
use super::record_replay::PendingInteraction;
use super::storage::{self, SessionInfo};
use allthecodes_types::message::Message;

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

    // Fallback: load from legacy JSON/SQLite
    let messages = storage::load_session(session_id)?;

    // Trigger lazy migration if the session was successfully loaded
    // (best effort — migration failure should not block resume)
    if record_config.enabled && !messages.is_empty() {
        match record_replay::migration::find_rollout_for_session(session_id) {
            Ok(Some(_)) => {
                // Rollout already exists from a previous migration
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
    let messages = record_replay::reconstruct::recorded_messages_to_typed(&reconstructed.messages);
    let record_schema_version = result.lines.iter().map(|line| line.schema_version).max();
    let last_seq = result.lines.iter().map(|line| line.seq).max();

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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_last_session_no_sessions() {
        // When no sessions directory exists, should return None, not an error.
        let result = get_last_session(Path::new("/nonexistent/path"));
        assert!(result.is_ok());
        // The result may or may not be None depending on whether there are
        // sessions on this machine, but it should not error.
    }
}
