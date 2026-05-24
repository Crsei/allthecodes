//! Audit export — produce a verifiable, machine-readable complete record of a session.
//!
//! Unlike the Markdown export (human-readable summary), the audit export produces
//! a JSON file that includes:
//!
//! - **Every message** (user, assistant, system, progress, attachment) — nothing omitted
//! - **Full content blocks** including tool inputs/outputs, thinking, images (base64 refs)
//! - **Per-message SHA-256 hashes** forming a sequential hash chain
//! - **Aggregate integrity hash** for tamper detection
//! - **Rich metadata**: model, cwd, timestamps, token usage, cost breakdown
//!
//! The resulting `.audit.json` file can be:
//! - Programmatically verified with `verify_audit_file()`
//! - Diffed against transcript NDJSON for cross-validation
//! - Fed into compliance/review tooling
//!
//! Storage: `~/.allthecodes/audits/<session_id>.audit.json`

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{TimeZone, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::storage::{self, SessionFile};
use allthecodes_bootstrap::PROCESS_STATE;
use allthecodes_types::message::{Message, MessageContent};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Top-level audit record — the complete, verifiable session export.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditRecord {
    /// Schema version for forward compatibility.
    pub format_version: String,
    /// Session-level metadata.
    pub metadata: AuditMetadata,
    /// Ordered message entries with per-entry hashes.
    pub entries: Vec<AuditEntry>,
    /// Integrity information for the whole record.
    pub integrity: IntegrityInfo,
}

/// Session-level metadata captured at export time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditMetadata {
    pub session_id: String,
    pub exported_at: String,
    pub exporter_version: String,
    pub working_directory: String,
    pub model: Option<String>,
    pub session_started: Option<String>,
    pub session_ended: Option<String>,
    pub total_messages: usize,
    pub total_cost_usd: f64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
}

/// A single message in the audit trail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    /// Zero-based sequence number.
    pub sequence: usize,
    /// Message UUID.
    pub uuid: String,
    /// ISO-8601 timestamp.
    pub timestamp: String,
    /// Message type tag: "user", "assistant", "system", "progress", "attachment".
    #[serde(rename = "type")]
    pub msg_type: String,
    /// SHA-256 hash of this entry's `data` (hex-encoded).
    pub hash: String,
    /// SHA-256 chain hash: H(prev_chain_hash || this_hash). First entry uses
    /// `"0000...0000"` as the initial chain value.
    pub chain_hash: String,
    /// Full message payload — preserves everything.
    pub data: serde_json::Value,
}

/// Aggregate integrity information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrityInfo {
    /// Hash algorithm used ("sha256").
    pub algorithm: String,
    /// Final chain hash (== last entry's `chain_hash`).
    pub final_chain_hash: String,
    /// Number of entries covered.
    pub entry_count: usize,
}

/// Result of verifying an audit record.
#[derive(Debug, Clone, Serialize)]
pub struct VerifyResult {
    pub valid: bool,
    pub entry_count: usize,
    /// If invalid, the first broken entry sequence number.
    pub first_broken_at: Option<usize>,
    pub details: String,
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const FORMAT_VERSION: &str = "1.0.0";
const EXPORTER_VERSION: &str = concat!("allthecodes/", env!("CARGO_PKG_VERSION"));
const ZERO_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

// ---------------------------------------------------------------------------
// Public API — export
// ---------------------------------------------------------------------------

/// Export a saved session (by ID) as an auditable JSON record.
///
/// If `output_path` is `None`, writes to `~/.allthecodes/audits/<session_id>.audit.json`.
/// Returns the path of the written file.
pub fn export_audit_record(session_id: &str, output_path: Option<&Path>) -> Result<PathBuf> {
    let session_file = load_session_file_raw(session_id)?;
    let record = build_audit_from_session_file(&session_file);
    write_audit_record(&record, session_id, output_path)
}

/// Export the current in-memory conversation as an auditable JSON record.
pub fn export_audit_messages(
    session_id: &str,
    messages: &[Message],
    cwd: &str,
    output_path: Option<&Path>,
) -> Result<PathBuf> {
    let record = build_audit_from_messages(session_id, messages, cwd);
    write_audit_record(&record, session_id, output_path)
}

/// List all audit export files.
pub fn list_audits() -> Result<Vec<PathBuf>> {
    let dir = get_audit_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.to_string_lossy().ends_with(".audit.json"))
        .collect();
    files.sort();
    Ok(files)
}

// ---------------------------------------------------------------------------
// Public API — verify
// ---------------------------------------------------------------------------

/// Verify the integrity of an audit record loaded from a file.
pub fn verify_audit_file(path: &Path) -> Result<VerifyResult> {
    let contents = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read audit file {}", path.display()))?;
    let record: AuditRecord = serde_json::from_str(&contents)
        .with_context(|| format!("Failed to parse audit file {}", path.display()))?;
    Ok(verify_audit_record(&record))
}

/// Verify the integrity of an in-memory audit record.
pub fn verify_audit_record(record: &AuditRecord) -> VerifyResult {
    if record.entries.is_empty() {
        return VerifyResult {
            valid: true,
            entry_count: 0,
            first_broken_at: None,
            details: "Empty record — trivially valid.".into(),
        };
    }

    let mut prev_chain = ZERO_HASH.to_string();

    for entry in &record.entries {
        // 1. Verify data hash
        let expected_data_hash = sha256_json(&entry.data);
        if entry.hash != expected_data_hash {
            return VerifyResult {
                valid: false,
                entry_count: record.entries.len(),
                first_broken_at: Some(entry.sequence),
                details: format!(
                    "Entry #{}: data hash mismatch (expected {}, got {})",
                    entry.sequence, expected_data_hash, entry.hash
                ),
            };
        }

        // 2. Verify chain hash
        let expected_chain = sha256_str(&format!("{}{}", prev_chain, entry.hash));
        if entry.chain_hash != expected_chain {
            return VerifyResult {
                valid: false,
                entry_count: record.entries.len(),
                first_broken_at: Some(entry.sequence),
                details: format!(
                    "Entry #{}: chain hash mismatch (expected {}, got {})",
                    entry.sequence, expected_chain, entry.chain_hash
                ),
            };
        }

        prev_chain = entry.chain_hash.clone();
    }

    // 3. Verify final chain hash in integrity block
    if record.integrity.final_chain_hash != prev_chain {
        return VerifyResult {
            valid: false,
            entry_count: record.entries.len(),
            first_broken_at: None,
            details: format!(
                "Integrity block: final_chain_hash mismatch (expected {}, got {})",
                prev_chain, record.integrity.final_chain_hash
            ),
        };
    }

    // 4. Verify entry count
    if record.integrity.entry_count != record.entries.len() {
        return VerifyResult {
            valid: false,
            entry_count: record.entries.len(),
            first_broken_at: None,
            details: format!(
                "Integrity block: entry_count mismatch (expected {}, got {})",
                record.entries.len(),
                record.integrity.entry_count
            ),
        };
    }

    VerifyResult {
        valid: true,
        entry_count: record.entries.len(),
        first_broken_at: None,
        details: format!(
            "All {} entries verified. Chain intact.",
            record.entries.len()
        ),
    }
}

// ---------------------------------------------------------------------------
// Build from saved session file
// ---------------------------------------------------------------------------

fn build_audit_from_session_file(session: &SessionFile) -> AuditRecord {
    let mut entries = Vec::with_capacity(session.messages.len());
    let mut prev_chain = ZERO_HASH.to_string();
    let mut total_cost = 0.0_f64;
    let mut total_input = 0_u64;
    let mut total_output = 0_u64;

    for (seq, msg) in session.messages.iter().enumerate() {
        // Accumulate cost/tokens from assistant messages
        if msg.msg_type == "assistant" {
            total_cost += msg
                .data
                .get("cost_usd")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            if let Some(usage) = msg.data.get("usage") {
                total_input += usage
                    .get("input_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                total_output += usage
                    .get("output_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
            }
        }

        let data_hash = sha256_json(&msg.data);
        let chain_hash = sha256_str(&format!("{}{}", prev_chain, data_hash));

        entries.push(AuditEntry {
            sequence: seq,
            uuid: msg.uuid.clone(),
            timestamp: format_ts_millis(msg.timestamp),
            msg_type: msg.msg_type.clone(),
            hash: data_hash,
            chain_hash: chain_hash.clone(),
            data: msg.data.clone(),
        });

        prev_chain = chain_hash;
    }

    let final_chain = entries
        .last()
        .map(|e| e.chain_hash.clone())
        .unwrap_or_else(|| ZERO_HASH.to_string());

    // Resolve model from ProcessState
    let model = PROCESS_STATE
        .read()
        .effective_model()
        .map(|s| s.to_string());

    let session_started = if session.created_at > 0 {
        Some(format_ts_secs(session.created_at))
    } else {
        None
    };
    let session_ended = if session.last_modified > 0 {
        Some(format_ts_secs(session.last_modified))
    } else {
        None
    };

    AuditRecord {
        format_version: FORMAT_VERSION.to_string(),
        metadata: AuditMetadata {
            session_id: session.session_id.clone(),
            exported_at: Utc::now().to_rfc3339(),
            exporter_version: EXPORTER_VERSION.to_string(),
            working_directory: session.cwd.clone(),
            model,
            session_started,
            session_ended,
            total_messages: entries.len(),
            total_cost_usd: total_cost,
            total_input_tokens: total_input,
            total_output_tokens: total_output,
        },
        integrity: IntegrityInfo {
            algorithm: "sha256".to_string(),
            final_chain_hash: final_chain,
            entry_count: entries.len(),
        },
        entries,
    }
}

// ---------------------------------------------------------------------------
// Build from live messages
// ---------------------------------------------------------------------------

fn build_audit_from_messages(session_id: &str, messages: &[Message], cwd: &str) -> AuditRecord {
    let mut entries = Vec::with_capacity(messages.len());
    let mut prev_chain = ZERO_HASH.to_string();
    let mut total_cost = 0.0_f64;
    let mut total_input = 0_u64;
    let mut total_output = 0_u64;
    let mut first_ts: Option<i64> = None;
    let mut last_ts: Option<i64> = None;

    for (seq, msg) in messages.iter().enumerate() {
        let ts = msg.timestamp();
        if first_ts.is_none() {
            first_ts = Some(ts);
        }
        last_ts = Some(ts);

        let (msg_type, data) = message_to_full_data(msg);

        // Accumulate cost/tokens
        if let Message::Assistant(a) = msg {
            total_cost += a.cost_usd;
            if let Some(ref usage) = a.usage {
                total_input += usage.input_tokens;
                total_output += usage.output_tokens;
            }
        }

        let data_hash = sha256_json(&data);
        let chain_hash = sha256_str(&format!("{}{}", prev_chain, data_hash));

        entries.push(AuditEntry {
            sequence: seq,
            uuid: msg.uuid().to_string(),
            timestamp: format_ts_millis(ts),
            msg_type,
            hash: data_hash,
            chain_hash: chain_hash.clone(),
            data,
        });

        prev_chain = chain_hash;
    }

    let final_chain = entries
        .last()
        .map(|e| e.chain_hash.clone())
        .unwrap_or_else(|| ZERO_HASH.to_string());

    let model = PROCESS_STATE
        .read()
        .effective_model()
        .map(|s| s.to_string());

    AuditRecord {
        format_version: FORMAT_VERSION.to_string(),
        metadata: AuditMetadata {
            session_id: session_id.to_string(),
            exported_at: Utc::now().to_rfc3339(),
            exporter_version: EXPORTER_VERSION.to_string(),
            working_directory: cwd.to_string(),
            model,
            session_started: first_ts.map(format_ts_millis),
            session_ended: last_ts.map(format_ts_millis),
            total_messages: entries.len(),
            total_cost_usd: total_cost,
            total_input_tokens: total_input,
            total_output_tokens: total_output,
        },
        integrity: IntegrityInfo {
            algorithm: "sha256".to_string(),
            final_chain_hash: final_chain,
            entry_count: entries.len(),
        },
        entries,
    }
}

// ---------------------------------------------------------------------------
// Message → full JSON data (preserves everything)
// ---------------------------------------------------------------------------

fn message_to_full_data(msg: &Message) -> (String, serde_json::Value) {
    match msg {
        Message::User(u) => {
            let content_value = match &u.content {
                MessageContent::Text(t) => serde_json::json!(t),
                MessageContent::Blocks(blocks) => serde_json::json!(blocks),
            };
            (
                "user".into(),
                serde_json::json!({
                    "role": "user",
                    "content": content_value,
                    "is_meta": u.is_meta,
                    "tool_use_result": u.tool_use_result,
                    "source_tool_assistant_uuid": u.source_tool_assistant_uuid,
                }),
            )
        }
        Message::Assistant(a) => (
            "assistant".into(),
            serde_json::json!({
                "role": "assistant",
                "content": a.content,
                "usage": a.usage,
                "stop_reason": a.stop_reason,
                "is_api_error_message": a.is_api_error_message,
                "api_error": a.api_error,
                "cost_usd": a.cost_usd,
            }),
        ),
        Message::System(s) => (
            "system".into(),
            serde_json::json!({
                "subtype": format!("{:?}", s.subtype),
                "content": s.content,
            }),
        ),
        Message::Progress(p) => (
            "progress".into(),
            serde_json::json!({
                "tool_use_id": p.tool_use_id,
                "data": p.data,
            }),
        ),
        Message::Attachment(a) => (
            "attachment".into(),
            serde_json::json!({
                "attachment": a.attachment,
            }),
        ),
    }
}

// ---------------------------------------------------------------------------
// Hashing helpers
// ---------------------------------------------------------------------------

fn sha256_json(value: &serde_json::Value) -> String {
    // Canonical JSON: serialize with sorted keys via serde_json default
    // (serde_json preserves insertion order, but for reproducibility we
    //  use the to_string output which is deterministic for identical Values)
    let bytes = serde_json::to_vec(value).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    hex::encode(hasher.finalize())
}

fn sha256_str(s: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(s.as_bytes());
    hex::encode(hasher.finalize())
}

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

fn get_audit_dir() -> PathBuf {
    allthecodes_config::paths::audits_dir()
}

fn write_audit_record(
    record: &AuditRecord,
    session_id: &str,
    output_path: Option<&Path>,
) -> Result<PathBuf> {
    let path = match output_path {
        Some(p) => p.to_path_buf(),
        None => {
            let dir = get_audit_dir();
            std::fs::create_dir_all(&dir)
                .with_context(|| format!("Failed to create audit directory {}", dir.display()))?;
            dir.join(format!("{}.audit.json", session_id))
        }
    };

    let json = serde_json::to_string_pretty(record).context("Failed to serialize audit record")?;

    std::fs::write(&path, json)
        .with_context(|| format!("Failed to write audit file {}", path.display()))?;

    Ok(path)
}

// ---------------------------------------------------------------------------
// Timestamp helpers
// ---------------------------------------------------------------------------

fn format_ts_millis(ts: i64) -> String {
    let secs = ts / 1000;
    let nanos = ((ts % 1000) * 1_000_000) as u32;
    Utc.timestamp_opt(secs, nanos)
        .single()
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_else(|| format!("{}", ts))
}

fn format_ts_secs(ts: i64) -> String {
    Utc.timestamp_opt(ts, 0)
        .single()
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_else(|| format!("{}", ts))
}

// ---------------------------------------------------------------------------
// Internal: load raw session file
// ---------------------------------------------------------------------------

fn load_session_file_raw(session_id: &str) -> Result<SessionFile> {
    let path = storage::get_session_file(session_id);
    let contents = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read session file {}", path.display()))?;
    let file: SessionFile = serde_json::from_str(&contents)
        .with_context(|| format!("Failed to parse session file {}", path.display()))?;
    Ok(file)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256_deterministic() {
        let v = serde_json::json!({"hello": "world", "num": 42});
        let h1 = sha256_json(&v);
        let h2 = sha256_json(&v);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64); // hex-encoded SHA-256
    }

    #[test]
    fn test_chain_hash() {
        let h1 = sha256_str("test1");
        let h2 = sha256_str("test2");
        let chain = sha256_str(&format!("{}{}", h1, h2));
        assert_eq!(chain.len(), 64);
        assert_ne!(chain, h1);
        assert_ne!(chain, h2);
    }

    #[test]
    fn test_verify_empty_record() {
        let record = AuditRecord {
            format_version: FORMAT_VERSION.to_string(),
            metadata: AuditMetadata {
                session_id: "test".into(),
                exported_at: "2026-01-01T00:00:00Z".into(),
                exporter_version: EXPORTER_VERSION.to_string(),
                working_directory: "/tmp".into(),
                model: None,
                session_started: None,
                session_ended: None,
                total_messages: 0,
                total_cost_usd: 0.0,
                total_input_tokens: 0,
                total_output_tokens: 0,
            },
            entries: vec![],
            integrity: IntegrityInfo {
                algorithm: "sha256".into(),
                final_chain_hash: ZERO_HASH.into(),
                entry_count: 0,
            },
        };
        let result = verify_audit_record(&record);
        assert!(result.valid);
    }

    #[test]
    fn test_verify_single_entry() {
        let data = serde_json::json!({"content": "hello"});
        let data_hash = sha256_json(&data);
        let chain_hash = sha256_str(&format!("{}{}", ZERO_HASH, data_hash));

        let record = AuditRecord {
            format_version: FORMAT_VERSION.to_string(),
            metadata: AuditMetadata {
                session_id: "test".into(),
                exported_at: "2026-01-01T00:00:00Z".into(),
                exporter_version: EXPORTER_VERSION.to_string(),
                working_directory: "/tmp".into(),
                model: None,
                session_started: None,
                session_ended: None,
                total_messages: 1,
                total_cost_usd: 0.0,
                total_input_tokens: 0,
                total_output_tokens: 0,
            },
            entries: vec![AuditEntry {
                sequence: 0,
                uuid: "test-uuid".into(),
                timestamp: "2026-01-01T00:00:00Z".into(),
                msg_type: "user".into(),
                hash: data_hash,
                chain_hash: chain_hash.clone(),
                data,
            }],
            integrity: IntegrityInfo {
                algorithm: "sha256".into(),
                final_chain_hash: chain_hash,
                entry_count: 1,
            },
        };
        let result = verify_audit_record(&record);
        assert!(result.valid, "details: {}", result.details);
    }

    #[test]
    fn test_verify_tampered_data() {
        let data = serde_json::json!({"content": "hello"});
        let data_hash = sha256_json(&data);
        let chain_hash = sha256_str(&format!("{}{}", ZERO_HASH, data_hash));

        let tampered_data = serde_json::json!({"content": "TAMPERED"});

        let record = AuditRecord {
            format_version: FORMAT_VERSION.to_string(),
            metadata: AuditMetadata {
                session_id: "test".into(),
                exported_at: "2026-01-01T00:00:00Z".into(),
                exporter_version: EXPORTER_VERSION.to_string(),
                working_directory: "/tmp".into(),
                model: None,
                session_started: None,
                session_ended: None,
                total_messages: 1,
                total_cost_usd: 0.0,
                total_input_tokens: 0,
                total_output_tokens: 0,
            },
            entries: vec![AuditEntry {
                sequence: 0,
                uuid: "test-uuid".into(),
                timestamp: "2026-01-01T00:00:00Z".into(),
                msg_type: "user".into(),
                hash: data_hash, // hash of original
                chain_hash: chain_hash.clone(),
                data: tampered_data, // but data was changed
            }],
            integrity: IntegrityInfo {
                algorithm: "sha256".into(),
                final_chain_hash: chain_hash,
                entry_count: 1,
            },
        };
        let result = verify_audit_record(&record);
        assert!(!result.valid);
        assert_eq!(result.first_broken_at, Some(0));
    }

    #[test]
    fn test_verify_multi_entry_chain() {
        let d1 = serde_json::json!({"msg": "first"});
        let d2 = serde_json::json!({"msg": "second"});
        let d3 = serde_json::json!({"msg": "third"});

        let h1 = sha256_json(&d1);
        let c1 = sha256_str(&format!("{}{}", ZERO_HASH, h1));

        let h2 = sha256_json(&d2);
        let c2 = sha256_str(&format!("{}{}", c1, h2));

        let h3 = sha256_json(&d3);
        let c3 = sha256_str(&format!("{}{}", c2, h3));

        let record = AuditRecord {
            format_version: FORMAT_VERSION.to_string(),
            metadata: AuditMetadata {
                session_id: "test".into(),
                exported_at: "2026-01-01T00:00:00Z".into(),
                exporter_version: EXPORTER_VERSION.to_string(),
                working_directory: "/tmp".into(),
                model: None,
                session_started: None,
                session_ended: None,
                total_messages: 3,
                total_cost_usd: 0.0,
                total_input_tokens: 0,
                total_output_tokens: 0,
            },
            entries: vec![
                AuditEntry {
                    sequence: 0,
                    uuid: "u1".into(),
                    timestamp: "t1".into(),
                    msg_type: "user".into(),
                    hash: h1,
                    chain_hash: c1,
                    data: d1,
                },
                AuditEntry {
                    sequence: 1,
                    uuid: "u2".into(),
                    timestamp: "t2".into(),
                    msg_type: "assistant".into(),
                    hash: h2,
                    chain_hash: c2,
                    data: d2,
                },
                AuditEntry {
                    sequence: 2,
                    uuid: "u3".into(),
                    timestamp: "t3".into(),
                    msg_type: "user".into(),
                    hash: h3,
                    chain_hash: c3.clone(),
                    data: d3,
                },
            ],
            integrity: IntegrityInfo {
                algorithm: "sha256".into(),
                final_chain_hash: c3,
                entry_count: 3,
            },
        };
        let result = verify_audit_record(&record);
        assert!(result.valid, "details: {}", result.details);
    }

    #[test]
    fn test_audit_dir() {
        let dir = get_audit_dir();
        assert!(dir.to_string_lossy().contains("audits"));
    }

    #[test]
    fn test_format_ts_millis() {
        let ts = format_ts_millis(1700000000000);
        assert!(ts.contains("2023"));
    }
}
