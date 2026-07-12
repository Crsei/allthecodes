//! IPC protocol types for headless mode.
//!
//! Defines the JSON-lines protocol between the Rust backend and a separate
//! UI process (e.g. an Ink/React terminal frontend).
//!
//! Both enums use `#[serde(tag = "type")]` so each JSON line carries a
//! discriminator field, e.g. `{"type":"submit_prompt","text":"hello","id":"…"}`.
//!
//! ## Module layout
//!
//! | File | Contents |
//! |------|----------|
//! | `base.rs` | Shared supporting types (`ToolResultContentInfo`, `ConversationMessage`) |
//! | `subsystem.rs` | Subsystem protocol serde tests |
//! | `agent.rs` | Agent protocol serde tests |
//! | `team.rs` | Team protocol serde tests |

mod base;

mod agent;
mod subsystem;
mod team;

pub use base::{ConversationMessage, ToolResultContentInfo};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use allthecodes_types::agent_events::{AgentCommand, AgentEvent, TeamCommand, TeamEvent};
use allthecodes_types::callbacks::SecurityDecisionDisplay;
use allthecodes_types::permission_events::{
    HookPermissionDecisionEvent, PermissionAutoReviewEvent, PermissionDecisionDebugEvent,
};
use allthecodes_types::plan_workflow::PlanWorkflowRecord;
use allthecodes_types::tool_operation::{OperationResultSummary, ToolOperation};

use crate::subsystem_events::{
    AgentSettingsCommand, AgentSettingsEvent, IdeCommand, IdeEvent, LspCommand, LspEvent,
    McpCommand, McpEvent, PluginCommand, PluginEvent, SkillCommand, SkillEvent,
};
use crate::subsystem_types::SubsystemStatusSnapshot;

// ---------------------------------------------------------------------------
// Frontend → Backend (deserialized from stdin)
// ---------------------------------------------------------------------------

/// Messages sent by the UI process to the Rust backend.
#[derive(Deserialize, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FrontendMessage {
    /// User submits a prompt.
    SubmitPrompt { text: String, id: String },
    /// User hits Ctrl+C to abort the current query.
    AbortQuery,
    /// User responds to a permission dialog.
    PermissionResponse {
        tool_use_id: String,
        /// One of "allow", "deny", "always_allow", "auto_review".
        decision: String,
        /// Optional feedback/instructions supplied with the decision.
        #[serde(default)]
        feedback: Option<String>,
        /// Optional envelope/session-aware correlation. Legacy JSONL clients
        /// omit this and are resolved by `tool_use_id`.
        #[serde(default)]
        session_id: Option<String>,
        /// Optional turn correlation for multi-run transports.
        #[serde(default)]
        turn_id: Option<String>,
    },
    /// User typed a slash command.
    SlashCommand { raw: String },
    /// Terminal was resized.
    Resize { cols: u16, rows: u16 },
    /// User responds to a [`BackendMessage::QuestionRequest`].
    QuestionResponse {
        /// Must match the `id` from the corresponding `QuestionRequest`.
        id: String,
        /// The user's answer text.
        text: String,
        /// Optional envelope/session-aware correlation. Legacy JSONL clients
        /// omit this and are resolved by `id`.
        #[serde(default)]
        session_id: Option<String>,
        /// Optional turn correlation for multi-run transports.
        #[serde(default)]
        turn_id: Option<String>,
    },
    /// User wants to exit.
    Quit,

    // ── Subsystem commands ───────────────────────────────────────
    /// LSP lifecycle command.
    LspCommand { command: LspCommand },
    /// MCP lifecycle command.
    McpCommand { command: McpCommand },
    /// Plugin lifecycle command.
    PluginCommand { command: PluginCommand },
    /// Skill management command.
    SkillCommand { command: SkillCommand },
    /// IDE-integration lifecycle command.
    IdeCommand { command: IdeCommand },
    /// Agent-definition settings command (backs the `/agents` editor UI).
    AgentSettingsCommand { command: Box<AgentSettingsCommand> },
    /// Query all subsystem statuses.
    QuerySubsystemStatus,

    // ── Agent / Team commands ────────────────────────────────────
    /// Agent management commands.
    AgentCommand { command: AgentCommand },
    /// Team management commands.
    TeamCommand { command: TeamCommand },

    /// Run a bounded ripgrep-backed file search across the workspace.
    ///
    /// Frontend-facing counterpart of the upstream
    /// `ui/examples/upstream-patterns/src/utils/ripgrep.ts`
    /// helper - allthecodes' frontend has no direct filesystem access, so
    /// the backend runs `rg` on its behalf. The backend responds with a
    /// single [`BackendMessage::FileSearchResult`] keyed on
    /// `request_id`; long searches are truncated once
    /// `max_results` is reached.
    SearchFiles {
        request_id: String,
        pattern: String,
        /// Search root — defaults to the engine's cwd when `None`.
        #[serde(default)]
        cwd: Option<String>,
        /// Case-insensitive search. Defaults to `true` to match the
        /// upstream dialog's `-i` flag.
        #[serde(default = "default_true")]
        case_insensitive: bool,
        /// Upper bound on results returned in a single response. The
        /// handler caps at 500 regardless of this value to keep the
        /// payload reasonable.
        #[serde(default)]
        max_results: Option<usize>,
    },

    // ── Phase 2 integration: completions, telemetry, LSP recommendations ──
    /// Request input completions for the given text and cursor position.
    RequestCompletions {
        input: String,
        cursor_pos: usize,
        request_id: String,
    },
    /// Accept a completion item at the given index from the last
    /// `RequestCompletions` response.
    AcceptCompletion { request_id: String, index: usize },
    /// Install a recommended plugin from the LSP recommendation engine.
    InstallRecommendedPlugin { plugin_id: String },
    /// Request a telemetry status refresh.
    RefreshPluginTelemetry,
    /// Request LSP recommendations for a specific language (or all known).
    RequestLspRecommendations {
        #[serde(default)]
        language: Option<String>,
    },
}

fn default_true() -> bool {
    true
}

// ---------------------------------------------------------------------------
// Backend → Frontend (serialized to stdout)
// ---------------------------------------------------------------------------

/// Messages sent by the Rust backend to the UI process.
#[derive(Serialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BackendMessage {
    // ── Core conversation ────────────────────────────────────────
    /// Backend has initialized and is ready to accept prompts.
    Ready {
        session_id: String,
        model: String,
        cwd: String,
        permission_mode: String,
        available_models: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        plan_workflow: Option<PlanWorkflowRecord>,
        #[serde(skip_serializing_if = "Option::is_none")]
        editor_mode: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        view_mode: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        keybindings: Option<serde_json::Value>,
    },
    /// Assistant started streaming a new content block.
    StreamStart {
        message_id: String,
    },
    /// Streaming text delta for an in-progress content block.
    StreamDelta {
        message_id: String,
        text: String,
    },
    /// Streaming thinking delta for an in-progress thinking block.
    ThinkingDelta {
        message_id: String,
        thinking: String,
    },
    /// Streaming for a content block has finished.
    StreamEnd {
        message_id: String,
    },
    /// A previously streamed partial assistant should be discarded.
    Tombstone {
        message_id: String,
    },
    /// Final assistant message (content is the serialized Vec<ContentBlock>).
    AssistantMessage {
        id: String,
        content: serde_json::Value,
        cost_usd: f64,
    },
    /// A tool invocation.
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        operation: Option<ToolOperation>,
    },
    /// Result of a tool invocation.
    ToolResult {
        tool_use_id: String,
        output: String,
        is_error: bool,
        /// Structured content blocks when the result includes non-text data
        /// (e.g. images from Computer Use screenshot).
        #[serde(skip_serializing_if = "Option::is_none")]
        content_blocks: Option<Vec<ToolResultContentInfo>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        result_summary: Option<OperationResultSummary>,
        #[serde(skip_serializing_if = "Option::is_none")]
        operation: Option<ToolOperation>,
    },
    /// Intermediate progress report from a long-running tool invocation.
    ///
    /// Primarily emitted by the Bash tool so the frontend can render a
    /// live "Running… (12s)" indicator plus a tail of recent output.
    /// `output` holds the tail-capped snapshot the UI should display;
    /// `total_lines` / `total_bytes` are the full-stream counters so the
    /// UI can show `+N lines` / `~N lines` and total bytes; `timeout_ms`
    /// surfaces the configured timeout.
    ToolProgress {
        tool_use_id: String,
        /// Tool name (e.g. `"Bash"`) — lets UI-side routing pick the
        /// right progress renderer without a second lookup.
        tool: String,
        /// Tail-capped snapshot suitable for display.
        output: String,
        /// Whole-seconds elapsed since the tool started.
        elapsed_seconds: u64,
        /// Total output lines observed so far.
        #[serde(skip_serializing_if = "Option::is_none")]
        total_lines: Option<u64>,
        /// Total output bytes observed so far.
        #[serde(skip_serializing_if = "Option::is_none")]
        total_bytes: Option<u64>,
        /// Configured tool timeout in milliseconds (if any).
        #[serde(skip_serializing_if = "Option::is_none")]
        timeout_ms: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        operation: Option<ToolOperation>,
    },
    /// Ask the UI to show a permission dialog for a tool call.
    PermissionRequest {
        tool_use_id: String,
        tool: String,
        command: String,
        #[serde(default)]
        input: Value,
        options: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        security: Option<SecurityDecisionDisplay>,
        #[serde(skip_serializing_if = "Option::is_none")]
        operation: Option<ToolOperation>,
    },
    /// Ask the user a question.  The frontend should display the question
    /// and send back a [`FrontendMessage::QuestionResponse`] with the same `id`.
    QuestionRequest {
        /// Unique question identifier.
        id: String,
        /// The question text to display.
        text: String,
        /// Optional structured choices rendered as a single-select list.
        #[serde(default)]
        choices: Vec<String>,
        /// Whether the UI should also allow free-text entry.
        #[serde(default = "default_true")]
        allow_free_text: bool,
    },
    HookPermissionDecision {
        event: HookPermissionDecisionEvent,
    },
    PermissionDecisionDebug {
        event: PermissionDecisionDebugEvent,
    },
    PermissionAutoReview {
        event: PermissionAutoReviewEvent,
    },
    /// Durable plan workflow state changed.
    PlanWorkflowEvent {
        event: String,
        summary: String,
        record: PlanWorkflowRecord,
    },
    /// Durable session goal state changed.
    GoalUpdated {
        event: String,
        goal: Value,
    },
    /// A system-level informational message.
    SystemInfo {
        text: String,
        /// One of "info", "warning", "error".
        level: String,
    },
    /// Replace the full visible conversation history in the frontend.
    ConversationReplaced {
        messages: Vec<ConversationMessage>,
    },
    /// Token usage update.
    ///
    /// New fields added in evolution of the protocol:
    /// - `cache_read_input_tokens`, `cache_creation_input_tokens`,
    ///   `reasoning_output_tokens`, `api_call_count` (default to 0 for
    ///   backward compatibility with older senders).
    /// - `kind`: `"cumulative"` (snapshot since session start) or
    ///   `"delta"` (since the last UsageUpdate). Absent means the
    ///   sender has not opted in to the distinction; receivers should
    ///   treat it as cumulative.
    UsageUpdate {
        input_tokens: u64,
        output_tokens: u64,
        cost_usd: f64,
        #[serde(default)]
        cache_read_input_tokens: u64,
        #[serde(default)]
        cache_creation_input_tokens: u64,
        #[serde(default)]
        reasoning_output_tokens: u64,
        #[serde(default)]
        api_call_count: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<String>,
    },
    /// Scriptable status-line snapshot (issue #11).
    ///
    /// `payload` is the same JSON the Rust TUI pipes to the user's
    /// `statusLine.command` — frontends that want to run their own script
    /// can use this. `lines` carries the already-rendered stdout (split
    /// on `\n`) so frontends that trust the Rust runner can skip spawning
    /// a second process. `error` is populated when the most recent run
    /// failed; in that case `lines` is typically empty and the frontend
    /// falls back to its built-in footer.
    StatusLineUpdate {
        /// Full payload — see `StatusLinePayload`.
        payload: serde_json::Value,
        /// Rendered stdout, one entry per line, trimmed to at most a few
        /// lines.
        #[serde(default)]
        lines: Vec<String>,
        /// Non-empty when the last run failed.
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// Prompt suggestions for the UI to display.
    Suggestions {
        items: Vec<String>,
    },
    /// An error occurred.
    Error {
        message: String,
        recoverable: bool,
    },
    /// A background agent has completed execution.
    BackgroundAgentComplete {
        agent_id: String,
        description: String,
        /// Truncated preview of the result (for UI display).
        result_preview: String,
        had_error: bool,
        duration_ms: u64,
    },
    /// Brief mode message from the model (via BriefTool).
    BriefMessage {
        message: String,
        status: String,
        attachments: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        level: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        source_tool_name: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        tool_use_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        timestamp: Option<i64>,
    },
    /// Autonomous action started (proactive tick).
    AutonomousStart {
        source: String,
        time: String,
    },
    /// Push notification sent.
    NotificationSent {
        title: String,
        level: String,
    },

    // ── Subsystem events ─────────────────────────────────────────
    /// LSP subsystem event.
    LspEvent {
        event: LspEvent,
    },
    /// MCP subsystem event.
    McpEvent {
        event: McpEvent,
    },
    /// Plugin subsystem event.
    PluginEvent {
        event: PluginEvent,
    },
    /// Skill subsystem event.
    SkillEvent {
        event: SkillEvent,
    },
    /// IDE-integration subsystem event.
    IdeEvent {
        event: IdeEvent,
    },
    /// Agent-definition settings event.
    AgentSettingsEvent {
        event: AgentSettingsEvent,
    },
    /// Aggregated subsystem status snapshot.
    SubsystemStatus {
        status: SubsystemStatusSnapshot,
    },

    // ── Agent / Team events ──────────────────────────────────────
    /// Agent lifecycle + streaming events.
    AgentEvent {
        event: AgentEvent,
    },
    /// Team events.
    TeamEvent {
        event: TeamEvent,
    },

    /// Response to a [`FrontendMessage::SearchFiles`] request.
    ///
    /// `request_id` echoes the client's id so the frontend can correlate
    /// responses to in-flight requests. `truncated` is `true` when the
    /// handler hit the result cap before ripgrep finished — the
    /// frontend should surface a "+ more" indicator to the user.
    FileSearchResult {
        request_id: String,
        matches: Vec<FileSearchMatch>,
        truncated: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },

    // ── Phase 2 integration: completions, telemetry, LSP recommendations ──
    /// Completion items for the frontend popup.
    Completions {
        items: Vec<CompletionItemDTO>,
        request_id: String,
    },
    /// Plugin installation progress update.
    PluginInstallProgress {
        plugin_id: String,
        status: InstallProgress,
    },
    /// Telemetry subsystem status snapshot.
    TelemetryStatus {
        enabled: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        #[serde(default)]
        errors: Vec<String>,
    },
    /// LSP recommendations from the recommendation engine.
    LspRecommendations {
        recommendations: Vec<LspRecommendationDTO>,
    },
}

/// A single file-search hit. Matches the upstream
/// `{ file, line, text }` shape from
/// `ui/examples/upstream-patterns/src/components/GlobalSearchDialog.tsx`.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FileSearchMatch {
    /// Path relative to the search root.
    pub file: String,
    /// 1-based line number.
    pub line: u64,
    /// Line text, trimmed of trailing whitespace.
    pub text: String,
}

// ---------------------------------------------------------------------------
// DTO types for Phase 2 integration (Serial Integration Lane)
// ---------------------------------------------------------------------------

/// A completion item sent to the frontend for display.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CompletionItemDTO {
    pub label: String,
    pub insert_text: String,
    /// Completion kind: "command", "path", "shell_history", "slack_channel",
    /// "skill", "arg".
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// An LSP recommendation sent to the frontend.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LspRecommendationDTO {
    pub plugin_id: String,
    pub plugin_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub languages: Vec<String>,
    pub confidence: f64,
    pub is_installed: bool,
    pub is_dismissed: bool,
}

/// Progress status for plugin installation.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum InstallProgress {
    Downloading,
    Extracting,
    Validating,
    Installed,
    Failed { error: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_types::status_line::{ModelInfo, StatusLinePayload};

    #[test]
    fn status_line_update_preserves_wire_shape() {
        let payload = StatusLinePayload {
            session_id: Some("session-1".into()),
            model: Some(ModelInfo {
                id: "claude-sonnet-4-20250514".into(),
                display_name: Some("sonnet-4".into()),
                backend: Some("native".into()),
            }),
            streaming: true,
            message_count: 2,
            ..StatusLinePayload::new()
        };
        let message = BackendMessage::StatusLineUpdate {
            payload: serde_json::to_value(payload).unwrap(),
            lines: vec!["model sonnet-4".into()],
            error: None,
        };

        let value = serde_json::to_value(&message).unwrap();
        assert_eq!(value["type"], "status_line_update");
        assert_eq!(value["payload"]["hookEventName"], "StatusLine");
        assert_eq!(value["payload"]["sessionId"], "session-1");
        assert_eq!(value["payload"]["model"]["displayName"], "sonnet-4");
        assert_eq!(value["payload"]["messageCount"], 2);
        assert_eq!(value["lines"][0], "model sonnet-4");
        assert!(value.get("error").is_none());
    }

    #[test]
    fn error_message_preserves_jsonl_field_order() {
        let message = BackendMessage::Error {
            message: "invalid FrontendMessage: expected value".into(),
            recoverable: true,
        };

        let encoded = serde_json::to_string(&message).unwrap();

        assert_eq!(
            encoded,
            r#"{"type":"error","message":"invalid FrontendMessage: expected value","recoverable":true}"#
        );
    }

    #[test]
    fn file_search_result_preserves_jsonl_field_order() {
        let message = BackendMessage::FileSearchResult {
            request_id: "search-1".into(),
            matches: vec![FileSearchMatch {
                file: "src/main.rs".into(),
                line: 7,
                text: "fn main() {}".into(),
            }],
            truncated: false,
            error: None,
        };

        let encoded = serde_json::to_string(&message).unwrap();

        assert_eq!(
            encoded,
            r#"{"type":"file_search_result","request_id":"search-1","matches":[{"file":"src/main.rs","line":7,"text":"fn main() {}"}],"truncated":false}"#
        );
    }

    #[test]
    fn brief_message_preserves_extended_optional_fields() {
        let message = BackendMessage::BriefMessage {
            message: "brief body".into(),
            status: "proactive".into(),
            attachments: vec!["docs/brief.md".into()],
            level: Some("warning".into()),
            source_tool_name: Some("Brief".into()),
            tool_use_id: Some("toolu-brief".into()),
            session_id: Some("session-1".into()),
            timestamp: Some(100),
        };

        let value = serde_json::to_value(&message).unwrap();
        assert_eq!(value["type"], "brief_message");
        assert_eq!(value["message"], "brief body");
        assert_eq!(value["status"], "proactive");
        assert_eq!(value["attachments"], serde_json::json!(["docs/brief.md"]));
        assert_eq!(value["level"], "warning");
        assert_eq!(value["tool_use_id"], "toolu-brief");
        assert_eq!(value["session_id"], "session-1");
        assert_eq!(value["timestamp"], 100);
    }

    #[test]
    fn file_search_match_roundtrips() {
        let original = FileSearchMatch {
            file: "src/lib.rs".into(),
            line: 12,
            text: "pub mod protocol;".into(),
        };

        let encoded = serde_json::to_string(&original).unwrap();
        let decoded: FileSearchMatch = serde_json::from_str(&encoded).unwrap();

        assert_eq!(decoded.file, original.file);
        assert_eq!(decoded.line, original.line);
        assert_eq!(decoded.text, original.text);
    }

    #[test]
    fn tool_result_image_content_roundtrips_without_losing_payload() {
        let original = ToolResultContentInfo::Image {
            media_type: "image/png".into(),
            size_bytes: Some(8),
            data: Some("aGVsbG8=".into()),
        };

        let encoded = serde_json::to_string(&original).unwrap();
        let decoded: ToolResultContentInfo = serde_json::from_str(&encoded).unwrap();

        assert!(matches!(
            decoded,
            ToolResultContentInfo::Image {
                media_type,
                size_bytes: Some(8),
                data: Some(data),
            } if media_type == "image/png" && data == "aGVsbG8="
        ));
    }
}
