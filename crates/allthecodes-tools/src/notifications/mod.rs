use std::fs;
use std::io::Write as _;
use std::sync::Arc;

use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use chrono::Utc;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use url::Url;

use crate::common::{preview_tool_result, string_param, validate_enum};
use crate::network::host_is_blocked;
use crate::tool::{
    PermissionResult, Tool, ToolProgress, ToolResult, ToolUseContext, Tools, ValidationResult,
};
use allthecodes_types::message::AssistantMessage;

pub fn tools() -> Tools {
    vec![Arc::new(PushNotificationTool)]
}

pub struct PushNotificationTool;

fn notification_body(input: &Value) -> Option<&str> {
    string_param(input, "body").or_else(|| string_param(input, "message"))
}

fn notification_priority(input: &Value) -> &str {
    match string_param(input, "priority").unwrap_or("normal") {
        "high" | "urgent" => "high",
        _ => "normal",
    }
}

#[async_trait]
impl Tool for PushNotificationTool {
    fn name(&self) -> &str {
        "PushNotification"
    }

    async fn description(&self, _input: &Value) -> String {
        "Send an audited local notification record or HTTPS webhook push notification.".into()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "title": {"type": "string"},
                "body": {"type": "string", "description": "Notification body. Preferred compatibility field."},
                "message": {"type": "string", "description": "Legacy alias for body."},
                "priority": {"type": "string", "enum": ["normal", "high", "urgent"], "description": "Use normal or high; urgent is retained as a legacy alias for high."},
                "target": {"type": "string", "description": "local, file, or webhook:https://..."}
            },
            "required": ["title"]
        })
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        if string_param(input, "title").is_none() || notification_body(input).is_none() {
            return ValidationResult::Error {
                message: "title and body are required".into(),
                error_code: 400,
            };
        }
        if let Some(result) = validate_enum(input, "priority", &["normal", "high", "urgent"]) {
            return result;
        }
        ValidationResult::Ok
    }

    async fn check_permissions(&self, input: &Value, _ctx: &ToolUseContext) -> PermissionResult {
        PermissionResult::Ask {
            message: format!(
                "Allow PushNotification to target {}?",
                string_param(input, "target").unwrap_or("local")
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
        let title = string_param(&input, "title")
            .ok_or_else(|| anyhow!("Missing required parameter: title"))?;
        let body =
            notification_body(&input).ok_or_else(|| anyhow!("Missing required parameter: body"))?;
        let priority = notification_priority(&input);
        let target = string_param(&input, "target").unwrap_or("local");
        let is_webhook_target = target.starts_with("webhook:");
        let remote_bridge_enabled = allthecodes_config::features::enabled(
            allthecodes_config::features::Feature::PushNotificationRemoteBridge,
        );
        let target_hash = {
            let mut hasher = Sha256::new();
            hasher.update(target.as_bytes());
            hex::encode(hasher.finalize())
        };
        let record = json!({
            "timestamp": Utc::now().to_rfc3339(),
            "title": title,
            "body": body,
            "priority": priority,
            "target_hash": target_hash,
            "provider": if is_webhook_target { "webhook" } else { "local" },
            "provider_disabled": is_webhook_target && !remote_bridge_enabled,
        });
        fs::create_dir_all(allthecodes_config::paths::notifications_dir())?;
        let audit_path = allthecodes_config::paths::notifications_dir().join("notifications.jsonl");
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&audit_path)?;
        writeln!(file, "{}", serde_json::to_string(&record)?)?;

        let mut delivered = "local";
        if let Some(webhook_url) = target.strip_prefix("webhook:") {
            let url = Url::parse(webhook_url)?;
            if url.scheme() != "https" || host_is_blocked(&url) {
                bail!("webhook target must be public HTTPS");
            }
            if !remote_bridge_enabled {
                return Ok(preview_tool_result(
                    json!({
                        "sent": false,
                        "provider": "webhook",
                        "provider_disabled": true,
                        "audit_path": audit_path,
                        "target_hash": target_hash,
                    }),
                    "PushNotification webhook provider disabled; audit record written",
                ));
            }
            reqwest::Client::new()
                .post(webhook_url)
                .json(&json!({"title": title, "body": body, "priority": priority}))
                .send()
                .await?
                .error_for_status()?;
            delivered = "webhook";
        }
        Ok(preview_tool_result(
            json!({
                "sent": true,
                "provider": delivered,
                "provider_disabled": false,
                "audit_path": audit_path,
                "target_hash": target_hash,
            }),
            format!("PushNotification delivered via {delivered}; audit record written"),
        ))
    }

    async fn prompt(&self) -> String {
        "Send a push notification only after permission. Local provider writes an audit record; webhook targets must be public HTTPS.".into()
    }
}
