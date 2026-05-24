use crate::run::{now_millis, GatewayDiagnostic};
use crate::{RunId, RunStatus};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type", content = "data")]
pub enum RunEventKind {
    Created,
    StatusChanged {
        status: RunStatus,
    },
    AssistantDelta {
        text: String,
    },
    ApprovalRequested {
        tool_use_id: String,
    },
    AskUserRequested {
        question_id: String,
    },
    Diagnostic {
        diagnostic: GatewayDiagnostic,
    },
    DeliveryFailed {
        diagnostic: GatewayDiagnostic,
    },
    SessionLockRecovered {
        previous_owner_pid: u32,
        new_owner_pid: u32,
    },
    Custom {
        name: String,
        payload: Value,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunEvent {
    pub run_id: RunId,
    pub sequence: u64,
    pub timestamp_ms: u128,
    pub kind: RunEventKind,
}

impl RunEvent {
    pub fn new(run_id: RunId, sequence: u64, kind: RunEventKind) -> Self {
        Self {
            run_id,
            sequence,
            timestamp_ms: now_millis(),
            kind,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_event_serializes_as_camel_case_record() {
        let event = RunEvent::new(
            RunId::from_string("run_abc123").unwrap(),
            7,
            RunEventKind::StatusChanged {
                status: RunStatus::Running,
            },
        );

        let json = serde_json::to_value(event).unwrap();
        assert_eq!(json["runId"], "run_abc123");
        assert_eq!(json["sequence"], 7);
        assert_eq!(json["kind"]["type"], "status_changed");
    }
}
