//! Session export — produce a structured, analyzable, replayable session record.
//!
//! Unlike the audit export (tamper-proof hash chain) or the Markdown export
//! (human-readable summary), the session export produces a JSON "data package"
//! that includes:
//!
//! - **Transcript**: all messages in their full JSON form + counts
//! - **Tool call timeline**: tool_use ↔ tool_result pairs reconstructed in order
//! - **Compression events**: compact boundaries, content replacements (detected
//!   from tool result preview markers)
//! - **Context snapshot**: token estimates, cost breakdown, tool usage statistics
//! - **Session metadata**: git branch, project path, model, timestamps
//!
//! Storage: `~/.allthecodes/exports/<session_id>.session.json`

mod builders;
mod compression;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{TimeZone, Utc};
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::storage::{self, SessionFile};
use allthecodes_types::message::Message;

pub use crate::request_snapshot::ApiRequestSnapshot;
pub use builders::build_context_snapshot;
pub use compression::{
    detect_content_replacement, extract_compression_events, reconstruct_tool_timeline,
};

pub const SESSION_EXPORT_SCHEMA_VERSION: u32 = 2;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Top-level session export — the full analyzable data package.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionExport {
    pub schema_version: u32,
    pub exported_at: String,
    pub session: SessionMeta,
    #[serde(default)]
    pub raw_transcript: TranscriptData,
    pub transcript: TranscriptData,
    #[serde(default)]
    pub api_view: ApiViewData,
    #[serde(default)]
    pub api_requests: Vec<ApiRequestSnapshot>,
    pub tool_calls: Vec<ToolCallRecord>,
    pub compression: CompressionData,
    pub context: ContextSnapshot,
}

/// Session-level metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMeta {
    pub session_id: String,
    pub project_path: Option<String>,
    pub git_branch: Option<String>,
    pub git_head_sha: Option<String>,
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_title: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
}

/// Raw transcript data — full messages + breakdown counts.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TranscriptData {
    pub messages: Vec<serde_json::Value>,
    pub message_count: usize,
    pub user_message_count: usize,
    pub assistant_message_count: usize,
    pub system_message_count: usize,
}

/// API-view summary derived from the recorded request snapshots.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ApiViewData {
    pub request_count: usize,
    pub providers: Vec<String>,
    pub models: Vec<String>,
    pub last_request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<String>,
    /// Rollout path if available (record-replay).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rollout_path: Option<String>,
    /// Last sequence number from the rollout log.
    #[serde(default)]
    pub last_seq: u64,
    /// Record log schema version.
    #[serde(default)]
    pub record_schema_version: u32,
}

/// A single tool call with its matched result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRecord {
    /// Sequential index in the tool call timeline.
    pub sequence: usize,
    /// UUID of the assistant message that initiated this tool call.
    pub assistant_uuid: String,
    /// Unique tool_use_id from the API.
    pub tool_use_id: String,
    /// Tool name (e.g. "Bash", "Read", "Edit").
    pub tool_name: String,
    /// Tool input parameters.
    pub input: serde_json::Value,
    /// Matched tool_result content (None if no result found).
    pub result: Option<serde_json::Value>,
    /// Whether the tool result was an error.
    pub is_error: bool,
    /// Timestamp of the assistant message (ISO-8601).
    pub timestamp: String,
    /// Timestamp of the user message carrying the tool_result.
    pub result_timestamp: Option<String>,
    /// Whether the tool result was replaced by tool_result_budget (large
    /// results saved to disk and replaced with a preview).
    pub was_content_replaced: bool,
}

/// Compression/compaction event data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressionData {
    pub compact_boundaries: Vec<CompactBoundaryRecord>,
    pub content_replacements: Vec<ContentReplacementRecord>,
    pub microcompact_replacements: Vec<MicrocompactRecord>,
    pub total_compactions: usize,
}

/// A compact boundary (conversation-level compaction event).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactBoundaryRecord {
    pub uuid: String,
    pub timestamp: String,
    pub pre_compact_tokens: Option<u64>,
    pub post_compact_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preserved_segment: Option<PreservedSegmentRecord>,
    pub boundary_text: String,
}

/// The summary and retained message UUIDs associated with a compact boundary.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PreservedSegmentRecord {
    pub summary_message_uuid: Option<String>,
    pub preserved_message_uuids: Vec<String>,
}

/// A content replacement record (tool result saved to disk).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentReplacementRecord {
    pub tool_use_id: String,
    pub original_size_hint: Option<usize>,
    pub file_path: Option<String>,
}

/// A microcompact replacement record (old large tool result truncated in-place).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MicrocompactRecord {
    pub tool_use_id: String,
    pub omitted_chars: usize,
}

/// Context snapshot — token and cost statistics at export time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextSnapshot {
    pub estimated_total_tokens: u64,
    pub context_window_size: u64,
    pub utilization_pct: f64,
    pub total_cost_usd: f64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub cache_read_tokens: u64,
    pub api_call_count: usize,
    pub tool_use_count: usize,
    pub unique_tools_used: Vec<String>,
}

// ---------------------------------------------------------------------------
// Public API — export
// ---------------------------------------------------------------------------

/// Export the current in-memory conversation as a structured session record.
/// Returns the path written and the export data (for summary display).
pub fn export_session(
    session_id: &str,
    messages: &[Message],
    cwd: &str,
    output_path: Option<&Path>,
) -> Result<(PathBuf, SessionExport)> {
    let export = build_session_export(session_id, messages, cwd);
    let path = write_session_export(&export, session_id, output_path)?;
    Ok((path, export))
}

/// Export a saved session (by ID) as a structured session record.
pub fn export_saved_session(
    session_id: &str,
    output_path: Option<&Path>,
) -> Result<(PathBuf, SessionExport)> {
    let (messages, cwd, custom_title) = load_saved_session_for_export(session_id)?;
    let mut export = build_session_export(session_id, &messages, &cwd);
    export.session.custom_title = custom_title;
    let path = write_session_export(&export, session_id, output_path)?;
    Ok((path, export))
}

fn load_saved_session_for_export(
    session_id: &str,
) -> Result<(Vec<Message>, String, Option<String>)> {
    match crate::record_replay::index::lookup_rollout(session_id) {
        Ok(Some(path)) => match crate::record_replay::reader::read_rollout_file(&path) {
            Ok(read) => {
                let reconstructed =
                    crate::record_replay::reconstruct::reconstruct_recorded_messages(&read.lines);
                let messages = crate::record_replay::reconstruct::recorded_messages_to_typed(
                    &reconstructed.messages,
                );
                let cwd = reconstructed
                    .metadata
                    .as_ref()
                    .map(|metadata| metadata.cwd.clone())
                    .filter(|cwd| !cwd.is_empty())
                    .or_else(|| {
                        storage::load_session_info(session_id)
                            .ok()
                            .map(|info| info.cwd)
                    })
                    .unwrap_or_else(|| ".".to_string());
                let custom_title = storage::load_session_info(session_id)
                    .ok()
                    .and_then(|info| info.custom_title);
                return Ok((messages, cwd, custom_title));
            }
            Err(error) => {
                warn!(
                    session_id,
                    path = %path.display(),
                    error = %error,
                    "failed to read rollout log for export, falling back to legacy session file"
                );
            }
        },
        Ok(None) => {}
        Err(error) => {
            warn!(
                session_id,
                error = %error,
                "rollout lookup failed for export, falling back to legacy session file"
            );
        }
    }

    let session_file = load_session_file_raw(session_id)?;
    let messages = storage::load_session(session_id)?;
    Ok((
        messages,
        session_file.cwd,
        session_file.custom_title.clone(),
    ))
}

/// List all session export files.
pub fn list_session_exports() -> Result<Vec<PathBuf>> {
    let dir = get_export_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.to_string_lossy().ends_with(".session.json"))
        .collect();
    files.sort();
    Ok(files)
}

// ---------------------------------------------------------------------------
// Core builder
// ---------------------------------------------------------------------------

/// Build the full SessionExport from in-memory messages.
pub fn build_session_export(session_id: &str, messages: &[Message], cwd: &str) -> SessionExport {
    let session_meta = builders::build_session_meta(session_id, messages, cwd);
    let transcript = builders::build_transcript_data(messages);
    let mut api_diagnostics = Vec::new();
    let api_requests = crate::request_snapshot::load_api_request_snapshots(session_id)
        .unwrap_or_else(|error| {
            tracing::warn!(session_id, %error, "failed to load API request snapshots for export");
            api_diagnostics.push(format!("failed to load API request snapshots: {error}"));
            Vec::new()
        });
    let mut api_view = builders::build_api_view_data(&api_requests);
    api_view.diagnostics = api_diagnostics;
    let tool_calls = compression::reconstruct_tool_timeline(messages);
    let compression = compression::extract_compression_events(messages);
    let context = builders::build_context_snapshot(messages);

    // Try to include rollout metadata
    let (rollout_path, record_schema_version, last_seq) =
        match crate::record_replay::index::lookup_rollout(session_id) {
            Ok(Some(path)) => {
                let version = crate::record_replay::RECORD_SCHEMA_VERSION;
                // Try to get last_seq from the rollout file
                let seq = crate::record_replay::reader::read_rollout_file(&path)
                    .ok()
                    .and_then(|r| r.lines.last().map(|l| l.seq))
                    .unwrap_or(0);
                (Some(path.to_string_lossy().to_string()), version, seq)
            }
            _ => (None, 0, 0),
        };
    api_view.rollout_path = rollout_path;
    api_view.record_schema_version = record_schema_version;
    api_view.last_seq = last_seq;

    SessionExport {
        schema_version: SESSION_EXPORT_SCHEMA_VERSION,
        exported_at: Utc::now().to_rfc3339(),
        session: session_meta,
        raw_transcript: transcript.clone(),
        transcript,
        api_view,
        api_requests,
        tool_calls,
        compression,
        context,
    }
}

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

pub(crate) fn get_export_dir() -> PathBuf {
    allthecodes_config::paths::exports_dir()
}

fn write_session_export(
    export: &SessionExport,
    session_id: &str,
    output_path: Option<&Path>,
) -> Result<PathBuf> {
    let path = match output_path {
        Some(p) => p.to_path_buf(),
        None => {
            let dir = get_export_dir();
            std::fs::create_dir_all(&dir)
                .with_context(|| format!("Failed to create export directory {}", dir.display()))?;
            dir.join(format!("{}.session.json", session_id))
        }
    };

    let json =
        serde_json::to_string_pretty(export).context("Failed to serialize session export")?;

    std::fs::write(&path, json)
        .with_context(|| format!("Failed to write session export {}", path.display()))?;

    Ok(path)
}

fn load_session_file_raw(session_id: &str) -> Result<SessionFile> {
    let path = storage::get_session_file(session_id);
    let contents = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read session file {}", path.display()))?;
    let file: SessionFile = serde_json::from_str(&contents)
        .with_context(|| format!("Failed to parse session file {}", path.display()))?;
    Ok(file)
}

// ---------------------------------------------------------------------------
// Timestamp helpers
// ---------------------------------------------------------------------------

pub(crate) fn format_ts_millis(ts: i64) -> String {
    let secs = ts / 1000;
    let nanos = ((ts % 1000) * 1_000_000) as u32;
    Utc.timestamp_opt(secs, nanos)
        .single()
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_else(|| format!("{}", ts))
}
