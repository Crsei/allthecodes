use std::path::PathBuf;

use serde_json::Value;

use crate::tool::{ToolResult, ToolUseContext, ValidationResult};

pub(crate) fn current_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| allthecodes_config::paths::data_root())
}

pub(crate) fn task_list_id_for_context(ctx: &ToolUseContext) -> String {
    let app_state = (ctx.get_app_state)();
    allthecodes_tasks::task_list_id_from_parts(allthecodes_tasks::TaskListScope {
        explicit_task_list_id: None,
        scoped_team_name: None,
        app_team_name: app_state
            .team_context
            .as_ref()
            .map(|team| team.team_name.clone()),
        session_id: Some(ctx.session_id.clone()),
    })
}

pub(crate) fn string_param<'a>(input: &'a Value, key: &str) -> Option<&'a str> {
    input
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

pub(crate) fn validate_enum(
    input: &Value,
    key: &str,
    allowed: &[&str],
) -> Option<ValidationResult> {
    let Some(value) = string_param(input, key) else {
        return None;
    };
    if allowed.contains(&value) {
        None
    } else {
        Some(ValidationResult::Error {
            message: format!("{key} must be one of: {}", allowed.join(", ")),
            error_code: 400,
        })
    }
}

pub(crate) fn truncate_utf8_bytes(text: &str, max_bytes: usize) -> (String, bool) {
    if text.len() <= max_bytes {
        return (text.to_string(), false);
    }
    let mut end = 0;
    for (idx, ch) in text.char_indices() {
        let next = idx + ch.len_utf8();
        if next > max_bytes {
            break;
        }
        end = next;
    }
    (text[..end].to_string(), true)
}

pub(crate) fn preview_tool_result(data: Value, preview: impl Into<String>) -> ToolResult {
    let preview = preview.into();
    ToolResult {
        data,
        display_preview: Some(preview),
        ..Default::default()
    }
}

macro_rules! tool_alias {
    ($alias:ident, $name:literal, $target:ident) => {
        #[async_trait::async_trait]
        impl crate::tool::Tool for $alias {
            fn name(&self) -> &str {
                $name
            }

            async fn description(&self, input: &serde_json::Value) -> String {
                $target.description(input).await
            }

            fn input_json_schema(&self) -> serde_json::Value {
                $target.input_json_schema()
            }

            fn is_enabled(&self) -> bool {
                $target.is_enabled()
            }

            fn is_concurrency_safe(&self, input: &serde_json::Value) -> bool {
                $target.is_concurrency_safe(input)
            }

            fn is_read_only(&self, input: &serde_json::Value) -> bool {
                $target.is_read_only(input)
            }

            fn is_destructive(&self, input: &serde_json::Value) -> bool {
                $target.is_destructive(input)
            }

            async fn validate_input(
                &self,
                input: &serde_json::Value,
                ctx: &crate::tool::ToolUseContext,
            ) -> crate::tool::ValidationResult {
                $target.validate_input(input, ctx).await
            }

            async fn check_permissions(
                &self,
                input: &serde_json::Value,
                ctx: &crate::tool::ToolUseContext,
            ) -> crate::tool::PermissionResult {
                $target.check_permissions(input, ctx).await
            }

            fn backfill_observable_input(
                &self,
                input: &mut serde_json::Map<String, serde_json::Value>,
            ) {
                $target.backfill_observable_input(input)
            }

            async fn call(
                &self,
                input: serde_json::Value,
                ctx: &crate::tool::ToolUseContext,
                parent: &allthecodes_types::message::AssistantMessage,
                on_progress: Option<Box<dyn Fn(crate::tool::ToolProgress) + Send + Sync>>,
            ) -> anyhow::Result<crate::tool::ToolResult> {
                $target.call(input, ctx, parent, on_progress).await
            }

            async fn prompt(&self) -> String {
                $target.prompt().await
            }

            fn user_facing_name(&self, input: Option<&serde_json::Value>) -> String {
                $target.user_facing_name(input)
            }

            fn max_result_size_chars(&self) -> usize {
                $target.max_result_size_chars()
            }

            fn get_path(&self, input: &serde_json::Value) -> Option<String> {
                $target.get_path(input)
            }

            fn interrupt_behavior(&self) -> crate::tool::InterruptBehavior {
                $target.interrupt_behavior()
            }

            fn to_auto_classifier_input(&self, input: &serde_json::Value) -> serde_json::Value {
                $target.to_auto_classifier_input(input)
            }
        }
    };
}

pub(crate) use tool_alias;
