//! Channel manager — routes external messages (MCP + webhook) to QueryEngine.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use tokio::sync::mpsc;
use tracing::{debug, warn};

use crate::protocol::{DaemonCommand, DaemonCommandKind};
use crate::supervisor::ASSISTANT_WORKER_ID;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ChannelOrigin {
    Mcp { server_name: String },
    Webhook { endpoint: String },
}

#[derive(Clone, Debug, Serialize)]
pub struct ChannelEvent {
    pub source: String,
    pub sender: Option<String>,
    pub content: String,
    pub meta: Value,
    pub origin: ChannelOrigin,
}

impl ChannelEvent {
    pub fn to_xml(&self) -> String {
        let sender_attr = self
            .sender
            .as_deref()
            .map(|s| format!(" sender=\"{}\"", s))
            .unwrap_or_default();
        format!(
            "<channel source=\"{}\"{}>\n{}\n</channel>",
            self.source, sender_attr, self.content
        )
    }
}

pub struct ChannelManager {
    allowlist: HashSet<String>,
    event_tx: mpsc::UnboundedSender<ChannelEvent>,
}

impl ChannelManager {
    pub fn new(allowlist: Vec<String>, event_tx: mpsc::UnboundedSender<ChannelEvent>) -> Self {
        Self {
            allowlist: allowlist.into_iter().collect(),
            event_tx,
        }
    }

    pub fn submit(&self, event: ChannelEvent) -> bool {
        let key = match &event.origin {
            ChannelOrigin::Mcp { server_name } => format!("mcp:{}", server_name),
            ChannelOrigin::Webhook { endpoint } => format!("webhook:{}", endpoint),
        };
        if !self.allowlist.contains(&key) {
            warn!("channel event from '{}' blocked by allowlist", key);
            return false;
        }
        debug!("channel event accepted from '{}'", key);
        if let Err(error) = enqueue_channel_event(&event) {
            warn!(error = %error, "failed to enqueue accepted channel event");
            return false;
        }
        let _ = self.event_tx.send(event);
        true
    }
}

pub fn enqueue_channel_event(event: &ChannelEvent) -> Result<DaemonCommand> {
    crate::protocol_store()
        .enqueue_command(
            ASSISTANT_WORKER_ID,
            DaemonCommandKind::Submit,
            channel_submit_payload(event),
            None,
        )
        .context("failed to enqueue channel event for assistant worker")
}

pub(crate) fn channel_submit_payload(event: &ChannelEvent) -> Value {
    json!({
        "text": event.to_xml(),
        "source": "channel",
        "channel": {
            "source": event.source.clone(),
            "sender": event.sender.clone(),
            "meta": event.meta.clone(),
            "origin": channel_origin_payload(&event.origin),
        },
    })
}

pub(crate) fn channel_origin_payload(origin: &ChannelOrigin) -> Value {
    match origin {
        ChannelOrigin::Mcp { server_name } => json!({
            "type": "mcp",
            "server_name": server_name,
        }),
        ChannelOrigin::Webhook { endpoint } => json!({
            "type": "webhook",
            "endpoint": endpoint,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::DaemonCommandKind;
    use crate::supervisor::ASSISTANT_WORKER_ID;

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    #[test]
    fn test_channel_event_to_xml() {
        let event = ChannelEvent {
            source: "slack".into(),
            sender: Some("alice".into()),
            content: "hello world".into(),
            meta: Value::Null,
            origin: ChannelOrigin::Mcp {
                server_name: "slack-mcp".into(),
            },
        };
        let xml = event.to_xml();
        assert_eq!(
            xml,
            "<channel source=\"slack\" sender=\"alice\">\nhello world\n</channel>"
        );

        // Without sender
        let event_no_sender = ChannelEvent {
            source: "github".into(),
            sender: None,
            content: "PR merged".into(),
            meta: Value::Null,
            origin: ChannelOrigin::Webhook {
                endpoint: "/hooks/github".into(),
            },
        };
        let xml2 = event_no_sender.to_xml();
        assert_eq!(xml2, "<channel source=\"github\">\nPR merged\n</channel>");
    }

    #[test]
    #[serial_test::serial]
    fn test_channel_manager_allowlist() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path().to_str().unwrap());
        let (tx, mut rx) = mpsc::unbounded_channel();

        let manager = ChannelManager::new(
            vec!["mcp:slack-mcp".into(), "webhook:/hooks/github".into()],
            tx,
        );

        // Allowed MCP source should pass
        let allowed_event = ChannelEvent {
            source: "slack".into(),
            sender: Some("bob".into()),
            content: "allowed message".into(),
            meta: Value::Null,
            origin: ChannelOrigin::Mcp {
                server_name: "slack-mcp".into(),
            },
        };
        assert!(manager.submit(allowed_event));
        let received = rx.try_recv().expect("should receive allowed event");
        assert_eq!(received.content, "allowed message");

        // Blocked source should fail
        let blocked_event = ChannelEvent {
            source: "unknown".into(),
            sender: None,
            content: "blocked message".into(),
            meta: Value::Null,
            origin: ChannelOrigin::Mcp {
                server_name: "unknown-server".into(),
            },
        };
        assert!(!manager.submit(blocked_event));
        assert!(rx.try_recv().is_err(), "blocked event should not be sent");

        // Allowed webhook source should pass
        let webhook_event = ChannelEvent {
            source: "github".into(),
            sender: None,
            content: "webhook message".into(),
            meta: Value::Null,
            origin: ChannelOrigin::Webhook {
                endpoint: "/hooks/github".into(),
            },
        };
        assert!(manager.submit(webhook_event));
        let received2 = rx.try_recv().expect("should receive webhook event");
        assert_eq!(received2.content, "webhook message");
    }

    #[test]
    #[serial_test::serial]
    fn accepted_channel_event_queues_assistant_submit_command() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path().to_str().unwrap());
        let event = ChannelEvent {
            source: "slack".into(),
            sender: Some("alice".into()),
            content: "please triage this incident".into(),
            meta: serde_json::json!({ "thread": "C123/456" }),
            origin: ChannelOrigin::Mcp {
                server_name: "slack-mcp".into(),
            },
        };

        let command = enqueue_channel_event(&event).unwrap();
        let commands = crate::protocol_store()
            .read_worker_commands(ASSISTANT_WORKER_ID)
            .unwrap();

        assert_eq!(command.kind, DaemonCommandKind::Submit);
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].kind, DaemonCommandKind::Submit);
        assert!(commands[0].payload["text"]
            .as_str()
            .unwrap()
            .contains("<channel source=\"slack\" sender=\"alice\">"));
        assert_eq!(commands[0].payload["source"], "channel");
        assert_eq!(commands[0].payload["channel"]["source"], "slack");
        assert_eq!(commands[0].payload["channel"]["origin"]["type"], "mcp");
    }
}
