//! /copy command -- copies the last assistant response to clipboard.

use std::sync::{OnceLock, RwLock};

use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandHandler, CommandResult};
use allthecodes_types::message::{ContentBlock, Message};

pub struct CopyHandler;

pub type ClipboardCopyProvider = fn(&str) -> std::result::Result<(), String>;

static CLIPBOARD_COPY_PROVIDER: OnceLock<RwLock<Option<ClipboardCopyProvider>>> = OnceLock::new();

pub fn set_clipboard_copy_provider(provider: ClipboardCopyProvider) {
    let slot = CLIPBOARD_COPY_PROVIDER.get_or_init(|| RwLock::new(None));
    if let Ok(mut guard) = slot.write() {
        *guard = Some(provider);
    }
}

fn copy_text_to_clipboard(text: &str) -> std::result::Result<(), String> {
    let Some(provider) = CLIPBOARD_COPY_PROVIDER
        .get()
        .and_then(|slot| slot.read().ok().and_then(|guard| *guard))
    else {
        return Err("clipboard runtime adapter is not installed".to_string());
    };

    provider(text)
}

/// Extract plain text from the last assistant message in the conversation.
fn last_assistant_text(messages: &[Message]) -> Option<String> {
    for msg in messages.iter().rev() {
        if let Message::Assistant(a) = msg {
            let mut parts = Vec::new();
            for block in &a.content {
                if let ContentBlock::Text { text } = block {
                    parts.push(text.as_str());
                }
            }
            if !parts.is_empty() {
                return Some(parts.join("\n"));
            }
        }
    }
    None
}

#[async_trait]
impl CommandHandler for CopyHandler {
    async fn execute(&self, _args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        match last_assistant_text(&ctx.messages) {
            Some(text) => {
                let copy_result = copy_text_to_clipboard(&text);
                let prefix = match copy_result {
                    Ok(()) => format!("Copied to clipboard ({} chars):", text.len()),
                    Err(error) => format!(
                        "Failed to copy to clipboard: {}\nClipboard text ({} chars):",
                        error,
                        text.len()
                    ),
                };
                Ok(CommandResult::Output(format!("{prefix}\n\n{text}")))
            }
            None => Ok(CommandResult::Output(
                "No assistant messages found.".to_string(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
    use allthecodes_types::message::AssistantMessage;
    use std::path::PathBuf;
    use uuid::Uuid;

    fn test_clipboard_provider(_text: &str) -> std::result::Result<(), String> {
        Ok(())
    }

    fn test_ctx() -> CommandContext {
        CommandContext {
            messages: Vec::new(),
            cwd: PathBuf::from("/test"),
            app_state: Default::default(),
            session_id: SessionId::from_string("test-session"),
        }
    }

    #[tokio::test]
    async fn test_no_messages() {
        let handler = CopyHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => assert!(text.contains("No assistant messages")),
            _ => panic!("Expected Output"),
        }
    }

    #[tokio::test]
    async fn test_copies_last_assistant() {
        set_clipboard_copy_provider(test_clipboard_provider);

        let handler = CopyHandler;
        let mut ctx = test_ctx();
        ctx.messages.push(Message::Assistant(AssistantMessage {
            uuid: Uuid::new_v4(),
            timestamp: 1,
            role: "assistant".into(),
            content: vec![ContentBlock::Text {
                text: "Hello, world!".into(),
            }],
            usage: None,
            stop_reason: None,
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        }));

        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Copied to clipboard"));
                assert!(text.contains("Hello, world!"));
            }
            _ => panic!("Expected Output"),
        }
    }
}
