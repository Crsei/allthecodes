//! File-based mailbox IPC for teammate communication.
//!
//! Corresponds to TypeScript: `utils/teammateMailbox.ts`
//!
//! Each teammate has an inbox file at:
//!   `{data_root}/teams/{team_name}/inboxes/{agent_name}.json`
//!
//! Messages are stored as a JSON array of `TeammateMessage`.
//! Write operations use file locking to prevent data loss from concurrent access.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use tracing::debug;

use super::constants::*;
use super::types::TeammateMessage;

// ---------------------------------------------------------------------------
// Path resolution
// ---------------------------------------------------------------------------

/// Get the base directory for a team's data.
///
/// Returns: `{data_root}/teams/{team_name}`
pub fn team_dir(team_name: &str) -> PathBuf {
    mailbox_teams_dir().join(sanitize_name(team_name))
}

fn mailbox_teams_dir() -> PathBuf {
    #[cfg(test)]
    if let Some(path) = TEST_TEAMS_DIR.with(|dir| dir.borrow().clone()) {
        return path;
    }

    crate::storage_paths::teams_dir()
}

#[cfg(test)]
thread_local! {
    static TEST_TEAMS_DIR: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// Get the inbox file path for an agent.
///
/// Returns: `{data_root}/teams/{team_name}/inboxes/{agent_name}.json`
pub fn inbox_path(agent_name: &str, team_name: &str) -> PathBuf {
    team_dir(team_name).join(INBOXES_DIR_NAME).join(format!(
        "{}.{}",
        sanitize_name(agent_name),
        INBOX_EXTENSION
    ))
}

/// Get the lock file path for an inbox.
fn lock_path(inbox: &Path) -> PathBuf {
    let mut p = inbox.as_os_str().to_owned();
    p.push(LOCK_FILE_SUFFIX);
    PathBuf::from(p)
}

/// Sanitize a name for use in file paths (replace problematic characters).
fn sanitize_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Core operations
// ---------------------------------------------------------------------------

/// Read all messages from a teammate's mailbox.
///
/// No locking — reads are best-effort.
///
/// Corresponds to TS: `readMailbox(name, team)`
pub fn read_mailbox(agent_name: &str, team_name: &str) -> Result<Vec<TeammateMessage>> {
    let path = inbox_path(agent_name, team_name);
    if !path.exists() {
        return Ok(vec![]);
    }
    let content = fs::read_to_string(&path)
        .with_context(|| format!("failed to read mailbox: {}", path.display()))?;
    if content.trim().is_empty() {
        return Ok(vec![]);
    }
    let messages: Vec<TeammateMessage> = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse mailbox: {}", path.display()))?;
    Ok(messages)
}

/// Read only unread messages from a teammate's mailbox.
///
/// Corresponds to TS: `readUnreadMessages(name, team)`
pub fn read_unread_messages(agent_name: &str, team_name: &str) -> Result<Vec<TeammateMessage>> {
    let messages = read_mailbox(agent_name, team_name)?;
    Ok(messages.into_iter().filter(|m| !m.read).collect())
}

/// Write a message to a teammate's mailbox.
///
/// Uses file locking to prevent concurrent write corruption.
/// Lock → read latest → append → write back.
///
/// Corresponds to TS: `writeToMailbox(name, msg, team)`
pub fn write_to_mailbox(agent_name: &str, message: TeammateMessage, team_name: &str) -> Result<()> {
    let path = inbox_path(agent_name, team_name);

    // Ensure directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    with_lock(&path, || {
        // Re-read to capture concurrent writes. Existing invalid content must
        // remain visible instead of being reset to an empty mailbox.
        let mut messages = read_mailbox(agent_name, team_name)?;

        messages.push(message);

        let json = serde_json::to_string_pretty(&messages)?;
        fs::write(&path, json)?;
        Ok(())
    })
}

/// Mark a specific message as read by index.
///
/// Corresponds to TS: `markAsReadByIndex(name, team, idx)`
pub fn mark_as_read_by_index(agent_name: &str, team_name: &str, index: usize) -> Result<()> {
    let path = inbox_path(agent_name, team_name);
    if !path.exists() {
        return Ok(());
    }

    with_lock(&path, || {
        let content = fs::read_to_string(&path)?;
        let mut messages: Vec<TeammateMessage> = serde_json::from_str(&content)?;
        if index < messages.len() {
            messages[index].read = true;
            let json = serde_json::to_string_pretty(&messages)?;
            fs::write(&path, json)?;
        }
        Ok(())
    })
}

/// Mark all messages as read.
///
/// Corresponds to TS: `markMessagesAsRead(name, team)`
pub fn mark_all_as_read(agent_name: &str, team_name: &str) -> Result<()> {
    let path = inbox_path(agent_name, team_name);
    if !path.exists() {
        return Ok(());
    }

    with_lock(&path, || {
        let content = fs::read_to_string(&path)?;
        let mut messages: Vec<TeammateMessage> = serde_json::from_str(&content)?;
        for msg in &mut messages {
            msg.read = true;
        }
        let json = serde_json::to_string_pretty(&messages)?;
        fs::write(&path, json)?;
        Ok(())
    })
}

/// Clear all messages from a mailbox.
///
/// No locking — overwrites with empty array.
///
/// Corresponds to TS: `clearMailbox(name, team)`
pub fn clear_mailbox(agent_name: &str, team_name: &str) -> Result<()> {
    let path = inbox_path(agent_name, team_name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, "[]")?;
    Ok(())
}

// ---------------------------------------------------------------------------
// File locking
// ---------------------------------------------------------------------------

/// Execute a closure while holding a file lock.
///
/// Uses a separate `.lock` file with exponential backoff retry.
/// Corresponds to TS: `proper-lockfile` usage.
fn with_lock<F, R>(inbox: &Path, f: F) -> Result<R>
where
    F: FnOnce() -> Result<R>,
{
    let lock = lock_path(inbox);

    // Ensure parent exists
    if let Some(parent) = lock.parent() {
        fs::create_dir_all(parent)?;
    }

    // Try to acquire lock with exponential backoff
    let mut delay_ms = MAILBOX_LOCK_MIN_TIMEOUT_MS;
    let mut acquired = false;

    for attempt in 0..MAILBOX_LOCK_RETRIES {
        // Try to create the lock file exclusively
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock)
        {
            Ok(mut file) => {
                // Write PID for debugging
                let _ = write!(file, "{}", std::process::id());
                acquired = true;
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                // Lock held by another process — check for stale lock
                if is_stale_lock(&lock) {
                    let _ = fs::remove_file(&lock);
                    continue;
                }
                debug!(attempt, delay_ms, "mailbox lock contention, backing off");
                thread::sleep(Duration::from_millis(delay_ms));
                delay_ms = (delay_ms * 2).min(MAILBOX_LOCK_MAX_TIMEOUT_MS);
            }
            Err(e) => return Err(e.into()),
        }
    }

    if !acquired {
        anyhow::bail!(
            "timed out acquiring mailbox lock after {} retries: {}",
            MAILBOX_LOCK_RETRIES,
            lock.display()
        );
    }

    // Execute under lock
    let result = f();

    // Release lock
    let _ = fs::remove_file(&lock);

    result
}

/// Check if a lock file is stale (older than 5 seconds).
fn is_stale_lock(lock: &Path) -> bool {
    fs::metadata(lock)
        .and_then(|m| m.modified())
        .map(|modified| modified.elapsed().map(|d| d.as_secs() > 5).unwrap_or(false))
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    struct TeamsDirGuard {
        previous: Option<PathBuf>,
    }

    impl TeamsDirGuard {
        fn set(path: PathBuf) -> Self {
            let previous = TEST_TEAMS_DIR.with(|dir| dir.replace(Some(path)));
            Self { previous }
        }
    }

    impl Drop for TeamsDirGuard {
        fn drop(&mut self) {
            let previous = self.previous.take();
            TEST_TEAMS_DIR.with(|dir| {
                dir.replace(previous);
            });
        }
    }

    struct TestTeam {
        team: String,
        agent: String,
        dir: PathBuf,
        _tmp: TempDir,
        _guard: TeamsDirGuard,
    }

    fn test_team() -> TestTeam {
        let tmp = TempDir::new().expect("tempdir");
        let guard = TeamsDirGuard::set(tmp.path().join("teams"));
        let id = uuid::Uuid::new_v4().to_string();
        let team = format!("test-team-{}", &id[..8]);
        let agent = "test-agent";
        let dir = team_dir(&team);
        TestTeam {
            team,
            agent: agent.to_string(),
            dir,
            _tmp: tmp,
            _guard: guard,
        }
    }

    fn cleanup(dir: &Path) {
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_sanitize_name() {
        assert_eq!(sanitize_name("hello-world"), "hello-world");
        assert_eq!(sanitize_name("hello world"), "hello_world");
        assert_eq!(sanitize_name("test@team"), "test_team");
    }

    #[test]
    fn test_inbox_path_format() {
        let path = inbox_path("researcher", "my-team");
        let path_str = path.to_string_lossy();
        assert!(path_str.contains("teams"));
        assert!(path_str.contains("my-team"));
        assert!(path_str.contains("inboxes"));
        assert!(path_str.contains("researcher.json"));
    }

    #[test]
    fn team_dir_uses_configured_teams_root() {
        let tmp = TempDir::new().expect("tempdir");
        let _guard = TeamsDirGuard::set(tmp.path().join("teams"));

        let team_path = team_dir("my team");
        let inbox = inbox_path("agent", "my team");

        assert_eq!(team_path, tmp.path().join("teams").join("my_team"));
        assert_eq!(
            inbox,
            tmp.path()
                .join("teams")
                .join("my_team")
                .join("inboxes")
                .join("agent.json")
        );
    }

    #[test]
    fn test_read_nonexistent_mailbox() {
        let messages = read_mailbox("nobody", "nonexistent-team-12345").unwrap();
        assert!(messages.is_empty());
    }

    #[test]
    fn test_write_and_read() {
        let fixture = test_team();
        let msg = TeammateMessage {
            from: "sender".into(),
            text: "Hello!".into(),
            timestamp: "2026-04-01T12:00:00Z".into(),
            read: false,
            color: None,
            summary: None,
        };

        write_to_mailbox(&fixture.agent, msg, &fixture.team).unwrap();
        let messages = read_mailbox(&fixture.agent, &fixture.team).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].from, "sender");
        assert_eq!(messages[0].text, "Hello!");
        assert!(!messages[0].read);

        cleanup(&fixture.dir);
    }

    #[test]
    fn test_write_multiple_and_read() {
        let fixture = test_team();

        for i in 0..3 {
            let msg = TeammateMessage {
                from: format!("sender-{}", i),
                text: format!("Message {}", i),
                timestamp: "t".into(),
                read: false,
                color: None,
                summary: None,
            };
            write_to_mailbox(&fixture.agent, msg, &fixture.team).unwrap();
        }

        let messages = read_mailbox(&fixture.agent, &fixture.team).unwrap();
        assert_eq!(messages.len(), 3);

        cleanup(&fixture.dir);
    }

    #[test]
    fn test_write_rejects_corrupt_mailbox_without_resetting() {
        let fixture = test_team();
        let path = inbox_path(&fixture.agent, &fixture.team);
        fs::create_dir_all(path.parent().expect("inbox parent")).unwrap();
        fs::write(&path, "{not valid json").unwrap();

        let msg = TeammateMessage {
            from: "sender".into(),
            text: "Hello!".into(),
            timestamp: "2026-04-01T12:00:00Z".into(),
            read: false,
            color: None,
            summary: None,
        };

        let err = write_to_mailbox(&fixture.agent, msg, &fixture.team).unwrap_err();

        assert!(err.to_string().contains("failed to parse mailbox"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "{not valid json");

        cleanup(&fixture.dir);
    }

    #[test]
    fn test_read_unread() {
        let fixture = test_team();

        // Write two messages
        for i in 0..2 {
            let msg = TeammateMessage {
                from: "s".into(),
                text: format!("m{}", i),
                timestamp: "t".into(),
                read: false,
                color: None,
                summary: None,
            };
            write_to_mailbox(&fixture.agent, msg, &fixture.team).unwrap();
        }

        // Mark first as read
        mark_as_read_by_index(&fixture.agent, &fixture.team, 0).unwrap();

        let unread = read_unread_messages(&fixture.agent, &fixture.team).unwrap();
        assert_eq!(unread.len(), 1);
        assert_eq!(unread[0].text, "m1");

        cleanup(&fixture.dir);
    }

    #[test]
    fn test_mark_all_as_read() {
        let fixture = test_team();

        for _ in 0..3 {
            let msg = TeammateMessage {
                from: "s".into(),
                text: "m".into(),
                timestamp: "t".into(),
                read: false,
                color: None,
                summary: None,
            };
            write_to_mailbox(&fixture.agent, msg, &fixture.team).unwrap();
        }

        mark_all_as_read(&fixture.agent, &fixture.team).unwrap();
        let unread = read_unread_messages(&fixture.agent, &fixture.team).unwrap();
        assert!(unread.is_empty());

        cleanup(&fixture.dir);
    }

    #[test]
    fn test_clear_mailbox() {
        let fixture = test_team();

        let msg = TeammateMessage {
            from: "s".into(),
            text: "m".into(),
            timestamp: "t".into(),
            read: false,
            color: None,
            summary: None,
        };
        write_to_mailbox(&fixture.agent, msg, &fixture.team).unwrap();
        assert_eq!(
            read_mailbox(&fixture.agent, &fixture.team).unwrap().len(),
            1
        );

        clear_mailbox(&fixture.agent, &fixture.team).unwrap();
        assert!(read_mailbox(&fixture.agent, &fixture.team)
            .unwrap()
            .is_empty());

        cleanup(&fixture.dir);
    }

    #[test]
    fn test_lock_path() {
        let inbox = PathBuf::from("/tmp/inbox.json");
        let lock = lock_path(&inbox);
        assert_eq!(lock, PathBuf::from("/tmp/inbox.json.lock"));
    }
}
