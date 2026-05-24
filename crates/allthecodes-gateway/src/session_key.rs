use crate::source::RemoteSource;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;

/// Controls how much source identity is exposed in the stable session key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SessionKeyPolicy {
    pub prefix: String,
    pub include_workspace: bool,
    pub hash_len: usize,
}

impl Default for SessionKeyPolicy {
    fn default() -> Self {
        Self {
            prefix: "remote".to_string(),
            include_workspace: true,
            hash_len: 16,
        }
    }
}

/// Stable, non-secret key used to map repeat source traffic to an allthecodes
/// assistant session.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SessionKey(String);

impl SessionKey {
    pub fn derive(source: &RemoteSource, policy: &SessionKeyPolicy) -> Self {
        let mut parts = source.key_material();
        if !policy.include_workspace {
            parts.remove(2);
        }

        let digest_material = parts.join("\n");
        let digest = Sha256::digest(digest_material.as_bytes());
        let digest = hex::encode(digest);
        let hash_len = policy.hash_len.min(digest.len()).max(8);
        let transport = source.transport.as_key_part();
        Self(format!(
            "{}:{}:{}",
            clean_prefix(&policy.prefix),
            transport,
            &digest[..hash_len]
        ))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Display for SessionKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn clean_prefix(value: &str) -> String {
    let prefix = value
        .trim()
        .chars()
        .filter_map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                Some(ch.to_ascii_lowercase())
            } else {
                None
            }
        })
        .collect::<String>();

    if prefix.is_empty() {
        "remote".to_string()
    } else {
        prefix
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::{RemoteSource, RemoteTransport};

    fn source(workspace: &str) -> RemoteSource {
        RemoteSource::new(
            RemoteTransport::Http,
            "local",
            workspace,
            "dashboard",
            "user",
            "thread",
        )
    }

    #[test]
    fn session_key_is_deterministic_for_same_source() {
        let source = source("F:/repo");
        let policy = SessionKeyPolicy::default();

        assert_eq!(
            SessionKey::derive(&source, &policy),
            SessionKey::derive(&source, &policy)
        );
    }

    #[test]
    fn session_key_separates_workspace_by_default() {
        let policy = SessionKeyPolicy::default();
        let a = SessionKey::derive(&source("F:/repo-a"), &policy);
        let b = SessionKey::derive(&source("F:/repo-b"), &policy);

        assert_ne!(a, b);
        assert!(a.as_str().starts_with("remote:http:"));
    }

    #[test]
    fn session_key_can_ignore_workspace_for_policy_compatibility() {
        let policy = SessionKeyPolicy {
            include_workspace: false,
            ..SessionKeyPolicy::default()
        };

        assert_eq!(
            SessionKey::derive(&source("F:/repo-a"), &policy),
            SessionKey::derive(&source("F:/repo-b"), &policy)
        );
    }

    #[test]
    fn session_key_prefix_is_sanitized_and_hash_has_floor() {
        let policy = SessionKeyPolicy {
            prefix: " Remote Control! ".to_string(),
            hash_len: 2,
            ..SessionKeyPolicy::default()
        };

        let key = SessionKey::derive(&source("F:/repo"), &policy);
        assert!(key.as_str().starts_with("remotecontrol:http:"));
        assert_eq!(key.as_str().rsplit(':').next().unwrap().len(), 8);
    }
}
