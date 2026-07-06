//! QueryEngine -- full session lifecycle implementation.
//!
//! Corresponds to TypeScript: QueryEngine.ts
//!
//! Owns a single conversation session. Implements the complete message dispatch
//! pipeline as described in QUERY_ENGINE_SESSION_LIFECYCLE.md:
//!
//!   Phase A: Input Processing
//!   Phase B: System Prompt Build
//!   Phase C: Pre-Query Setup (SystemInit, local-command fast path)
//!   Phase D: Query Loop -- full message dispatch (assistant, user, progress,
//!            system, attachment, stream, request_start, tombstone, tool_use_summary)
//!   Phase E: Result Generation (SdkResult)
//!
//! The stream returned by `submit_message` yields `SdkMessage` items.

mod deps;
mod helpers;
mod state;
mod submit_message;
#[cfg(test)]
mod tests;
mod types;

pub(crate) use state::{EngineSharedState, QueryEngineState};
pub use types::AbortReason;

use parking_lot::{Mutex, RwLock};
use std::collections::{HashSet, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use allthecodes_types::sdk::{PermissionDenial, UsageTracking};
use tracing::{info, warn};

use crate::bootstrap::SessionId;
use crate::observability::AuditContext;
use crate::runtime_services::RuntimeServices;
use crate::services::session_memory::{
    extract_session_insight, SessionMemoryConfig, SessionMemoryService,
};
use crate::session::record_replay::types::{MessageRecord, SessionMetaRecord};
use crate::session::record_replay::{
    RecordItem, RecorderOpenMode, RecorderStats, SessionRecorderHandle,
};
use crate::types::app_state::AppState;
use crate::types::config::QueryEngineConfig;
use crate::types::message::{ContentBlock, Message, MessageContent};
use crate::types::tool::Tools;

pub(crate) type AutoClassifierFn = Arc<
    dyn Fn(
            String,
            serde_json::Value,
            serde_json::Value,
            Vec<Message>,
            String,
        ) -> Pin<
            Box<
                dyn Future<Output = Option<crate::permissions::decision::AutoClassifierDecision>>
                    + Send,
            >,
        > + Send
        + Sync,
>;

#[derive(Debug, Default)]
pub(crate) struct AutoReviewTracker {
    attempted_tool_use_ids: HashSet<String>,
    consecutive_denials: usize,
}

impl AutoReviewTracker {
    const MAX_CONSECUTIVE_DENIALS: usize = 2;

    pub(crate) fn try_start(&mut self, tool_use_id: &str) -> Result<(), &'static str> {
        if self.consecutive_denials >= Self::MAX_CONSECUTIVE_DENIALS {
            return Err("circuit_open");
        }
        if !self.attempted_tool_use_ids.insert(tool_use_id.to_string()) {
            return Err("duplicate_review");
        }
        Ok(())
    }

    pub(crate) fn record_allowed(&mut self) {
        self.consecutive_denials = 0;
    }

    pub(crate) fn record_denied(&mut self) {
        self.consecutive_denials = self.consecutive_denials.saturating_add(1);
    }
}

#[derive(Default)]
pub(crate) struct ActiveSteerState {
    active: bool,
    pending: VecDeque<String>,
}

impl ActiveSteerState {
    fn activate(&mut self) {
        self.active = true;
        self.pending.clear();
    }

    fn deactivate(&mut self) {
        self.active = false;
        self.pending.clear();
    }
}

fn pause_active_goal_for_abort(session_id: &str) -> anyhow::Result<()> {
    let Some(goal) = allthecodes_tools::goals::load_goal_for_session(session_id)? else {
        return Ok(());
    };
    if !allthecodes_tools::goals::goal_is_active(&goal) {
        return Ok(());
    }
    allthecodes_tools::goals::mark_goal_paused_for_session(
        session_id,
        Some(&goal.goal_id),
        "task aborted by user",
    )?;
    Ok(())
}

pub(crate) fn set_proactive_context_blocked(blocked: bool, reason: &str) {
    allthecodes_types::proactive_context::set_context_blocked(blocked, reason);
    if let Err(error) = write_durable_proactive_context_blocked(blocked, reason) {
        warn!(%error, "failed to persist proactive context block state");
    }
}

fn write_durable_proactive_context_blocked(blocked: bool, reason: &str) -> anyhow::Result<()> {
    let path = allthecodes_config::paths::daemon_dir().join("proactive-state.json");
    let existing = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
    let active = existing
        .as_ref()
        .and_then(|state| state.get("active"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true);
    let next_tick_at = if blocked || !active {
        serde_json::Value::Null
    } else {
        serde_json::Value::String(
            (chrono::Utc::now() + chrono::Duration::milliseconds(30_000)).to_rfc3339(),
        )
    };
    let blocked_reason = if blocked {
        let trimmed = reason.trim();
        if trimmed.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::Value::String(trimmed.to_string())
        }
    } else {
        serde_json::Value::Null
    };
    let state = serde_json::json!({
        "schema_version": 2,
        "active": active,
        "next_tick_at": next_tick_at,
        "context_blocked": blocked,
        "blocked_reason": blocked_reason,
        "updated_at": chrono::Utc::now().to_rfc3339(),
    });
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_vec_pretty(&state)?)?;
    Ok(())
}

pub struct SteerError {
    message: String,
}

impl SteerError {
    fn no_active_turn() -> Self {
        Self {
            message: "No active turn is available to steer.".to_string(),
        }
    }
}

impl std::fmt::Debug for SteerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("SteerError").field(&self.message).finish()
    }
}

impl std::fmt::Display for SteerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for SteerError {}

pub(crate) struct ActiveSteerGuard {
    state: Arc<Mutex<ActiveSteerState>>,
}

impl ActiveSteerGuard {
    pub(crate) fn activate(state: Arc<Mutex<ActiveSteerState>>) -> Self {
        state.lock().activate();
        Self { state }
    }
}

impl Drop for ActiveSteerGuard {
    fn drop(&mut self) {
        self.state.lock().deactivate();
    }
}

pub(crate) async fn ensure_session_recorder_handle(
    recorder_ref: &Arc<Mutex<Option<SessionRecorderHandle>>>,
    config: &QueryEngineConfig,
    session_id: &SessionId,
) -> anyhow::Result<Option<SessionRecorderHandle>> {
    let existing_handle = { recorder_ref.lock().clone() };
    if let Some(handle) = existing_handle {
        if handle.session_id() == session_id.as_str() {
            return Ok(Some(handle));
        }
        let previous_session_id = handle.session_id().to_string();
        if let Err(error) = handle.shutdown().await {
            warn!(
                session_id = %previous_session_id,
                error = %error,
                "failed to shutdown previous session recorder"
            );
        }
    }

    let record_config = crate::session::record_replay::RecordReplayConfig::from_env();
    if !record_config.enabled {
        return Ok(None);
    }

    let session_id_string = session_id.as_str().to_string();
    let mode = match crate::session::record_replay::lookup_rollout(&session_id_string)? {
        Some(rollout_path) => {
            let read = crate::session::record_replay::read_rollout_file(&rollout_path)?;
            let next_seq = read
                .lines
                .iter()
                .map(|line| line.seq)
                .max()
                .unwrap_or(0)
                .saturating_add(1);
            RecorderOpenMode::Resume {
                session_id: session_id_string.clone(),
                rollout_path,
                next_seq,
            }
        }
        None => {
            let created_at = chrono::Utc::now();
            let cwd_path = std::path::Path::new(&config.cwd);
            let workspace_root = crate::session::storage::workspace_root(cwd_path);
            RecorderOpenMode::Create {
                session_id: session_id_string.clone(),
                cwd: std::path::PathBuf::from(&config.cwd),
                created_at,
                metadata: SessionMetaRecord {
                    created_at,
                    cwd: config.cwd.clone(),
                    workspace_key: Some(crate::session::storage::workspace_key(cwd_path)),
                    workspace_root: Some(workspace_root.to_string_lossy().to_string()),
                    workspace_name: Some(crate::session::storage::workspace_name(&workspace_root)),
                    model: config
                        .resolved_model
                        .clone()
                        .or_else(|| config.user_specified_model.clone()),
                    config_summary: None,
                    parent_session_id: None,
                    branch_from_seq: None,
                    migrated_from: None,
                },
            }
        }
    };

    let handle = SessionRecorderHandle::open(mode, record_config).await?;
    let mut guard = recorder_ref.lock();
    match guard.as_ref() {
        Some(existing) if existing.session_id() == session_id.as_str() => {
            Ok(Some(existing.clone()))
        }
        _ => {
            *guard = Some(handle.clone());
            Ok(Some(handle))
        }
    }
}

pub(crate) async fn record_session_items(
    recorder_ref: &Arc<Mutex<Option<SessionRecorderHandle>>>,
    config: &QueryEngineConfig,
    session_id: &SessionId,
    items: Vec<RecordItem>,
) -> anyhow::Result<()> {
    if items.is_empty() {
        return Ok(());
    }
    if let Some(handle) = ensure_session_recorder_handle(recorder_ref, config, session_id).await? {
        handle.add(items).await?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// QueryEngine
// ---------------------------------------------------------------------------

/// QueryEngine -- owns the full lifecycle of a single conversation session.
///
/// Each session creates exactly one `QueryEngine`. It wraps the inner
/// `query::loop_impl::query()` generator, intercepting every yielded item to
/// maintain cross-turn state and produce `SdkMessage` items for the caller.
pub struct QueryEngine {
    /// Session identifier (UUID v4).
    pub session_id: SessionId,
    /// Active session identifier used for new turns.
    ///
    /// `session_id` is kept for existing construction-time integrations. This
    /// mutable slot lets `/clear` detach from the previous transcript without
    /// rebuilding every `Arc<QueryEngine>` owner.
    pub(crate) active_session_id: Arc<RwLock<SessionId>>,
    /// Immutable configuration snapshot.
    pub(crate) config: QueryEngineConfig,
    /// Explicit runtime services for this engine instance.
    pub(crate) runtime_services: Arc<RuntimeServices>,

    /// Consolidated mutable session state.
    pub(crate) state: Arc<RwLock<QueryEngineState>>,
    /// Atomic abort flag (fast path for the query loop — no lock needed).
    pub(crate) aborted: Arc<AtomicBool>,
    /// Shared buffer of completed background agents.
    /// Event loop pushes; query loop drains.
    pub(crate) pending_bg_results: crate::agent_runtime::PendingBackgroundResults,
    pub(crate) active_steer_state: Arc<Mutex<ActiveSteerState>>,
    /// Hook runner for the tool-execution hook system.
    ///
    /// Defaults to [`allthecodes_types::hooks::NoopHookRunner`]. Call sites that want
    /// real shell-command hooks wire in the concrete `ShellHookRunner` via
    /// [`QueryEngine::set_hook_runner`] at construction time.
    pub(crate) hook_runner: Arc<dyn allthecodes_types::hooks::HookRunner>,
    /// Slash-command dispatcher used by input processing.
    ///
    /// Defaults to [`allthecodes_types::commands::NoopCommandDispatcher`]. Call sites
    /// wire in `DefaultCommandDispatcher` from the main crate's `commands::`
    /// module via [`QueryEngine::set_command_dispatcher`].
    pub(crate) command_dispatcher: Arc<dyn allthecodes_types::commands::CommandDispatcher>,
    /// Slash-command executor used after the dispatcher has parsed input.
    pub(crate) command_executor: Arc<dyn crate::command_runtime::CommandExecutor>,
    /// Async callback for computing auto-mode classifier decisions.
    /// Called with (tool_name, tool_input, classifier_input, messages, cwd)
    /// when mode is Auto.
    /// Returns `None` if the classifier is unavailable or skipped.
    pub(crate) auto_classifier_fn: Option<AutoClassifierFn>,
    /// Durable append-only recorder for the active session.
    pub(crate) session_recorder: Arc<Mutex<Option<SessionRecorderHandle>>>,
}

impl QueryEngine {
    // -- Construction --------------------------------------------------------

    /// Create a new QueryEngine with the given configuration.
    pub fn new(config: QueryEngineConfig) -> Self {
        let runtime_services = Arc::new(RuntimeServices::from_static_tools(config.tools.clone()));
        Self::new_with_services(config, runtime_services)
    }

    /// Create a new QueryEngine with explicit runtime services.
    pub fn new_with_services(
        config: QueryEngineConfig,
        runtime_services: Arc<RuntimeServices>,
    ) -> Self {
        let initial_messages = config.initial_messages.clone().unwrap_or_default();
        let tools = runtime_services.tool_registry.active_tools();
        let session_id = SessionId::new();

        // Initialize AppState with resolved model from config
        let mut app_state = AppState::default();
        if let Some(ref model) = config.resolved_model {
            app_state.main_loop_model = model.clone();
            app_state.settings.model = Some(model.clone());
        }
        if let Some(agent_context) = config.agent_context.as_ref() {
            app_state.team_context = agent_context.team_context.clone();
            if let Some(permission_context) = agent_context.tool_permission_context.clone() {
                app_state.tool_permission_context = permission_context;
            }
        }

        // Initialize session memory service and load existing entries
        let mut session_memory = SessionMemoryService::new(SessionMemoryConfig::default());
        if let Err(e) = session_memory.load_from_disk() {
            tracing::warn!(error = %e, "failed to load session memory from disk");
        }

        Self {
            session_id: session_id.clone(),
            active_session_id: Arc::new(RwLock::new(session_id)),
            config,
            runtime_services: runtime_services.clone(),
            state: Arc::new(RwLock::new(EngineSharedState {
                transcript: state::TranscriptState {
                    messages: initial_messages,
                    usage: UsageTracking::default(),
                    total_turn_count: 0,
                },
                permissions: state::PermissionState {
                    denials: Vec::new(),
                    permission_callback: None,
                    ask_user_callback: None,
                    permission_event_callback: None,
                    auto_denial_tracker: crate::permissions::decision::DenialTracker::default(),
                    auto_review_tracker: AutoReviewTracker::default(),
                },
                tools: state::ToolRuntimeState {
                    registry: tools,
                    file_state_cache: crate::types::tool::FileStateCache::default(),
                    discovered_skill_names: HashSet::new(),
                    loaded_nested_memory_paths: HashSet::new(),
                },
                runtime: state::SessionRuntimeState {
                    abort_reason: None,
                    goal_runtime: types::GoalRuntimeState::default(),
                    bg_agent_tx: None,
                    tool_progress_callback: None,
                    sleep_until: None,
                    session_memory,
                    audit_ctx: AuditContext::noop("pending"),
                },
                app_state,
            })),
            aborted: Arc::new(AtomicBool::new(false)),
            pending_bg_results: crate::agent_runtime::PendingBackgroundResults::new(),
            active_steer_state: Arc::new(Mutex::new(ActiveSteerState::default())),
            hook_runner: runtime_services.hook_runner.hook_runner(),
            command_dispatcher: runtime_services.command_dispatcher.command_dispatcher(),
            command_executor: crate::command_runtime::global_command_executor(),
            auto_classifier_fn: None,
            session_recorder: Arc::new(Mutex::new(None)),
        }
    }

    /// Ensure the active session has an open durable record log.
    pub async fn ensure_session_recorder(&self) -> anyhow::Result<Option<SessionRecorderHandle>> {
        ensure_session_recorder_handle(
            &self.session_recorder,
            &self.config,
            &self.current_session_id(),
        )
        .await
    }

    /// Append canonical record items for the active session.
    pub async fn record_items(&self, items: Vec<RecordItem>) -> anyhow::Result<()> {
        record_session_items(
            &self.session_recorder,
            &self.config,
            &self.current_session_id(),
            items,
        )
        .await
    }

    /// Append one typed message to the durable record log.
    pub async fn record_message(&self, message: &Message) -> anyhow::Result<()> {
        self.record_items(vec![RecordItem::Message(MessageRecord::from_message(
            message,
        ))])
        .await
    }

    /// Flush pending record items without closing the recorder.
    pub async fn flush_session_record(&self) -> anyhow::Result<Option<RecorderStats>> {
        let handle = self.session_recorder.lock().clone();
        match handle {
            Some(handle) => handle.flush().await.map(Some),
            None => Ok(None),
        }
    }

    /// Flush and close the active session recorder.
    pub async fn shutdown_session_record(&self) -> anyhow::Result<Option<RecorderStats>> {
        let handle = self.session_recorder.lock().take();
        match handle {
            Some(handle) => match handle.shutdown().await {
                Ok(stats) => Ok(Some(stats)),
                Err(error) => {
                    *self.session_recorder.lock() = Some(handle);
                    Err(error)
                }
            },
            None => Ok(None),
        }
    }

    /// Install a concrete hook runner (normally `allthecodes_tools::hooks::ShellHookRunner`).
    ///
    /// Must be called before `submit_message` if runtime hook firing is
    /// desired; otherwise the [`allthecodes_types::hooks::NoopHookRunner`] default is
    /// used and no hooks execute.
    pub fn set_hook_runner(&mut self, runner: Arc<dyn allthecodes_types::hooks::HookRunner>) {
        self.hook_runner = runner;
    }

    /// Clone of the current hook runner.  Useful when rebuilding a sibling
    /// engine that should share the same runner as an existing one.
    pub fn hook_runner(&self) -> Arc<dyn allthecodes_types::hooks::HookRunner> {
        self.hook_runner.clone()
    }

    /// Install a concrete command dispatcher (normally
    /// `DefaultCommandDispatcher` from `crate::commands`).
    pub fn set_command_dispatcher(
        &mut self,
        dispatcher: Arc<dyn allthecodes_types::commands::CommandDispatcher>,
    ) {
        self.command_dispatcher = dispatcher;
    }

    /// Clone of the current command dispatcher.
    pub fn command_dispatcher(&self) -> Arc<dyn allthecodes_types::commands::CommandDispatcher> {
        self.command_dispatcher.clone()
    }

    pub fn set_command_executor(
        &mut self,
        executor: Arc<dyn crate::command_runtime::CommandExecutor>,
    ) {
        self.command_executor = executor;
    }

    pub fn command_executor(&self) -> Arc<dyn crate::command_runtime::CommandExecutor> {
        self.command_executor.clone()
    }

    /// Install an auto-mode classifier callback.
    ///
    /// When set, the engine will call this closure in Auto mode for
    /// non-allowlisted tools to decide whether to allow/deny/ask.
    pub fn set_auto_classifier_fn(&mut self, f: Option<AutoClassifierFn>) {
        self.auto_classifier_fn = f;
    }

    pub fn pending_background_results(&self) -> crate::agent_runtime::PendingBackgroundResults {
        self.pending_bg_results.clone()
    }

    // -- Permission callback --------------------------------------------------

    /// Set the async permission callback used by headless/TUI mode.
    /// When a tool requires `Ask` permission, this callback is invoked
    /// to prompt the user via IPC instead of immediately denying.
    pub fn set_permission_callback(&self, cb: crate::types::tool::PermissionCallback) {
        self.state.write().permissions.permission_callback = Some(cb);
    }

    /// Replace the async permission callback and return the previous callback.
    pub fn replace_permission_callback(
        &self,
        cb: Option<crate::types::tool::PermissionCallback>,
    ) -> Option<crate::types::tool::PermissionCallback> {
        std::mem::replace(&mut self.state.write().permissions.permission_callback, cb)
    }

    /// Remove the permission callback, restoring default behaviour (deny).
    pub fn clear_permission_callback(&self) {
        self.state.write().permissions.permission_callback = None;
    }

    /// Set the async AskUserQuestion callback used by headless/TUI mode.
    pub fn set_ask_user_callback(&self, cb: crate::types::tool::AskUserCallback) {
        self.state.write().permissions.ask_user_callback = Some(cb);
    }

    /// Remove the AskUserQuestion callback.
    pub fn clear_ask_user_callback(&self) {
        self.state.write().permissions.ask_user_callback = None;
    }

    pub fn set_permission_event_callback(&self, cb: crate::types::tool::PermissionEventCallback) {
        self.state.write().permissions.permission_event_callback = Some(cb);
    }

    /// Set the background agent sender (called by headless/TUI at startup).
    pub fn set_bg_agent_tx(&self, tx: allthecodes_types::agent_channel::AgentSender) {
        self.state.write().runtime.bg_agent_tx = Some(tx);
    }

    /// Install a `ToolProgress` callback.
    ///
    /// Headless/TUI mode wires this to a closure that serializes the
    /// progress event into a `BackendMessage::ToolProgress` and sends it
    /// through [`FrontendSink`]. The query loop reads the callback via
    /// [`QueryEngineDeps::tool_progress_callback`] on every tool batch.
    pub fn set_tool_progress_callback(
        &self,
        cb: Arc<dyn Fn(crate::types::tool::ToolProgress) + Send + Sync>,
    ) {
        self.state.write().runtime.tool_progress_callback = Some(cb);
    }

    // -- Sleep control -------------------------------------------------------

    /// Put the engine to sleep until the given instant.
    /// The proactive tick loop will skip ticks while `is_sleeping()` returns true.
    pub fn set_sleep_until(&self, until: std::time::Instant) {
        let mut state = self.state.write();
        state.runtime.sleep_until = Some(until);
    }

    /// Check whether the engine is currently sleeping.
    pub fn is_sleeping(&self) -> bool {
        let state = self.state.read();
        state
            .runtime
            .sleep_until
            .is_some_and(|t| std::time::Instant::now() < t)
    }

    /// Wake the engine up, clearing any pending sleep.
    /// Called on user messages, webhooks, or other external events.
    pub fn wake_up(&self) {
        let mut state = self.state.write();
        state.runtime.sleep_until = None;
    }

    // -- Abort control -------------------------------------------------------

    /// Abort the currently running query.
    pub fn abort(&self) {
        info!("aborting query engine");
        self.aborted.store(true, Ordering::SeqCst);
        let session_id = self.current_session_id();
        {
            let mut state = self.state.write();
            state.runtime.abort_reason = Some(AbortReason::UserAbort);
            state.runtime.goal_runtime.clear_active();
        }
        if let Err(error) = pause_active_goal_for_abort(session_id.as_str()) {
            warn!(%error, "failed to pause active goal after abort");
        }
    }

    pub fn submit_steer_message(&self, text: String) -> Result<(), SteerError> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Ok(());
        }
        let mut state = self.active_steer_state.lock();
        if !state.active {
            return Err(SteerError::no_active_turn());
        }
        state.pending.push_back(trimmed.to_string());
        Ok(())
    }

    /// Reset the abort flag before starting a new `submit_message` call.
    pub fn reset_abort(&self) {
        self.aborted.store(false, Ordering::SeqCst);
        self.state.write().runtime.abort_reason = None;
    }

    /// Check whether the engine has been aborted.
    pub fn is_aborted(&self) -> bool {
        self.aborted.load(Ordering::Relaxed)
    }

    /// Get the abort reason (if any).
    pub fn abort_reason(&self) -> Option<AbortReason> {
        self.state.read().runtime.abort_reason.clone()
    }

    // -- Accessors -----------------------------------------------------------

    /// Get a snapshot of the current message history.
    pub fn messages(&self) -> Vec<Message> {
        self.state.read().transcript.messages.clone()
    }

    /// Get the session id that new turns should use.
    pub fn current_session_id(&self) -> SessionId {
        self.active_session_id.read().clone()
    }

    /// Set the active session id for future turns.
    pub fn set_current_session_id(&self, session_id: SessionId) {
        *self.active_session_id.write() = session_id;
        crate::bootstrap::PROCESS_STATE.write().session_id = self.current_session_id();
    }

    /// Clear runtime conversation state and start writing future turns to a
    /// fresh session id.
    pub fn start_new_session(&self) -> SessionId {
        let previous_id = self.current_session_id();
        let previous_messages = self.messages();
        if self.config.auto_save_session && !previous_messages.is_empty() {
            if let Err(err) = crate::session::storage::save_session(
                previous_id.as_str(),
                &previous_messages,
                &self.config.cwd,
            ) {
                warn!(
                    error = %err,
                    session = %previous_id,
                    "failed to save previous session before starting a new one"
                );
            }
        }

        let session_id = SessionId::new();
        {
            let mut state = self.state.write();
            state.transcript.messages.clear();
            state.transcript.usage = UsageTracking::default();
            state.permissions.denials.clear();
            state.transcript.total_turn_count = 0;
        }
        self.set_current_session_id(session_id.clone());
        session_id
    }

    /// Replace the full conversation history.
    pub fn replace_messages(&self, messages: Vec<Message>) {
        self.state.write().transcript.messages = messages;
    }

    /// Get a snapshot of usage tracking.
    pub fn usage(&self) -> UsageTracking {
        self.state.read().transcript.usage.clone()
    }

    /// Get a snapshot of permission denials.
    pub fn permission_denials(&self) -> Vec<PermissionDenial> {
        self.state.read().permissions.denials.clone()
    }

    /// Record a permission denial.
    pub fn record_permission_denial(&self, denial: PermissionDenial) {
        self.state.write().record_permission_denial(denial);
    }

    /// Get the total turn count (across all submit_message calls).
    pub fn total_turn_count(&self) -> usize {
        self.state.read().transcript.total_turn_count
    }

    /// Get a snapshot of the application state.
    pub fn app_state(&self) -> AppState {
        self.state.read().app_state.clone()
    }

    /// Update the application state with a closure.
    pub fn update_app_state<F>(&self, updater: F)
    where
        F: FnOnce(&mut AppState),
    {
        updater(&mut self.state.write().app_state);
    }

    /// Get the working directory.
    pub fn cwd(&self) -> &str {
        &self.config.cwd
    }

    /// Get a reference to the engine's immutable configuration.
    ///
    /// Used by the web layer to clone the config when rebuilding an engine
    /// on `/api/sessions/new` or `/api/sessions/:id/resume`.
    pub fn config_ref(&self) -> &QueryEngineConfig {
        &self.config
    }

    /// Replace the tool registry.
    pub fn set_tools(&self, tools: Tools) {
        self.state.write().set_tools(tools);
    }

    /// Get the names of registered tools (used by web/state API).
    pub fn tool_names(&self) -> Vec<String> {
        self.state
            .read()
            .tools
            .registry
            .iter()
            .map(|t| t.name().to_string())
            .collect()
    }

    /// Get a snapshot of the current tool registry.
    pub fn tools_snapshot(&self) -> Tools {
        self.state.read().tools.registry.clone()
    }

    /// Set the audit context (called after AuditSink is initialized).
    pub fn set_audit_context(&self, ctx: AuditContext) {
        self.state.write().runtime.audit_ctx = ctx;
    }

    /// Get a clone of the current audit context.
    pub fn audit_context(&self) -> AuditContext {
        self.state.read().runtime.audit_ctx.clone()
    }

    /// Get discovered skill names from the current turn.
    pub fn discovered_skill_names(&self) -> HashSet<String> {
        self.state.read().tools.discovered_skill_names.clone()
    }

    /// Get loaded nested memory paths.
    pub fn loaded_nested_memory_paths(&self) -> HashSet<String> {
        self.state.read().tools.loaded_nested_memory_paths.clone()
    }

    /// Check if session memory extraction should be triggered, and if so,
    /// extract a simple insight from the last assistant turn.
    pub fn try_extract_session_memory(&self) {
        let mut state = self.state.write();
        let msg_count = state.transcript.messages.len();
        if !state.runtime.session_memory.should_extract(msg_count) {
            return;
        }

        // Find the last assistant message content for extraction.
        let last_assistant = state
            .transcript
            .messages
            .iter()
            .rev()
            .find_map(|m| match m {
                Message::Assistant(a) => {
                    let text: String = a
                        .content
                        .iter()
                        .filter_map(|b| match b {
                            ContentBlock::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    if text.is_empty() {
                        None
                    } else {
                        Some(text)
                    }
                }
                _ => None,
            });

        let Some(assistant_text) = last_assistant else {
            return;
        };

        let last_user = state
            .transcript
            .messages
            .iter()
            .rev()
            .find_map(|m| match m {
                Message::User(u) if !u.is_meta && u.tool_use_result.is_none() => match &u.content {
                    MessageContent::Text(text) => Some(text.clone()),
                    MessageContent::Blocks(blocks) => {
                        let text = blocks
                            .iter()
                            .filter_map(|block| match block {
                                ContentBlock::Text { text } => Some(text.as_str()),
                                _ => None,
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                        (!text.is_empty()).then_some(text)
                    }
                },
                _ => None,
            });

        let Some(insight) = extract_session_insight(last_user.as_deref(), &assistant_text) else {
            return;
        };

        let entry = crate::services::session_memory::MemoryEntry {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            session_id: self.session_id.to_string(),
            workspace: Some(self.config.cwd.clone()),
            content: insight.content,
            tags: insight.tags,
        };

        if let Err(e) = state.runtime.session_memory.save_entry(entry) {
            tracing::warn!(error = %e, "failed to save session memory entry");
        }
    }
}
