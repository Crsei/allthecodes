use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Origin categories whose content must never be treated as user authority.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum UntrustedSourceKind {
    IssueBody,
    PullRequestBody,
    ReviewComment,
    WebhookPayload,
    WebContent,
    McpResult,
    LocalMemory,
    RepositoryDocument,
    ToolOutput,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TrustLevel {
    TrustedUser,
    TrustedPolicy,
    Untrusted,
}

/// Redacted provenance for content that may influence a later tool call.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct TaintMark {
    pub source: UntrustedSourceKind,
    pub source_id: String,
    pub digest: String,
    pub trust: TrustLevel,
}

impl TaintMark {
    /// Creates an untrusted mark while retaining only a digest of the content.
    pub fn from_content(
        source: UntrustedSourceKind,
        source_id: impl Into<String>,
        content: &[u8],
    ) -> Self {
        Self {
            source,
            source_id: source_id.into(),
            digest: sha256_hex(content),
            trust: TrustLevel::Untrusted,
        }
    }
}

fn sha256_hex(content: &[u8]) -> String {
    format!("{:x}", Sha256::digest(content))
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaintContext {
    pub marks: Vec<TaintMark>,
}

impl TaintContext {
    pub fn from_marks(marks: impl IntoIterator<Item = TaintMark>) -> Self {
        let mut marks: Vec<_> = marks.into_iter().collect();
        marks.sort_by(|a, b| {
            (&a.source, &a.source_id, &a.digest, &a.trust).cmp(&(
                &b.source,
                &b.source_id,
                &b.digest,
                &b.trust,
            ))
        });
        marks.dedup();
        Self { marks }
    }

    pub fn is_untrusted(&self) -> bool {
        self.marks
            .iter()
            .any(|mark| mark.trust == TrustLevel::Untrusted)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaintSink {
    ReadOnly,
    FileWrite,
    Shell,
    NetworkDownload,
    WorkflowWrite,
    PackageInstall,
    Deploy,
    CredentialAccess,
    ExternalMessage,
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaintDecisionKind {
    Allow,
    Ask,
    Deny,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaintDecision {
    pub decision: TaintDecisionKind,
    pub sink: TaintSink,
    pub rule_id: String,
    pub reason: String,
    pub source_digests: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_marks_are_redacted_serializable_and_deterministic() {
        let issue = TaintMark::from_content(UntrustedSourceKind::IssueBody, "issue:42", b"body");
        let web = TaintMark::from_content(UntrustedSourceKind::WebContent, "tool:web-1", b"page");
        let context = TaintContext::from_marks([issue.clone(), web, issue]);

        assert_eq!(context.marks.len(), 2);
        assert!(context.is_untrusted());
        let serialized = serde_json::to_value(&context).unwrap();
        assert_eq!(serialized["marks"][0]["trust"], "untrusted");
        assert_eq!(context.marks[0].digest.len(), 64);
        assert_ne!(context.marks[0].digest, "body");
        assert!(serialized["marks"][0].get("content").is_none());
    }

    #[test]
    fn empty_context_is_not_untrusted() {
        assert!(!TaintContext::default().is_untrusted());
    }
}
