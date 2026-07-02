//! Structured execution record for agent tool invocations.
//!
//! `AgentRuntimeExecutionRecord` captures a single tool execution with all
//! metadata fields (session, agent, tool name, command, exit code, digests,
//! permission decision, model, retry/fallback state) in one stable schema.
//! Non-shell tools produce records where shell-specific fields are `None`.
//!
//! The record is designed for audit, replay, and external consumption. It does
//! **not** replace the existing `AgentEvent::ToolUse` / `AgentEvent::ToolResult`
//! pair - those remain the primary UI-facing events.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Final permission decision for a runtime tool invocation.
///
/// Serde uses lower_snake_case so the wire format stays the existing string
/// contract while Rust call sites use typed variants.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentRuntimePermissionDecision {
    AllowedByPolicy,
    AllowedByUser,
    AllowedByHook,
    DeniedByPolicy,
    DeniedByUser,
    DeniedByHook,
    NotRequired,
}

impl AgentRuntimePermissionDecision {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AllowedByPolicy => "allowed_by_policy",
            Self::AllowedByUser => "allowed_by_user",
            Self::AllowedByHook => "allowed_by_hook",
            Self::DeniedByPolicy => "denied_by_policy",
            Self::DeniedByUser => "denied_by_user",
            Self::DeniedByHook => "denied_by_hook",
            Self::NotRequired => "not_required",
        }
    }
}

/// A single structured record of one agent tool execution.
///
/// Optional fields are still serialized as `null` so external consumers can
/// rely on a stable schema across shell and non-shell tools.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct AgentRuntimeExecutionRecord {
    /// Opaque session identifier shared across all events in a session.
    pub session_id: String,

    /// The agent that executed the tool.
    pub agent_id: String,

    /// Parent agent of the executing agent, if any.
    pub parent_agent_id: Option<String>,

    /// Human-readable role label for the agent (e.g. "build-agent").
    /// Falls back to `agent_type` from the spawn event if no explicit role.
    pub agent_role: Option<String>,

    /// Normalised tool identifier (e.g. `"shell"`, `"read"`, `"write"`).
    pub tool: String,

    /// The tool-use correlation id used to match `ToolUse` / `ToolResult` pairs.
    pub tool_use_id: Option<String>,

    /// The command string for shell-family tools; `None` for all others.
    pub command: Option<String>,

    /// Resolved working directory (absolute path) at execution time.
    pub cwd: Option<PathBuf>,

    /// Process exit code for shell-family tools; `None` for non-process tools.
    pub exit_code: Option<i32>,

    /// SHA-256 hex digest of the raw stdout content.
    pub stdout_digest: Option<String>,

    /// SHA-256 hex digest of the raw stderr content.
    pub stderr_digest: Option<String>,

    /// Number of model-level retries that occurred before this execution completed.
    /// `0` means no retries; does **not** include command-internal retries.
    pub retry_count: u32,

    /// The model identifier used for the agent turn that produced this tool call.
    pub model: Option<String>,

    /// Whether a model/fallback occurred during the agent turn.
    /// `false` by default; `true` only when a different model or provider was
    /// actually used.
    pub fallback_used: bool,

    /// Final permission decision for this tool invocation.
    ///
    /// `None` when the decision could not be determined.
    pub permission_decision: Option<AgentRuntimePermissionDecision>,

    /// Wall-clock duration of the tool execution in milliseconds.
    pub duration_ms: Option<u64>,

    /// Whether the tool execution ended with an error condition.
    /// For shell tools this corresponds to a non-zero exit code.
    /// For non-shell tools this is `true` when the tool produced an error result.
    pub had_error: bool,

    /// Schema version for forward-compatibility.
    ///
    /// Current version: `1`.
    pub schema_version: u32,
}

// ---------------------------------------------------------------------------
// Default
// ---------------------------------------------------------------------------

impl Default for AgentRuntimeExecutionRecord {
    fn default() -> Self {
        Self {
            session_id: String::new(),
            agent_id: String::new(),
            parent_agent_id: None,
            agent_role: None,
            tool: String::new(),
            tool_use_id: None,
            command: None,
            cwd: None,
            exit_code: None,
            stdout_digest: None,
            stderr_digest: None,
            retry_count: 0,
            model: None,
            fallback_used: false,
            permission_decision: None,
            duration_ms: None,
            had_error: false,
            schema_version: 1,
        }
    }
}

// ---------------------------------------------------------------------------
// Public helpers
// ---------------------------------------------------------------------------

/// Compute the SHA-256 hex digest of a byte slice.
///
/// Uses the `sha2` crate (stable, widely audited).  The output is a
/// lower-case hex string of 64 characters.
///
/// ```
/// # use allthecodes_types::agent_runtime_record::compute_digest;
/// let d = compute_digest(b"hello");
/// assert_eq!(d.len(), 64);
/// ```
pub fn compute_digest(data: &[u8]) -> String {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_values() {
        let r = AgentRuntimeExecutionRecord::default();
        assert!(r.session_id.is_empty());
        assert!(r.agent_id.is_empty());
        assert!(r.parent_agent_id.is_none());
        assert!(r.agent_role.is_none());
        assert!(r.tool.is_empty());
        assert!(r.tool_use_id.is_none());
        assert!(r.command.is_none());
        assert!(r.cwd.is_none());
        assert!(r.exit_code.is_none());
        assert!(r.stdout_digest.is_none());
        assert!(r.stderr_digest.is_none());
        assert_eq!(r.retry_count, 0);
        assert!(r.model.is_none());
        assert!(!r.fallback_used);
        assert!(r.permission_decision.is_none());
        assert!(r.duration_ms.is_none());
        assert!(!r.had_error);
        assert_eq!(r.schema_version, 1);
    }

    #[test]
    fn test_serde_round_trip_full() {
        let r = AgentRuntimeExecutionRecord {
            session_id: "ses-001".into(),
            agent_id: "agent-42".into(),
            parent_agent_id: Some("agent-1".into()),
            agent_role: Some("build-agent".into()),
            tool: "shell".into(),
            tool_use_id: Some("tu-abc".into()),
            command: Some("npm test".into()),
            cwd: Some(PathBuf::from("/home/project")),
            exit_code: Some(1),
            stdout_digest: Some(compute_digest(b"output")),
            stderr_digest: Some(compute_digest(b"errors")),
            retry_count: 2,
            model: Some("claude-sonnet-5".into()),
            fallback_used: true,
            permission_decision: Some(AgentRuntimePermissionDecision::AllowedByPolicy),
            duration_ms: Some(1234),
            had_error: true,
            schema_version: 1,
        };

        let json = serde_json::to_string_pretty(&r).expect("serialize");
        assert!(json.contains(r#""session_id": "ses-001""#));
        assert!(json.contains(r#""agent_id": "agent-42""#));
        assert!(json.contains(r#""parent_agent_id": "agent-1""#));
        assert!(json.contains(r#""agent_role": "build-agent""#));
        assert!(json.contains(r#""tool": "shell""#));
        assert!(json.contains(r#""tool_use_id": "tu-abc""#));
        assert!(json.contains(r#""command": "npm test""#));
        assert!(json.contains(r#""exit_code": 1"#));
        assert!(json.contains(r#""retry_count": 2"#));
        assert!(json.contains(r#""model": "claude-sonnet-5""#));
        assert!(json.contains(r#""fallback_used": true"#));
        assert!(json.contains(r#""permission_decision": "allowed_by_policy""#));
        assert!(json.contains(r#""had_error": true"#));
        assert!(json.contains(r#""schema_version": 1"#));

        assert!(json.contains(r#""stdout_digest""#));
        assert!(json.contains(r#""stderr_digest""#));

        let deserialized: AgentRuntimeExecutionRecord =
            serde_json::from_str(&json).expect("deserialize");
        assert_eq!(deserialized, r);
    }

    #[test]
    fn test_serde_round_trip_shell_defaults() {
        // A shell execution with only required fields set must still serialize
        // every nullable field as `null`.
        let r = AgentRuntimeExecutionRecord {
            session_id: "s1".into(),
            agent_id: "a1".into(),
            tool: "shell".into(),
            ..Default::default()
        };

        let json = serde_json::to_string_pretty(&r).expect("serialize");
        assert!(json.contains(r#""session_id": "s1""#));
        assert!(json.contains(r#""agent_id": "a1""#));
        assert!(json.contains(r#""tool": "shell""#));

        for key in [
            "parent_agent_id",
            "agent_role",
            "tool_use_id",
            "command",
            "cwd",
            "exit_code",
            "stdout_digest",
            "stderr_digest",
            "model",
            "permission_decision",
            "duration_ms",
        ] {
            assert!(json.contains(&format!(r#""{key}": null"#)), "missing {key}");
        }

        let deserialized: AgentRuntimeExecutionRecord =
            serde_json::from_str(&json).expect("deserialize");
        assert_eq!(deserialized.session_id, "s1");
        assert!(deserialized.parent_agent_id.is_none());
        assert_eq!(deserialized.retry_count, 0);
    }

    #[test]
    fn test_serde_round_trip_tool_use_id_null() {
        let r = AgentRuntimeExecutionRecord {
            session_id: "s1".into(),
            agent_id: "a1".into(),
            tool: "shell".into(),
            tool_use_id: Some("tu-001".into()),
            ..Default::default()
        };
        let json = serde_json::to_string_pretty(&r).expect("serialize");
        assert!(json.contains(r#""tool_use_id": "tu-001""#));

        // Without tool_use_id it should serialize as null.
        let r2 = AgentRuntimeExecutionRecord {
            session_id: "s1".into(),
            agent_id: "a1".into(),
            tool: "shell".into(),
            tool_use_id: None,
            ..Default::default()
        };
        let json2 = serde_json::to_string_pretty(&r2).expect("serialize");
        assert!(json2.contains(r#""tool_use_id": null"#));
    }

    #[test]
    fn test_digest_hex_length() {
        let d = compute_digest(b"test data");
        assert_eq!(d.len(), 64);
        // hex chars only
        assert!(d.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_digest_deterministic() {
        let a = compute_digest(b"hello world");
        let b = compute_digest(b"hello world");
        assert_eq!(a, b);
    }

    #[test]
    fn test_digest_empty() {
        let d = compute_digest(b"");
        assert_eq!(d.len(), 64);
        // sha256("") = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
        assert_eq!(
            d,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_permission_decision_wire_values_are_stable() {
        let cases = [
            (
                AgentRuntimePermissionDecision::AllowedByPolicy,
                "allowed_by_policy",
            ),
            (
                AgentRuntimePermissionDecision::AllowedByUser,
                "allowed_by_user",
            ),
            (
                AgentRuntimePermissionDecision::AllowedByHook,
                "allowed_by_hook",
            ),
            (
                AgentRuntimePermissionDecision::DeniedByPolicy,
                "denied_by_policy",
            ),
            (
                AgentRuntimePermissionDecision::DeniedByUser,
                "denied_by_user",
            ),
            (
                AgentRuntimePermissionDecision::DeniedByHook,
                "denied_by_hook",
            ),
            (AgentRuntimePermissionDecision::NotRequired, "not_required"),
        ];

        for (decision, wire) in cases {
            assert_eq!(decision.as_str(), wire);
            let json = serde_json::to_string(&decision).expect("serialize");
            assert_eq!(json, format!(r#""{wire}""#));
            let parsed: AgentRuntimePermissionDecision =
                serde_json::from_str(&json).expect("deserialize");
            assert_eq!(parsed, decision);
        }

        let err = serde_json::from_str::<AgentRuntimePermissionDecision>(r#""allowed_by_typo""#)
            .unwrap_err();
        assert!(
            err.to_string().contains("unknown variant"),
            "unexpected error: {err}"
        );
    }
}
