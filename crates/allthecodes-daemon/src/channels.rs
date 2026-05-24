//! Channel manager — routes external messages (MCP + webhook) to QueryEngine.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use tokio::sync::mpsc;
use tracing::{debug, warn};

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
        let _ = self.event_tx.send(event);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_channel_manager_allowlist() {
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
}
