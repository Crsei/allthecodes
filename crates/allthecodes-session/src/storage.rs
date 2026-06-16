//! Session storage -- persisting conversation state to SQLite and JSON.
//!
//! New writes prefer the shared SQLite state database while retaining JSON
//! files under `~/.allthecodes/sessions/` for migration compatibility and
//! file-based tooling.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Utc;
use git2::Repository;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

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
    /// Optional per-session chat mode override. `None` means the session
    /// follows its workspace default mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_mode_override: Option<String>,
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

/// Opaque keyset cursor for paginated session listing.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionListCursor {
    pub last_modified: i64,
    pub created_at: i64,
    pub session_id: String,
}

/// One page of session metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionListPage {
    pub sessions: Vec<SessionInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<SessionListCursor>,
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
    /// Optional per-session chat mode override. `None` means this session
    /// follows the workspace default mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_mode_override: Option<String>,
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
        chat_mode_override: file.chat_mode_override,
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
    let now = Utc::now().timestamp();

    // Preserve metadata fields when updating.
    let (created_at, custom_title, chat_mode_override) =
        match load_existing_session_file_for_path(session_id, path) {
            Some(file) => (file.created_at, file.custom_title, file.chat_mode_override),
            None => (now, None, None),
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
        chat_mode_override,
        messages: serializable_messages,
    };

    persist_session_file_for_path(session_id, &session_file, path)?;

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
    let mut file = load_session_file_for_path(session_id, path)?;

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

    persist_session_file_for_path(session_id, &file, path)?;

    debug!(
        session_id = session_id,
        title = ?new_title,
        "session title updated"
    );

    Ok(new_title)
}

/// Set or clear the per-session chat mode override.
///
/// `mode = None` clears the override, so callers should treat the session as
/// following its workspace default mode. When the session file does not yet
/// exist, a metadata-only file is created with no messages so later saves can
/// preserve the override.
pub fn set_session_chat_mode_override(
    session_id: &str,
    mode: Option<&str>,
    cwd: &str,
) -> Result<Option<String>> {
    let path = get_session_file(session_id);
    set_session_chat_mode_override_in_file(session_id, mode, cwd, &path)
}

pub(crate) fn set_session_chat_mode_override_in_file(
    session_id: &str,
    mode: Option<&str>,
    cwd: &str,
    path: &Path,
) -> Result<Option<String>> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("Failed to create session directory {}", dir.display()))?;
    }

    let now = Utc::now().timestamp();
    let mut file = match load_existing_session_file_for_path(session_id, path) {
        Some(file) => file,
        None => SessionFile {
            session_id: session_id.to_string(),
            created_at: now,
            last_modified: now,
            cwd: normalize_display_path(&stable_workspace_path(Path::new(cwd))),
            custom_title: None,
            chat_mode_override: None,
            messages: Vec::new(),
        },
    };

    let new_mode = mode.and_then(|raw| {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    });

    file.chat_mode_override = new_mode.clone();
    file.last_modified = now;

    persist_session_file_for_path(session_id, &file, path)?;

    debug!(
        session_id = session_id,
        chat_mode_override = ?new_mode,
        "session chat mode override updated"
    );

    Ok(new_mode)
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
    persist_session_file_for_path(session_id, &file, &path)?;

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
    let archived_in_sqlite = archive_session_in_sqlite(session_id);
    if !src.exists() && !archived_in_sqlite {
        anyhow::bail!(
            "Session file for {} does not exist at {}",
            session_id,
            src.display()
        );
    }
    if !src.exists() {
        debug!(session_id = session_id, "session archived in sqlite");
        return Ok(());
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
    load_session_file_for_path(session_id, &path)
}

fn load_session_file_for_path(session_id: &str, path: &Path) -> Result<SessionFile> {
    if is_default_session_path(session_id, path) {
        #[cfg(feature = "sqlite-storage")]
        match sqlite_store::lookup_session_file(session_id) {
            Ok(sqlite_store::SessionLookup::Active(file)) => return Ok(file),
            Ok(sqlite_store::SessionLookup::Archived) => {
                anyhow::bail!("Session {} is archived", session_id);
            }
            Ok(sqlite_store::SessionLookup::Missing) => {}
            Err(err) => {
                warn!(
                    session_id,
                    error = %err,
                    "failed to load session from sqlite; falling back to JSON session file"
                );
            }
        }
    }

    load_session_file_from_path(path)
}

fn load_existing_session_file_for_path(session_id: &str, path: &Path) -> Option<SessionFile> {
    load_session_file_for_path(session_id, path).ok()
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
    #[cfg(feature = "sqlite-storage")]
    match sqlite_store::list_session_index() {
        Ok(index) => {
            let mut sessions = index.sessions;
            let mut excluded_ids: HashSet<String> = sessions
                .iter()
                .map(|session| session.session_id.clone())
                .collect();
            excluded_ids.extend(index.archived_ids);

            sessions.extend(list_json_sessions_excluding(&excluded_ids)?);
            sort_session_infos(&mut sessions);

            debug!(
                count = sessions.len(),
                "sessions listed from sqlite and JSON fallback"
            );
            return Ok(sessions);
        }
        Err(err) => {
            warn!(
                error = %err,
                "failed to list sessions from sqlite; falling back to JSON session directory"
            );
        }
    }

    let mut sessions = list_json_sessions_excluding(&HashSet::new())?;
    sort_session_infos(&mut sessions);
    debug!(count = sessions.len(), "sessions listed from JSON");
    Ok(sessions)
}

/// List one page of active sessions using stable keyset ordering.
///
/// Ordering is always `last_modified DESC, created_at DESC, session_id ASC`.
/// `limit` is clamped to `1..=200`.
pub fn list_sessions_page(
    limit: usize,
    cursor: Option<SessionListCursor>,
) -> Result<SessionListPage> {
    let limit = clamp_session_page_limit(limit);

    #[cfg(feature = "sqlite-storage")]
    match sqlite_store::list_session_page(limit, cursor.clone()) {
        Ok(page) => return Ok(page),
        Err(err) => {
            warn!(
                error = %err,
                "failed to page sessions from sqlite; falling back to JSON session directory"
            );
        }
    }

    let sessions = list_sessions()?;
    Ok(page_sessions_in_memory(sessions, limit, cursor.as_ref()))
}

/// List one page of sessions in the same workspace/repository as `cwd`.
pub fn list_workspace_sessions_page(
    cwd: &Path,
    limit: usize,
    cursor: Option<SessionListCursor>,
) -> Result<SessionListPage> {
    let limit = clamp_session_page_limit(limit);

    #[cfg(feature = "sqlite-storage")]
    match sqlite_store::list_workspace_session_page(cwd, limit, cursor.clone()) {
        Ok(page) => return Ok(page),
        Err(err) => {
            warn!(
                error = %err,
                cwd = %cwd.display(),
                "failed to page workspace sessions from sqlite; falling back to JSON session directory"
            );
        }
    }

    let sessions = filter_sessions_for_workspace(list_sessions()?, cwd);
    Ok(page_sessions_in_memory(sessions, limit, cursor.as_ref()))
}

fn list_json_sessions_excluding(excluded_ids: &HashSet<String>) -> Result<Vec<SessionInfo>> {
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
            Err(err) => {
                warn!(
                    path = %path.display(),
                    error = %err,
                    "skipping unreadable legacy session JSON"
                );
                continue;
            }
        };

        let file: SessionFile = match serde_json::from_str(&contents) {
            Ok(f) => f,
            Err(err) => {
                warn!(
                    path = %path.display(),
                    error = %err,
                    "skipping invalid legacy session JSON"
                );
                continue;
            }
        };

        if excluded_ids.contains(&file.session_id) {
            continue;
        }

        sessions.push(build_session_info(file));
    }

    Ok(sessions)
}

fn sort_session_infos(sessions: &mut [SessionInfo]) {
    sessions.sort_by(|a, b| {
        b.last_modified
            .cmp(&a.last_modified)
            .then_with(|| b.created_at.cmp(&a.created_at))
            .then_with(|| a.session_id.cmp(&b.session_id))
    });
}

fn clamp_session_page_limit(limit: usize) -> usize {
    limit.clamp(1, 200)
}

fn cursor_for_session(session: &SessionInfo) -> SessionListCursor {
    SessionListCursor {
        last_modified: session.last_modified,
        created_at: session.created_at,
        session_id: session.session_id.clone(),
    }
}

fn session_is_after_cursor(session: &SessionInfo, cursor: &SessionListCursor) -> bool {
    session.last_modified < cursor.last_modified
        || (session.last_modified == cursor.last_modified && session.created_at < cursor.created_at)
        || (session.last_modified == cursor.last_modified
            && session.created_at == cursor.created_at
            && session.session_id > cursor.session_id)
}

fn page_sessions_in_memory(
    mut sessions: Vec<SessionInfo>,
    limit: usize,
    cursor: Option<&SessionListCursor>,
) -> SessionListPage {
    sort_session_infos(&mut sessions);
    if let Some(cursor) = cursor {
        sessions.retain(|session| session_is_after_cursor(session, cursor));
    }
    let next_cursor = if sessions.len() > limit {
        Some(cursor_for_session(&sessions[limit - 1]))
    } else {
        None
    };
    sessions.truncate(limit);
    SessionListPage {
        sessions,
        next_cursor,
    }
}

fn persist_session_file_for_path(
    session_id: &str,
    session_file: &SessionFile,
    path: &Path,
) -> Result<()> {
    if is_default_session_path(session_id, path) {
        #[cfg(feature = "sqlite-storage")]
        if let Err(err) = sqlite_store::save_session_file(session_file) {
            warn!(
                session_id,
                error = %err,
                "failed to save session to sqlite; falling back to JSON session file"
            );
        }
    }

    write_session_file_to_path(session_file, path)
}

fn write_session_file_to_path(session_file: &SessionFile, path: &Path) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("Failed to create session directory {}", dir.display()))?;
    }

    let json = serde_json::to_string_pretty(session_file).context("Failed to serialize session")?;
    std::fs::write(path, json)
        .with_context(|| format!("Failed to write session file {}", path.display()))
}

fn is_default_session_path(session_id: &str, path: &Path) -> bool {
    path == get_session_file(session_id)
}

fn archive_session_in_sqlite(session_id: &str) -> bool {
    #[cfg(feature = "sqlite-storage")]
    {
        match sqlite_store::archive_session(session_id) {
            Ok(archived) => return archived,
            Err(err) => {
                warn!(
                    session_id,
                    error = %err,
                    "failed to archive session in sqlite; falling back to JSON archive"
                );
            }
        }
    }
    false
}

/// List sessions that belong to the same workspace/repository as `cwd`.
///
/// For git repositories, this groups together all worktrees that share the
/// same git common directory. For non-git directories, it falls back to the
/// stable workspace path (repo root if inside git, otherwise the exact path).
pub fn list_workspace_sessions(cwd: &Path) -> Result<Vec<SessionInfo>> {
    Ok(filter_sessions_for_workspace(list_sessions()?, cwd))
}

#[cfg(feature = "sqlite-storage")]
mod sqlite_store {
    use super::*;
    use allthecodes_db::{Migration, MigrationRunner};
    use sqlx::{Row, SqlitePool};

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
    #[cfg(feature = "sqlite-storage")]
    use sqlx::Row;
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
                chat_mode_override: None,
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
            chat_mode_override: None,
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

    #[cfg(feature = "sqlite-storage")]
    fn query_sqlite_counts() -> Result<(i64, i64)> {
        allthecodes_db::run_sqlite_sync("allthecodes-session-test-sqlite", async move {
            let pool = allthecodes_db::DbPoolManager::new().state_pool()?;
            let sessions: i64 = sqlx::query("SELECT COUNT(*) FROM sessions")
                .fetch_one(&pool)
                .await?
                .try_get(0)?;
            let messages: i64 = sqlx::query("SELECT COUNT(*) FROM session_messages")
                .fetch_one(&pool)
                .await?
                .try_get(0)?;
            Ok((sessions, messages))
        })
    }

    #[cfg(feature = "sqlite-storage")]
    #[test]
    #[serial_test::serial]
    fn test_sqlite_session_save_creates_rows_and_load_roundtrips() {
        let temp = tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        save_session("sqlite-roundtrip", &[user_message("hello sqlite")], "/proj").unwrap();

        assert!(temp.path().join("state").join("state_5.sqlite").exists());
        let (sessions, messages) = query_sqlite_counts().unwrap();
        assert_eq!(sessions, 1);
        assert_eq!(messages, 1);

        let loaded = load_session("sqlite-roundtrip").unwrap();
        assert_eq!(loaded.len(), 1);
        assert!(matches!(
            &loaded[0],
            Message::User(UserMessage {
                content: MessageContent::Text(text),
                ..
            }) if text == "hello sqlite"
        ));
    }

    #[cfg(feature = "sqlite-storage")]
    #[test]
    #[serial_test::serial]
    fn test_sqlite_list_sessions_uses_last_modified_order() {
        let temp = tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        let older = SessionFile {
            session_id: "older-sqlite".into(),
            created_at: 10,
            last_modified: 10,
            cwd: "/proj".into(),
            custom_title: Some("Older".into()),
            chat_mode_override: None,
            messages: vec![user_sm("older", "00000000-0000-0000-0000-000000000201")],
        };
        let newer = SessionFile {
            session_id: "newer-sqlite".into(),
            created_at: 20,
            last_modified: 20,
            cwd: "/proj".into(),
            custom_title: Some("Newer".into()),
            chat_mode_override: None,
            messages: vec![user_sm("newer", "00000000-0000-0000-0000-000000000202")],
        };
        persist_session_file_for_path("older-sqlite", &older, &get_session_file("older-sqlite"))
            .unwrap();
        persist_session_file_for_path("newer-sqlite", &newer, &get_session_file("newer-sqlite"))
            .unwrap();

        let ids: Vec<_> = list_sessions()
            .unwrap()
            .into_iter()
            .map(|session| session.session_id)
            .collect();
        assert_eq!(ids, vec!["newer-sqlite", "older-sqlite"]);
    }

    #[cfg(feature = "sqlite-storage")]
    #[test]
    #[serial_test::serial]
    fn test_list_sessions_includes_json_only_sessions_when_sqlite_exists() {
        let temp = tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        save_session("sqlite-listed", &[user_message("sqlite")], "/proj").unwrap();
        write_fixture_session(
            "json-listed",
            vec![user_sm("json", "00000000-0000-0000-0000-000000000204")],
            "/proj",
        )
        .unwrap();

        let ids: Vec<_> = list_sessions()
            .unwrap()
            .into_iter()
            .map(|session| session.session_id)
            .collect();
        assert!(ids.iter().any(|id| id == "sqlite-listed"));
        assert!(ids.iter().any(|id| id == "json-listed"));
    }

    #[cfg(feature = "sqlite-storage")]
    #[test]
    #[serial_test::serial]
    fn test_list_sessions_prefers_sqlite_metadata_for_duplicate_json_id() {
        let temp = tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        let sql_file = SessionFile {
            session_id: "duplicate-session".into(),
            created_at: 10,
            last_modified: 20,
            cwd: "/proj".into(),
            custom_title: Some("SQLite title".into()),
            chat_mode_override: Some("sqlite-mode".into()),
            messages: vec![user_sm(
                "sqlite text",
                "00000000-0000-0000-0000-000000000205",
            )],
        };
        persist_session_file_for_path(
            "duplicate-session",
            &sql_file,
            &get_session_file("duplicate-session"),
        )
        .unwrap();

        let json_file = SessionFile {
            session_id: "duplicate-session".into(),
            created_at: 1,
            last_modified: 1,
            cwd: "/proj".into(),
            custom_title: Some("JSON title".into()),
            chat_mode_override: Some("json-mode".into()),
            messages: vec![user_sm("json text", "00000000-0000-0000-0000-000000000206")],
        };
        write_session_file_to_path(&json_file, &get_session_file("duplicate-session")).unwrap();

        let listed: Vec<_> = list_sessions()
            .unwrap()
            .into_iter()
            .filter(|session| session.session_id == "duplicate-session")
            .collect();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].title, "SQLite title");
        assert_eq!(listed[0].chat_mode_override.as_deref(), Some("sqlite-mode"));
    }

    #[cfg(feature = "sqlite-storage")]
    #[test]
    #[serial_test::serial]
    fn test_list_sessions_skips_json_when_sqlite_marks_session_archived() {
        let temp = tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        save_session(
            "archived-with-json",
            &[user_message("still has json")],
            "/proj",
        )
        .unwrap();
        sqlite_store::archive_session("archived-with-json").unwrap();

        assert!(get_session_file("archived-with-json").exists());
        let ids: Vec<_> = list_sessions()
            .unwrap()
            .into_iter()
            .map(|session| session.session_id)
            .collect();
        assert!(!ids.iter().any(|id| id == "archived-with-json"));
    }

    #[cfg(feature = "sqlite-storage")]
    #[test]
    #[serial_test::serial]
    fn test_load_session_skips_json_when_sqlite_marks_session_archived() {
        let temp = tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        save_session(
            "archived-load-with-json",
            &[user_message("still has json")],
            "/proj",
        )
        .unwrap();
        sqlite_store::archive_session("archived-load-with-json").unwrap();

        assert!(get_session_file("archived-load-with-json").exists());
        let error = load_session("archived-load-with-json").unwrap_err();
        assert!(error.to_string().contains("archived-load-with-json"));
    }

    #[cfg(feature = "sqlite-storage")]
    #[test]
    #[serial_test::serial]
    fn test_list_workspace_sessions_merges_sqlite_and_json_only_sessions() {
        let temp = tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());
        let project = temp.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        let cwd = project.to_string_lossy().to_string();

        save_session("workspace-sqlite", &[user_message("sqlite")], &cwd).unwrap();
        write_fixture_session(
            "workspace-json",
            vec![user_sm("json", "00000000-0000-0000-0000-000000000207")],
            &normalize_display_path(&project),
        )
        .unwrap();

        let ids: Vec<_> = list_workspace_sessions(&project)
            .unwrap()
            .into_iter()
            .map(|session| session.session_id)
            .collect();
        assert!(ids.iter().any(|id| id == "workspace-sqlite"));
        assert!(ids.iter().any(|id| id == "workspace-json"));
    }

    #[cfg(feature = "sqlite-storage")]
    #[test]
    #[serial_test::serial]
    fn test_list_sessions_page_imports_legacy_json_and_uses_cursor_order() {
        let temp = tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        for (id, created_at, last_modified) in
            [("page-c", 30, 30), ("page-a", 10, 20), ("page-b", 15, 20)]
        {
            let file = SessionFile {
                session_id: id.into(),
                created_at,
                last_modified,
                cwd: "/proj".into(),
                custom_title: Some(id.into()),
                chat_mode_override: None,
                messages: vec![user_sm(id, "00000000-0000-0000-0000-000000000301")],
            };
            write_session_file_to_path(&file, &get_session_file(id)).unwrap();
        }
        std::fs::write(get_session_dir().join("broken.json"), "{").unwrap();

        let first = list_sessions_page(2, None).unwrap();
        let first_ids: Vec<_> = first
            .sessions
            .iter()
            .map(|session| session.session_id.as_str())
            .collect();
        assert_eq!(first_ids, vec!["page-c", "page-b"]);
        assert!(first.next_cursor.is_some());

        let second = list_sessions_page(2000, first.next_cursor).unwrap();
        let second_ids: Vec<_> = second
            .sessions
            .iter()
            .map(|session| session.session_id.as_str())
            .collect();
        assert_eq!(second_ids, vec!["page-a"]);
        assert!(second.next_cursor.is_none());

        let (sessions, messages) = query_sqlite_counts().unwrap();
        assert_eq!(sessions, 3);
        assert_eq!(messages, 3);
    }

    #[cfg(feature = "sqlite-storage")]
    #[test]
    #[serial_test::serial]
    fn test_list_workspace_sessions_page_filters_before_cursoring() {
        let temp = tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());
        let project = temp.path().join("project");
        let other = temp.path().join("other");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&other).unwrap();

        save_session(
            "workspace-page-1",
            &[user_message("one")],
            &project.to_string_lossy(),
        )
        .unwrap();
        save_session(
            "workspace-page-2",
            &[user_message("two")],
            &project.to_string_lossy(),
        )
        .unwrap();
        save_session(
            "workspace-page-other",
            &[user_message("other")],
            &other.to_string_lossy(),
        )
        .unwrap();

        let page = list_workspace_sessions_page(&project, 10, None).unwrap();
        let ids: Vec<_> = page
            .sessions
            .into_iter()
            .map(|session| session.session_id)
            .collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.iter().all(|id| id.starts_with("workspace-page-")));
        assert!(!ids.iter().any(|id| id == "workspace-page-other"));
    }

    #[cfg(feature = "sqlite-storage")]
    #[test]
    #[serial_test::serial]
    fn test_list_sessions_falls_back_to_json_when_sqlite_path_is_blocked() {
        let temp = tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());
        std::fs::create_dir_all(temp.path().join("state").join("state_5.sqlite")).unwrap();
        write_fixture_session(
            "json-list-fallback",
            vec![user_sm("fallback", "00000000-0000-0000-0000-000000000208")],
            "/proj",
        )
        .unwrap();

        let ids: Vec<_> = list_sessions()
            .unwrap()
            .into_iter()
            .map(|session| session.session_id)
            .collect();
        assert_eq!(ids, vec!["json-list-fallback"]);
    }

    #[cfg(feature = "sqlite-storage")]
    #[test]
    #[serial_test::serial]
    fn test_json_only_session_loads_when_sqlite_misses() {
        let temp = tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        write_fixture_session(
            "json-only",
            vec![user_sm(
                "legacy json",
                "00000000-0000-0000-0000-000000000203",
            )],
            "/proj",
        )
        .unwrap();

        let loaded = load_session("json-only").unwrap();
        assert_eq!(loaded.len(), 1);
        assert!(matches!(
            &loaded[0],
            Message::User(UserMessage {
                content: MessageContent::Text(text),
                ..
            }) if text == "legacy json"
        ));
    }

    #[cfg(feature = "sqlite-storage")]
    #[test]
    #[serial_test::serial]
    fn test_session_save_falls_back_to_json_when_sqlite_path_is_blocked() {
        let temp = tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());
        std::fs::create_dir_all(temp.path().join("state").join("state_5.sqlite")).unwrap();

        save_session("blocked-sqlite", &[user_message("json survives")], "/proj").unwrap();

        assert!(get_session_file("blocked-sqlite").exists());
        let loaded = load_session("blocked-sqlite").unwrap();
        assert_eq!(loaded.len(), 1);
        assert!(matches!(
            &loaded[0],
            Message::User(UserMessage {
                content: MessageContent::Text(text),
                ..
            }) if text == "json survives"
        ));
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
    fn test_set_session_chat_mode_override_roundtrip() {
        let temp = tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        write_fixture_session(
            "mode-session",
            vec![user_sm("hello", "00000000-0000-0000-0000-000000000101")],
            "/proj",
        )
        .unwrap();

        let stored =
            set_session_chat_mode_override("mode-session", Some("eco-boost"), "/proj").unwrap();
        assert_eq!(stored.as_deref(), Some("eco-boost"));
        let info = load_session_info("mode-session").unwrap();
        assert_eq!(info.chat_mode_override.as_deref(), Some("eco-boost"));

        save_session("mode-session", &[], "/proj").unwrap();
        let info = load_session_info("mode-session").unwrap();
        assert_eq!(info.chat_mode_override.as_deref(), Some("eco-boost"));

        let stored = set_session_chat_mode_override("mode-session", None, "/proj").unwrap();
        assert_eq!(stored, None);
        let info = load_session_info("mode-session").unwrap();
        assert_eq!(info.chat_mode_override, None);
    }

    #[test]
    #[serial_test::serial]
    fn test_set_session_chat_mode_override_creates_metadata_file() {
        let temp = tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        set_session_chat_mode_override("new-mode-session", Some("normal"), "/proj").unwrap();

        let info = load_session_info("new-mode-session").unwrap();
        assert_eq!(info.chat_mode_override.as_deref(), Some("normal"));
        assert_eq!(info.message_count, 0);
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

    #[cfg(feature = "sqlite-storage")]
    #[test]
    #[serial_test::serial]
    fn test_archive_session_hides_sqlite_session_and_keeps_archive_json() {
        let temp = tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        save_session("archive-sqlite", &[user_message("archive")], "/proj").unwrap();
        archive_session("archive-sqlite").unwrap();

        assert!(!get_session_file("archive-sqlite").exists());
        assert!(get_archived_session_file("archive-sqlite").exists());
        let listed_ids: Vec<_> = list_sessions()
            .unwrap()
            .into_iter()
            .map(|session| session.session_id)
            .collect();
        assert!(!listed_ids.iter().any(|id| id == "archive-sqlite"));
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
