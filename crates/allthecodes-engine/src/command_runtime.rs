use std::path::PathBuf;
use std::sync::{Arc, OnceLock, RwLock};

use anyhow::Result;
use async_trait::async_trait;

use crate::bootstrap::SessionId;
use crate::types::app_state::AppState;
use crate::types::message::Message;

/// Mutable state passed to a slash-command executor.
pub struct CommandContext {
    pub messages: Vec<Message>,
    pub cwd: PathBuf,
    pub app_state: AppState,
    pub session_id: SessionId,
}

/// Engine-facing result of executing a slash command.
pub enum CommandResult {
    Output(String),
    Query(Vec<Message>),
    SwitchSession {
        session_id: SessionId,
        messages: Vec<Message>,
        notice: String,
    },
    Clear,
    Exit(String),
    None,
}

#[async_trait]
pub trait CommandExecutor: Send + Sync {
    async fn execute(
        &self,
        parsed: allthecodes_types::commands::ParsedCommand,
        command_name: String,
        ctx: &mut CommandContext,
    ) -> Result<CommandResult>;
}

pub struct NoopCommandExecutor;

impl NoopCommandExecutor {
    pub fn new() -> Self {
        Self
    }
}

impl Default for NoopCommandExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl CommandExecutor for NoopCommandExecutor {
    async fn execute(
        &self,
        _parsed: allthecodes_types::commands::ParsedCommand,
        command_name: String,
        _ctx: &mut CommandContext,
    ) -> Result<CommandResult> {
        anyhow::bail!("no slash-command executor configured for /{command_name}")
    }
}

static GLOBAL_COMMAND_EXECUTOR: OnceLock<RwLock<Option<Arc<dyn CommandExecutor>>>> =
    OnceLock::new();

pub fn set_global_command_executor(executor: Arc<dyn CommandExecutor>) {
    let slot = GLOBAL_COMMAND_EXECUTOR.get_or_init(|| RwLock::new(None));
    if let Ok(mut guard) = slot.write() {
        *guard = Some(executor);
    }
}

pub fn global_command_executor() -> Arc<dyn CommandExecutor> {
    GLOBAL_COMMAND_EXECUTOR
        .get()
        .and_then(|slot| slot.read().ok().and_then(|guard| guard.clone()))
        .unwrap_or_else(|| Arc::new(NoopCommandExecutor::new()))
}
