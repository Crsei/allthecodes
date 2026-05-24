use anyhow::Result;

use crate::types::message::{AssistantMessage, ContentBlock, Message};
use allthecodes_types::hooks::{HookEventConfig, HookRunner, PostToolHookResult};

#[derive(Debug, Clone)]
pub enum StopHookResult {
    AllowStop,
    PreventStop { continuation_message: String },
    BlockingError { error: String },
}

pub async fn run_stop_hooks(
    runner: &dyn HookRunner,
    _assistant_message: &AssistantMessage,
    _messages: &[Message],
    stop_hook_active: Option<bool>,
    hook_configs: &[HookEventConfig],
) -> Result<StopHookResult> {
    if stop_hook_active == Some(true) {
        return Ok(StopHookResult::AllowStop);
    }

    match runner.run_stop_hooks(hook_configs).await {
        Ok(PostToolHookResult::Continue) => Ok(StopHookResult::AllowStop),
        Ok(PostToolHookResult::StopContinuation { message }) => Ok(StopHookResult::PreventStop {
            continuation_message: message,
        }),
        Err(error) => Ok(StopHookResult::BlockingError {
            error: error.to_string(),
        }),
    }
}

pub fn has_tool_use(assistant_message: &AssistantMessage) -> bool {
    assistant_message
        .content
        .iter()
        .any(|block| matches!(block, ContentBlock::ToolUse { .. }))
}

pub fn extract_tool_uses(
    assistant_message: &AssistantMessage,
) -> Vec<(String, String, serde_json::Value)> {
    assistant_message
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::ToolUse { id, name, input } => {
                Some((id.clone(), name.clone(), input.clone()))
            }
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::message::ContentBlock;
    use uuid::Uuid;

    fn make_assistant_message(content: Vec<ContentBlock>) -> AssistantMessage {
        AssistantMessage {
            uuid: Uuid::new_v4(),
            timestamp: 0,
            role: "assistant".to_string(),
            content,
            usage: None,
            stop_reason: Some("end_turn".to_string()),
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        }
    }

    #[test]
    fn test_has_tool_use_empty() {
        let msg = make_assistant_message(vec![ContentBlock::Text {
            text: "Hello".to_string(),
        }]);
        assert!(!has_tool_use(&msg));
    }

    #[test]
    fn test_has_tool_use_with_tool() {
        let msg = make_assistant_message(vec![
            ContentBlock::Text {
                text: "Let me check".to_string(),
            },
            ContentBlock::ToolUse {
                id: "tu_1".to_string(),
                name: "Bash".to_string(),
                input: serde_json::json!({"command": "ls"}),
            },
        ]);
        assert!(has_tool_use(&msg));
    }

    #[test]
    fn test_extract_tool_uses() {
        let msg = make_assistant_message(vec![
            ContentBlock::Text {
                text: "Running".to_string(),
            },
            ContentBlock::ToolUse {
                id: "tu_1".to_string(),
                name: "Bash".to_string(),
                input: serde_json::json!({"command": "ls"}),
            },
            ContentBlock::ToolUse {
                id: "tu_2".to_string(),
                name: "Read".to_string(),
                input: serde_json::json!({"file_path": "/tmp/x"}),
            },
        ]);
        let uses = extract_tool_uses(&msg);
        assert_eq!(uses.len(), 2);
        assert_eq!(uses[0].1, "Bash");
        assert_eq!(uses[1].1, "Read");
    }

    #[tokio::test]
    async fn test_stop_hooks_allow_by_default() {
        let msg = make_assistant_message(vec![]);
        let runner = allthecodes_types::hooks::NoopHookRunner;
        let result = run_stop_hooks(&runner, &msg, &[], None, &[]).await.unwrap();
        assert!(matches!(result, StopHookResult::AllowStop));
    }
}
