//! Session storage -- persisting conversation state to disk.
//!
//! Sessions are stored as JSON files under `~/.allthecodes/sessions/`.
//! Each session is identified by a UUID and contains the full message history
//! along with metadata (creation time, working directory, etc.).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Utc;
use git2::Repository;
use serde::{Deserialize, Serialize};
use tracing::debug;

use allthecodes_types::message::Message;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Metadata about a saved session, returned by `list_sessions`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    /// Unique session identifier (UUID v4).
    pub session_id: String,
    /// Unix timestamp (seconds) when the session was created.
    pub created_at: i64,
    /// Unix timestamp (seconds) of the last modification.
    pub last_modified: i64,
    /// Number of messages in the session.
    pub message_count: usize,
    /// Working directory at the time the session was created.
    pub cwd: String,
    /// Display title — prefers `custom_title` (set via `/rename`), falls back to
    /// the derived title (first user message text, truncated). Empty when
    /// neither is available.
    #[serde(default)]
    pub title: String,
    /// Explicit user-assigned title, or `None` to fall back to the derived
    /// title. Populated by `/rename` and persisted on the `SessionFile`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_title: Option<String>,
    /// Stable grouping key for the workspace. Sessions sharing the same git
    /// common-dir (or canonical path for non-git dirs) get the same key.
    #[serde(default)]
    pub workspace_key: String,
    /// Root directory for the workspace (git root or the session cwd).
    #[serde(default)]
    pub workspace_root: String,
    /// Human-readable workspace label (basename of the root).
    #[serde(default)]
    pub workspace_name: String,
}

/// On-disk representation of a saved session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionFile {
    pub session_id: String,
    pub created_at: i64,
    pub last_modified: i64,
    pub cwd: String,
    /// Custom user-assigned title (via `/rename`). `None` means use the
    /// auto-derived title from the first user message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_title: Option<String>,
    pub messages: Vec<SerializableMessage>,
}

/// Simplified serializable message wrapper.
///
/// `Message` itself is a complex enum. For persistence we flatten it into a
/// tagged JSON representation. The real implementation would use a custom
/// Serialize/Deserialize impl on `Message`; for now we store the JSON value
/// directly so we don't lose data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializableMessage {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub uuid: String,
    pub timestamp: i64,
    pub data: serde_json::Value,
}

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

/// Return the base directory for session storage. Resolves through
/// [`allthecodes_config::paths::sessions_dir`].
pub fn get_session_dir() -> PathBuf {
    allthecodes_config::paths::sessions_dir()
}

/// Return the file path for a specific session.
pub fn get_session_file(session_id: &str) -> PathBuf {
    get_session_dir().join(format!("{}.json", session_id))
}

/// Return the directory for archived sessions.
pub fn get_archived_session_dir() -> PathBuf {
    get_session_dir().join("archive")
}

/// Return the primary archive file path for a specific session.
pub fn get_archived_session_file(session_id: &str) -> PathBuf {
    get_archived_session_dir().join(format!("{}.json", session_id))
}

fn normalize_display_path(path: &Path) -> String {
    std::fs::canonicalize(path)
        .unwrap_or_else(|_| path.components().collect())
        .to_string_lossy()
        .to_string()
}

fn stable_workspace_path(path: &Path) -> PathBuf {
    allthecodes_utils::git::find_git_root(path).unwrap_or_else(|| path.to_path_buf())
}

fn normalize_match_key(path: &Path) -> String {
    let normalized: PathBuf =
        std::fs::canonicalize(path).unwrap_or_else(|_| path.components().collect());
    let mut value = normalized.to_string_lossy().to_string();

    if cfg!(windows) {
        value = value.replace('/', "\\").to_lowercase();
    }

    value
}

fn git_common_dir(repo: &Repository) -> PathBuf {
    if repo.is_worktree() {
        repo.path()
            .parent()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .unwrap_or_else(|| repo.path().to_path_buf())
    } else {
        repo.path().to_path_buf()
    }
}

/// Stable workspace key for grouping sessions that belong to the same repo.
///
/// For git repositories (including worktrees) this is the canonical form of
/// the git common directory, so all worktrees of the same repo share a key.
/// For non-git directories it's the canonical path (or stable workspace path).
pub fn workspace_key(path: &Path) -> String {
    if let Ok(repo) = Repository::discover(path) {
        return normalize_match_key(&git_common_dir(&repo));
    }

    normalize_match_key(&stable_workspace_path(path))
}

/// Return the root directory for the workspace (git root when available, else
/// the path itself). This is a display-friendly absolute path.
pub fn workspace_root(path: &Path) -> PathBuf {
    if let Ok(repo) = Repository::discover(path) {
        let common = git_common_dir(&repo);
        // `.git` or `worktrees/<name>` live under the root -- strip one level
        // so the root points at the working directory, not the git metadata.
        if common.file_name().and_then(|s| s.to_str()) == Some(".git") {
            if let Some(parent) = common.parent() {
                return parent.to_path_buf();
            }
        }
        return common;
    }
    stable_workspace_path(path)
}

/// Display name for a workspace — the basename of the workspace root.
pub fn workspace_name(root: &Path) -> String {
    root.file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| root.to_string_lossy().to_string())
}

/// Extract a title from the first non-meta user message in a SessionFile.
/// Returns an empty string if no suitable message exists.
fn derive_title(messages: &[SerializableMessage]) -> String {
    for sm in messages {
        if sm.msg_type != "user" {
            continue;
        }
        // Skip meta messages (system-injected context).
        if sm
            .data
            .get("is_meta")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            continue;
        }

        let text = extract_user_text(&sm.data);
        if text.is_empty() {
            continue;
        }

        let trimmed = text.trim();
        // Pick the first non-empty line, then truncate.
        let first_line = trimmed.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
        let first_line = first_line.trim();
        if first_line.is_empty() {
            continue;
        }

        const MAX: usize = 80;
        let out: String = first_line.chars().take(MAX).collect();
        if first_line.chars().count() > MAX {
            return format!("{}…", out);
        }
        return out;
    }
    String::new()
}

/// Pull plain text from the `content` field of a serialized user message.
/// Accepts the two historical representations: a bare string or a list of
/// content blocks with `{type: "text", text: ...}` entries.
fn extract_user_text(data: &serde_json::Value) -> String {
    let Some(content) = data.get("content") else {
        return String::new();
    };

    match content {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(blocks) => blocks
            .iter()
            .filter_map(|b| {
                let ty = b.get("type").and_then(|v| v.as_str()).unwrap_or("");
                if ty != "text" {
                    return None;
                }
                b.get("text")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn build_session_info(file: SessionFile) -> SessionInfo {
    let cwd_path = Path::new(&file.cwd);
    let ws_root = workspace_root(cwd_path);
    let ws_key = workspace_key(cwd_path);
    let ws_name = workspace_name(&ws_root);
    let derived = derive_title(&file.messages);
    let title = file
        .custom_title
        .as_ref()
        .filter(|t| !t.is_empty())
        .cloned()
        .unwrap_or(derived);
    SessionInfo {
        session_id: file.session_id,
        created_at: file.created_at,
        last_modified: file.last_modified,
        message_count: file.messages.len(),
        cwd: file.cwd,
        title,
        custom_title: file.custom_title,
        workspace_key: ws_key,
        workspace_root: ws_root.to_string_lossy().to_string(),
        workspace_name: ws_name,
    }
}

fn filter_sessions_for_workspace(mut sessions: Vec<SessionInfo>, cwd: &Path) -> Vec<SessionInfo> {
    let current_workspace = workspace_key(cwd);
    sessions.retain(|session| {
        // Use the cached workspace_key when present (new code), fall back to
        // computing from cwd for sessions written by older builds.
        if !session.workspace_key.is_empty() {
            session.workspace_key == current_workspace
        } else {
            workspace_key(Path::new(&session.cwd)) == current_workspace
        }
    });
    sessions
}

// ---------------------------------------------------------------------------
// Persistence operations
// ---------------------------------------------------------------------------

/// Save a session to disk.
///
/// Creates the sessions directory if it does not exist. Overwrites any
/// existing file for the same `session_id`.
pub fn save_session(session_id: &str, messages: &[Message], cwd: &str) -> Result<()> {
    let path = get_session_file(session_id);
    save_session_to_file(session_id, messages, cwd, &path)
}

pub(crate) fn save_session_to_file(
    session_id: &str,
    messages: &[Message],
    cwd: &str,
    path: &Path,
) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("Failed to create session directory {}", dir.display()))?;
    }

    let now = Utc::now().timestamp();

    // Preserve the original created_at and custom_title when updating.
    let (created_at, custom_title) = if path.exists() {
        match load_session_file_from_path(path) {
            Ok(f) => (f.created_at, f.custom_title),
            Err(_) => (now, None),
        }
    } else {
        (now, None)
    };

    let serializable_messages = messages_to_serializable(messages);
    let msg_count = serializable_messages.len();

    let stable_cwd = normalize_display_path(&stable_workspace_path(Path::new(cwd)));

    let session_file = SessionFile {
        session_id: session_id.to_string(),
        created_at,
        last_modified: now,
        cwd: stable_cwd,
        custom_title,
        messages: serializable_messages,
    };

    let json =
        serde_json::to_string_pretty(&session_file).context("Failed to serialize session")?;

    std::fs::write(path, json)
        .with_context(|| format!("Failed to write session file {}", path.display()))?;

    debug!(
        session_id = session_id,
        messages = msg_count,
        "session saved"
    );

    Ok(())
}

/// Maximum length (in chars) for a custom session title.
pub const MAX_CUSTOM_TITLE_LEN: usize = 200;

/// Set or clear the user-assigned title for a session.
///
/// `title = None` clears the custom title (falling back to the auto-derived
/// one). A non-empty title is trimmed and truncated to [`MAX_CUSTOM_TITLE_LEN`]
/// characters before persistence.
///
/// Updates `last_modified` to the current time. Returns the final stored title
/// (after trimming/truncation) or `None` if cleared.
pub fn set_session_title(session_id: &str, title: Option<&str>) -> Result<Option<String>> {
    let path = get_session_file(session_id);
    set_session_title_in_file(session_id, title, &path)
}

pub(crate) fn set_session_title_in_file(
    session_id: &str,
    title: Option<&str>,
    path: &Path,
) -> Result<Option<String>> {
    let mut file = load_session_file_from_path(path)?;

    let new_title = title.and_then(|raw| {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }
        let truncated: String = trimmed.chars().take(MAX_CUSTOM_TITLE_LEN).collect();
        Some(truncated)
    });

    file.custom_title = new_title.clone();
    file.last_modified = Utc::now().timestamp();

    let json = serde_json::to_string_pretty(&file).context("Failed to serialize session")?;
    std::fs::write(path, json)
        .with_context(|| format!("Failed to write session file {}", path.display()))?;

    debug!(
        session_id = session_id,
        title = ?new_title,
        "session title updated"
    );

    Ok(new_title)
}

/// Truncate the stored session to its first `keep` messages, producing a
/// backup copy of the pre-truncation file for recovery.
///
/// Returns the new length after truncation. The backup is written next to the
/// session file as `{session_id}.rewind-{epoch_seconds}.json`. When `keep`
/// exceeds the current message count, this is a no-op and no backup is
/// produced.
///
/// Use [`load_session`] afterwards to fetch the truncated messages.
pub fn truncate_session(session_id: &str, keep: usize) -> Result<usize> {
    let mut file = load_session_file(session_id)?;

    if keep >= file.messages.len() {
        return Ok(file.messages.len());
    }

    let backup_path = rewind_backup_path(session_id);
    let original =
        serde_json::to_string_pretty(&file).context("Failed to serialize session for backup")?;
    std::fs::write(&backup_path, original)
        .with_context(|| format!("Failed to write rewind backup {}", backup_path.display()))?;

    file.messages.truncate(keep);
    file.last_modified = Utc::now().timestamp();
    let new_len = file.messages.len();

    let path = get_session_file(session_id);
    let json = serde_json::to_string_pretty(&file).context("Failed to serialize session")?;
    std::fs::write(&path, json)
        .with_context(|| format!("Failed to write session file {}", path.display()))?;

    debug!(
        session_id = session_id,
        kept = new_len,
        backup = %backup_path.display(),
        "session truncated"
    );

    Ok(new_len)
}

fn rewind_backup_path(session_id: &str) -> PathBuf {
    let ts = Utc::now().timestamp();
    get_session_dir().join(format!("{}.rewind-{}.json", session_id, ts))
}

fn available_archive_path(session_id: &str) -> PathBuf {
    let archive_dir = get_archived_session_dir();
    let primary = get_archived_session_file(session_id);
    if !primary.exists() {
        return primary;
    }

    let ts = Utc::now().timestamp();
    for suffix in 0.. {
        let file_name = if suffix == 0 {
            format!("{}.archived-{}.json", session_id, ts)
        } else {
            format!("{}.archived-{}-{}.json", session_id, ts, suffix)
        };
        let candidate = archive_dir.join(file_name);
        if !candidate.exists() {
            return candidate;
        }
    }

    unreachable!("unbounded archive suffix search should always return");
}

/// Move a top-level session file into the archive directory.
///
/// The archived JSON remains intact for future restore/listing support, but
/// disappears from [`list_sessions`] because that function only reads the
/// top-level sessions directory.
pub fn archive_session(session_id: &str) -> Result<()> {
    let src = get_session_file(session_id);
    if !src.exists() {
        anyhow::bail!(
            "Session file for {} does not exist at {}",
            session_id,
            src.display()
        );
    }

    let archive_dir = get_archived_session_dir();
    std::fs::create_dir_all(&archive_dir).with_context(|| {
        format!(
            "Failed to create archived session directory {}",
            archive_dir.display()
        )
    })?;

    let dest = available_archive_path(session_id);

    match std::fs::rename(&src, &dest) {
        Ok(()) => {}
        Err(rename_err) => {
            std::fs::copy(&src, &dest).with_context(|| {
                format!(
                    "Failed to archive session {} from {} to {} after rename failed: {}",
                    session_id,
                    src.display(),
                    dest.display(),
                    rename_err
                )
            })?;
            std::fs::remove_file(&src).with_context(|| {
                format!(
                    "Failed to remove original session file {} after copying archive",
                    src.display()
                )
            })?;
        }
    }

    debug!(
        session_id = session_id,
        archive_path = %dest.display(),
        "session archived"
    );

    Ok(())
}

/// Return the raw on-disk view of a single session file.
///
/// Useful for analytics and tooling that need the canonical view before
/// rebuilding the derived [`SessionInfo`].
pub fn load_session_info(session_id: &str) -> Result<SessionInfo> {
    let file = load_session_file(session_id)?;
    Ok(build_session_info(file))
}

pub(crate) fn load_session_info_from_file(path: &Path) -> Result<SessionInfo> {
    let file = load_session_file_from_path(path)?;
    Ok(build_session_info(file))
}

/// Load a session from disk and return the messages.
pub fn load_session(session_id: &str) -> Result<Vec<Message>> {
    let file = load_session_file(session_id)?;
    let messages = serializable_to_messages(&file.messages);
    debug!(
        session_id = session_id,
        messages = messages.len(),
        "session loaded"
    );
    Ok(messages)
}

/// Load the raw session file.
fn load_session_file(session_id: &str) -> Result<SessionFile> {
    let path = get_session_file(session_id);
    load_session_file_from_path(&path)
}

fn load_session_file_from_path(path: &Path) -> Result<SessionFile> {
    let contents = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read session file {}", path.display()))?;
    let file: SessionFile = serde_json::from_str(&contents)
        .with_context(|| format!("Failed to parse session file {}", path.display()))?;
    Ok(file)
}

/// List all available sessions, sorted by last_modified (most recent first).
pub fn list_sessions() -> Result<Vec<SessionInfo>> {
    let dir = get_session_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut sessions: Vec<SessionInfo> = Vec::new();

    for entry in std::fs::read_dir(&dir)
        .with_context(|| format!("Failed to read session directory {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();

        if path.extension().is_none_or(|ext| ext != "json") {
            continue;
        }

        let contents = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let file: SessionFile = match serde_json::from_str(&contents) {
            Ok(f) => f,
            Err(_) => continue,
        };

        sessions.push(build_session_info(file));
    }

    // Most recently modified first.
    sessions.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));

    debug!(count = sessions.len(), "sessions listed");

    Ok(sessions)
}

/// List sessions that belong to the same workspace/repository as `cwd`.
///
/// For git repositories, this groups together all worktrees that share the
/// same git common directory. For non-git directories, it falls back to the
/// stable workspace path (repo root if inside git, otherwise the exact path).
pub fn list_workspace_sessions(cwd: &Path) -> Result<Vec<SessionInfo>> {
    Ok(filter_sessions_for_workspace(list_sessions()?, cwd))
}

// ---------------------------------------------------------------------------
// Serialization helpers
// ---------------------------------------------------------------------------

/// Convert the internal `Message` enum to a serializable form.
fn messages_to_serializable(messages: &[Message]) -> Vec<SerializableMessage> {
    messages
        .iter()
        .map(|msg| {
            let (msg_type, data) = match msg {
                Message::User(u) => {
                    let content_value = match &u.content {
                        allthecodes_types::message::MessageContent::Text(t) => {
                            serde_json::json!(t)
                        }
                        allthecodes_types::message::MessageContent::Blocks(blocks) => {
                            serde_json::json!(blocks)
                        }
                    };
                    (
                        "user".to_string(),
                        serde_json::json!({
                            "content": content_value,
                            "is_meta": u.is_meta,
                        }),
                    )
                }
                Message::Assistant(a) => (
                    "assistant".to_string(),
                    serde_json::json!({
                        "content": a.content,
                        "stop_reason": a.stop_reason,
                        "cost_usd": a.cost_usd,
                        "usage": a.usage,
                    }),
                ),
                Message::System(s) => {
                    ("system".to_string(), system_message_to_serializable_data(s))
                }
                Message::Progress(p) => (
                    "progress".to_string(),
                    serde_json::json!({
                        "tool_use_id": p.tool_use_id,
                        "data": p.data,
                    }),
                ),
                Message::Attachment(a) => (
                    "attachment".to_string(),
                    serde_json::json!({
                        "attachment": a.attachment,
                    }),
                ),
            };
            SerializableMessage {
                msg_type,
                uuid: msg.uuid().to_string(),
                timestamp: msg.timestamp(),
                data,
            }
        })
        .collect()
}

fn system_message_to_serializable_data(
    system: &allthecodes_types::message::SystemMessage,
) -> serde_json::Value {
    use allthecodes_types::message::SystemSubtype;

    let mut data = serde_json::json!({
        "content": system.content,
    });
    match &system.subtype {
        SystemSubtype::CompactBoundary { compact_metadata } => {
            data["subtype"] = serde_json::json!("CompactBoundary");
            if let Some(metadata) = compact_metadata {
                data["compact_metadata"] = serde_json::json!(metadata);
            }
        }
        SystemSubtype::MicrocompactBoundary {
            microcompact_metadata,
        } => {
            data["subtype"] = serde_json::json!("MicrocompactBoundary");
            if let Some(metadata) = microcompact_metadata {
                data["microcompact_metadata"] = serde_json::json!(metadata);
            }
        }
        SystemSubtype::LocalCommand { content } => {
            data["subtype"] = serde_json::json!("LocalCommand");
            data["local_command_content"] = serde_json::json!(content);
        }
        SystemSubtype::Warning => {
            data["subtype"] = serde_json::json!("Warning");
        }
        _ => {}
    }
    data
}

/// Convert serializable messages back to `Message` instances.
///
/// This is a best-effort reconstruction. Fields that cannot be recovered from
/// the simplified serialization are set to defaults. A production
/// implementation would store the full typed data.
fn serializable_to_messages(msgs: &[SerializableMessage]) -> Vec<Message> {
    use allthecodes_types::message::*;
    use uuid::Uuid;

    msgs.iter()
        .filter_map(|sm| {
            let uuid = Uuid::parse_str(&sm.uuid).unwrap_or_else(|_| Uuid::new_v4());

            match sm.msg_type.as_str() {
                "user" => Some(Message::User(UserMessage {
                    uuid,
                    timestamp: sm.timestamp,
                    role: "user".into(),
                    content: match sm.data.get("content") {
                        Some(serde_json::Value::String(s)) => MessageContent::Text(s.clone()),
                        Some(serde_json::Value::Array(blocks)) => {
                            match serde_json::from_value::<
                                Vec<allthecodes_types::message::ContentBlock>,
                            >(serde_json::Value::Array(
                                blocks.clone(),
                            )) {
                                Ok(cb) => MessageContent::Blocks(cb),
                                Err(_) => MessageContent::Text(
                                    blocks
                                        .iter()
                                        .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                                        .collect::<Vec<_>>()
                                        .join("\n"),
                                ),
                            }
                        }
                        // Backwards compat: old Debug format like Text("hello")
                        _ => MessageContent::Text(
                            sm.data
                                .get("content")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                        ),
                    },
                    is_meta: sm
                        .data
                        .get("is_meta")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                    tool_use_result: None,
                    source_tool_assistant_uuid: None,
                })),
                "assistant" => Some(Message::Assistant(AssistantMessage {
                    uuid,
                    timestamp: sm.timestamp,
                    role: "assistant".into(),
                    content: sm
                        .data
                        .get("content")
                        .and_then(|v| {
                            serde_json::from_value::<Vec<allthecodes_types::message::ContentBlock>>(
                                v.clone(),
                            )
                            .ok()
                        })
                        .unwrap_or_default(),
                    usage: sm.data.get("usage").and_then(|v| {
                        serde_json::from_value::<allthecodes_types::message::Usage>(v.clone()).ok()
                    }),
                    stop_reason: sm
                        .data
                        .get("stop_reason")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    is_api_error_message: false,
                    api_error: None,
                    cost_usd: sm
                        .data
                        .get("cost_usd")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0),
                })),
                "system" => Some(Message::System(SystemMessage {
                    uuid,
                    timestamp: sm.timestamp,
                    subtype: system_subtype_from_serialized_data(&sm.data),
                    content: sm
                        .data
                        .get("content")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                })),
                _ => None,
            }
        })
        .collect()
}

fn system_subtype_from_serialized_data(
    data: &serde_json::Value,
) -> allthecodes_types::message::SystemSubtype {
    use allthecodes_types::message::{InfoLevel, SystemSubtype};

    match data.get("subtype").and_then(|v| v.as_str()) {
        Some("CompactBoundary") => SystemSubtype::CompactBoundary {
            compact_metadata: data
                .get("compact_metadata")
                .and_then(compact_metadata_from_value),
        },
        Some("LocalCommand") => SystemSubtype::LocalCommand {
            content: data
                .get("local_command_content")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
        },
        Some("Warning") => SystemSubtype::Warning,
        _ => SystemSubtype::Informational {
            level: InfoLevel::Info,
        },
    }
}

fn compact_metadata_from_value(
    value: &serde_json::Value,
) -> Option<allthecodes_types::message::CompactMetadata> {
    Some(allthecodes_types::message::CompactMetadata {
        pre_compact_token_count: value.get("pre_compact_token_count")?.as_u64()?,
        post_compact_token_count: value.get("post_compact_token_count")?.as_u64()?,
        preserved_segment: value
            .get("preserved_segment")
            .and_then(preserved_segment_from_value),
        pre_compact_discovered_tools: value
            .get("pre_compact_discovered_tools")
            .and_then(string_vec_from_value),
    })
}

fn preserved_segment_from_value(
    value: &serde_json::Value,
) -> Option<allthecodes_types::message::PreservedSegment> {
    let preserved_message_uuids = value
        .get("preserved_message_uuids")?
        .as_array()?
        .iter()
        .filter_map(|v| v.as_str().map(ToString::to_string))
        .collect::<Vec<_>>();

    Some(allthecodes_types::message::PreservedSegment {
        summary_message_uuid: value
            .get("summary_message_uuid")
            .and_then(|v| v.as_str())
            .map(ToString::to_string),
        preserved_message_uuids,
    })
}

fn string_vec_from_value(value: &serde_json::Value) -> Option<Vec<String>> {
    Some(
        value
            .as_array()?
            .iter()
            .filter_map(|v| v.as_str().map(ToString::to_string))
            .collect(),
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_types::message::{Message, MessageContent, SystemSubtype, UserMessage};
    use tempfile::tempdir;
    use uuid::Uuid;

    #[test]
    fn test_session_dir_path() {
        let dir = get_session_dir();
        let dir_str = dir.to_string_lossy();
        assert!(dir_str.contains("sessions"));
    }

    #[test]
    fn test_session_file_path() {
        let path = get_session_file("abc-123");
        assert!(path.to_string_lossy().ends_with("abc-123.json"));
    }

    #[test]
    fn test_stable_workspace_path_uses_git_root() {
        let temp = tempdir().unwrap();
        let repo_dir = temp.path().join("repo");
        std::fs::create_dir_all(repo_dir.join("target").join("release")).unwrap();
        Repository::init(&repo_dir).unwrap();

        let nested = repo_dir.join("target").join("release");
        assert_eq!(stable_workspace_path(&nested), repo_dir);
    }

    #[test]
    fn test_filter_sessions_for_workspace_matches_same_repo_subdirs() {
        let temp = tempdir().unwrap();
        let repo_dir = temp.path().join("repo");
        let nested_dir = repo_dir.join("target").join("release");
        let other_dir = temp.path().join("other");

        std::fs::create_dir_all(&nested_dir).unwrap();
        std::fs::create_dir_all(&other_dir).unwrap();
        Repository::init(&repo_dir).unwrap();

        fn info(id: &str, cwd: &Path, modified: i64, messages: usize) -> SessionInfo {
            SessionInfo {
                session_id: id.into(),
                created_at: 0,
                last_modified: modified,
                message_count: messages,
                cwd: normalize_display_path(cwd),
                title: String::new(),
                custom_title: None,
                workspace_key: workspace_key(cwd),
                workspace_root: workspace_root(cwd).to_string_lossy().to_string(),
                workspace_name: workspace_name(&workspace_root(cwd)),
            }
        }

        let sessions = vec![
            info("repo-root", &repo_dir, 3, 10),
            info("repo-nested", &nested_dir, 2, 8),
            info("other", &other_dir, 1, 2),
        ];

        let filtered = filter_sessions_for_workspace(sessions, &nested_dir);
        let ids: Vec<_> = filtered.into_iter().map(|s| s.session_id).collect();
        assert_eq!(ids, vec!["repo-root", "repo-nested"]);
    }

    // ------------------------------------------------------------------
    // Round-trip tests for the new title / truncate / info APIs. These
    // all pin ALLTHECODES_HOME to a tempdir and run serially so they cannot
    // stomp on each other or on the user's real session directory.
    // ------------------------------------------------------------------

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

    fn write_fixture_session(
        id: &str,
        messages: Vec<SerializableMessage>,
        cwd: &str,
    ) -> Result<()> {
        let file = SessionFile {
            session_id: id.into(),
            created_at: 1_700_000_000,
            last_modified: 1_700_000_000,
            cwd: cwd.into(),
            custom_title: None,
            messages,
        };
        std::fs::create_dir_all(get_session_dir())?;
        let json = serde_json::to_string_pretty(&file)?;
        std::fs::write(get_session_file(id), json)?;
        Ok(())
    }

    fn user_sm(text: &str, uuid: &str) -> SerializableMessage {
        SerializableMessage {
            msg_type: "user".into(),
            uuid: uuid.into(),
            timestamp: 0,
            data: serde_json::json!({ "content": text, "is_meta": false }),
        }
    }

    fn user_message(text: &str) -> Message {
        Message::User(UserMessage {
            uuid: Uuid::new_v4(),
            timestamp: 0,
            role: "user".into(),
            content: MessageContent::Text(text.into()),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        })
    }

    #[test]
    #[serial_test::serial]
    fn test_set_session_title_roundtrip() {
        let temp = tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        write_fixture_session(
            "s1",
            vec![user_sm(
                "hello world",
                "00000000-0000-0000-0000-000000000001",
            )],
            "/proj",
        )
        .unwrap();

        // Initially empty, derived title used.
        let info = load_session_info("s1").unwrap();
        assert_eq!(info.custom_title, None);
        assert_eq!(info.title, "hello world");

        // Set a custom title — preferred over derived.
        let stored = set_session_title("s1", Some("  Renamed  ")).unwrap();
        assert_eq!(stored.as_deref(), Some("Renamed"));
        let info = load_session_info("s1").unwrap();
        assert_eq!(info.custom_title.as_deref(), Some("Renamed"));
        assert_eq!(info.title, "Renamed");

        // save_session preserves custom_title even though the saver does not
        // know about it.
        save_session("s1", &[], "/proj").unwrap();
        let info = load_session_info("s1").unwrap();
        assert_eq!(info.custom_title.as_deref(), Some("Renamed"));

        // Clear via explicit None.
        let stored = set_session_title("s1", None).unwrap();
        assert_eq!(stored, None);
        let info = load_session_info("s1").unwrap();
        assert_eq!(info.custom_title, None);

        // Empty / whitespace also clears.
        set_session_title("s1", Some("x")).unwrap();
        let stored = set_session_title("s1", Some("   ")).unwrap();
        assert_eq!(stored, None);
    }

    #[test]
    #[serial_test::serial]
    fn test_set_session_title_truncates_long_input() {
        let temp = tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        write_fixture_session(
            "s2",
            vec![user_sm("x", "00000000-0000-0000-0000-000000000002")],
            "/proj",
        )
        .unwrap();

        let long = "a".repeat(MAX_CUSTOM_TITLE_LEN + 50);
        let stored = set_session_title("s2", Some(&long)).unwrap().unwrap();
        assert_eq!(stored.chars().count(), MAX_CUSTOM_TITLE_LEN);
    }

    #[test]
    #[serial_test::serial]
    fn test_truncate_session_keeps_prefix_and_writes_backup() {
        let temp = tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        let messages = (0..5)
            .map(|i| {
                user_sm(
                    &format!("msg {}", i),
                    &format!("00000000-0000-0000-0000-00000000000{}", i),
                )
            })
            .collect();
        write_fixture_session("s3", messages, "/proj").unwrap();

        let new_len = truncate_session("s3", 2).unwrap();
        assert_eq!(new_len, 2);

        let info = load_session_info("s3").unwrap();
        assert_eq!(info.message_count, 2);

        // Backup file must exist alongside the session.
        let backup_count = std::fs::read_dir(get_session_dir())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().starts_with("s3.rewind-"))
            .count();
        assert_eq!(backup_count, 1);
    }

    #[test]
    #[serial_test::serial]
    fn test_truncate_session_noop_when_keep_exceeds_len() {
        let temp = tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        write_fixture_session(
            "s4",
            vec![user_sm("only", "00000000-0000-0000-0000-000000000010")],
            "/proj",
        )
        .unwrap();

        let new_len = truncate_session("s4", 99).unwrap();
        assert_eq!(new_len, 1);
        let backups: Vec<_> = std::fs::read_dir(get_session_dir())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().starts_with("s4.rewind-"))
            .collect();
        assert!(backups.is_empty(), "expected no backup when no truncation");
    }

    #[test]
    #[serial_test::serial]
    fn test_archive_session_moves_file_out_of_default_list() {
        let temp = tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        write_fixture_session(
            "archive-move",
            vec![user_sm(
                "archive me",
                "00000000-0000-0000-0000-000000000020",
            )],
            "/proj",
        )
        .unwrap();

        archive_session("archive-move").unwrap();

        assert!(!get_session_file("archive-move").exists());
        assert!(get_archived_session_file("archive-move").exists());
        let listed_ids: Vec<_> = list_sessions()
            .unwrap()
            .into_iter()
            .map(|session| session.session_id)
            .collect();
        assert!(!listed_ids.iter().any(|id| id == "archive-move"));
    }

    #[test]
    #[serial_test::serial]
    fn test_archive_session_missing_file_returns_error() {
        let temp = tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        let error = archive_session("missing-archive").unwrap_err();
        let message = error.to_string();

        assert!(message.contains("missing-archive"));
    }

    #[test]
    #[serial_test::serial]
    fn test_archive_session_does_not_overwrite_existing_archive() {
        let temp = tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        write_fixture_session(
            "archive-conflict",
            vec![user_sm("top level", "00000000-0000-0000-0000-000000000021")],
            "/proj",
        )
        .unwrap();
        std::fs::create_dir_all(get_archived_session_dir()).unwrap();
        std::fs::write(get_archived_session_file("archive-conflict"), "existing").unwrap();

        archive_session("archive-conflict").unwrap();

        assert!(get_archived_session_file("archive-conflict").exists());
        let archived: Vec<_> = std::fs::read_dir(get_archived_session_dir())
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .collect();

        assert!(archived.iter().any(|name| name == "archive-conflict.json"));
        assert!(archived.iter().any(|name| {
            name.starts_with("archive-conflict.archived-") && name.ends_with(".json")
        }));
        assert_eq!(archived.len(), 2);
    }

    #[test]
    #[serial_test::serial]
    fn test_partial_compact_resume_roundtrip_preserves_boundary_metadata() {
        let temp = tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        let anchor = user_message("anchor");
        let anchor_uuid = anchor.uuid().to_string();
        let tail = user_message("tail");
        let tail_uuid = tail.uuid().to_string();
        let messages = vec![
            user_message(&format!("old {}", "x ".repeat(500))),
            anchor,
            tail,
        ];
        let result = allthecodes_compact::partial_compact::partial_compact(
            messages,
            &allthecodes_compact::partial_compact::PartialCompactConfig {
                anchor_uuid: anchor_uuid.clone(),
                direction: allthecodes_compact::partial_compact::PartialCompactDirection::UpTo,
                summary: "old segment summary".into(),
            },
        )
        .expect("partial compact should apply");

        save_session("partial-roundtrip", &result.messages, "/proj").unwrap();
        let loaded = load_session("partial-roundtrip").unwrap();

        assert_eq!(loaded.len(), result.messages.len());
        assert!(!loaded.iter().any(|message| {
            matches!(
                message,
                Message::User(UserMessage {
                    content: MessageContent::Text(text),
                    is_meta: false,
                    ..
                }) if text.contains("old ")
            )
        }));

        let metadata = loaded
            .iter()
            .find_map(|message| {
                if let Message::System(system) = message {
                    if let SystemSubtype::CompactBoundary {
                        compact_metadata: Some(metadata),
                    } = &system.subtype
                    {
                        return Some(metadata);
                    }
                }
                None
            })
            .expect("compact boundary metadata should round trip");
        let segment = metadata
            .preserved_segment
            .as_ref()
            .expect("preserved segment should round trip");

        assert!(segment.summary_message_uuid.is_some());
        assert_eq!(
            segment.preserved_message_uuids,
            vec![anchor_uuid, tail_uuid]
        );
    }

    #[cfg(windows)]
    #[test]
    fn test_workspace_key_is_case_insensitive_on_windows() {
        let temp = tempdir().unwrap();
        let repo_dir = temp.path().join("Repo");
        std::fs::create_dir_all(&repo_dir).unwrap();
        Repository::init(&repo_dir).unwrap();

        let upper = repo_dir.to_string_lossy().to_uppercase().replace('\\', "/");
        let lower = repo_dir.to_string_lossy().to_lowercase();

        assert_eq!(
            workspace_key(Path::new(&upper)),
            workspace_key(Path::new(&lower))
        );
    }
}
