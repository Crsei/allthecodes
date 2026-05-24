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

// ---------------------------------------------------------------------------
// Windows Toast
// ---------------------------------------------------------------------------

/// Show a native Windows toast notification via `notify-rust`.
///
/// On non-Windows platforms the call is a no-op (logged at debug level).
pub fn send_windows_toast(notif: &FullNotification) {
    #[cfg(target_os = "windows")]
    {
        if let Err(e) = notify_rust::Notification::new()
            .appname("allthecodes")
            .summary(&notif.title)
            .body(&notif.body)
            .show()
        {
            error!("failed to show Windows toast: {}", e);
        } else {
            debug!("Windows toast sent: {}", notif.title);
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
        let full = FullNotification {
            title: notif.title,
            body: notif.body,
            level: NotificationLevel::Info,
            source: NotificationSource::ProactiveAction {
                summary: "notification".into(),
            },
        };

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
}
