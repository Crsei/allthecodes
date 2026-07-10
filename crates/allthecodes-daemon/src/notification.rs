//! Push notification system — Windows Toast + webhook callback.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{debug, error, info};

// ---------------------------------------------------------------------------
// Notification level
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NotificationLevel {
    Info,
    Warning,
    Error,
    Success,
}

// ---------------------------------------------------------------------------
// Notification source
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum NotificationSource {
    TaskComplete { task_id: String },
    BackgroundAgentDone { agent_id: String },
    ChannelMessage { source: String },
    ProactiveAction { summary: String },
    Error { detail: String },
}

// ---------------------------------------------------------------------------
// Configuration types
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Deserialize, Default)]
pub struct NotificationConfig {
    pub windows_toast: Option<ToastConfig>,
    pub webhook: Option<WebhookConfig>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ToastConfig {
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub only_when_detached: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct WebhookConfig {
    pub enabled: bool,
    pub url: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub events: Vec<String>,
}

fn default_true() -> bool {
    true
}

// ---------------------------------------------------------------------------
// Full notification payload
// ---------------------------------------------------------------------------

/// A fully resolved notification ready for dispatch.
#[derive(Clone, Debug, Serialize)]
pub struct FullNotification {
    pub title: String,
    pub body: String,
    pub level: NotificationLevel,
    pub source: NotificationSource,
}

pub fn full_notification_from_daemon(notif: super::state::Notification) -> FullNotification {
    FullNotification {
        title: notif.title,
        body: notif.body,
        level: notification_level_from_str(&notif.level),
        source: notification_source_from_value(notif.source),
    }
}

fn notification_level_from_str(raw: &str) -> NotificationLevel {
    match raw.trim().to_ascii_lowercase().as_str() {
        "warning" | "warn" => NotificationLevel::Warning,
        "error" => NotificationLevel::Error,
        "success" | "ok" => NotificationLevel::Success,
        _ => NotificationLevel::Info,
    }
}

fn notification_source_from_value(value: serde_json::Value) -> NotificationSource {
    serde_json::from_value(value.clone()).unwrap_or_else(|_| {
        let source_type = value
            .get("type")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        match source_type {
            "task_complete" => NotificationSource::TaskComplete {
                task_id: value
                    .get("task_id")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            },
            "background_agent_done" => NotificationSource::BackgroundAgentDone {
                agent_id: value
                    .get("agent_id")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            },
            "channel_message" => NotificationSource::ChannelMessage {
                source: value
                    .get("source")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            },
            "error" => NotificationSource::Error {
                detail: value
                    .get("detail")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            },
            _ => NotificationSource::ProactiveAction {
                summary: "notification".into(),
            },
        }
    })
}

// ---------------------------------------------------------------------------
// Windows Toast
// ---------------------------------------------------------------------------

/// Show a native Windows toast notification through the built-in WinRT API.
///
/// On non-Windows platforms the call is a no-op (logged at debug level).
pub fn send_windows_toast(notif: &FullNotification) {
    #[cfg(target_os = "windows")]
    {
        const TOAST_SCRIPT: &str = r#"
[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] > $null
[Windows.UI.Notifications.ToastTemplateType, Windows.UI.Notifications, ContentType = WindowsRuntime] > $null
$xml = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent([Windows.UI.Notifications.ToastTemplateType]::ToastText02)
$nodes = $xml.GetElementsByTagName('text')
$nodes.Item(0).AppendChild($xml.CreateTextNode($env:ALLTHECODES_TOAST_TITLE)) > $null
$nodes.Item(1).AppendChild($xml.CreateTextNode($env:ALLTHECODES_TOAST_BODY)) > $null
$toast = [Windows.UI.Notifications.ToastNotification]::new($xml)
[Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('allthecodes').Show($toast)
"#;
        let result = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", TOAST_SCRIPT])
            .env("ALLTHECODES_TOAST_TITLE", &notif.title)
            .env("ALLTHECODES_TOAST_BODY", &notif.body)
            .status();
        match result {
            Ok(status) if status.success() => debug!("Windows toast sent: {}", notif.title),
            Ok(status) => error!("Windows toast command exited with status {}", status),
            Err(error) => error!("failed to show Windows toast: {}", error),
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        debug!("Windows toast skipped (not on Windows): {}", notif.title);
    }
}

// ---------------------------------------------------------------------------
// Webhook
// ---------------------------------------------------------------------------

/// POST a JSON payload to the configured webhook endpoint.
pub async fn send_webhook(notif: &FullNotification, config: &WebhookConfig) {
    let payload = json!({
        "title": notif.title,
        "body": notif.body,
        "level": notif.level,
        "source": notif.source,
        "timestamp": chrono::Utc::now().to_rfc3339(),
    });

    let client = reqwest::Client::new();
    let mut req = client.post(&config.url).json(&payload);
    for (k, v) in &config.headers {
        req = req.header(k, v);
    }

    match req.send().await {
        Ok(resp) => debug!(
            "webhook notification sent: {} ({})",
            notif.title,
            resp.status()
        ),
        Err(e) => error!("webhook notification failed: {}", e),
    }
}

// ---------------------------------------------------------------------------
// Consumer loop
// ---------------------------------------------------------------------------

/// Long-running task that drains the notification channel and dispatches
/// each notification to the configured sinks (Windows Toast, webhook).
///
/// * `has_clients` — callback returning `true` when at least one frontend SSE
///   client is connected. Used to honour `only_when_detached` on toast config.
pub async fn notification_consumer(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<super::state::Notification>,
    config: NotificationConfig,
    has_clients: impl Fn() -> bool,
) {
    info!("notification consumer started");
    while let Some(notif) = rx.recv().await {
        let full = full_notification_from_daemon(notif);

        // Windows Toast dispatch
        if let Some(tc) = &config.windows_toast {
            if tc.enabled && (!tc.only_when_detached || !has_clients()) {
                send_windows_toast(&full);
            }
        }

        // Webhook dispatch
        if let Some(wc) = &config.webhook {
            if wc.enabled {
                send_webhook(&full, wc).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_config_deserializes_webhook_contract() {
        let config: NotificationConfig = serde_json::from_value(json!({
            "windows_toast": {
                "enabled": true
            },
            "webhook": {
                "enabled": true,
                "url": "https://example.invalid/hook",
                "headers": { "x-test": "1" },
                "events": ["task_complete"]
            }
        }))
        .unwrap();

        let toast = config.windows_toast.expect("toast config");
        assert!(toast.enabled);
        assert!(toast.only_when_detached);

        let webhook = config.webhook.expect("webhook config");
        assert!(webhook.enabled);
        assert_eq!(webhook.url, "https://example.invalid/hook");
        assert_eq!(webhook.headers.get("x-test").map(String::as_str), Some("1"));
        assert_eq!(webhook.events, vec!["task_complete".to_string()]);
    }

    #[test]
    fn full_notification_serializes_source_and_level() {
        let notif = FullNotification {
            title: "Done".into(),
            body: "Task complete".into(),
            level: NotificationLevel::Success,
            source: NotificationSource::TaskComplete {
                task_id: "task-1".into(),
            },
        };

        let value = serde_json::to_value(notif).unwrap();
        assert_eq!(value["level"], "success");
        assert_eq!(value["source"]["type"], "TaskComplete");
        assert_eq!(value["source"]["task_id"], "task-1");
    }

    #[test]
    fn daemon_notification_preserves_task_complete_payload() {
        let notif = crate::state::Notification {
            title: "Task done".into(),
            body: "Background task completed".into(),
            level: "success".into(),
            source: json!({
                "type": "TaskComplete",
                "task_id": "task-42"
            }),
        };

        let full = full_notification_from_daemon(notif);
        let value = serde_json::to_value(full).unwrap();

        assert_eq!(value["title"], "Task done");
        assert_eq!(value["level"], "success");
        assert_eq!(value["source"]["type"], "TaskComplete");
        assert_eq!(value["source"]["task_id"], "task-42");
    }

    #[tokio::test]
    async fn notification_consumer_drains_task_complete_without_client() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        tx.send(crate::state::Notification {
            title: "Task done".into(),
            body: "Background task completed".into(),
            level: "success".into(),
            source: json!({
                "type": "TaskComplete",
                "task_id": "task-42"
            }),
        })
        .unwrap();
        drop(tx);

        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            notification_consumer(rx, NotificationConfig::default(), || false),
        )
        .await
        .expect("consumer should exit after channel closes");
    }
}
