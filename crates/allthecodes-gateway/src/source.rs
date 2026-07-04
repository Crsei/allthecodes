use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

const REDACTED: &str = "<redacted>";

/// Transport that delivered a remote-control request.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteTransport {
    Http,
    Webhook,
    Telegram,
    Lark,
    Scheduled,
    Local,
    Other(String),
}

impl RemoteTransport {
    pub fn as_key_part(&self) -> String {
        match self {
            Self::Http => "http".to_string(),
            Self::Webhook => "webhook".to_string(),
            Self::Telegram => "telegram".to_string(),
            Self::Lark => "lark".to_string(),
            Self::Scheduled => "scheduled".to_string(),
            Self::Local => "local".to_string(),
            Self::Other(value) => normalize_key_part(value),
        }
    }
}

/// Non-secret source metadata. Values are still redacted before persistence or
/// display because external providers commonly put credentials in headers.
pub type RemoteSourceMetadata = BTreeMap<String, String>;

/// Stable identity for the external actor or route that submitted a request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteSource {
    pub transport: RemoteTransport,
    pub tenant: String,
    pub workspace: String,
    pub client_id: String,
    pub user_id: String,
    pub thread_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    #[serde(default)]
    pub metadata: RemoteSourceMetadata,
}

impl RemoteSource {
    pub fn new(
        transport: RemoteTransport,
        tenant: impl Into<String>,
        workspace: impl Into<String>,
        client_id: impl Into<String>,
        user_id: impl Into<String>,
        thread_id: impl Into<String>,
    ) -> Self {
        Self {
            transport,
            tenant: tenant.into(),
            workspace: workspace.into(),
            client_id: client_id.into(),
            user_id: user_id.into(),
            thread_id: thread_id.into(),
            message_id: None,
            metadata: BTreeMap::new(),
        }
    }

    pub fn with_message_id(mut self, message_id: impl Into<String>) -> Self {
        self.message_id = Some(message_id.into());
        self
    }

    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Return a redacted clone suitable for logs, run metadata, and command
    /// output. Identity fields remain useful, while secrets in metadata are
    /// replaced.
    pub fn redacted(&self) -> Self {
        let metadata = self
            .metadata
            .iter()
            .map(|(key, value)| {
                if is_sensitive_key(key) || is_sensitive_value(value) {
                    (key.clone(), REDACTED.to_string())
                } else {
                    (key.clone(), value.clone())
                }
            })
            .collect();

        Self {
            transport: self.transport.clone(),
            tenant: self.tenant.clone(),
            workspace: self.workspace.clone(),
            client_id: self.client_id.clone(),
            user_id: self.user_id.clone(),
            thread_id: self.thread_id.clone(),
            message_id: self.message_id.clone(),
            metadata,
        }
    }

    /// Serialize to JSON after applying redaction.
    pub fn redacted_json(&self) -> Value {
        serde_json::to_value(self.redacted()).unwrap_or_else(|_| Value::Object(Map::new()))
    }

    pub(crate) fn key_material(&self) -> Vec<String> {
        [
            self.transport.as_key_part(),
            normalize_key_part(&self.tenant),
            normalize_key_part(&self.workspace),
            normalize_key_part(&self.client_id),
            normalize_key_part(&self.user_id),
            normalize_key_part(&self.thread_id),
        ]
        .into_iter()
        .collect()
    }
}

pub(crate) fn normalize_key_part(value: &str) -> String {
    let normalized: String = value
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();

    let trimmed = normalized.trim_matches('_');
    if trimmed.is_empty() {
        "unknown".to_string()
    } else {
        trimmed.to_string()
    }
}

pub(crate) fn is_sensitive_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    lower.contains("authorization")
        || lower.contains("bearer")
        || lower.contains("token")
        || lower.contains("secret")
        || lower.contains("password")
        || lower.contains("credential")
        || lower.contains("signature")
        || lower.contains("hmac")
        || lower.contains("api_key")
        || lower.contains("apikey")
}

pub(crate) fn is_sensitive_value(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.starts_with("bearer ")
        || lower.contains("authorization:")
        || lower.contains("x-api-key")
        || lower.contains("token=")
        || lower.contains("secret=")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_redaction_removes_sensitive_metadata() {
        let source = RemoteSource::new(
            RemoteTransport::Http,
            "local",
            "F:/workspace",
            "dashboard",
            "user-1",
            "thread-1",
        )
        .with_metadata("Authorization", "Bearer raw-token")
        .with_metadata("x-route", "route-1")
        .with_metadata("webhook_secret", "raw-secret");

        let redacted = source.redacted();
        assert_eq!(redacted.metadata["Authorization"], REDACTED);
        assert_eq!(redacted.metadata["webhook_secret"], REDACTED);
        assert_eq!(redacted.metadata["x-route"], "route-1");
    }

    #[test]
    fn source_redacted_json_does_not_include_raw_token_values() {
        let source = RemoteSource::new(
            RemoteTransport::Webhook,
            "local",
            "repo",
            "github",
            "octocat",
            "pr-1",
        )
        .with_metadata("header", "Authorization: Bearer raw-token");

        let json = source.redacted_json().to_string();
        assert!(json.contains(REDACTED));
        assert!(!json.contains("raw-token"));
        assert!(!json.to_ascii_lowercase().contains("bearer raw-token"));
    }

    #[test]
    fn source_key_material_is_normalized() {
        let source = RemoteSource::new(
            RemoteTransport::Other("Custom Provider".to_string()),
            " Tenant A ",
            "F:/AI Class",
            "Client/1",
            "User@Example",
            "",
        );

        assert_eq!(
            source.key_material(),
            vec![
                "custom_provider",
                "tenant_a",
                "f__ai_class",
                "client_1",
                "user_example",
                "unknown",
            ]
        );
    }
}
