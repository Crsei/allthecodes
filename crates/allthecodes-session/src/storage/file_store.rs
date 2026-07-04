use std::collections::HashSet;
use std::path::{Path, PathBuf};

use allthecodes_types::message::Message;
use anyhow::{Context, Result};
use chrono::Utc;
use tracing::{debug, warn};

use super::serialization::{messages_to_serializable, serializable_to_messages};
use super::session_search::{json_search_results, SessionSearchResult};
#[cfg(feature = "sqlite-storage")]
use super::sqlite_store;
use super::{
    build_session_info, filter_sessions_for_workspace, get_archived_session_dir,
    get_archived_session_file, get_session_dir, get_session_file, normalize_display_path,
    stable_workspace_path, workspace_key, SessionFile, SessionInfo, SessionListCursor,
    SessionListPage,
};

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

pub(super) fn load_session_file_from_path(path: &Path) -> Result<SessionFile> {
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

/// Search saved sessions across all workspaces.
///
/// SQLite is used when available. If the state database cannot be opened or
/// queried, this falls back to scanning legacy JSON session files.
pub fn search_sessions(query: &str, limit: usize) -> Result<Vec<SessionSearchResult>> {
    #[cfg(feature = "sqlite-storage")]
    match sqlite_store::search_session_messages(query, limit, None) {
        Ok(results) => return Ok(results),
        Err(err) => {
            warn!(
                error = %err,
                "failed to search sessions from sqlite; falling back to JSON session directory"
            );
        }
    }

    json_search_results(query, limit, None, &HashSet::new())
}

/// Search saved sessions that already belong to `workspace_key`.
pub fn search_sessions_by_workspace_key(
    workspace_key: &str,
    query: &str,
    limit: usize,
) -> Result<Vec<SessionSearchResult>> {
    let workspace_key = workspace_key.trim();
    if workspace_key.is_empty() {
        return Ok(Vec::new());
    }

    #[cfg(feature = "sqlite-storage")]
    match sqlite_store::search_session_messages(query, limit, Some(workspace_key.to_string())) {
        Ok(results) => return Ok(results),
        Err(err) => {
            warn!(
                error = %err,
                workspace_key,
                "failed to search workspace-key sessions from sqlite; falling back to JSON session directory"
            );
        }
    }

    json_search_results(query, limit, Some(workspace_key), &HashSet::new())
}

/// Search saved sessions in the same workspace/repository as `cwd`.
pub fn search_workspace_sessions(
    cwd: &Path,
    query: &str,
    limit: usize,
) -> Result<Vec<SessionSearchResult>> {
    let current_workspace = workspace_key(cwd);

    #[cfg(feature = "sqlite-storage")]
    match sqlite_store::search_session_messages(query, limit, Some(current_workspace.clone())) {
        Ok(results) => return Ok(results),
        Err(err) => {
            warn!(
                error = %err,
                cwd = %cwd.display(),
                "failed to search workspace sessions from sqlite; falling back to JSON session directory"
            );
        }
    }

    json_search_results(query, limit, Some(&current_workspace), &HashSet::new())
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

pub(super) fn cursor_for_session(session: &SessionInfo) -> SessionListCursor {
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

pub(super) fn persist_session_file_for_path(
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

pub(super) fn write_session_file_to_path(session_file: &SessionFile, path: &Path) -> Result<()> {
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
    #[cfg(not(feature = "sqlite-storage"))]
    let _ = session_id;
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
