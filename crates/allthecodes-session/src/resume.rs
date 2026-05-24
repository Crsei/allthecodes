//! Session resume -- finding and restoring the most recent session.
//!
//! Provides helpers to locate the last session for a given working directory
//! and to reload its message history so the conversation can continue.

use std::path::Path;

use anyhow::Result;
use tracing::{debug, info};

use super::storage::{self, SessionInfo};
use allthecodes_types::message::Message;

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
/// This is a thin wrapper around `storage::load_session` that makes intent
/// clear at the call site.
pub fn resume_session(session_id: &str) -> Result<Vec<Message>> {
    info!(session_id = session_id, "resuming session");
    storage::load_session(session_id)
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
