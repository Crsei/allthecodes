//! Transcript / focus view state machine (issue #12).
//!
//! The TUI cycles through three modes with `Ctrl+O`:
//!
//! 1. [`ViewMode::Prompt`] — the default REPL view with a live input.
//! 2. [`ViewMode::Transcript`] — a *less*-style read-only page through the
//!    conversation with incremental search and `e`-to-edit export.
//! 3. [`ViewMode::Focus`] — the same transcript body but with the spinner
//!    / status-bar chrome hidden, for screenshots / screen readers.
//!
//! The [`TranscriptState`] structure holds the extra state that only the
//! transcript/focus modes need (scroll offset, search query, match list,
//! current match index). Keeping it out of the main `App` struct means
//! prompt-mode frames are still cheap.
//!
//! # Rendering
//!
//! Transcript mode reuses [`super::messages::render_messages`] on a body
//! pane and adds a one-line header + one-line footer:
//!
//! ```text
//! ─── Transcript · 12 messages · search: "tool_use" (3/5) ─────
//!     (message body rendered by render_messages)
//!     ...
//! [Esc] prompt · [/] search · [n]/[N] next/prev · [e] editor · [q] quit
//! ```
//!
//! # Search
//!
//! Search is case-insensitive substring matching against the flattened
//! plaintext of each message. When a query is non-empty, matches are
//! recomputed lazily and the viewport snaps to the first hit. `n` / `N`
//! cycle through hits, wrapping at the ends.

use allthecodes_types::message::{
    Attachment, ContentBlock, Message, MessageContent, SystemSubtype,
};

/// The three view modes the TUI can be in. Cycling is `Prompt →
/// Transcript → Focus → Prompt` via `Ctrl+O`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    #[default]
    Prompt,
    Transcript,
    Focus,
}

impl ViewMode {
    /// Parse the persisted `settings.json::viewMode` value.
    ///
    /// Accepted values are intentionally narrow so `/config set viewMode`
    /// and startup behavior stay predictable.
    pub fn parse_configured(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "prompt" => Ok(ViewMode::Prompt),
            "transcript" => Ok(ViewMode::Transcript),
            "focus" => Ok(ViewMode::Focus),
            other => Err(format!(
                "Unknown viewMode '{}'. Expected prompt, transcript, or focus.",
                other
            )),
        }
    }

    /// Advance to the next mode in the `Prompt → Transcript → Focus`
    /// cycle.
    pub fn next(self) -> Self {
        match self {
            ViewMode::Prompt => ViewMode::Transcript,
            ViewMode::Transcript => ViewMode::Focus,
            ViewMode::Focus => ViewMode::Prompt,
        }
    }

    /// True in any read-only / fullscreen mode.
    pub fn is_transcript_like(self) -> bool {
        matches!(self, ViewMode::Transcript | ViewMode::Focus)
    }

    /// Human-readable label — used in the transcript header.
    pub fn label(self) -> &'static str {
        match self {
            ViewMode::Prompt => "Prompt",
            ViewMode::Transcript => "Transcript",
            ViewMode::Focus => "Focus",
        }
    }
}

/// Whether the transcript is waiting for search input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TranscriptInputMode {
    /// Normal navigation: j/k, g/G, PgUp/PgDn, n/N, e, q, /.
    #[default]
    Normal,
    /// Typing a search query after `/`.
    Search,
}

/// Per-match descriptor returned by [`search_messages`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchMatch {
    /// Index into the messages slice.
    pub message_index: usize,
}

/// Lightweight transcript entry that future renderers can consume without
/// re-matching the raw `Message` enum on every draw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptEntry {
    pub message_index: usize,
    pub role: TranscriptEntryRole,
    pub body: String,
}

impl TranscriptEntry {
    pub fn label(&self) -> &'static str {
        self.role.label()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptEntryRole {
    User,
    Assistant,
    System,
    Progress,
    Attachment,
}

impl TranscriptEntryRole {
    pub fn label(self) -> &'static str {
        match self {
            TranscriptEntryRole::User => "User",
            TranscriptEntryRole::Assistant => "Assistant",
            TranscriptEntryRole::System => "System",
            TranscriptEntryRole::Progress => "Progress",
            TranscriptEntryRole::Attachment => "Attachment",
        }
    }
}

/// Render-ready focus-view model derived from the current message list.
/// The leader can wire this into `app.rs` later without re-deriving the
/// prompt/summary/response slices in the hot render path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FocusView {
    pub prompt: Option<TranscriptEntry>,
    pub tool_summary: Option<String>,
    pub response: Option<TranscriptEntry>,
    pub edited_files: Vec<String>,
}

/// All transient state needed for transcript/focus modes.
#[derive(Debug, Clone, Default)]
pub struct TranscriptState {
    /// Current scroll offset in global line space.
    pub scroll_offset: usize,
    /// Input mode — normal nav vs. typing a search.
    pub input_mode: TranscriptInputMode,
    /// Current search query. Empty when there's no active search.
    pub query: String,
    /// Committed match list (recomputed lazily when the query changes).
    pub matches: Vec<SearchMatch>,
    /// Index into [`Self::matches`] currently focused. `None` when no
    /// matches.
    pub focused: Option<usize>,
}

impl TranscriptState {
    /// Clear any search state. Called when leaving transcript mode or
    /// pressing `Esc` during search.
    pub fn clear_search(&mut self) {
        self.query.clear();
        self.matches.clear();
        self.focused = None;
        self.input_mode = TranscriptInputMode::Normal;
    }

    /// Replace the match list after a query change and point `focused` at
    /// the first hit (or `None` if the new list is empty).
    pub fn set_matches(&mut self, matches: Vec<SearchMatch>) {
        self.focused = if matches.is_empty() { None } else { Some(0) };
        self.matches = matches;
    }

    /// Advance `focused` to the next match, wrapping at the end.
    pub fn next_match(&mut self) {
        if self.matches.is_empty() {
            self.focused = None;
            return;
        }
        self.focused = Some(match self.focused {
            None => 0,
            Some(i) => (i + 1) % self.matches.len(),
        });
    }

    /// Move `focused` to the previous match, wrapping at the beginning.
    pub fn prev_match(&mut self) {
        if self.matches.is_empty() {
            self.focused = None;
            return;
        }
        self.focused = Some(match self.focused {
            None => self.matches.len() - 1,
            Some(i) => {
                if i == 0 {
                    self.matches.len() - 1
                } else {
                    i - 1
                }
            }
        });
    }

    /// Currently-focused match, if any.
    pub fn current_match(&self) -> Option<SearchMatch> {
        self.focused.and_then(|i| self.matches.get(i).copied())
    }
}

/// Flatten a message into plain text suitable for `contains`-style search.
/// Mirrors what the user sees in the terminal: we strip tool-use input
/// JSON down to the tool name because users almost never search for
/// bracket-laden JSON payloads.
pub fn message_plaintext(message: &Message) -> String {
    match message {
        Message::User(u) => match &u.content {
            MessageContent::Text(t) => t.clone(),
            MessageContent::Blocks(blocks) => {
                let mut text = blocks_to_text(blocks);
                if let Some(preview) = &u.tool_use_result {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(
                        &file_edit_preview_plaintext(preview)
                            .unwrap_or_else(|| preview.to_string()),
                    );
                }
                text
            }
        },
        Message::Assistant(a) => blocks_to_text(&a.content),
        Message::System(s) => match &s.subtype {
            SystemSubtype::LocalCommand { content } => content.clone(),
            SystemSubtype::Informational { .. }
            | SystemSubtype::Warning
            | SystemSubtype::CompactBoundary { .. }
            | SystemSubtype::MicrocompactBoundary { .. }
            | SystemSubtype::ApiError { .. } => s.content.clone(),
        },
        // Non-conversation meta messages (progress/attachments) are
        // surfaced inline elsewhere; the transcript doesn't search or
        // export them.
        _ => String::new(),
    }
}

/// Flatten the visible transcript into role-labelled entries that can be
/// reused by search, export, and future focus-mode renderers.
pub fn transcript_entries(messages: &[Message]) -> Vec<TranscriptEntry> {
    messages
        .iter()
        .enumerate()
        .map(|(message_index, message)| TranscriptEntry {
            message_index,
            role: transcript_entry_role(message),
            body: message_plaintext(message),
        })
        .collect()
}

fn blocks_to_text(blocks: &[ContentBlock]) -> String {
    let mut out = String::new();
    for b in blocks {
        match b {
            ContentBlock::Text { text } => {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(text);
            }
            ContentBlock::Thinking { thinking, .. } => {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(thinking);
            }
            ContentBlock::ToolUse { name, .. } => {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str("tool_use: ");
                out.push_str(name);
            }
            ContentBlock::ToolResult { content, .. } => {
                if !out.is_empty() {
                    out.push('\n');
                }
                match content {
                    allthecodes_types::message::ToolResultContent::Text(t) => out.push_str(t),
                    allthecodes_types::message::ToolResultContent::Blocks(inner) => {
                        out.push_str(&blocks_to_text(inner));
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn file_edit_preview_plaintext(preview: &str) -> Option<String> {
    let value = serde_json::from_str::<serde_json::Value>(preview).ok()?;
    if value.get("kind").and_then(|v| v.as_str()) != Some("file_edit") {
        return None;
    }
    let path = value.get("path").and_then(|v| v.as_str()).unwrap_or("");
    let output = value.get("output").and_then(|v| v.as_str()).unwrap_or("");
    let replacements = value
        .get("replacements")
        .and_then(|v| v.as_u64())
        .map(|count| format!(" ({count} replacement(s))"))
        .unwrap_or_default();
    Some(format!("file_edit: {path}{replacements}\n{output}"))
}

/// Search every message for `query` (case-insensitive substring).
/// Returns matches in message-index order; empty when the query is blank.
pub fn search_messages(messages: &[Message], query: &str) -> Vec<SearchMatch> {
    let q = query.trim();
    if q.is_empty() {
        return Vec::new();
    }
    let needle = q.to_ascii_lowercase();
    let mut hits = Vec::new();
    for (i, m) in messages.iter().enumerate() {
        let body = message_plaintext(m).to_ascii_lowercase();
        if body.contains(&needle) {
            hits.push(SearchMatch { message_index: i });
        }
    }
    hits
}

/// Build a render-ready focus view from the current transcript.
///
/// Selection rules are intentionally simple and stable:
/// - `prompt`: the last non-empty user message
/// - `tool_summary`: the last `[tool summary] ...` system line after that
///   prompt, otherwise a synthesized edited-file summary from attachments
/// - `response`: the last non-empty assistant message after that prompt
pub fn build_focus_view(messages: &[Message]) -> FocusView {
    let entries = transcript_entries(messages);
    let prompt = entries
        .iter()
        .rev()
        .find(|entry| entry.role == TranscriptEntryRole::User && !entry.body.trim().is_empty())
        .cloned();
    let prompt_index = prompt
        .as_ref()
        .map(|entry| entry.message_index)
        .unwrap_or(0);

    let response = entries
        .iter()
        .rev()
        .find(|entry| {
            entry.message_index >= prompt_index
                && entry.role == TranscriptEntryRole::Assistant
                && !entry.body.trim().is_empty()
        })
        .cloned();

    let edited_files = messages
        .iter()
        .skip(prompt_index)
        .filter_map(|message| match message {
            Message::Attachment(attachment) => match &attachment.attachment {
                Attachment::EditedTextFile { path } => Some(path.clone()),
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>();

    let tool_summary = messages
        .iter()
        .enumerate()
        .rev()
        .filter(|(idx, _)| *idx >= prompt_index)
        .find_map(|(_, message)| extract_tool_summary(message))
        .or_else(|| synthesize_edit_summary(&edited_files));

    FocusView {
        prompt,
        tool_summary,
        response,
        edited_files,
    }
}

/// Render the entire conversation as plain-text markdown suitable for
/// dumping into `$EDITOR` (issue #12 acceptance criterion).
pub fn render_markdown_dump(messages: &[Message]) -> String {
    let mut out = String::new();
    out.push_str("# Conversation transcript\n\n");
    for entry in transcript_entries(messages) {
        out.push_str(&format!(
            "## [{}] {}\n\n",
            entry.message_index,
            entry.label()
        ));
        let body = entry.body;
        if body.is_empty() {
            out.push_str("_(empty)_\n");
        } else {
            out.push_str(&body);
            if !body.ends_with('\n') {
                out.push('\n');
            }
        }
        out.push('\n');
    }
    out
}

fn transcript_entry_role(message: &Message) -> TranscriptEntryRole {
    match message {
        Message::User(_) => TranscriptEntryRole::User,
        Message::Assistant(_) => TranscriptEntryRole::Assistant,
        Message::System(_) => TranscriptEntryRole::System,
        Message::Progress(_) => TranscriptEntryRole::Progress,
        Message::Attachment(_) => TranscriptEntryRole::Attachment,
    }
}

fn extract_tool_summary(message: &Message) -> Option<String> {
    let Message::System(system) = message else {
        return None;
    };
    let prefix = "[tool summary]";
    system
        .content
        .strip_prefix(prefix)
        .map(str::trim)
        .filter(|summary| !summary.is_empty())
        .map(ToOwned::to_owned)
}

fn synthesize_edit_summary(edited_files: &[String]) -> Option<String> {
    if edited_files.is_empty() {
        return None;
    }

    let preview = edited_files
        .iter()
        .take(3)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    let extra = edited_files.len().saturating_sub(3);

    Some(if extra == 0 {
        format!("edited {}", preview)
    } else {
        format!("edited {} (+{} more)", preview, extra)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_types::message::{
        AssistantMessage, AttachmentMessage, InfoLevel, MessageContent, SystemMessage,
        SystemSubtype, UserMessage,
    };

    fn user(text: &str) -> Message {
        Message::User(UserMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 0,
            role: "user".into(),
            content: MessageContent::Text(text.into()),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        })
    }

    fn assistant(text: &str) -> Message {
        Message::Assistant(AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 0,
            role: "assistant".into(),
            content: vec![ContentBlock::Text { text: text.into() }],
            usage: None,
            stop_reason: None,
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        })
    }

    fn system(level: InfoLevel, text: &str) -> Message {
        Message::System(SystemMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 0,
            subtype: SystemSubtype::Informational { level },
            content: text.into(),
        })
    }

    fn attachment(path: &str) -> Message {
        Message::Attachment(AttachmentMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 0,
            attachment: Attachment::EditedTextFile { path: path.into() },
        })
    }

    #[test]
    fn view_mode_cycles_through_three() {
        assert_eq!(ViewMode::Prompt.next(), ViewMode::Transcript);
        assert_eq!(ViewMode::Transcript.next(), ViewMode::Focus);
        assert_eq!(ViewMode::Focus.next(), ViewMode::Prompt);
    }

    #[test]
    fn is_transcript_like_excludes_prompt() {
        assert!(!ViewMode::Prompt.is_transcript_like());
        assert!(ViewMode::Transcript.is_transcript_like());
        assert!(ViewMode::Focus.is_transcript_like());
    }

    #[test]
    fn search_finds_case_insensitive_substrings() {
        let msgs = vec![
            user("What is the FIZZBUZZ algorithm?"),
            assistant("fizzbuzz prints Fizz for multiples of 3."),
            system(InfoLevel::Info, "Aborted by user"),
        ];
        let hits = search_messages(&msgs, "fizz");
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].message_index, 0);
        assert_eq!(hits[1].message_index, 1);
    }

    #[test]
    fn search_empty_query_yields_no_hits() {
        let msgs = vec![user("anything")];
        assert!(search_messages(&msgs, "").is_empty());
        assert!(search_messages(&msgs, "   ").is_empty());
    }

    #[test]
    fn match_navigation_wraps_around() {
        let mut st = TranscriptState::default();
        st.set_matches(vec![
            SearchMatch { message_index: 0 },
            SearchMatch { message_index: 2 },
            SearchMatch { message_index: 5 },
        ]);
        assert_eq!(st.focused, Some(0));
        st.next_match();
        assert_eq!(st.focused, Some(1));
        st.next_match();
        st.next_match(); // wrap
        assert_eq!(st.focused, Some(0));
        st.prev_match(); // wrap back to end
        assert_eq!(st.focused, Some(2));
    }

    #[test]
    fn match_navigation_is_safe_with_empty_matches() {
        let mut st = TranscriptState::default();
        st.next_match();
        st.prev_match();
        assert_eq!(st.focused, None);
    }

    #[test]
    fn clear_search_resets_all_search_fields() {
        let mut st = TranscriptState {
            query: "query".into(),
            ..Default::default()
        };
        st.set_matches(vec![SearchMatch { message_index: 0 }]);
        st.input_mode = TranscriptInputMode::Search;
        st.clear_search();
        assert_eq!(st.query, "");
        assert!(st.matches.is_empty());
        assert_eq!(st.focused, None);
        assert_eq!(st.input_mode, TranscriptInputMode::Normal);
    }

    #[test]
    fn markdown_dump_includes_every_role_and_body() {
        let msgs = vec![user("hello"), assistant("hi there")];
        let dump = render_markdown_dump(&msgs);
        assert!(dump.contains("# Conversation transcript"));
        assert!(dump.contains("[0] User"));
        assert!(dump.contains("hello"));
        assert!(dump.contains("[1] Assistant"));
        assert!(dump.contains("hi there"));
    }

    #[test]
    fn transcript_entries_preserve_role_labels() {
        let entries = transcript_entries(&[user("hello"), assistant("hi")]);
        assert_eq!(entries[0].label(), "User");
        assert_eq!(entries[1].label(), "Assistant");
    }

    #[test]
    fn focus_view_uses_last_user_summary_and_last_assistant() {
        let msgs = vec![
            user("first prompt"),
            assistant("intermediate answer"),
            user("final prompt"),
            system(
                InfoLevel::Info,
                "[tool summary] ran cargo test; edited 2 files",
            ),
            assistant("final answer"),
        ];

        let focus = build_focus_view(&msgs);
        assert_eq!(
            focus.prompt.as_ref().map(|entry| entry.body.as_str()),
            Some("final prompt")
        );
        assert_eq!(
            focus.tool_summary.as_deref(),
            Some("ran cargo test; edited 2 files")
        );
        assert_eq!(
            focus.response.as_ref().map(|entry| entry.body.as_str()),
            Some("final answer")
        );
    }

    #[test]
    fn focus_view_falls_back_to_attachment_summary() {
        let msgs = vec![
            user("prompt"),
            attachment("src/ui/app.rs"),
            attachment("src/ui/messages.rs"),
            assistant("answer"),
        ];

        let focus = build_focus_view(&msgs);
        assert_eq!(
            focus.tool_summary.as_deref(),
            Some("edited src/ui/app.rs, src/ui/messages.rs")
        );
        assert_eq!(focus.edited_files.len(), 2);
    }
}
