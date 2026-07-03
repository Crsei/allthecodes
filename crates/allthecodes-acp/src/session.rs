//! ACP session management -- AcpSession and AcpSessionManager.
//!
//! Each ACP session wraps a per-session QueryEngine. The session manager
//! provides session/new, session/load, session/resume, session/list, and
//! session/close.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use agent_client_protocol_schema::v2;
use allthecodes_engine::lifecycle::QueryEngine;
use allthecodes_types::message::Message;
use tokio::sync::{Mutex, RwLock};
use tokio::time::{sleep, timeout, Duration};

use crate::engine_factory::AcpEngineFactory;
use crate::jsonrpc;
use crate::transport::AcpSink;
use crate::AcpEngineParams;

/// Handle to an active turn within a session.
#[derive(Debug)]
pub struct AcpTurnHandle {
    /// Whether cancellation has been requested.
    pub cancel_requested: bool,
}

/// An active ACP session.
pub struct AcpSession {
    pub session_id: agent_client_protocol_schema::v2::SessionId,
    pub cwd: std::path::PathBuf,
    pub additional_directories: Vec<std::path::PathBuf>,
    pub engine: Arc<QueryEngine>,
    pub active_turn: Mutex<Option<AcpTurnHandle>>,
}

impl std::fmt::Debug for AcpSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AcpSession")
            .field("session_id", &self.session_id)
            .field("cwd", &self.cwd)
            .field("additional_directories", &self.additional_directories)
            .finish()
    }
}

/// The central session manager for ACP.
pub struct AcpSessionManager {
    sessions: RwLock<HashMap<String, Arc<AcpSession>>>,
    engine_factory: Arc<dyn AcpEngineFactory>,
}

impl std::fmt::Debug for AcpSessionManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AcpSessionManager")
            .field(
                "session_count",
                &self.sessions.try_read().map(|s| s.len()).unwrap_or(0),
            )
            .finish()
    }
}

impl AcpSessionManager {
    pub fn new(engine_factory: Arc<dyn AcpEngineFactory>) -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            engine_factory,
        }
    }

    /// Create a new session.
    pub async fn create_session(
        &self,
        session_id: agent_client_protocol_schema::v2::SessionId,
        cwd: std::path::PathBuf,
        additional_directories: Vec<std::path::PathBuf>,
        initial_messages: Option<Vec<allthecodes_types::message::Message>>,
    ) -> anyhow::Result<Arc<AcpSession>> {
        if !cwd.is_absolute() {
            anyhow::bail!("cwd must be an absolute path");
        }
        if !cwd.is_dir() {
            anyhow::bail!("cwd must be an existing directory");
        }

        for dir in &additional_directories {
            if !dir.is_absolute() {
                anyhow::bail!("additional directory must be absolute: {}", dir.display());
            }
            if !dir.is_dir() {
                anyhow::bail!("additional directory must exist: {}", dir.display());
            }
        }

        let engine = self.engine_factory.create_engine(AcpEngineParams {
            session_id: Some(session_id.0.to_string()),
            cwd: cwd.clone(),
            additional_directories: additional_directories.clone(),
            initial_messages,
        })?;

        let session = Arc::new(AcpSession {
            session_id: session_id.clone(),
            cwd,
            additional_directories,
            engine,
            active_turn: Mutex::new(None),
        });

        self.sessions
            .write()
            .await
            .insert(session_id.0.to_string(), session.clone());

        Ok(session)
    }

    /// Get a session by ID.
    pub async fn get_session(&self, session_id: &str) -> Option<Arc<AcpSession>> {
        self.sessions.read().await.get(session_id).cloned()
    }

    /// Remove and return a session by ID (for session/close).
    pub async fn remove_session(&self, session_id: &str) -> Option<Arc<AcpSession>> {
        self.sessions.write().await.remove(session_id)
    }

    /// Validate that cwd is absolute and exists.
    pub fn validate_cwd(cwd: &Path) -> Result<(), String> {
        if !cwd.is_absolute() {
            return Err("cwd must be an absolute path".to_string());
        }
        if !cwd.is_dir() {
            return Err("cwd must be an existing directory".to_string());
        }
        Ok(())
    }

    /// Validate additional directories.
    pub fn validate_additional_dirs(dirs: &[std::path::PathBuf]) -> Result<(), String> {
        for dir in dirs {
            if !dir.is_absolute() {
                return Err(format!(
                    "additional directory must be absolute: {}",
                    dir.display()
                ));
            }
            if !dir.is_dir() {
                return Err(format!(
                    "additional directory must exist: {}",
                    dir.display()
                ));
            }
        }
        Ok(())
    }
}

/// Close a session: abort any active turn, flush recorder, remove from map.
pub async fn close_session(
    session: &AcpSession,
    session_manager: &AcpSessionManager,
    sink: &AcpSink,
) {
    let had_active_turn = {
        let mut turn = session.active_turn.lock().await;
        if let Some(ref mut handle) = *turn {
            handle.cancel_requested = true;
            true
        } else {
            false
        }
    };

    session.engine.abort();

    if had_active_turn {
        let idle_observed = timeout(Duration::from_millis(50), async {
            loop {
                if session.active_turn.lock().await.is_none() {
                    break;
                }
                sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .is_ok();

        if !idle_observed {
            *session.active_turn.lock().await = None;
            send_session_update(
                sink,
                session.session_id.clone(),
                crate::updates::state_idle_update(Some(v2::StopReason::Cancelled)),
            );
        }
    }

    let _ = session.engine.flush_session_record().await;

    session_manager
        .remove_session(&session.session_id.0.to_string())
        .await;
}

fn send_session_update(sink: &AcpSink, session_id: v2::SessionId, update: v2::SessionUpdate) {
    let notification = v2::UpdateSessionNotification::new(session_id, update);
    sink.send(jsonrpc::build_agent_notification(
        v2::AgentNotification::UpdateSessionNotification(Box::new(notification)),
    ));
}

/// Replay loaded visible transcript messages as ACP session/update notifications.
pub fn replay_loaded_messages(
    session_id: v2::SessionId,
    messages: &[Message],
    cwd: &Path,
    sink: &AcpSink,
) -> anyhow::Result<()> {
    for (index, message) in messages.iter().enumerate() {
        for update in crate::updates::loaded_message_to_updates(message, index, cwd) {
            send_session_update(sink, session_id.clone(), update);
        }
    }
    Ok(())
}

/// Build a `session/list` response using the persisted session store.
pub fn list_sessions(
    params: Option<&v2::ListSessionsRequest>,
) -> Result<v2::ListSessionsResponse, v2::Error> {
    let cursor = params
        .and_then(|req| req.cursor.as_deref())
        .map(|cursor| {
            serde_json::from_str::<allthecodes_session::storage::SessionListCursor>(cursor)
                .map_err(|e| v2::Error::invalid_params().data(format!("invalid cursor: {e}")))
        })
        .transpose()?;

    let limit = 100;
    let page = if let Some(cwd) = params.and_then(|req| req.cwd.as_ref()) {
        AcpSessionManager::validate_cwd(cwd).map_err(|e| v2::Error::invalid_params().data(e))?;
        allthecodes_session::storage::list_workspace_sessions_page(cwd, limit, cursor)
    } else {
        allthecodes_session::storage::list_sessions_page(limit, cursor)
    }
    .map_err(|e| v2::Error::internal_error().data(e.to_string()))?;

    let sessions = page
        .sessions
        .into_iter()
        .map(storage_session_to_acp)
        .collect();

    let next_cursor = page
        .next_cursor
        .map(|cursor| {
            serde_json::to_string(&cursor)
                .map_err(|e| v2::Error::internal_error().data(format!("cursor error: {e}")))
        })
        .transpose()?;

    Ok(v2::ListSessionsResponse::new(sessions).next_cursor(next_cursor))
}

fn storage_session_to_acp(info: allthecodes_session::storage::SessionInfo) -> v2::SessionInfo {
    let updated_at =
        chrono::DateTime::from_timestamp(info.last_modified, 0).map(|dt| dt.to_rfc3339());
    let cwd = if info.cwd.is_empty() {
        PathBuf::from(&info.workspace_root)
    } else {
        PathBuf::from(&info.cwd)
    };
    let mut meta = serde_json::Map::new();
    meta.insert("messageCount".into(), serde_json::json!(info.message_count));
    if !info.workspace_key.is_empty() {
        meta.insert("workspaceKey".into(), serde_json::json!(info.workspace_key));
    }
    if !info.workspace_root.is_empty() {
        meta.insert(
            "workspaceRoot".into(),
            serde_json::json!(info.workspace_root),
        );
    }
    if !info.workspace_name.is_empty() {
        meta.insert(
            "workspaceName".into(),
            serde_json::json!(info.workspace_name),
        );
    }

    v2::SessionInfo::new(v2::SessionId::new(info.session_id), cwd)
        .title(info.title)
        .updated_at(updated_at)
        .meta(meta)
}
