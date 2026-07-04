//! User input processing: slash-command detection, bash mode detection,
//! skill detection, message construction.
//!
//! Corresponds to the input-handling portion of `submitMessage()` in
//! TypeScript's QueryEngine.ts. This module serves as the central input
//! contract for both TUI and headless frontends.

use uuid::Uuid;

use allthecodes_types::commands::{CommandDispatcher, ParsedCommand};

use crate::types::message::{ContentBlock, Message, MessageContent, UserMessage};

// ---------------------------------------------------------------------------
// AttachmentInfo
// ---------------------------------------------------------------------------

/// Metadata about an attachment (image, file, IDE selection) being submitted.
#[derive(Debug, Clone)]
pub struct AttachmentInfo {
    /// Path to the attached file (if file-based).
    pub file_path: Option<String>,
    /// MIME type (e.g. "image/png", "text/plain").
    pub mime_type: String,
    /// Raw content bytes (for small attachments like pasted images).
    pub content: Option<Vec<u8>>,
    /// Content block representation built from this attachment.
    pub content_block: Option<ContentBlock>,
}

// ---------------------------------------------------------------------------
// ProcessedInput
// ---------------------------------------------------------------------------

/// Result of processing raw user input.
#[derive(Debug, Clone)]
pub struct ProcessedInput {
    /// Messages to append to the conversation (user message + any attachments).
    pub messages: Vec<Message>,
    /// Whether to send the updated conversation to the model.
    /// `false` for purely local slash commands (e.g. `/help`, `/clear`).
    pub should_query: bool,
    /// Tool allow-list overridden by a slash command (e.g. `/allowed-tools`).
    pub allowed_tools: Option<Vec<String>>,
    /// Model override from a slash command.
    pub model: Option<String>,
    /// Text result for local commands (displayed without querying the model).
    pub result_text: Option<String>,
    /// Parsed slash command to execute in the async lifecycle layer.
    pub parsed_command: Option<ParsedCommand>,
    /// Attached files/images for this submission.
    pub attachments: Vec<AttachmentInfo>,
    /// Whether this input is in bash mode (starting with `!`).
    pub bash_mode: bool,
    /// Detected skill invocation (direct `/skill-name` style).
    pub skill_invocation: Option<String>,
}

impl ProcessedInput {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            should_query: false,
            allowed_tools: None,
            model: None,
            result_text: None,
            parsed_command: None,
            attachments: Vec::new(),
            bash_mode: false,
            skill_invocation: None,
        }
    }
}

impl Default for ProcessedInput {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// process_user_input
// ---------------------------------------------------------------------------

/// Process raw user input: detect slash commands, bash mode, skill invocation,
/// and build user message.
///
/// 1. If the input starts with `/`, try to match a registered command.
///    - Known commands return a parsed command with `should_query = false`.
///    - Unknown `/something` falls through to skill detection.
/// 2. If the input starts with `!`, route to bash mode.
/// 3. Otherwise, wrap the input in a plain `UserMessage` with
///    `should_query = true`.
///
/// `dispatcher` is the command dispatcher used to parse slash commands.
pub fn process_user_input(
    input: &str,
    _messages: &[Message],
    cwd: &str,
    dispatcher: &dyn CommandDispatcher,
) -> ProcessedInput {
    let trimmed = input.trim();

    // -- Empty input -------------------------------------------------------
    if trimmed.is_empty() {
        return ProcessedInput {
            messages: vec![simple_user_message("")],
            should_query: true,
            ..ProcessedInput::new()
        };
    }

    // -- Bash mode (`!` prefix) -------------------------------------------
    if let Some(bash_input) = trimmed.strip_prefix('!') {
        if !bash_input.starts_with('/') && !bash_input.starts_with('!') && !bash_input.is_empty() {
            return process_bash_input(bash_input);
        }
    }

    // -- Slash-command path ------------------------------------------------
    if let Some(stripped) = trimmed.strip_prefix('/') {
        if let Some(parsed) =
            dispatcher.parse_command_input_for_cwd(trimmed, std::path::Path::new(cwd))
        {
            return ProcessedInput {
                messages: Vec::new(),
                should_query: false,
                allowed_tools: None,
                model: None,
                result_text: None,
                parsed_command: Some(parsed),
                attachments: Vec::new(),
                bash_mode: false,
                skill_invocation: None,
            };
        }

        // Unrecognised slash command — check if it's a user-invocable skill.
        // If so, mark it as a skill invocation. Otherwise, fall through to
        // treat as regular user text (matches TypeScript behaviour).
        let skill_name = stripped.split_whitespace().next().unwrap_or("");
        if !skill_name.is_empty() && is_known_skill(skill_name) {
            return process_skill_input(trimmed, skill_name);
        }

        // Fall through to regular text
    }

    // -- Regular user text (or unrecognised slash) -------------------------
    process_text_input(trimmed, Vec::new())
}

// ---------------------------------------------------------------------------
// process_slash_command_input
// ---------------------------------------------------------------------------

/// Process input specifically as a slash command.
///
/// Called when we know the input starts with `/`.
pub fn process_slash_command_input(_input: &str) -> ProcessedInput {
    // The caller will set parsed_command after dispatch

    ProcessedInput {
        messages: Vec::new(),
        should_query: false,
        allowed_tools: None,
        model: None,
        result_text: None,
        parsed_command: None, // Will be set by caller after dispatch
        attachments: Vec::new(),
        bash_mode: false,
        skill_invocation: None,
    }
}

// ---------------------------------------------------------------------------
// process_bash_input
// ---------------------------------------------------------------------------

/// Process input as a bash/shell command.
///
/// Wraps the shell command text in a user message with bash mode metadata.
pub fn process_bash_input(input: &str) -> ProcessedInput {
    let shell_text = if let Some(stripped) = input.strip_prefix('!') {
        stripped
    } else {
        input
    };

    let user_message = UserMessage {
        uuid: Uuid::new_v4(),
        timestamp: chrono::Utc::now().timestamp_millis(),
        role: "user".to_string(),
        content: MessageContent::Text(format!("!{}", shell_text)),
        is_meta: false,
        tool_use_result: None,
        source_tool_assistant_uuid: None,
    };

    ProcessedInput {
        messages: vec![Message::User(user_message)],
        should_query: true,
        allowed_tools: None,
        model: None,
        result_text: None,
        parsed_command: None,
        attachments: Vec::new(),
        bash_mode: true,
        skill_invocation: None,
    }
}

// ---------------------------------------------------------------------------
// process_skill_input
// ---------------------------------------------------------------------------

/// Process input as a direct skill invocation.
///
/// Skills are invoked with `/skill-name` just like commands, but they
/// expand to skill prompt text rather than executing a handler.
pub fn process_skill_input(_input: &str, skill_name: &str) -> ProcessedInput {
    ProcessedInput {
        messages: Vec::new(),
        should_query: true,
        allowed_tools: None,
        model: None,
        result_text: None,
        parsed_command: None,
        attachments: Vec::new(),
        bash_mode: false,
        skill_invocation: Some(skill_name.to_string()),
    }
}

// ---------------------------------------------------------------------------
// process_text_input
// ---------------------------------------------------------------------------

/// Process plain text (and optional attachments) as a user message.
///
/// Builds a `UserMessage` from the text and any attached images/files.
/// Content blocks are constructed from attachments when present.
pub fn process_text_input(text: &str, attachments: Vec<AttachmentInfo>) -> ProcessedInput {
    let content = if attachments.is_empty() {
        MessageContent::Text(text.to_string())
    } else {
        let mut blocks: Vec<ContentBlock> = Vec::new();
        if !text.trim().is_empty() {
            blocks.push(ContentBlock::Text {
                text: text.to_string(),
            });
        }
        for attachment in &attachments {
            if let Some(block) = &attachment.content_block {
                blocks.push(block.clone());
            }
        }
        MessageContent::Blocks(blocks)
    };

    let user_message = UserMessage {
        uuid: Uuid::new_v4(),
        timestamp: chrono::Utc::now().timestamp_millis(),
        role: "user".to_string(),
        content,
        is_meta: false,
        tool_use_result: None,
        source_tool_assistant_uuid: None,
    };

    ProcessedInput {
        messages: vec![Message::User(user_message)],
        should_query: true,
        allowed_tools: None,
        model: None,
        result_text: None,
        parsed_command: None,
        attachments,
        bash_mode: false,
        skill_invocation: None,
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn simple_user_message(text: &str) -> Message {
    Message::User(UserMessage {
        uuid: Uuid::new_v4(),
        timestamp: chrono::Utc::now().timestamp_millis(),
        role: "user".to_string(),
        content: MessageContent::Text(text.to_string()),
        is_meta: false,
        tool_use_result: None,
        source_tool_assistant_uuid: None,
    })
}

/// Check if a name corresponds to a known user-invocable skill.
///
/// Queries the global cc-skills registry. Returns `false` if the skill
/// registry is empty or the skill name is not found.
fn is_known_skill(name: &str) -> bool {
    // Try to access the skill registry. If skills haven't been loaded yet,
    // this returns false and the input is treated as regular text.
    let skills = allthecodes_skills::get_all_skills();
    skills
        .iter()
        .any(|s| s.name == name || s.frontmatter.name.as_deref().is_some_and(|n| n == name))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_types::commands::{CommandDispatcher, ParsedCommand};

    /// Minimal dispatcher used only in tests.  Recognises `/help` and
    /// `/config`; everything else is treated as regular text.
    struct TestDispatcher;

    impl CommandDispatcher for TestDispatcher {
        fn parse_command_input(&self, input: &str) -> Option<ParsedCommand> {
            let trimmed = input.trim();
            if !trimmed.starts_with('/') {
                return None;
            }
            let without_slash = &trimmed[1..];
            let name = without_slash.split_whitespace().next().unwrap_or("");
            let args = without_slash
                .strip_prefix(name)
                .unwrap_or("")
                .trim()
                .to_string();
            let index = match name {
                "help" => 0,
                "config" => 1,
                _ => return None,
            };
            Some(ParsedCommand { index, args })
        }

        fn command_name(&self, index: usize) -> Option<String> {
            match index {
                0 => Some("help".to_string()),
                1 => Some("config".to_string()),
                _ => None,
            }
        }
    }

    #[test]
    fn test_regular_text() {
        let d = TestDispatcher;
        let result = process_user_input("Hello, Claude!", &[], "/tmp", &d);
        assert!(result.should_query);
        assert_eq!(result.messages.len(), 1);
        assert!(result.allowed_tools.is_none());
        assert!(result.model.is_none());
        assert!(result.result_text.is_none());
        assert!(!result.bash_mode);
        assert!(result.skill_invocation.is_none());
        assert!(result.attachments.is_empty());
    }

    #[test]
    fn test_slash_command_known() {
        let d = TestDispatcher;
        let result = process_user_input("/help", &[], "/tmp", &d);
        assert!(!result.should_query);
        assert!(result.messages.is_empty());
        let parsed = result.parsed_command.expect("parsed command");
        assert_eq!(parsed.index, 0);
        assert_eq!(parsed.args, "");
        assert!(!result.bash_mode);
    }

    #[test]
    fn test_slash_command_with_args() {
        let d = TestDispatcher;
        let result = process_user_input("/config set model SOTA", &[], "/tmp", &d);
        assert!(!result.should_query);
        let parsed = result.parsed_command.expect("parsed command");
        assert_eq!(parsed.index, 1);
        assert_eq!(parsed.args, "set model SOTA");
    }

    #[test]
    fn test_unknown_slash_command() {
        let d = TestDispatcher;
        let result = process_user_input("/nonexistent_command", &[], "/tmp", &d);
        // Unknown slash falls through — should still produce a message
        assert!(result.should_query);
        assert_eq!(result.messages.len(), 1);
    }

    #[test]
    fn test_empty_input() {
        let d = TestDispatcher;
        let result = process_user_input("", &[], "/tmp", &d);
        assert!(result.should_query);
        assert_eq!(result.messages.len(), 1);
    }

    #[test]
    fn test_bash_mode_detection() {
        let d = TestDispatcher;
        let result = process_user_input("!git status", &[], "/tmp", &d);
        assert!(result.should_query);
        assert!(result.bash_mode, "expected bash mode for ! prefix");
        assert_eq!(result.messages.len(), 1);
    }

    #[test]
    fn test_bash_mode_single_bang() {
        let d = TestDispatcher;
        let result = process_user_input("!ls -la", &[], "/tmp", &d);
        assert!(result.bash_mode);
    }

    #[test]
    fn test_text_input_with_attachments() {
        let attachment = AttachmentInfo {
            file_path: Some("/tmp/image.png".to_string()),
            mime_type: "image/png".to_string(),
            content: None,
            content_block: Some(ContentBlock::Image {
                source: crate::types::message::ImageSource {
                    source_type: "base64".to_string(),
                    media_type: "image/png".to_string(),
                    data: "AAAA".to_string(),
                },
            }),
        };

        let result = process_text_input("Check this image", vec![attachment]);
        assert!(result.should_query);
        assert_eq!(result.messages.len(), 1);

        // Should have blocks content (text + image)
        if let Message::User(ref user) = result.messages[0] {
            match &user.content {
                MessageContent::Blocks(blocks) => {
                    assert_eq!(blocks.len(), 2, "expected text + image blocks");
                }
                _ => panic!("expected Blocks content"),
            }
        } else {
            panic!("expected User message");
        }

        assert_eq!(result.attachments.len(), 1);
    }

    #[test]
    fn test_skill_fallback() {
        let d = TestDispatcher;
        let result = process_user_input("/my-skill do something", &[], "/tmp", &d);
        // /my-skill is not a registered command and not a known skill
        // (skills aren't loaded in the test). Falls through to regular text.
        assert!(result.should_query);
        assert_eq!(result.messages.len(), 1);
        assert_eq!(result.skill_invocation, None);
    }

    #[test]
    fn test_text_input_no_attachments() {
        let result = process_text_input("Hello", vec![]);
        assert!(result.should_query);
        assert_eq!(result.messages.len(), 1);
        assert!(result.attachments.is_empty());

        if let Message::User(ref user) = result.messages[0] {
            match &user.content {
                MessageContent::Text(text) => assert_eq!(text, "Hello"),
                _ => panic!("expected Text content"),
            }
        }
    }

    #[test]
    fn test_whitespace_only() {
        let d = TestDispatcher;
        let result = process_user_input("   ", &[], "/tmp", &d);
        assert!(result.should_query);
        assert_eq!(result.messages.len(), 1);
    }

    #[test]
    fn test_bash_input_processing() {
        let result = process_bash_input("!cargo build");
        let result2 = process_bash_input("cargo build"); // without leading !
        assert!(result.bash_mode);
        assert!(result2.bash_mode);
    }

    #[test]
    fn test_slash_command_input() {
        let result = process_slash_command_input("/help");
        assert!(!result.should_query);
    }

    #[test]
    fn test_text_input_edge_cases() {
        // Empty text, no attachments
        let result = process_text_input("  ", vec![]);
        assert!(result.should_query);

        // Text with empty attachment
        let result = process_text_input("text", vec![]);
        if let Message::User(ref user) = result.messages[0] {
            match &user.content {
                MessageContent::Text(t) => assert_eq!(t, "text"),
                _ => panic!("expected Text"),
            }
        }
    }

    #[test]
    fn test_processed_input_default() {
        let pi = ProcessedInput::new();
        assert!(pi.messages.is_empty());
        assert!(!pi.should_query);
        assert!(pi.allowed_tools.is_none());
        assert!(pi.model.is_none());
        assert!(pi.result_text.is_none());
        assert!(pi.parsed_command.is_none());
        assert!(pi.attachments.is_empty());
        assert!(!pi.bash_mode);
        assert!(pi.skill_invocation.is_none());
    }
}
