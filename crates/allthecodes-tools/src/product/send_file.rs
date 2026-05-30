//! User file attachment product tool.

use std::fs;
use std::path::PathBuf;

use allthecodes_types::message::{
    AssistantMessage, Attachment, AttachmentMessage, Message, ToolResultContent,
};
use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use chrono::Utc;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::tool::{
    PermissionResult, Tool, ToolProgress, ToolResult, ToolUseContext, ValidationResult,
};

pub(super) struct SendUserFileTool;

#[async_trait]
impl Tool for SendUserFileTool {
    fn name(&self) -> &str {
        "SendUserFile"
    }

    async fn description(&self, _input: &Value) -> String {
        "Send a local file's contents to the user as a structured attachment.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "max_bytes": { "type": "integer", "minimum": 1, "maximum": 5242880, "default": 262144 }
            },
            "required": ["path"]
        })
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        match parse_send_file_input(input).and_then(|spec| validate_send_file_path(&spec)) {
            Ok(()) => ValidationResult::Ok,
            Err(err) => ValidationResult::Error {
                message: err.to_string(),
                error_code: 400,
            },
        }
    }

    async fn check_permissions(&self, input: &Value, _ctx: &ToolUseContext) -> PermissionResult {
        PermissionResult::Ask {
            message: format!(
                "Send file '{}' to the user?",
                input.get("path").and_then(Value::as_str).unwrap_or("")
            ),
        }
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let spec = parse_send_file_input(&input)?;
        validate_send_file_path(&spec)?;
        let bytes = fs::read(&spec.path)
            .with_context(|| format!("failed to read {}", spec.path.display()))?;
        if bytes.len() > spec.max_bytes {
            bail!(
                "{} is {} bytes, larger than max_bytes {}",
                spec.path.display(),
                bytes.len(),
                spec.max_bytes
            );
        }
        let byte_len = bytes.len();
        let text = String::from_utf8(bytes.clone()).ok();
        let is_binary = text.is_none();
        let content = text
            .clone()
            .unwrap_or_else(|| format!("[binary file omitted: {} bytes]", byte_len));
        let attachment = Message::Attachment(AttachmentMessage {
            uuid: Uuid::new_v4(),
            timestamp: Utc::now().timestamp_millis(),
            attachment: Attachment::StructuredOutput {
                data: json!({
                    "kind": "send_user_file",
                    "path": spec.path.display().to_string(),
                    "bytes": byte_len,
                    "content": text,
                    "binary": is_binary,
                }),
            },
        });
        Ok(ToolResult {
            data: json!({
                "path": spec.path.display().to_string(),
                "bytes": byte_len,
                "sent": true,
            }),
            model_content: Some(ToolResultContent::Text(content)),
            display_preview: Some(format!("Sent {}", spec.path.display())),
            new_messages: vec![attachment],
        })
    }

    async fn prompt(&self) -> String {
        "Send a local file to the user when they explicitly ask for file contents or an attachment."
            .to_string()
    }
}

#[derive(Debug)]
pub(super) struct SendFileInput {
    path: PathBuf,
    max_bytes: usize,
}

fn parse_send_file_input(input: &Value) -> Result<SendFileInput> {
    let path = input
        .get("path")
        .or_else(|| input.get("file_path"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("path is required"))?;
    let max_bytes = input
        .get("max_bytes")
        .and_then(Value::as_u64)
        .unwrap_or(262_144);
    if !(1..=5_242_880).contains(&max_bytes) {
        bail!("max_bytes must be between 1 and 5242880");
    }
    Ok(SendFileInput {
        path: PathBuf::from(path),
        max_bytes: max_bytes as usize,
    })
}

fn validate_send_file_path(spec: &SendFileInput) -> Result<()> {
    let metadata = fs::metadata(&spec.path)
        .with_context(|| format!("failed to inspect {}", spec.path.display()))?;
    if !metadata.is_file() {
        bail!("{} is not a file", spec.path.display());
    }
    if metadata.len() > spec.max_bytes as u64 {
        bail!(
            "{} is {} bytes, larger than max_bytes {}",
            spec.path.display(),
            metadata.len(),
            spec.max_bytes
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_file_input_accepts_file_path_alias_and_size_limit() {
        let spec = parse_send_file_input(&json!({
            "file_path": "notes.txt",
            "max_bytes": 10
        }))
        .unwrap();

        assert_eq!(spec.path, PathBuf::from("notes.txt"));
        assert_eq!(spec.max_bytes, 10);
        assert!(parse_send_file_input(&json!({"path": "x", "max_bytes": 0})).is_err());
    }

    #[test]
    fn validate_send_file_rejects_directories_and_large_files() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("payload.txt");
        fs::write(&file, "hello").unwrap();

        assert!(validate_send_file_path(&SendFileInput {
            path: dir.path().to_path_buf(),
            max_bytes: 100,
        })
        .is_err());
        assert!(validate_send_file_path(&SendFileInput {
            path: file.clone(),
            max_bytes: 4,
        })
        .is_err());
        validate_send_file_path(&SendFileInput {
            path: file,
            max_bytes: 5,
        })
        .unwrap();
    }
}
