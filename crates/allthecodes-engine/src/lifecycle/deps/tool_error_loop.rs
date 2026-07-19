use std::collections::HashMap;

use sha2::{Digest, Sha256};

use super::QueryEngineDeps;
use crate::query::deps::ToolExecRequest;
use crate::session::record_replay::types::{QueryEventRecord, RecordItem};

const TOOL_VALIDATION_FAILURE_LIMIT: u32 = 3;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ToolValidationFailureFingerprint {
    tool_name: String,
    input_digest: String,
    validation_error_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ToolErrorLoopTermination {
    tool_name: String,
    input_digest: String,
    validation_error_digest: String,
    attempts: u32,
}

impl ToolErrorLoopTermination {
    fn message(&self) -> String {
        format!(
            "tool_error_loop: {} produced the same validation failure {} times \
             (input_digest={}, validation_error_digest={}); stopping to prevent an unbounded \
             tool retry loop",
            self.tool_name,
            self.attempts,
            self.input_digest,
            self.validation_error_digest,
        )
    }
}

#[derive(Debug, Default)]
pub(crate) struct ToolErrorLoopGuard {
    counts: HashMap<ToolValidationFailureFingerprint, u32>,
    terminal: Option<ToolErrorLoopTermination>,
}

impl ToolErrorLoopGuard {
    fn observe(
        &mut self,
        tool_name: &str,
        input: &serde_json::Value,
        validation_error: &str,
    ) -> Option<ToolErrorLoopTermination> {
        if self.terminal.is_some() {
            return None;
        }

        let input_digest = digest_bytes(
            &serde_json::to_vec(input).unwrap_or_else(|_| input.to_string().into_bytes()),
        );
        let validation_error_digest = digest_bytes(validation_error.as_bytes());
        let fingerprint = ToolValidationFailureFingerprint {
            tool_name: tool_name.to_string(),
            input_digest,
            validation_error_digest,
        };
        let attempts = self.counts.entry(fingerprint.clone()).or_default();
        *attempts = attempts.saturating_add(1);
        if *attempts < TOOL_VALIDATION_FAILURE_LIMIT {
            return None;
        }

        let terminal = ToolErrorLoopTermination {
            tool_name: fingerprint.tool_name,
            input_digest: fingerprint.input_digest,
            validation_error_digest: fingerprint.validation_error_digest,
            attempts: *attempts,
        };
        self.terminal = Some(terminal.clone());
        Some(terminal)
    }

    pub(crate) fn terminal_message(&self) -> Option<String> {
        self.terminal
            .as_ref()
            .map(ToolErrorLoopTermination::message)
    }
}

impl QueryEngineDeps {
    pub(crate) async fn record_tool_validation_failure(
        &self,
        request: &ToolExecRequest,
        input: &serde_json::Value,
        validation_error: &str,
    ) {
        let terminal = self.tool_error_loop_guard.lock().observe(
            &request.tool_name,
            input,
            validation_error,
        );
        let Some(terminal) = terminal else {
            return;
        };

        self.record_replay_items(
            vec![RecordItem::QueryEvent(QueryEventRecord::ToolErrorLoop {
                tool_name: terminal.tool_name,
                input_digest: terminal.input_digest,
                validation_error_digest: terminal.validation_error_digest,
                attempts: terminal.attempts,
            })],
            "tool_error_loop",
        )
        .await;
    }
}

fn digest_bytes(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn third_identical_validation_failure_terminates_without_raw_input() {
        let mut guard = ToolErrorLoopGuard::default();
        let input = json!({"secret": "do-not-record", "path": "/tmp/a"});

        assert!(guard.observe("Edit", &input, "same error").is_none());
        assert!(guard.observe("Edit", &input, "same error").is_none());
        let terminal = guard
            .observe("Edit", &input, "same error")
            .expect("third matching failure must terminate");

        let message = terminal.message();
        assert!(message.contains("tool_error_loop"));
        assert!(message.contains("sha256:"));
        assert!(!message.contains("do-not-record"));
        assert!(!message.contains("same error"));
    }

    #[test]
    fn different_input_or_error_has_an_independent_count() {
        let mut guard = ToolErrorLoopGuard::default();
        let first = json!({"path": "/tmp/a"});
        let second = json!({"path": "/tmp/b"});

        assert!(guard.observe("Edit", &first, "error-a").is_none());
        assert!(guard.observe("Edit", &first, "error-a").is_none());
        assert!(guard.observe("Edit", &second, "error-a").is_none());
        assert!(guard.observe("Edit", &first, "error-b").is_none());
        assert!(guard.terminal_message().is_none());
    }
}
