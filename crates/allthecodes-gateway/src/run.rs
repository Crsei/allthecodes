use crate::{RemoteSource, SessionKey, SessionKeyPolicy};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static RUN_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Stable diagnostic shape for external gateway errors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayDiagnostic {
    pub code: String,
    pub message: String,
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
}

impl GatewayDiagnostic {
    pub fn new(
        code: impl Into<String>,
        message: impl Into<String>,
        action: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            action: action.into(),
            context: None,
        }
    }

    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        self.context = Some(context.into());
        self
    }
}

/// Gateway error wrapper that preserves the machine-readable diagnostic.
#[derive(Debug)]
pub struct GatewayError {
    diagnostic: GatewayDiagnostic,
}

impl GatewayError {
    pub fn new(diagnostic: GatewayDiagnostic) -> Self {
        Self { diagnostic }
    }

    pub fn diagnostic(&self) -> &GatewayDiagnostic {
        &self.diagnostic
    }

    pub fn into_diagnostic(self) -> GatewayDiagnostic {
        self.diagnostic
    }

    pub fn io(
        code: impl Into<String>,
        message: impl Into<String>,
        action: impl Into<String>,
        path: impl AsRef<std::path::Path>,
        error: &std::io::Error,
    ) -> Self {
        Self::new(
            GatewayDiagnostic::new(code, message, action).with_context(format!(
                "path={}, os_error={}",
                path.as_ref().display(),
                error
            )),
        )
    }
}

impl fmt::Display for GatewayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: {} ({})",
            self.diagnostic.code, self.diagnostic.message, self.diagnostic.action
        )
    }
}

impl std::error::Error for GatewayError {}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RunId(String);

impl RunId {
    pub fn new() -> Self {
        let now = now_millis();
        let seq = RUN_COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let digest = Sha256::digest(format!("{now}:{pid}:{seq}").as_bytes());
        Self(format!("run_{}", &hex::encode(digest)[..20]))
    }

    pub fn from_string(value: impl Into<String>) -> Result<Self, GatewayError> {
        let value = value.into();
        if is_safe_id(&value) && value.starts_with("run_") {
            Ok(Self(value))
        } else {
            Err(GatewayError::new(GatewayDiagnostic::new(
                "invalid_run_id",
                "Run id is not a valid gateway run identifier.",
                "Use a run id returned by the gateway.",
            )))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for RunId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for RunId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Queued,
    Running,
    WaitingApproval,
    WaitingUser,
    Completed,
    Failed,
    Cancelled,
    Recoverable,
}

impl RunStatus {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    pub fn can_transition_to(self, next: Self) -> bool {
        use RunStatus::*;
        match (self, next) {
            (current, same) if current == same => true,
            (Queued, Running | Cancelled | Failed) => true,
            (
                Running,
                WaitingApproval | WaitingUser | Completed | Failed | Cancelled | Recoverable,
            ) => true,
            (
                WaitingApproval | WaitingUser,
                Running | Completed | Failed | Cancelled | Recoverable,
            ) => true,
            (Recoverable, Queued | Running | Failed | Cancelled) => true,
            (Completed | Failed | Cancelled, _) => false,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BusyPolicy {
    #[default]
    Queue,
    Reject,
    Interrupt,
    Steer,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunPolicy {
    pub busy: BusyPolicy,
    pub permission_mode: String,
    #[serde(default)]
    pub delivery: Vec<String>,
}

impl Default for RunPolicy {
    fn default() -> Self {
        Self {
            busy: BusyPolicy::Queue,
            permission_mode: "ask".to_string(),
            delivery: vec!["origin".to_string(), "local".to_string()],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunRequest {
    pub prompt: String,
    pub source: RemoteSource,
    #[serde(default)]
    pub policy: RunPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

impl RunRequest {
    pub fn redacted(&self) -> Self {
        Self {
            prompt: self.prompt.clone(),
            source: self.source.redacted(),
            policy: self.policy.clone(),
            idempotency_key: self.idempotency_key.as_ref().map(|key| redact_key(key)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunMeta {
    pub run_id: RunId,
    pub session_key: SessionKey,
    pub status: RunStatus,
    pub request: RunRequest,
    #[serde(default)]
    pub metadata: Map<String, Value>,
    pub created_at_ms: u128,
    pub updated_at_ms: u128,
}

impl RunMeta {
    pub fn new(run_id: RunId, request: RunRequest, session_policy: &SessionKeyPolicy) -> Self {
        let now = now_millis();
        let session_key = SessionKey::derive(&request.source, session_policy);
        Self {
            run_id,
            session_key,
            status: RunStatus::Queued,
            request: request.redacted(),
            metadata: Map::new(),
            created_at_ms: now,
            updated_at_ms: now,
        }
    }

    pub fn set_status(&mut self, status: RunStatus) -> Result<(), GatewayError> {
        if self.status.can_transition_to(status) {
            self.status = status;
            self.updated_at_ms = now_millis();
            Ok(())
        } else {
            Err(GatewayError::new(
                GatewayDiagnostic::new(
                    "invalid_run_status_transition",
                    "Run status transition is not allowed.",
                    "Reload the run and apply a valid lifecycle transition.",
                )
                .with_context(format!("from={:?}, to={:?}", self.status, status)),
            ))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateRunOutcome {
    Created(RunMeta),
    Existing(RunMeta),
}

impl CreateRunOutcome {
    pub fn meta(&self) -> &RunMeta {
        match self {
            Self::Created(meta) | Self::Existing(meta) => meta,
        }
    }
}

pub(crate) fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

pub(crate) fn idempotency_hash(source: &RemoteSource, key: &str) -> String {
    let mut hasher = Sha256::new();
    for part in source.key_material() {
        hasher.update(part.as_bytes());
        hasher.update(b"\n");
    }
    hasher.update(key.as_bytes());
    hex::encode(hasher.finalize())
}

pub(crate) fn is_safe_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
}

fn redact_key(key: &str) -> String {
    let digest = Sha256::digest(key.as_bytes());
    format!("<redacted:{}>", &hex::encode(digest)[..12])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RemoteSource, RemoteTransport};

    fn request() -> RunRequest {
        RunRequest {
            prompt: "hello".to_string(),
            source: RemoteSource::new(
                RemoteTransport::Http,
                "local",
                "F:/repo",
                "client",
                "user",
                "thread",
            )
            .with_metadata("Authorization", "Bearer raw-token"),
            policy: RunPolicy::default(),
            idempotency_key: Some("provider-delivery-id".to_string()),
        }
    }

    #[test]
    fn run_meta_redacts_source_and_idempotency_key() {
        let meta = RunMeta::new(RunId::new(), request(), &SessionKeyPolicy::default());
        let json = serde_json::to_string(&meta).unwrap();

        assert!(!json.contains("raw-token"));
        assert!(!json.contains("provider-delivery-id"));
        assert!(json.contains("<redacted"));
    }

    #[test]
    fn run_status_blocks_terminal_transition() {
        assert!(RunStatus::Queued.can_transition_to(RunStatus::Running));
        assert!(!RunStatus::Completed.can_transition_to(RunStatus::Running));
    }

    #[test]
    fn invalid_run_id_is_diagnostic_error() {
        let err = RunId::from_string("../run_bad").unwrap_err();
        assert_eq!(err.diagnostic().code, "invalid_run_id");
    }
}
