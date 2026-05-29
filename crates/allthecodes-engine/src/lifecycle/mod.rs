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
mod submit_message;
#[cfg(test)]
mod tests;
mod types;

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
use crate::services::session_memory::{
    extract_session_insight, SessionMemoryConfig, SessionMemoryService,
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

// ---------------------------------------------------------------------------
// QueryEngineState — consolidated mutable session state
// ---------------------------------------------------------------------------

/// All mutable session state behind a single `Arc<RwLock<_>>`.
///
/// Previously each field was an independent `Arc<Mutex<T>>` or `Arc<RwLock<T>>`,
/// requiring 10 individual clones in `submit_message`. Now there's one lock to
/// rule them all — simpler to reason about and fewer clones.
pub(crate) struct QueryEngineState {
    /// Conversation message history.
    pub(crate) messages: Vec<Message>,
    /// Abort reason (if aborted).
    pub(crate) abort_reason: Option<AbortReason>,
    /// Accumulated usage across all API calls.
    pub(crate) usage: UsageTracking,
    /// History of permission denials.
    pub(crate) permission_denials: Vec<PermissionDenial>,
    /// Total turn count across all `submit_message` invocations.
    pub(crate) total_turn_count: usize,
    /// Application-wide state (shared with deps).
    pub(crate) app_state: AppState,
    /// Current tool registry (shared with deps).
    pub(crate) tools: Tools,
    /// File snapshots observed by Read/Edit tools, used to reject stale edits.
    pub(crate) file_state_cache: crate::types::tool::FileStateCache,
    /// Skills discovered during this session (dedup).
    pub(crate) discovered_skill_names: HashSet<String>,
    /// Nested memory paths already loaded (dedup).
    pub(crate) loaded_nested_memory_paths: HashSet<String>,
    /// Async callback for interactive permission prompts (set by headless/TUI).
    pub(crate) permission_callback: Option<crate::types::tool::PermissionCallback>,
    /// Async callback for AskUserQuestion prompts (set by headless/TUI).
    pub(crate) ask_user_callback: Option<crate::types::tool::AskUserCallback>,
    /// Callback for permission-side informational UI events.
    pub(crate) permission_event_callback: Option<crate::types::tool::PermissionEventCallback>,
    /// Sender for background agent completion channel.
    /// Set by headless/TUI mode; cloned into ToolUseContext.
    pub(crate) bg_agent_tx: Option<allthecodes_types::agent_channel::AgentSender>,
    /// Callback invoked on every [`ToolProgress`] emitted by a tool.
    /// Set by headless/TUI mode; read by `QueryEngineDeps::tool_progress_callback`
    /// and plumbed down to `execute_tool_calls` so tools (notably Bash) can
    /// surface live output back to the frontend.
    pub(crate) tool_progress_callback:
        Option<Arc<dyn Fn(crate::types::tool::ToolProgress) + Send + Sync>>,
    /// If set, the engine is "sleeping" until this instant.
    /// The proactive tick loop skips ticks while `Instant::now() < sleep_until`.
    /// Cleared by `wake_up()` on user messages or external events.
    pub(crate) sleep_until: Option<std::time::Instant>,
    /// Session memory service for extracting and persisting conversation insights.
    pub(crate) session_memory: SessionMemoryService,
    /// Runtime audit context for emitting structured events.
    pub(crate) audit_ctx: AuditContext,
    /// Auto-mode classifier denial state for interactive fallback.
    pub(crate) auto_denial_tracker: crate::permissions::decision::DenialTracker,
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
}

impl QueryEngine {
    // -- Construction --------------------------------------------------------

    /// Create a new QueryEngine with the given configuration.
    pub fn new(config: QueryEngineConfig) -> Self {
        let initial_messages = config.initial_messages.clone().unwrap_or_default();
        let tools = config.tools.clone();
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
            state: Arc::new(RwLock::new(QueryEngineState {
                messages: initial_messages,
                abort_reason: None,
                usage: UsageTracking::default(),
                permission_denials: Vec::new(),
                total_turn_count: 0,
                app_state,
                tools,
                file_state_cache: crate::types::tool::FileStateCache::default(),
                discovered_skill_names: HashSet::new(),
                loaded_nested_memory_paths: HashSet::new(),
                permission_callback: None,
                ask_user_callback: None,
                permission_event_callback: None,
                bg_agent_tx: None,
                tool_progress_callback: None,
                sleep_until: None,
                session_memory,
                audit_ctx: AuditContext::noop("pending"),
                auto_denial_tracker: crate::permissions::decision::DenialTracker::default(),
            })),
            aborted: Arc::new(AtomicBool::new(false)),
            pending_bg_results: crate::agent_runtime::PendingBackgroundResults::new(),
            active_steer_state: Arc::new(Mutex::new(ActiveSteerState::default())),
            hook_runner: Arc::new(allthecodes_types::hooks::NoopHookRunner::new()),
            command_dispatcher: Arc::new(allthecodes_types::commands::NoopCommandDispatcher::new()),
            command_executor: crate::command_runtime::global_command_executor(),
            auto_classifier_fn: None,
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
        self.state.write().permission_callback = Some(cb);
    }

    /// Remove the permission callback, restoring default behaviour (deny).
    pub fn clear_permission_callback(&self) {
        self.state.write().permission_callback = None;
    }

    /// Set the async AskUserQuestion callback used by headless/TUI mode.
    pub fn set_ask_user_callback(&self, cb: crate::types::tool::AskUserCallback) {
        self.state.write().ask_user_callback = Some(cb);
    }

    /// Remove the AskUserQuestion callback.
    pub fn clear_ask_user_callback(&self) {
        self.state.write().ask_user_callback = None;
    }

    pub fn set_permission_event_callback(&self, cb: crate::types::tool::PermissionEventCallback) {
        self.state.write().permission_event_callback = Some(cb);
    }

    /// Set the background agent sender (called by headless/TUI at startup).
    pub fn set_bg_agent_tx(&self, tx: allthecodes_types::agent_channel::AgentSender) {
        self.state.write().bg_agent_tx = Some(tx);
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
        self.state.write().tool_progress_callback = Some(cb);
    }

    // -- Sleep control -------------------------------------------------------

    /// Put the engine to sleep until the given instant.
    /// The proactive tick loop will skip ticks while `is_sleeping()` returns true.
    pub fn set_sleep_until(&self, until: std::time::Instant) {
        let mut state = self.state.write();
        state.sleep_until = Some(until);
    }

    /// Check whether the engine is currently sleeping.
    pub fn is_sleeping(&self) -> bool {
        let state = self.state.read();
        state
            .sleep_until
            .is_some_and(|t| std::time::Instant::now() < t)
    }

    /// Wake the engine up, clearing any pending sleep.
    /// Called on user messages, webhooks, or other external events.
    pub fn wake_up(&self) {
        let mut state = self.state.write();
        state.sleep_until = None;
    }

    // -- Abort control -------------------------------------------------------

    /// Abort the currently running query.
    pub fn abort(&self) {
        info!("aborting query engine");
        self.aborted.store(true, Ordering::SeqCst);
        self.state.write().abort_reason = Some(AbortReason::UserAbort);
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
        self.state.write().abort_reason = None;
    }

    /// Check whether the engine has been aborted.
    pub fn is_aborted(&self) -> bool {
        self.aborted.load(Ordering::Relaxed)
    }

    /// Get the abort reason (if any).
    pub fn abort_reason(&self) -> Option<AbortReason> {
        self.state.read().abort_reason.clone()
    }

    // -- Accessors -----------------------------------------------------------

    /// Get a snapshot of the current message history.
    pub fn messages(&self) -> Vec<Message> {
        self.state.read().messages.clone()
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
            state.messages.clear();
            state.usage = UsageTracking::default();
            state.permission_denials.clear();
            state.total_turn_count = 0;
        }
        self.set_current_session_id(session_id.clone());
        session_id
    }

    /// Replace the full conversation history.
    pub fn replace_messages(&self, messages: Vec<Message>) {
        self.state.write().messages = messages;
    }

    /// Get a snapshot of usage tracking.
    pub fn usage(&self) -> UsageTracking {
        self.state.read().usage.clone()
    }

    /// Get a snapshot of permission denials.
    pub fn permission_denials(&self) -> Vec<PermissionDenial> {
        self.state.read().permission_denials.clone()
    }

    /// Record a permission denial.
    pub fn record_permission_denial(&self, denial: PermissionDenial) {
        self.state.write().permission_denials.push(denial);
    }

    /// Get the total turn count (across all submit_message calls).
    pub fn total_turn_count(&self) -> usize {
        self.state.read().total_turn_count
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
        self.state.write().tools = tools;
    }

    /// Get the names of registered tools (used by web/state API).
    pub fn tool_names(&self) -> Vec<String> {
        self.state
            .read()
            .tools
            .iter()
            .map(|t| t.name().to_string())
            .collect()
    }

    /// Set the audit context (called after AuditSink is initialized).
    pub fn set_audit_context(&self, ctx: AuditContext) {
        self.state.write().audit_ctx = ctx;
    }

    /// Get a clone of the current audit context.
    pub fn audit_context(&self) -> AuditContext {
        self.state.read().audit_ctx.clone()
    }

    /// Get discovered skill names from the current turn.
    pub fn discovered_skill_names(&self) -> HashSet<String> {
        self.state.read().discovered_skill_names.clone()
    }

    /// Get loaded nested memory paths.
    pub fn loaded_nested_memory_paths(&self) -> HashSet<String> {
        self.state.read().loaded_nested_memory_paths.clone()
    }

    /// Check if session memory extraction should be triggered, and if so,
    /// extract a simple insight from the last assistant turn.
    pub fn try_extract_session_memory(&self) {
        let mut state = self.state.write();
        let msg_count = state.messages.len();
        if !state.session_memory.should_extract(msg_count) {
            return;
        }

        // Find the last assistant message content for extraction.
        let last_assistant = state.messages.iter().rev().find_map(|m| match m {
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

        let last_user = state.messages.iter().rev().find_map(|m| match m {
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

        if let Err(e) = state.session_memory.save_entry(entry) {
            tracing::warn!(error = %e, "failed to save session memory entry");
        }
    }
}
