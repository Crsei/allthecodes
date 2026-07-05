pub mod agent_navigation;
mod agent_tree_dialog;
pub mod app_event;
pub mod app_event_sender;
mod domain;
mod input;
mod overlays;
mod render;
mod runtime_state;
pub mod status;
#[cfg(test)]
#[path = "app/tests.rs"]
mod tests;
mod transcript_mode;
mod voice;
mod workspace_trust;

use agent_navigation::{AgentThreadEntry, AgentThreadStatus};
use agent_tree_dialog::AgentTreeDialog;
use allthecodes_config::settings::{SpinnerTipsSettings, StatusLineSettings};
use allthecodes_ipc_protocol::BackendMessage;
use allthecodes_keybindings::KeybindingRegistry;
use allthecodes_services::prompt_suggestion::PromptSuggestion;
use allthecodes_types::agent_events::{AgentEvent, TeamEvent};
use allthecodes_types::callbacks::AskUserRequestPayload;
use allthecodes_types::message::{Message, ProgressMessage};
use allthecodes_types::tool_operation::{OperationKind, ToolOperation};
use allthecodes_voice::VoiceController;
use ratatui::layout::Rect;
use status::SessionUsageSnapshot;
use workspace_trust::is_workspace_trusted;

use super::command_palette::CommandPalette;
use super::command_surface::{CommandSurface, CommandSurfaceTarget};
use super::history_search_dialog::HistorySearchEntry;
use super::notifications::in_app::{
    InAppNotification, NotificationPriority, NotificationState, NotificationTone,
};
use super::overlays::dialog::ExitGuard;
use super::permissions::worker_pending_permission::render_worker_pending_permission;
use super::permissions::{
    BypassPermissionsModeChoice, BypassPermissionsModeDialog, PermissionChoice, PermissionDialog,
    PermissionDialogRequest, QuestionDialog,
};
use super::prompt_input::PromptInput;
use super::spinner::SpinnerState;
use super::status_line::StatusLineRunner;
use super::terminal_env::TerminalEnvConfig;
use super::theme::{Theme, ThemeProvider};
use super::transcript::{TranscriptState, ViewMode};
use super::vim::VimState;
use app_event::AppEvent;
use domain::{ConversationStore, PromptQueueStore, RenderLayoutStore, SessionUiStore};
use overlays::OverlayState;
use runtime_state::RuntimeViewState;

#[cfg(test)]
pub(crate) use overlays::ActiveOverlay;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoalStatusSnapshot {
    pub event: String,
    pub objective: String,
    pub status: String,
    pub tokens_used: u64,
    pub time_used_seconds: u64,
}

/// Actions produced by the app in response to user input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppAction {
    None,
    Submit(String),
    Steer(String),
    Abort,
    Quit,
    ScrollUp,
    ScrollDown,
    Queue(String),
    PermissionResponse(PermissionChoice),
    QuestionResponse(String),
    BypassPermissionsModeResponse(BypassPermissionsModeChoice),
    AgentThreadSelected(String),
    KillAgentThreads(Vec<String>),
    LspRecommendationResponse {
        request_id: String,
        plugin_name: String,
        decision: String,
    },
    /// Transcript mode requested an export to `$EDITOR`. Carries the
    /// pre-rendered markdown body; the caller writes it to disk and
    /// spawns the editor so `App` stays free of IO.
    ExportTranscript(String),
    CopyMessage(String),
    OpenPath(String),
    DebugSnapshot,
}

#[derive(Debug, Clone, Copy)]
struct SessionScrollbarState {
    area: Rect,
    total_lines: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MouseFocus {
    Messages,
    Prompt,
}

fn notification_from_app_event(
    key: String,
    message: String,
    level: String,
    timeout_ms: Option<u64>,
) -> InAppNotification {
    let (priority, tone) = match level.to_ascii_lowercase().as_str() {
        "immediate" => (NotificationPriority::Immediate, NotificationTone::Error),
        "error" => (NotificationPriority::High, NotificationTone::Error),
        "warning" | "warn" => (NotificationPriority::High, NotificationTone::Warning),
        "high" => (NotificationPriority::High, NotificationTone::Info),
        "low" => (NotificationPriority::Low, NotificationTone::Dim),
        "dim" | "debug" => (NotificationPriority::Low, NotificationTone::Dim),
        "medium" | "info" | "notice" => (NotificationPriority::Medium, NotificationTone::Info),
        _ => (NotificationPriority::Medium, NotificationTone::Info),
    };
    let mut notification = InAppNotification::new(key, priority, message)
        .with_tone(tone)
        .with_fold(true);
    if let Some(timeout_ms) = timeout_ms {
        notification = notification.with_timeout_ms(timeout_ms);
    }
    notification
}

fn notification_from_backend_message(message: &BackendMessage) -> Option<InAppNotification> {
    match message {
        BackendMessage::NotificationSent { title, level } => Some(notification_from_app_event(
            "backend-notification".to_string(),
            title.clone(),
            level.clone(),
            Some(4500),
        )),
        BackendMessage::Error { message, .. } => Some(notification_from_app_event(
            "backend-error".to_string(),
            message.clone(),
            "error".to_string(),
            Some(8000),
        )),
        BackendMessage::HookPermissionDecision { event } => Some(
            InAppNotification::new(
                "hook-permission-decision",
                NotificationPriority::Low,
                format!(
                    "hook: {}\nmatcher: {}\ndecision: {}",
                    event.hook_name, event.matcher, event.decision
                ),
            )
            .with_tone(NotificationTone::Info)
            .with_fold(true)
            .with_timeout_ms(5000),
        ),
        BackendMessage::PermissionDecisionDebug { event } => Some(
            InAppNotification::new(
                "permission-decision-debug",
                NotificationPriority::Low,
                format!(
                    "tool: {}\nmatcher: {}\nsource: {}\nbehavior: {}",
                    event.tool_name, event.matcher, event.source, event.behavior
                ),
            )
            .with_tone(NotificationTone::Dim)
            .with_fold(true)
            .with_timeout_ms(5000),
        ),
        BackendMessage::PermissionAutoReview { event } => Some(
            InAppNotification::new(
                "permission-auto-review",
                NotificationPriority::Low,
                format!(
                    "auto review: {}\naction: {}\nsource: {}",
                    event.status,
                    event.action.as_deref().unwrap_or("permission request"),
                    event.decision_source
                ),
            )
            .with_tone(match event.status.as_str() {
                "approved" => NotificationTone::Info,
                "denied" | "failed" | "timed_out" | "circuit_open" => NotificationTone::Warning,
                _ => NotificationTone::Dim,
            })
            .with_fold(true)
            .with_timeout_ms(5000),
        ),
        _ => None,
    }
}

/// Main TUI application state.
pub struct App {
    conversation: ConversationStore,
    prompt: PromptInput,
    is_streaming: bool,
    prompt_queue: PromptQueueStore,
    spinner_state: SpinnerState,
    overlays: OverlayState,
    exit_guard: ExitGuard,
    should_quit: bool,
    design_theme_provider: ThemeProvider,
    theme: Theme,
    session_ui: SessionUiStore,
    verbose: bool,
    brief_only: bool,
    permission_mode_label: String,
    sandbox_label: String,
    effort_label: Option<String>,
    remote_indicator_label: Option<String>,
    active_goal: Option<GoalStatusSnapshot>,
    session_cost_usd: f64,
    /// Whether the welcome screen is currently shown.
    show_welcome: bool,
    /// Startup trust gate shown before the normal welcome panel.
    workspace_trust_pending: bool,
    workspace_trust_selection: usize,
    history_index: Option<usize>,
    saved_input: String,
    command_palette: CommandPalette,
    pending_command_surface_after_submit: Option<CommandSurfaceTarget>,
    runtime_view: RuntimeViewState,
    show_agent_footer: bool,

    // Prompt suggestions
    /// Next-prompt suggestions shown after an assistant turn completes.
    suggestions: Option<Vec<PromptSuggestion>>,
    notifications: NotificationState,

    // Optimizations
    render_layout: RenderLayoutStore,
    /// Area the user last clicked or scrolled over.
    mouse_focus: MouseFocus,
    /// Dirty flag; when false, the TUI skips `terminal.draw()`.
    dirty: bool,
    /// Last rendered terminal frame captured as plain text for debug export.
    last_render_snapshot: Option<String>,
    /// Tick counter for throttling spinner frame advances.
    tick_counter: u32,
    keybindings: KeybindingRegistry,
    pending_chord: Vec<allthecodes_keybindings::keystroke::Keystroke>,
    vim: VimState,

    // Scriptable status line (issue #11)
    /// Resolved status-line configuration (command, padding, intervals).
    /// Populated from effective settings at TUI startup; further updates
    /// go through [`Self::update_status_line_settings`] when the user
    /// reloads config.
    status_line_settings: StatusLineSettings,
    /// Subprocess runner shared handle used by `/statusline` as well.
    status_line_runner: StatusLineRunner,
    /// Accumulated usage / cost for the current session (fed to the
    /// status-line payload). Updated from engine `Result` events.
    session_usage: SessionUsageSnapshot,

    // Transcript / focus view + terminal env (issue #12)
    /// Which view the user is currently in; cycled with `Ctrl+O`.
    view_mode: ViewMode,
    /// Extra state only used in transcript/focus modes.
    transcript_state: TranscriptState,
    /// Env-driven terminal config: `ALLTHECODES_NO_FLICKER`,
    /// `ALLTHECODES_ENABLE_MOUSE_CAPTURE`, `ALLTHECODES_DISABLE_MOUSE`,
    /// `ALLTHECODES_SCROLL_SPEED`.
    terminal_env: TerminalEnvConfig,

    // Voice dictation (issue #13)
    /// Push-to-talk controller. `None` until the TUI runner installs
    /// one via [`Self::set_voice_controller`]. Default construction
    /// (e.g. in tests) leaves it `None` so nothing accidentally records.
    voice: Option<VoiceController>,
    /// Snapshot of the effective `voiceEnabled` + `language` settings.
    /// Updated whenever `/voice` / `/config set` flips them so the
    /// push-to-talk key handler doesn't need to re-read AppState.
    voice_enabled: bool,
    /// Whether this build/runtime supports actual recording and STT.
    voice_supported: bool,
    /// Normalized STT language passed to the controller on press.
    voice_language: String,

    /// Whether audible/desktop terminal notifications are enabled.
    /// This does not affect in-app notifications.
    sound_effects: bool,
    /// Whether supported terminals should receive OSC 9;4 progress updates.
    terminal_progress_bar_enabled: bool,

    // Completion state (Lane E)
    /// Tracks the active completion session for the input prompt.
    completion_state: input::CompletionState,
}

impl App {
    pub fn new() -> Self {
        let design_theme_provider = ThemeProvider::from_user_settings();
        let theme = design_theme_provider.legacy_theme();
        Self {
            conversation: ConversationStore::default(),
            prompt: PromptInput::new(),
            is_streaming: false,
            prompt_queue: PromptQueueStore::default(),
            spinner_state: SpinnerState::new(),
            overlays: OverlayState::default(),
            exit_guard: ExitGuard::new(),
            should_quit: false,
            design_theme_provider,
            theme,
            session_ui: SessionUiStore::default(),
            verbose: false,
            brief_only: false,
            permission_mode_label: String::new(),
            sandbox_label: String::new(),
            effort_label: None,
            remote_indicator_label: None,
            active_goal: None,
            session_cost_usd: 0.0,
            show_welcome: true,
            workspace_trust_pending: false,
            workspace_trust_selection: 0,
            suggestions: None,
            notifications: NotificationState::default(),
            history_index: None,
            saved_input: String::new(),
            command_palette: CommandPalette::new(),
            pending_command_surface_after_submit: None,
            runtime_view: RuntimeViewState::default(),
            show_agent_footer: true,
            render_layout: RenderLayoutStore::default(),
            mouse_focus: MouseFocus::Messages,
            dirty: true,
            last_render_snapshot: None,
            tick_counter: 0,
            keybindings: KeybindingRegistry::with_defaults(),
            pending_chord: Vec::new(),
            vim: VimState::new(),
            status_line_settings: StatusLineSettings::default(),
            status_line_runner: StatusLineRunner::new(),
            session_usage: SessionUsageSnapshot::default(),
            view_mode: ViewMode::default(),
            transcript_state: TranscriptState::default(),
            terminal_env: TerminalEnvConfig::default(),
            voice: None,
            voice_enabled: false,
            voice_supported: false,
            voice_language: "en".to_string(),
            sound_effects: true,
            terminal_progress_bar_enabled: true,
            completion_state: input::CompletionState::new(),
        }
    }

    // Dirty flag

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn mark_clean(&mut self) {
        self.dirty = false;
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn export_debug_snapshot(&self) -> std::io::Result<std::path::PathBuf> {
        let cwd = if self.session_ui.cwd.is_empty() {
            std::env::current_dir()?
        } else {
            std::path::PathBuf::from(&self.session_ui.cwd)
        };
        let dir = cwd.join("target").join("tui-snapshots");
        std::fs::create_dir_all(&dir)?;
        let timestamp = chrono::Local::now()
            .format("%Y%m%d-%H%M%S%.9f")
            .to_string()
            .replace('.', "-");
        let mut path = dir.join(format!("snapshot-{timestamp}.txt"));
        for suffix in 1.. {
            if !path.exists() {
                break;
            }
            path = dir.join(format!("snapshot-{timestamp}-{suffix}.txt"));
        }
        let body = self.debug_snapshot_body();
        std::fs::write(&path, body)?;
        Ok(path)
    }

    fn debug_snapshot_body(&self) -> String {
        let mut body = String::new();
        body.push_str("# allthecodes TUI debug snapshot\n\n");
        body.push_str(&format!(
            "exported_at: {}\n",
            chrono::Local::now().to_rfc3339()
        ));
        body.push_str(&format!("cwd: {}\n", self.session_ui.cwd));
        body.push_str(&format!("session_id: {}\n", self.session_ui.session_id));
        body.push_str(&format!("model: {}\n", self.session_ui.model_name));
        body.push_str(&format!("backend: {}\n", self.session_ui.backend_name));
        body.push_str(&format!(
            "command_surface: {}\n",
            self.overlays
                .command_surface
                .as_ref()
                .map(CommandSurface::title)
                .unwrap_or("none")
        ));
        body.push_str("\n```text\n");
        if let Some(snapshot) = &self.last_render_snapshot {
            body.push_str(snapshot);
            if !snapshot.ends_with('\n') {
                body.push('\n');
            }
        } else {
            body.push_str("<no rendered frame captured yet>\n");
        }
        body.push_str("```\n");
        body
    }

    // Public API

    pub fn add_message(&mut self, msg: Message) {
        // Dismiss welcome screen on first user or assistant message.
        if self.show_welcome && matches!(msg, Message::User(_) | Message::Assistant(_)) {
            self.show_welcome = false;
        }
        self.conversation.add_message(msg);
        self.sync_primary_agent_thread();
        self.scroll_to_bottom_deferred();
        self.dirty = true;
    }

    pub fn replace_last_message(&mut self, msg: Message) {
        self.conversation.replace_last_message(msg);
        self.sync_primary_agent_thread();
        self.scroll_to_bottom_deferred();
        self.dirty = true;
    }

    pub fn remove_last_message(&mut self) {
        if self.conversation.remove_last_message() {
            self.sync_primary_agent_thread();
            self.scroll_to_bottom_deferred();
            self.dirty = true;
        }
    }

    pub fn messages(&self) -> &[Message] {
        self.conversation.messages()
    }

    #[cfg(test)]
    pub fn selected_message(&self) -> Option<usize> {
        self.conversation.selection()
    }

    pub fn clear_messages(&mut self) {
        self.conversation.clear();
        self.runtime_view.agent_nav_mut().clear();
        self.runtime_view.set_current_agent_thread(None);
        self.sync_primary_agent_thread();
        self.dirty = true;
    }

    pub fn set_streaming(&mut self, streaming: bool) {
        if self.is_streaming != streaming {
            self.is_streaming = streaming;
            if streaming {
                self.spinner_state.start(Some("Thinking...".to_string()));
                self.prompt.is_active = true;
                self.suggestions = None; // clear stale suggestions
            } else {
                self.spinner_state.stop();
                self.prompt.is_active = true;
            }
            self.dirty = true;
        }
    }

    #[cfg(test)]
    pub fn show_permission_dialog(&mut self, tool_name: &str, input: &str, message: &str) {
        self.overlays.permission_dialog = Some(PermissionDialog::new(tool_name, input, message));
        self.dirty = true;
    }

    pub fn show_question_dialog(&mut self, id: impl Into<String>, request: AskUserRequestPayload) {
        self.overlays.question_dialog = Some(QuestionDialog::new(id, request));
        self.dirty = true;
    }

    pub fn show_permission_request(&mut self, request: PermissionDialogRequest) {
        self.add_permission_marker(&request);
        self.overlays.permission_dialog = Some(if request.tool_use_id.is_empty() {
            let input = request.tool_input.to_string();
            PermissionDialog::new(&request.tool_name, &input, &request.message)
        } else {
            PermissionDialog::from_request(request)
        });
        self.dirty = true;
    }

    fn add_permission_marker(&mut self, request: &PermissionDialogRequest) {
        if request.tool_use_id.is_empty() {
            return;
        }
        let text = permission_marker_text(request);
        let marker = Message::Progress(ProgressMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: chrono::Utc::now().timestamp(),
            tool_use_id: request.tool_use_id.clone(),
            data: serde_json::json!({
                "message": text,
                "permission": true,
                "tool": request.tool_name,
            }),
        });
        let replace_last = self.messages().last().is_some_and(|message| {
            matches!(
                message,
                Message::Progress(existing) if existing.tool_use_id == request.tool_use_id
            )
        });
        if replace_last {
            self.replace_last_message(marker);
        } else {
            self.add_message(marker);
        }
    }

    pub fn show_bypass_permissions_mode_dialog(&mut self, disabled: bool) {
        self.overlays.bypass_permissions_mode_dialog =
            Some(BypassPermissionsModeDialog::new(disabled));
        self.dirty = true;
    }

    #[cfg(test)]
    pub fn dismiss_permission_dialog(&mut self) {
        self.overlays.clear_permission();
        self.dirty = true;
    }

    #[cfg(test)]
    pub(crate) fn active_overlay_for_tests(&self) -> Option<ActiveOverlay> {
        self.overlays.active_overlay()
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn is_streaming(&self) -> bool {
        self.is_streaming
    }

    fn thinking_animation_frame(&self) -> Option<usize> {
        self.is_streaming
            .then_some((self.tick_counter / 5) as usize)
    }

    pub fn queue_prompt(&mut self, text: String) -> usize {
        let count = self.prompt_queue.queue(text);
        self.dirty = true;
        count
    }

    pub fn pop_next_queued(&mut self) -> Option<String> {
        let next = self.prompt_queue.pop_next();
        if next.is_some() {
            self.dirty = true;
        }
        next
    }

    pub fn queued_count(&self) -> usize {
        self.prompt_queue.len()
    }

    /// Tick the spinner. Called at 16ms interval; spinner frame advances
    /// every 5th tick (~80ms) to keep a pleasant animation speed.
    pub fn tick(&mut self) {
        self.tick_counter = self.tick_counter.wrapping_add(1);
        if self.spinner_state.active {
            self.spinner_state.tick_tip(16);
        }
        if self.spinner_state.active && self.tick_counter.is_multiple_of(5) {
            self.spinner_state.tick();
            self.dirty = true;
        }
        if self.is_streaming && self.tick_counter.is_multiple_of(5) {
            self.dirty = true;
        }
        if self.notifications.process_queue() {
            self.dirty = true;
        }
    }

    pub fn set_model_name(&mut self, name: String) {
        self.session_ui.model_name = name;
        self.dirty = true;
    }

    pub fn set_backend_name(&mut self, name: String) {
        self.session_ui.backend_name = name;
        self.dirty = true;
    }

    pub fn set_session_id(&mut self, id: String) {
        let previous_session_id = self.session_ui.session_id.clone();
        self.session_ui.session_id = id;
        if !previous_session_id.is_empty() && previous_session_id != self.session_ui.session_id {
            self.runtime_view
                .agent_nav_mut()
                .remove(&previous_session_id);
            self.active_goal = None;
        }
        self.sync_primary_agent_thread();
        self.dirty = true;
    }

    pub fn session_id(&self) -> &str {
        &self.session_ui.session_id
    }

    pub fn set_cwd(&mut self, cwd: String) {
        self.session_ui.cwd = cwd;
        self.workspace_trust_pending =
            !self.session_ui.cwd.is_empty() && !is_workspace_trusted(&self.session_ui.cwd);
        self.workspace_trust_selection = 0;
        self.dirty = true;
    }

    pub fn cwd(&self) -> &str {
        &self.session_ui.cwd
    }

    pub fn set_output_style(&mut self, output_style: Option<String>) {
        self.session_ui.output_style = output_style;
        self.dirty = true;
    }

    pub fn set_theme_setting(&mut self, theme: Option<&str>) {
        self.design_theme_provider = ThemeProvider::from_setting_str(theme);
        self.theme = self.design_theme_provider.legacy_theme();
        self.dirty = true;
    }

    pub fn set_keybindings(&mut self, keybindings: KeybindingRegistry) {
        self.keybindings = keybindings;
        self.pending_chord.clear();
    }

    pub fn set_editor_mode(&mut self, editor_mode: Option<&str>) {
        self.vim = VimState::from_editor_mode(editor_mode);
        self.dirty = true;
    }

    pub fn update_session_cost(&mut self, cost_usd: f64) {
        self.session_cost_usd = cost_usd;
        self.dirty = true;
    }

    // Terminal env + transcript (issue #12)

    /// Install a resolved terminal-env config. Called by the TUI runner
    /// once at startup; `ScrollSpeed` is then consumed by the scroll
    /// helpers in both prompt and transcript modes.
    pub fn set_terminal_env(&mut self, cfg: TerminalEnvConfig) {
        self.terminal_env = cfg;
        self.dirty = true;
    }

    /// Current terminal-env config (read by the TUI runner to decide
    /// whether to emit synchronized-update escapes).
    pub fn terminal_env(&self) -> TerminalEnvConfig {
        self.terminal_env
    }

    /// Current view mode; tests and the TUI key binding use this to
    /// verify Ctrl+O cycling.
    #[cfg(test)]
    pub fn view_mode(&self) -> ViewMode {
        self.view_mode
    }

    /// Advance the view mode (`Prompt -> Transcript -> Focus -> Prompt`).
    /// Exposed separately so `/` slash commands could also drive it in
    /// the future.
    pub fn cycle_view_mode(&mut self) {
        // Leaving transcript throws away search state; entering fresh
        // next time is less surprising than stale matches hanging around.
        if self.view_mode.is_transcript_like() {
            self.transcript_state.clear_search();
        }
        self.view_mode = self.view_mode.next();
        // Seed the transcript scroll offset to the bottom on entry so the
        // user starts reading the latest exchange.
        if self.view_mode.is_transcript_like() {
            self.transcript_state.scroll_offset = usize::MAX;
        }
        self.dirty = true;
    }

    /// Apply a configured view mode at startup or after `/config set`.
    pub fn set_view_mode(&mut self, mode: ViewMode) {
        if self.view_mode == mode {
            return;
        }
        if self.view_mode.is_transcript_like() {
            self.transcript_state.clear_search();
        }
        self.view_mode = mode;
        if self.view_mode.is_transcript_like() {
            self.transcript_state.scroll_offset = usize::MAX;
        }
        self.dirty = true;
    }

    /// Open a modal slash-command surface above the normal prompt.
    pub fn open_command_surface(&mut self, mut surface: CommandSurface) {
        if let CommandSurface::Tasks(tasks_surface) = &mut surface {
            self.runtime_view.refresh_task_items(&tasks_surface.items);
            tasks_surface.sync_items(self.runtime_view.task_items());
        }
        self.overlays.command_surface = Some(surface);
        self.command_palette.close();
        self.dirty = true;
    }

    pub fn set_pending_command_surface_after_submit(&mut self, target: CommandSurfaceTarget) {
        self.pending_command_surface_after_submit = Some(target);
    }

    pub fn take_pending_command_surface_after_submit(&mut self) -> Option<CommandSurfaceTarget> {
        self.pending_command_surface_after_submit.take()
    }

    #[cfg(test)]
    pub fn command_surface_active(&self) -> bool {
        self.overlays.command_surface.is_some()
    }

    #[cfg(test)]
    pub fn history_search_active(&self) -> bool {
        self.overlays.history_search_dialog.is_some()
    }

    /// Current transcript state exposed read-only so tests can assert
    /// search invariants without going through the render path.
    #[cfg(test)]
    pub fn transcript_state(&self) -> &TranscriptState {
        &self.transcript_state
    }

    pub fn set_spinner_message(&mut self, msg: String) {
        self.spinner_state.set_message(msg);
        self.dirty = true;
    }

    pub fn set_spinner_tips_settings(&mut self, settings: SpinnerTipsSettings) {
        self.spinner_state.configure_tips(
            settings.enabled,
            settings.interval_ms,
            settings.custom_tips,
        );
        self.dirty = true;
    }

    #[cfg(test)]
    pub fn spinner_message(&self) -> &str {
        &self.spinner_state.message
    }

    pub fn set_suggestions(&mut self, suggestions: Vec<PromptSuggestion>) {
        self.suggestions = Some(suggestions);
        self.dirty = true;
    }

    pub fn add_notification(&mut self, notification: InAppNotification) {
        if self.notifications.add_notification(notification) {
            self.dirty = true;
        }
    }

    pub fn set_sound_effects(&mut self, enabled: bool) {
        self.sound_effects = enabled;
    }

    pub fn sound_effects_enabled(&self) -> bool {
        self.sound_effects
    }

    pub fn set_terminal_progress_bar_enabled(&mut self, enabled: bool) {
        self.terminal_progress_bar_enabled = enabled;
    }

    pub fn terminal_progress_bar_enabled(&self) -> bool {
        self.terminal_progress_bar_enabled
    }

    pub fn handle_app_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::Notification {
                key,
                message,
                level,
                timeout_ms,
            } => {
                self.add_notification(notification_from_app_event(key, message, level, timeout_ms));
            }
            AppEvent::LocalNotice { message } => {
                self.add_notification(
                    InAppNotification::new("local-notice", NotificationPriority::Medium, message)
                        .with_timeout_ms(4500)
                        .with_fold(true),
                );
            }
            AppEvent::Tick => self.tick(),
            AppEvent::Shutdown => {
                self.should_quit = true;
                self.dirty = true;
            }
            AppEvent::Backend { message } => {
                let message = message.as_ref();
                if backend_message_updates_tasks(message) {
                    self.runtime_view.apply_task_event(message);
                    if let Some(CommandSurface::Tasks(surface)) =
                        self.overlays.command_surface.as_mut()
                    {
                        surface.sync_items(self.runtime_view.task_items());
                        self.dirty = true;
                    }
                }
                self.update_agent_navigation_from_backend(message);
                match message {
                    BackendMessage::BackgroundAgentComplete {
                        agent_id,
                        description,
                        result_preview,
                        had_error,
                        duration_ms,
                    } => {
                        self.apply_background_agent_complete(
                            agent_id,
                            description,
                            result_preview,
                            *had_error,
                            *duration_ms,
                        );
                    }
                    BackendMessage::ToolProgress {
                        tool_use_id,
                        tool,
                        output,
                        ..
                    } => {
                        self.apply_primary_tool_progress(tool_use_id, tool, output);
                    }
                    _ => {}
                }
                if let Some(notification) = notification_from_backend_message(message) {
                    self.add_notification(notification);
                }
            }
        }
    }

    pub(super) fn agent_footer_visible(&self) -> bool {
        self.show_agent_footer && self.runtime_view.agent_nav().active_non_primary_count() > 0
    }

    pub(super) fn agent_footer_height(&self) -> u16 {
        if !self.agent_footer_visible() {
            return 0;
        }
        let active_agents = self.runtime_view.agent_nav().active_non_primary_count();
        let visible_agents = active_agents.min(3);
        let overflow_row = usize::from(active_agents > visible_agents);
        (1 + visible_agents + overflow_row) as u16
    }

    pub(super) fn agent_footer_lines(&self) -> Vec<String> {
        let active_agent_ids = self.active_agent_thread_ids();
        if active_agent_ids.is_empty() {
            return Vec::new();
        }

        let mut lines = Vec::with_capacity(1 + active_agent_ids.len().min(3));
        let agent_label = if active_agent_ids.len() == 1 {
            "agent"
        } else {
            "agents"
        };
        lines.push(format!(
            "Running {} {agent_label}... Ctrl+X Ctrl+A open tree",
            active_agent_ids.len()
        ));

        let visible_count = active_agent_ids.len().min(3);
        for (index, agent_id) in active_agent_ids.iter().take(visible_count).enumerate() {
            let Some(entry) = self.runtime_view.agent_nav().entry(agent_id) else {
                continue;
            };
            let branch = if index + 1 == visible_count {
                "`-"
            } else {
                "|-"
            };
            let runtime = self.runtime_view.agent_nav().runtime_info(agent_id);
            let status = runtime
                .map(|info| info.status.label())
                .unwrap_or(if entry.is_closed { "closed" } else { "active" });
            let tool_count = runtime.map_or(0, |info| info.tool_use_count());
            let mut parts = vec![
                compact_inline(&entry.label(), 32),
                status.to_string(),
                format!("{tool_count} tool uses"),
            ];
            if let Some(duration) = runtime.and_then(|info| info.duration_ms) {
                parts.push(format_duration_ms(duration));
            }
            if let Some(summary) = runtime.and_then(|info| info.status_summary.as_deref()) {
                if !summary.is_empty() {
                    parts.push(compact_inline(summary, 48));
                }
            } else if let Some(tool) = runtime.and_then(|info| info.recent_tool_uses().next_back())
            {
                parts.push(compact_inline(&tool.summary, 48));
            }
            lines.push(format!("   {branch} {}", parts.join(" | ")));
        }

        if active_agent_ids.len() > visible_count {
            lines.push(format!(
                "   ... {} more",
                active_agent_ids.len() - visible_count
            ));
        }
        lines
    }

    pub(super) fn active_agent_thread_ids(&self) -> Vec<String> {
        self.runtime_view
            .agent_nav()
            .active_non_primary_thread_ids()
    }

    fn runtime_state(&self) -> &RuntimeViewState {
        &self.runtime_view
    }

    pub(super) fn current_agent_thread_id(&self) -> &str {
        self.runtime_view
            .current_agent_thread_id()
            .map(String::as_str)
            .filter(|id| self.runtime_view.agent_nav().contains_thread(id))
            .or_else(|| {
                (!self.session_ui.session_id.is_empty())
                    .then_some(self.session_ui.session_id.as_str())
            })
            .or_else(|| {
                self.runtime_view
                    .agent_nav()
                    .ordered_threads()
                    .first()
                    .map(|entry| entry.thread_id.as_str())
            })
            .unwrap_or("")
    }

    pub(super) fn toggle_agent_tree_dialog(&mut self) {
        if self.overlays.agent_tree_dialog.is_some() {
            self.overlays.agent_tree_dialog = None;
        } else if self.runtime_view.agent_nav().thread_count() > 1 {
            self.overlays.agent_tree_dialog = Some(AgentTreeDialog::from_state(
                self.runtime_view.agent_nav(),
                self.current_agent_thread_id(),
            ));
        }
        self.dirty = true;
    }

    fn sync_primary_agent_thread(&mut self) {
        if self.session_ui.session_id.is_empty() {
            return;
        }
        self.runtime_view.upsert_agent(AgentThreadEntry {
            thread_id: self.session_ui.session_id.clone(),
            agent_nickname: Some("Primary".to_string()),
            agent_role: Some("main".to_string()),
            is_primary: true,
            is_closed: false,
        });

        if self
            .runtime_view
            .current_agent_thread_id()
            .map(String::as_str)
            .is_none_or(|id| !self.runtime_view.agent_nav().contains_thread(id))
        {
            self.runtime_view
                .set_current_agent_thread(Some(self.session_ui.session_id.clone()));
        }
    }

    fn update_agent_navigation_from_backend(&mut self, message: &BackendMessage) {
        match message {
            BackendMessage::AgentEvent { event } => self.apply_agent_event(event),
            BackendMessage::TeamEvent { event } => self.apply_team_event(event),
            _ => {}
        }
    }

    fn apply_agent_event(&mut self, event: &AgentEvent) {
        self.sync_primary_agent_thread();
        match event {
            AgentEvent::Spawned {
                agent_id,
                description,
                agent_type,
                ..
            } => {
                self.runtime_view.upsert_agent(AgentThreadEntry {
                    thread_id: agent_id.clone(),
                    agent_nickname: short_agent_label(description, agent_id),
                    agent_role: agent_type.clone(),
                    is_primary: false,
                    is_closed: false,
                });
                self.runtime_view.agent_nav_mut().mark_status(
                    agent_id,
                    AgentThreadStatus::Running,
                    Some(compact_inline(description, 80)),
                );
                self.runtime_view
                    .set_current_agent_thread(Some(agent_id.clone()));
            }
            AgentEvent::Completed {
                agent_id,
                result_preview,
                had_error,
                duration_ms,
                ..
            } => {
                self.runtime_view.agent_nav_mut().mark_status(
                    agent_id,
                    if *had_error {
                        AgentThreadStatus::Failed
                    } else {
                        AgentThreadStatus::Succeeded
                    },
                    Some(compact_inline(result_preview, 80)),
                );
                self.runtime_view
                    .agent_nav_mut()
                    .set_duration_ms(agent_id, Some(*duration_ms));
                self.runtime_view.agent_nav_mut().mark_closed(agent_id);
                self.normalize_current_agent_thread();
            }
            AgentEvent::Error {
                agent_id,
                error,
                duration_ms,
            } => {
                self.runtime_view.agent_nav_mut().mark_status(
                    agent_id,
                    AgentThreadStatus::Failed,
                    Some(compact_inline(error, 80)),
                );
                self.runtime_view
                    .agent_nav_mut()
                    .set_duration_ms(agent_id, Some(*duration_ms));
                self.runtime_view.agent_nav_mut().mark_closed(agent_id);
                self.normalize_current_agent_thread();
            }
            AgentEvent::Aborted { agent_id } => {
                self.runtime_view.agent_nav_mut().mark_status(
                    agent_id,
                    AgentThreadStatus::Canceled,
                    Some("agent aborted".to_string()),
                );
                self.runtime_view.agent_nav_mut().mark_closed(agent_id);
                self.normalize_current_agent_thread();
            }
            AgentEvent::PermissionQueued {
                agent_id,
                tool_name,
                summary,
                tool_use_id,
                queue_position,
                ..
            } => {
                if self.runtime_view.agent_nav().contains_thread(agent_id) {
                    self.runtime_view
                        .set_current_agent_thread(Some(agent_id.clone()));
                }
                self.runtime_view.agent_nav_mut().mark_tool_use(
                    agent_id,
                    tool_use_id,
                    tool_name,
                    compact_inline(summary, 80),
                );
                self.runtime_view.agent_nav_mut().mark_status(
                    agent_id,
                    AgentThreadStatus::WaitingPermission,
                    Some(compact_inline(summary, 80)),
                );
                let agent_label = self
                    .runtime_view
                    .agent_nav()
                    .entry(agent_id)
                    .and_then(|entry| entry.agent_nickname.as_deref())
                    .unwrap_or(agent_id)
                    .to_string();
                self.add_notification(
                    InAppNotification::new(
                        "worker-permission",
                        NotificationPriority::Low,
                        render_worker_pending_permission(
                            &agent_label,
                            tool_name,
                            summary,
                            *queue_position,
                        ),
                    )
                    .with_tone(NotificationTone::Warning)
                    .with_fold(true)
                    .with_timeout_ms(5000),
                );
            }
            AgentEvent::PermissionResolved {
                agent_id,
                tool_name,
                decision,
                ..
            } => {
                if self.runtime_view.agent_nav().contains_thread(agent_id) {
                    self.runtime_view
                        .set_current_agent_thread(Some(agent_id.clone()));
                }
                self.runtime_view.agent_nav_mut().mark_status(
                    agent_id,
                    AgentThreadStatus::Running,
                    Some(compact_inline(&format!("{tool_name}: {decision}"), 80)),
                );
            }
            AgentEvent::TreeSnapshot { roots } => {
                let current = self.current_agent_thread_id().to_string();
                self.runtime_view.agent_nav_mut().clear();
                self.sync_primary_agent_thread();
                for node in roots {
                    self.collect_agent_tree_node(node);
                }
                if self.runtime_view.agent_nav().contains_thread(&current) {
                    self.runtime_view.set_current_agent_thread(Some(current));
                }
            }
            AgentEvent::StreamDelta { agent_id, text } => {
                if self.runtime_view.agent_nav().contains_thread(agent_id) {
                    self.runtime_view
                        .set_current_agent_thread(Some(agent_id.clone()));
                }
                self.runtime_view.agent_nav_mut().mark_status(
                    agent_id,
                    AgentThreadStatus::Streaming,
                    Some(compact_inline(text, 80)),
                );
            }
            AgentEvent::ThinkingDelta { agent_id, thinking } => {
                if self.runtime_view.agent_nav().contains_thread(agent_id) {
                    self.runtime_view
                        .set_current_agent_thread(Some(agent_id.clone()));
                }
                self.runtime_view.agent_nav_mut().mark_status(
                    agent_id,
                    AgentThreadStatus::Thinking,
                    Some(compact_inline(thinking, 80)),
                );
            }
            AgentEvent::ToolUse {
                agent_id,
                tool_use_id,
                tool_name,
                input,
            } => {
                if self.runtime_view.agent_nav().contains_thread(agent_id) {
                    self.runtime_view
                        .set_current_agent_thread(Some(agent_id.clone()));
                }
                self.runtime_view.agent_nav_mut().mark_tool_use(
                    agent_id,
                    tool_use_id,
                    tool_name,
                    compact_inline(&format!("{tool_name} {input}"), 80),
                );
            }
            AgentEvent::ToolResult {
                agent_id,
                output,
                is_error,
                ..
            } => {
                if self.runtime_view.agent_nav().contains_thread(agent_id) {
                    self.runtime_view
                        .set_current_agent_thread(Some(agent_id.clone()));
                }
                let summary = if *is_error {
                    format!("tool error: {}", compact_inline(output, 72))
                } else {
                    compact_inline(output, 80)
                };
                self.runtime_view.agent_nav_mut().mark_status(
                    agent_id,
                    AgentThreadStatus::Running,
                    Some(summary),
                );
            }
            AgentEvent::OutputBatch {
                agent_id, output, ..
            } => {
                if self.runtime_view.agent_nav().contains_thread(agent_id) {
                    self.runtime_view
                        .set_current_agent_thread(Some(agent_id.clone()));
                }
                if let Some(last_chunk) = output
                    .events
                    .iter()
                    .rev()
                    .find_map(|event| (!event.chunk.is_empty()).then_some(event.chunk.as_str()))
                {
                    self.runtime_view.agent_nav_mut().mark_status(
                        agent_id,
                        AgentThreadStatus::Running,
                        Some(compact_inline(last_chunk, 80)),
                    );
                }
            }
            #[allow(unreachable_patterns)]
            _ => {}
        }
        self.dirty = true;
    }

    fn apply_team_event(&mut self, event: &TeamEvent) {
        self.sync_primary_agent_thread();
        match event {
            TeamEvent::MemberJoined {
                team_name,
                agent_id,
                agent_name,
                role,
                ..
            } => {
                self.runtime_view.upsert_agent(AgentThreadEntry {
                    thread_id: agent_id.clone(),
                    agent_nickname: Some(agent_name.clone()),
                    agent_role: role.clone(),
                    is_primary: false,
                    is_closed: false,
                });
                self.runtime_view.agent_nav_mut().mark_status(
                    agent_id,
                    AgentThreadStatus::Running,
                    Some(format!("{agent_name} joined {team_name}")),
                );
            }
            TeamEvent::MemberLeft { agent_id, .. } => {
                self.runtime_view.agent_nav_mut().mark_status(
                    agent_id,
                    AgentThreadStatus::Closed,
                    Some("teammate left".to_string()),
                );
                self.runtime_view.agent_nav_mut().mark_closed(agent_id);
                self.normalize_current_agent_thread();
            }
            TeamEvent::StatusSnapshot { members, .. } => {
                for member in members {
                    self.runtime_view.upsert_agent(AgentThreadEntry {
                        thread_id: member.agent_id.clone(),
                        agent_nickname: Some(member.agent_name.clone()),
                        agent_role: member.role.clone(),
                        is_primary: false,
                        is_closed: !member.is_active,
                    });
                    self.runtime_view.agent_nav_mut().mark_status(
                        &member.agent_id,
                        if member.is_active {
                            AgentThreadStatus::Running
                        } else {
                            AgentThreadStatus::Closed
                        },
                        Some(format!("{} unread message(s)", member.unread_messages)),
                    );
                }
            }
            TeamEvent::MessageRouted { from, to, .. } => {
                if self.runtime_view.agent_nav().contains_thread(from) {
                    self.runtime_view
                        .set_current_agent_thread(Some(from.clone()));
                } else if self.runtime_view.agent_nav().contains_thread(to) {
                    self.runtime_view.set_current_agent_thread(Some(to.clone()));
                }
            }
        }
        self.dirty = true;
    }

    fn normalize_current_agent_thread(&mut self) {
        let current_is_active = self
            .runtime_view
            .current_agent_thread_id()
            .map(String::as_str)
            .and_then(|id| self.runtime_view.agent_nav().entry(id))
            .is_some_and(|entry| !entry.is_closed);
        if current_is_active {
            return;
        }
        if !self.session_ui.session_id.is_empty()
            && self
                .runtime_view
                .agent_nav()
                .contains_thread(&self.session_ui.session_id)
        {
            self.runtime_view
                .set_current_agent_thread(Some(self.session_ui.session_id.clone()));
            return;
        }
        let current = self
            .runtime_view
            .agent_nav()
            .ordered_threads()
            .into_iter()
            .find(|entry| !entry.is_closed)
            .map(|entry| entry.thread_id.clone());
        self.runtime_view.set_current_agent_thread(current);
    }

    fn collect_agent_tree_node(&mut self, node: &allthecodes_types::agent_types::AgentNode) {
        self.runtime_view.upsert_agent(AgentThreadEntry {
            thread_id: node.agent_id.clone(),
            agent_nickname: short_agent_label(&node.description, &node.agent_id),
            agent_role: node.agent_type.clone(),
            is_primary: false,
            is_closed: !agent_state_is_active(&node.state),
        });
        self.runtime_view.agent_nav_mut().mark_status(
            &node.agent_id,
            agent_status_from_tree_state(&node.state, node.had_error),
            Some(compact_inline(
                node.result_preview.as_deref().unwrap_or(&node.state),
                80,
            )),
        );
        self.runtime_view
            .agent_nav_mut()
            .set_duration_ms(&node.agent_id, node.duration_ms);
        for child in &node.children {
            self.collect_agent_tree_node(child);
        }
    }

    fn apply_background_agent_complete(
        &mut self,
        agent_id: &str,
        description: &str,
        result_preview: &str,
        had_error: bool,
        duration_ms: u64,
    ) {
        self.sync_primary_agent_thread();
        if !self.runtime_view.agent_nav().contains_thread(agent_id) {
            self.runtime_view.upsert_agent(AgentThreadEntry {
                thread_id: agent_id.to_string(),
                agent_nickname: short_agent_label(description, agent_id),
                agent_role: Some("agent".to_string()),
                is_primary: false,
                is_closed: false,
            });
        }
        self.runtime_view.agent_nav_mut().mark_status(
            agent_id,
            if had_error {
                AgentThreadStatus::Failed
            } else {
                AgentThreadStatus::Succeeded
            },
            Some(compact_inline(result_preview, 80)),
        );
        self.runtime_view
            .agent_nav_mut()
            .set_duration_ms(agent_id, Some(duration_ms));
        self.runtime_view.agent_nav_mut().mark_closed(agent_id);
        self.normalize_current_agent_thread();
        self.dirty = true;
    }

    fn apply_primary_tool_progress(&mut self, tool_use_id: &str, tool: &str, output: &str) {
        self.sync_primary_agent_thread();
        let primary_thread = self.current_primary_thread_id().map(ToString::to_string);
        let Some(primary_thread) = primary_thread else {
            return;
        };
        self.runtime_view.agent_nav_mut().mark_tool_use(
            &primary_thread,
            tool_use_id,
            tool,
            format!("{tool} {}", compact_inline(output, 64)),
        );
        self.dirty = true;
    }

    fn current_primary_thread_id(&self) -> Option<&str> {
        if !self.session_ui.session_id.is_empty()
            && self
                .runtime_view
                .agent_nav()
                .contains_thread(&self.session_ui.session_id)
        {
            return Some(self.session_ui.session_id.as_str());
        }
        self.runtime_view
            .agent_nav()
            .ordered_threads()
            .into_iter()
            .find(|entry| entry.is_primary)
            .map(|entry| entry.thread_id.as_str())
    }

    pub fn remove_notification(&mut self, key: &str) {
        if self.notifications.remove_notification(key) {
            self.dirty = true;
        }
    }

    pub fn current_notification(&self) -> Option<&InAppNotification> {
        self.notifications.current()
    }

    #[cfg(test)]
    pub fn clear_suggestions(&mut self) {
        if self.suggestions.is_some() {
            self.suggestions = None;
            self.dirty = true;
        }
    }

    #[cfg(test)]
    pub fn suggestions(&self) -> Option<&[PromptSuggestion]> {
        self.suggestions.as_deref()
    }

    #[cfg(test)]
    pub fn prompt_text(&self) -> &str {
        &self.prompt.input
    }

    pub fn push_history(&mut self, text: String) {
        if self
            .session_ui
            .history
            .last()
            .map(|entry| entry.display.as_str())
            != Some(text.as_str())
        {
            self.session_ui
                .history
                .push(HistorySearchEntry::new(text, current_unix_secs()));
        }
        self.history_index = None;
        self.saved_input.clear();
    }

    /// Seed Ctrl+R prompt history from backend session storage.
    ///
    /// Callers pass entries newest-first (the shape returned by the session
    /// reader). `App` keeps history oldest-first so arrow-history and Ctrl+R
    /// continue to prefer live prompts appended during this TUI run.
    pub fn seed_persistent_history(&mut self, entries: Vec<HistorySearchEntry>) {
        for entry in entries.into_iter().rev() {
            if self
                .session_ui
                .history
                .iter()
                .any(|existing| existing.display == entry.display)
            {
                continue;
            }
            self.session_ui.history.push(entry);
        }
        self.history_index = None;
        self.dirty = true;
    }

    #[cfg(test)]
    pub(crate) fn history_len(&self) -> usize {
        self.session_ui.history.len()
    }

    // Event handling
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

fn current_unix_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

fn permission_marker_text(request: &PermissionDialogRequest) -> String {
    let summary = request
        .operation
        .as_ref()
        .and_then(|operation| permission_marker_from_operation(request, operation))
        .unwrap_or_else(|| {
            let summary = request.input_summary();
            if summary == "(no details supplied)" {
                request.tool_name.clone()
            } else {
                format!("{} {}", request.tool_name, compact_inline(&summary, 80))
            }
        });
    format!("Permission requested: {summary}")
}

fn permission_marker_from_operation(
    request: &PermissionDialogRequest,
    operation: &ToolOperation,
) -> Option<String> {
    let normalized_tool = request.tool_name.to_ascii_lowercase();
    let target = operation
        .target
        .as_deref()
        .filter(|value| !value.is_empty());
    let command = operation
        .command_summary
        .as_deref()
        .filter(|value| !value.is_empty());

    if normalized_tool.contains("bash") || normalized_tool.contains("powershell") {
        return command.or(target).map(|value| compact_inline(value, 100));
    }
    if normalized_tool.contains("web") || normalized_tool.contains("fetch") {
        return target.map(|value| format!("Fetch {}", compact_inline(value, 90)));
    }
    if normalized_tool.contains("write") || operation.kind == OperationKind::Create {
        return target.map(|value| format!("Write {}", compact_inline(value, 90)));
    }
    if normalized_tool.contains("edit")
        || normalized_tool.contains("patch")
        || normalized_tool.contains("sed")
        || operation.kind == OperationKind::Modify
    {
        return target.map(|value| format!("Modify {}", compact_inline(value, 90)));
    }
    target
        .map(|value| format!("{} {}", operation.kind.label(), compact_inline(value, 90)))
        .or_else(|| {
            command
                .map(|value| compact_inline(value, 100))
                .or_else(|| strip_permission_prefix(&operation.label).map(ToOwned::to_owned))
        })
}

fn strip_permission_prefix(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    trimmed
        .strip_prefix("Permission: ")
        .or(Some(trimmed))
        .filter(|value| !value.is_empty())
}

fn short_agent_label(description: &str, fallback: &str) -> Option<String> {
    let trimmed = description.trim();
    if trimmed.is_empty() {
        return Some(fallback.to_string());
    }
    let mut words = trimmed.split_whitespace();
    let first = words.next().unwrap_or_default();
    let second = words.next().unwrap_or_default();
    let label = if second.is_empty() {
        first.to_string()
    } else {
        format!("{first} {second}")
    };
    Some(label)
}

fn backend_message_updates_tasks(message: &BackendMessage) -> bool {
    matches!(
        message,
        BackendMessage::ToolProgress { .. }
            | BackendMessage::BackgroundAgentComplete { .. }
            | BackendMessage::AgentEvent { .. }
            | BackendMessage::TeamEvent { .. }
    )
}

fn compact_inline(value: &str, max_chars: usize) -> String {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = normalized.chars();
    let preview = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{preview}...")
    } else {
        preview
    }
}

fn format_duration_ms(duration_ms: u64) -> String {
    if duration_ms < 1_000 {
        format!("{duration_ms}ms")
    } else {
        format!("{}s", duration_ms / 1_000)
    }
}

fn agent_status_from_tree_state(state: &str, had_error: bool) -> AgentThreadStatus {
    if had_error {
        return AgentThreadStatus::Failed;
    }
    match state.to_ascii_lowercase().as_str() {
        "completed" | "complete" | "succeeded" | "success" => AgentThreadStatus::Succeeded,
        "error" | "failed" => AgentThreadStatus::Failed,
        "cancelled" | "canceled" | "aborted" | "stopped" => AgentThreadStatus::Canceled,
        "thinking" => AgentThreadStatus::Thinking,
        "streaming" => AgentThreadStatus::Streaming,
        "tool" | "tool_running" => AgentThreadStatus::ToolRunning,
        "permission" | "waiting_permission" => AgentThreadStatus::WaitingPermission,
        "closed" => AgentThreadStatus::Closed,
        _ => AgentThreadStatus::Running,
    }
}

fn agent_state_is_active(state: &str) -> bool {
    matches!(
        state.to_ascii_lowercase().as_str(),
        "running"
            | "active"
            | "spawned"
            | "thinking"
            | "streaming"
            | "tool"
            | "tool_running"
            | "permission"
            | "waiting_permission"
    )
}
