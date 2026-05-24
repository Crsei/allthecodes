//! /files command -- list files referenced in the current conversation.
//!
//! Scans messages for file paths mentioned in tool calls (FileRead, FileWrite,
//! FileEdit, Grep, Glob, etc.) and displays them relative to the working
//! directory.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandHandler, CommandResult};
use allthecodes_types::message::{ContentBlock, Message};

/// Handler for the `/files` slash command.
pub struct FilesHandler;

/// Extract file paths from tool use inputs and results across all messages.
fn extract_referenced_files(messages: &[Message]) -> BTreeSet<String> {
    let mut files = BTreeSet::new();

    for msg in messages {
        if let Message::Assistant(a) = msg {
            for block in &a.content {
                if let ContentBlock::ToolUse { name, input, .. } = block {
                    // Extract file paths from common tool inputs.
                    match name.as_str() {
                        "Read" | "FileRead" | "file_read" => {
                            if let Some(path) = input.get("file_path").and_then(|v| v.as_str()) {
                                files.insert(path.to_string());
                            }
                        }
                        "Write" | "FileWrite" | "file_write" => {
                            if let Some(path) = input.get("file_path").and_then(|v| v.as_str()) {
                                files.insert(path.to_string());
                            }
                        }
                        "Edit" | "FileEdit" | "file_edit" => {
                            if let Some(path) = input.get("file_path").and_then(|v| v.as_str()) {
                                files.insert(path.to_string());
                            }
                        }
                        "Grep" | "grep" => {
                            if let Some(path) = input.get("path").and_then(|v| v.as_str()) {
                                files.insert(path.to_string());
                            }
                        }
                        "Bash" | "bash" => {
                            // Best-effort: don't parse bash commands.
                        }
                        _ => {}
                    }
                }
            }
        }

        // Also check for attachment messages referencing edited files.
        if let Message::Attachment(att) = msg {
            if let allthecodes_types::message::Attachment::EditedTextFile { path } = &att.attachment
            {
                files.insert(path.clone());
            }
        }
    }

    files
}

/// Make a path relative to the working directory if possible.
fn make_relative(path: &str, cwd: &Path) -> String {
    let p = Path::new(path);
    match p.strip_prefix(cwd) {
        Ok(rel) => rel.display().to_string(),
        Err(_) => path.to_string(),
    }
}

#[async_trait]
impl CommandHandler for FilesHandler {
    async fn execute(&self, _args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let files = extract_referenced_files(&ctx.messages);

        if files.is_empty() {
            return Ok(CommandResult::Output("No files in context".into()));
        }

        let file_list: Vec<String> = files.iter().map(|f| make_relative(f, &ctx.cwd)).collect();

        Ok(CommandResult::Output(format!(
            "Files in context:\n{}",
            file_list.join("\n")
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
    use allthecodes_types::message::AssistantMessage;
    use std::path::PathBuf;
    use uuid::Uuid;

    fn make_assistant_with_tool_use(name: &str, file_path: &str) -> Message {
        Message::Assistant(AssistantMessage {
            uuid: Uuid::new_v4(),
            timestamp: 0,
            role: "assistant".into(),
            content: vec![ContentBlock::ToolUse {
                id: "test-id".into(),
                name: name.into(),
                input: serde_json::json!({ "file_path": file_path }),
            }],
            usage: None,
            stop_reason: None,
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        })
    }

    #[tokio::test]
    async fn test_files_empty() {
        let handler = FilesHandler;
        let mut ctx = CommandContext {
            messages: Vec::new(),
            cwd: PathBuf::from("/test"),
            app_state: Default::default(),
            session_id: SessionId::from_string("test-session"),
        };

        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("No files"));
            }
            _ => panic!("Expected Output result"),
        }
    }

    #[tokio::test]
    async fn test_files_with_tool_uses() {
        let handler = FilesHandler;
        let mut ctx = CommandContext {
            messages: vec![
                make_assistant_with_tool_use("Read", "/test/src/main.rs"),
                make_assistant_with_tool_use("Write", "/test/src/lib.rs"),
            ],
            cwd: PathBuf::from("/test"),
            app_state: Default::default(),
            session_id: SessionId::from_string("test-session"),
        };

        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Files in context"));
                assert!(text.contains("main.rs"));
                assert!(text.contains("lib.rs"));
            }
            _ => panic!("Expected Output result"),
        }
    }
}
