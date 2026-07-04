//! Session storage -- persisting conversation state to SQLite and JSON.
//!
//! New writes prefer the shared SQLite state database while retaining JSON
//! files under `~/.allthecodes/sessions/` for migration compatibility and
//! file-based tooling.

use std::path::{Path, PathBuf};

use git2::Repository;
use serde::{Deserialize, Serialize};
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

mod file_store;
mod serialization;
mod session_search;
#[cfg(feature = "sqlite-storage")]
mod sqlite_store;

pub(crate) use serialization::serializable_to_messages;
pub use session_search::SessionSearchResult;

pub use file_store::{
    archive_session, list_sessions, list_sessions_page, list_workspace_sessions,
    list_workspace_sessions_page, load_session, load_session_info, save_session, search_sessions,
    search_sessions_by_workspace_key, search_workspace_sessions, set_session_chat_mode_override,
    set_session_title, truncate_session, MAX_CUSTOM_TITLE_LEN,
};
pub(crate) use file_store::{
    load_session_info_from_file, save_session_to_file, set_session_title_in_file,
};

#[allow(dead_code)]
pub(crate) fn set_session_chat_mode_override_in_file(
    session_id: &str,
    mode: Option<&str>,
    cwd: &str,
    path: &Path,
) -> anyhow::Result<Option<String>> {
    file_store::set_session_chat_mode_override_in_file(session_id, mode, cwd, path)
}

#[cfg(all(test, feature = "sqlite-storage"))]
use file_store::{persist_session_file_for_path, write_session_file_to_path};
// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_types::message::{
        AssistantMessage, Attachment, AttachmentMessage, ContentBlock, Message, MessageContent,
        ProgressMessage, SystemMessage, SystemSubtype, ToolResultContent, UserMessage,
    };
    use anyhow::Result;
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

    fn phase0_legacy_serializable_messages() -> Vec<SerializableMessage> {
        vec![
            SerializableMessage {
                msg_type: "user".into(),
                uuid: "10000000-0000-0000-0000-000000000001".into(),
                timestamp: 1,
                data: serde_json::json!({
                    "content": "phase0 user text",
                    "is_meta": false
                }),
            },
            SerializableMessage {
                msg_type: "assistant".into(),
                uuid: "10000000-0000-0000-0000-000000000002".into(),
                timestamp: 2,
                data: serde_json::json!({
                    "content": [
                        { "type": "text", "text": "phase0 assistant text" },
                        {
                            "type": "tool_use",
                            "id": "toolu_phase0",
                            "name": "Read",
                            "input": { "file_path": "src/lib.rs" }
                        }
                    ],
                    "stop_reason": "tool_use",
                    "cost_usd": 0.01,
                    "usage": {
                        "input_tokens": 10,
                        "output_tokens": 20,
                        "reasoning_output_tokens": 0,
                        "cache_read_input_tokens": 0,
                        "cache_creation_input_tokens": 0
                    }
                }),
            },
            SerializableMessage {
                msg_type: "user".into(),
                uuid: "10000000-0000-0000-0000-000000000003".into(),
                timestamp: 3,
                data: serde_json::json!({
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": "toolu_phase0",
                        "content": "file contents",
                        "is_error": false
                    }],
                    "is_meta": false
                }),
            },
            SerializableMessage {
                msg_type: "system".into(),
                uuid: "10000000-0000-0000-0000-000000000004".into(),
                timestamp: 4,
                data: serde_json::json!({
                    "content": "phase0 system warning",
                    "subtype": "Warning"
                }),
            },
            SerializableMessage {
                msg_type: "progress".into(),
                uuid: "10000000-0000-0000-0000-000000000005".into(),
                timestamp: 5,
                data: serde_json::json!({
                    "tool_use_id": "toolu_phase0",
                    "data": { "message": "running" }
                }),
            },
            SerializableMessage {
                msg_type: "attachment".into(),
                uuid: "10000000-0000-0000-0000-000000000006".into(),
                timestamp: 6,
                data: serde_json::json!({
                    "attachment": {
                        "type": "edited_text_file",
                        "path": "src/lib.rs"
                    }
                }),
            },
        ]
    }

    fn phase0_typed_messages() -> Vec<Message> {
        vec![
            Message::User(UserMessage {
                uuid: Uuid::parse_str("20000000-0000-0000-0000-000000000001").unwrap(),
                timestamp: 1,
                role: "user".into(),
                content: MessageContent::Text("phase0 user text".into()),
                is_meta: false,
                tool_use_result: None,
                source_tool_assistant_uuid: None,
            }),
            Message::Assistant(AssistantMessage {
                uuid: Uuid::parse_str("20000000-0000-0000-0000-000000000002").unwrap(),
                timestamp: 2,
                role: "assistant".into(),
                content: vec![
                    ContentBlock::Text {
                        text: "phase0 assistant text".into(),
                    },
                    ContentBlock::ToolUse {
                        id: "toolu_phase0".into(),
                        name: "Read".into(),
                        input: serde_json::json!({ "file_path": "src/lib.rs" }),
                    },
                ],
                usage: None,
                stop_reason: Some("tool_use".into()),
                is_api_error_message: false,
                api_error: None,
                cost_usd: 0.01,
            }),
            Message::User(UserMessage {
                uuid: Uuid::parse_str("20000000-0000-0000-0000-000000000003").unwrap(),
                timestamp: 3,
                role: "user".into(),
                content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                    tool_use_id: "toolu_phase0".into(),
                    content: ToolResultContent::Text("file contents".into()),
                    is_error: false,
                }]),
                is_meta: false,
                tool_use_result: None,
                source_tool_assistant_uuid: None,
            }),
            Message::System(SystemMessage {
                uuid: Uuid::parse_str("20000000-0000-0000-0000-000000000004").unwrap(),
                timestamp: 4,
                subtype: SystemSubtype::Warning,
                content: "phase0 system warning".into(),
            }),
            Message::Progress(ProgressMessage {
                uuid: Uuid::parse_str("20000000-0000-0000-0000-000000000005").unwrap(),
                timestamp: 5,
                tool_use_id: "toolu_phase0".into(),
                data: serde_json::json!({ "message": "running" }),
            }),
            Message::Attachment(AttachmentMessage {
                uuid: Uuid::parse_str("20000000-0000-0000-0000-000000000006").unwrap(),
                timestamp: 6,
                attachment: Attachment::EditedTextFile {
                    path: "src/lib.rs".into(),
                },
            }),
        ]
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
    fn query_sqlite_message_types(session_id: &str) -> Result<Vec<String>> {
        let session_id = session_id.to_string();
        allthecodes_db::run_sqlite_sync("allthecodes-session-test-sqlite", async move {
            let pool = allthecodes_db::DbPoolManager::new().state_pool()?;
            let rows = sqlx::query(
                "SELECT msg_type FROM session_messages WHERE session_id = ? ORDER BY position",
            )
            .bind(session_id)
            .fetch_all(&pool)
            .await?;
            rows.into_iter()
                .map(|row| row.try_get::<String, _>(0))
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(Into::into)
        })
    }

    #[test]
    #[serial_test::serial]
    fn test_phase0_legacy_session_json_load_and_resume_baseline() {
        let temp = tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());
        write_fixture_session(
            "phase0-json",
            phase0_legacy_serializable_messages(),
            "/proj",
        )
        .unwrap();

        let loaded = load_session("phase0-json").unwrap();
        let resumed = crate::resume::resume_session("phase0-json").unwrap();

        assert_eq!(loaded.len(), 4);
        assert_eq!(resumed.len(), loaded.len());
        assert!(loaded.iter().any(|message| {
            matches!(
                message,
                Message::Assistant(AssistantMessage { content, .. })
                    if content.iter().any(|block| matches!(
                        block,
                        ContentBlock::ToolUse { name, .. } if name == "Read"
                    ))
            )
        }));
        assert!(loaded.iter().any(|message| {
            matches!(
                message,
                Message::User(UserMessage {
                    content: MessageContent::Blocks(blocks),
                    ..
                }) if blocks.iter().any(|block| matches!(
                    block,
                    ContentBlock::ToolResult { tool_use_id, .. }
                        if tool_use_id == "toolu_phase0"
                ))
            )
        }));
        assert!(loaded
            .iter()
            .any(|message| matches!(message, Message::System(_))));
        assert!(!loaded
            .iter()
            .any(|message| matches!(message, Message::Progress(_))));
        assert!(!loaded
            .iter()
            .any(|message| matches!(message, Message::Attachment(_))));
    }

    #[test]
    #[serial_test::serial]
    fn test_phase0_transcript_fixture_payload_baseline() {
        let temp = tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());
        let messages = phase0_typed_messages();

        crate::transcript::record_transcript("phase0-transcript", &messages).unwrap();

        let content =
            std::fs::read_to_string(crate::transcript::get_transcript_file("phase0-transcript"))
                .unwrap();
        let lines = content
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect::<Vec<_>>();

        assert_eq!(lines.len(), 6);
        assert_eq!(lines[0]["msg_type"], "user");
        assert_eq!(
            lines[1]["payload"]["content_summary"][0],
            "phase0 assistant text"
        );
        assert_eq!(
            lines[1]["payload"]["content_summary"][1],
            "[tool_use: Read]"
        );
        assert_eq!(lines[4]["msg_type"], "progress");
        assert_eq!(lines[4]["payload"]["tool_use_id"], "toolu_phase0");
        assert_eq!(lines[5]["msg_type"], "attachment");
        assert_eq!(
            lines[5]["payload"]["attachment"]["type"],
            "edited_text_file"
        );
    }

    #[cfg(feature = "sqlite-storage")]
    #[test]
    #[serial_test::serial]
    fn test_phase0_sqlite_projection_keeps_current_message_rows() {
        let temp = tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());
        let messages = phase0_typed_messages();

        save_session("phase0-sqlite", &messages, "/proj").unwrap();

        let message_types = query_sqlite_message_types("phase0-sqlite").unwrap();
        assert_eq!(
            message_types,
            vec![
                "user",
                "assistant",
                "user",
                "system",
                "progress",
                "attachment"
            ]
        );

        let loaded = load_session("phase0-sqlite").unwrap();
        assert_eq!(loaded.len(), 4);
        assert!(!loaded
            .iter()
            .any(|message| matches!(message, Message::Progress(_))));
        assert!(!loaded
            .iter()
            .any(|message| matches!(message, Message::Attachment(_))));
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

    #[test]
    #[serial_test::serial]
    fn test_search_sessions_finds_saved_message_text() {
        let temp = tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        save_session(
            "hermes-search-one",
            &[user_message("remember hermes runtime planning details")],
            "/proj",
        )
        .unwrap();

        let hits = search_sessions("hermes runtime", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].session_id, "hermes-search-one");
        assert_eq!(hits[0].message_index, 0);
        assert!(hits[0].snippet.contains("hermes runtime"));
    }

    #[test]
    #[serial_test::serial]
    fn test_search_workspace_sessions_filters_by_workspace() {
        let temp = tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());
        let project = temp.path().join("project");
        let other = temp.path().join("other");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&other).unwrap();

        save_session(
            "hermes-workspace-hit",
            &[user_message("gateway cron memory")],
            project.to_str().unwrap(),
        )
        .unwrap();
        save_session(
            "hermes-workspace-miss",
            &[user_message("gateway cron memory")],
            other.to_str().unwrap(),
        )
        .unwrap();

        let hits = search_workspace_sessions(&project, "gateway", 10).unwrap();
        let ids = hits
            .iter()
            .map(|hit| hit.session_id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["hermes-workspace-hit"]);
    }

    #[test]
    #[serial_test::serial]
    fn test_search_sessions_uses_json_fallback_when_sqlite_is_blocked() {
        let temp = tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());
        std::fs::create_dir_all(temp.path().join("state").join("state_5.sqlite")).unwrap();

        write_fixture_session(
            "hermes-json-fallback",
            vec![user_sm(
                "json fallback searchable memory",
                "00000000-0000-0000-0000-000000000701",
            )],
            "/proj",
        )
        .unwrap();

        let hits = search_sessions("searchable memory", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].session_id, "hermes-json-fallback");
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

    // ------------------------------------------------------------------
    // Phase 0: Behavior locking tests per TDD plan
    // ------------------------------------------------------------------

    /// Lock storage_load_session_fixture_roundtrip: JSON fixture loaded via
    /// load_session -> messages roundtrip back through SessionFile serialization.
    #[test]
    #[serial_test::serial]
    fn test_storage_load_session_fixture_roundtrip() {
        let temp = tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());
        write_fixture_session("fixture-rt", phase0_legacy_serializable_messages(), "/proj")
            .unwrap();

        let loaded = load_session("fixture-rt").unwrap();
        assert_eq!(loaded.len(), 4);

        // Re-serialize to SessionFile and verify the JSON reproduces the
        // original messages (user text, tool_use, tool_result, system).
        let resaved = SessionFile {
            session_id: "fixture-rt".into(),
            created_at: 0,
            last_modified: 0,
            cwd: "/proj".into(),
            custom_title: None,
            chat_mode_override: None,
            messages: serialization::messages_to_serializable(&loaded),
        };
        let json = serde_json::to_string_pretty(&resaved).unwrap();
        let reparsed: SessionFile = serde_json::from_str(&json).unwrap();
        assert_eq!(reparsed.messages.len(), 4);
        assert!(reparsed.messages.iter().any(|sm| sm.msg_type == "user"));
        assert!(reparsed
            .messages
            .iter()
            .any(|sm| sm.msg_type == "assistant"));
        assert!(reparsed.messages.iter().any(|sm| sm.msg_type == "system"));
    }

    /// Lock resume_session_baseline: resume from known fixture yields correct
    /// message count and types.
    #[test]
    #[serial_test::serial]
    fn test_resume_session_baseline() {
        let temp = tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());
        write_fixture_session(
            "resume-baseline",
            phase0_legacy_serializable_messages(),
            "/proj",
        )
        .unwrap();

        let resumed = crate::resume::resume_session("resume-baseline").unwrap();
        assert_eq!(resumed.len(), 4);
        // Same assertions as legacy baseline: user, assistant (with tool_use),
        // tool_result user, system all survive; progress and attachment dropped.
        assert!(resumed.iter().any(|msg| matches!(msg, Message::User(_))));
        assert!(resumed
            .iter()
            .any(|msg| matches!(msg, Message::Assistant(_))));
        assert!(resumed.iter().any(|msg| matches!(msg, Message::System(_))));
        // Tool messages survived
        assert!(resumed.iter().any(|msg| {
            matches!(
                msg,
                Message::Assistant(AssistantMessage { content, .. })
                    if content.iter().any(|b| matches!(b, ContentBlock::ToolUse { name, .. } if name == "Read"))
            )
        }));
        assert!(resumed.iter().any(|msg| {
            matches!(
                msg,
                Message::User(UserMessage {
                    content: MessageContent::Blocks(blocks),
                    ..
                }) if blocks.iter().any(|b| matches!(b, ContentBlock::ToolResult { tool_use_id, .. } if tool_use_id == "toolu_phase0"))
            )
        }));
        // Progress and attachment dropped
        assert!(!resumed
            .iter()
            .any(|msg| matches!(msg, Message::Progress(_))));
        assert!(!resumed
            .iter()
            .any(|msg| matches!(msg, Message::Attachment(_))));
    }

    /// SQLite projection: when a session with all message types is saved via
    /// SQLite, the projection matches what the session JSON carries.
    #[cfg(feature = "sqlite-storage")]
    #[test]
    #[serial_test::serial]
    fn test_sqlite_projection_matches_session_json() {
        let temp = tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        let messages = phase0_typed_messages();
        save_session("sqlite-proj", &messages, "/proj").unwrap();

        // Read back via SQLite (load_session is wired to use SQLite when available)
        let loaded = load_session("sqlite-proj").unwrap();
        assert_eq!(loaded.len(), 4);

        // Read from JSON file as well
        let file_path = get_session_file("sqlite-proj");
        let json_content = std::fs::read_to_string(&file_path).unwrap();
        let file: SessionFile = serde_json::from_str(&json_content).unwrap();

        // Both projections carry the same count (both drop progress+attachment on load)
        assert_eq!(loaded.len(), serializable_to_messages(&file.messages).len());

        // The SQLite representation of the same data in the DB should have
        // also recorded progress and attachment rows even though load_session
        // drops them.
        let sqlite_types = query_sqlite_message_types("sqlite-proj").unwrap();
        assert!(sqlite_types.contains(&"progress".to_string()));
        assert!(sqlite_types.contains(&"attachment".to_string()));
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
