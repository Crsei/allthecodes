//! Durable plan-mode workflow types.
//!
//! Plan mode is not only a permission flag: it has an approval lifecycle,
//! human-readable plan artifact, and execution trace that must survive IPC or
//! daemon transport changes. This module stays in `cc-types` so the engine,
//! IPC, daemon, and UI protocol can all share the same record shape.

use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

pub const PLAN_WORKFLOW_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanWorkflowStatus {
    Draft,
    PendingApproval,
    Approved,
    Rejected,
    Implementing,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanApprovalState {
    NotRequested,
    Pending,
    Approved,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanTraceKind {
    Created,
    EnteredPlanMode,
    ClassifierEntered,
    ApprovalRequested,
    ApprovalApproved,
    ApprovalRejected,
    ImplementationLinked,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanWorkflowTraceEvent {
    pub id: String,
    pub kind: PlanTraceKind,
    pub at: String,
    pub source: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanWorkflowRecord {
    pub schema_version: u32,
    pub id: String,
    pub file_path: String,
    pub status: PlanWorkflowStatus,
    pub approval_state: PlanApprovalState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub linked_task_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_text: Option<String>,
    #[serde(default)]
    pub trace: Vec<PlanWorkflowTraceEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanEntryClassifierDecision {
    pub should_enter: bool,
    pub reason: String,
    pub matched_rule: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanWorkflowEventPayload {
    pub event: String,
    pub summary: String,
    pub record: PlanWorkflowRecord,
}

impl PlanWorkflowEventPayload {
    pub fn new(
        event: impl Into<String>,
        summary: impl Into<String>,
        record: PlanWorkflowRecord,
    ) -> Self {
        Self {
            event: event.into(),
            summary: summary.into(),
            record,
        }
    }
}

impl PlanWorkflowRecord {
    pub fn new(file_path: impl Into<String>, owner: Option<String>, source: &str) -> Self {
        let now = timestamp();
        let mut record = Self {
            schema_version: PLAN_WORKFLOW_SCHEMA_VERSION,
            id: format!("plan_{}", Uuid::new_v4().as_simple()),
            file_path: file_path.into(),
            status: PlanWorkflowStatus::Draft,
            approval_state: PlanApprovalState::NotRequested,
            owner,
            created_at: now.clone(),
            updated_at: now,
            linked_task_ids: Vec::new(),
            plan_text: None,
            trace: Vec::new(),
        };
        record.push_trace(
            PlanTraceKind::Created,
            source,
            "Plan workflow created",
            None,
        );
        record
    }

    pub fn enter_plan_mode(
        &mut self,
        source: &str,
        description: Option<&str>,
        classifier_reason: Option<&str>,
    ) {
        self.status = PlanWorkflowStatus::Draft;
        self.approval_state = PlanApprovalState::NotRequested;

        if let Some(reason) = classifier_reason {
            self.push_trace(
                PlanTraceKind::ClassifierEntered,
                source,
                "Classifier entered plan mode",
                Some(serde_json::json!({ "reason": reason })),
            );
        }

        let mut data = serde_json::Map::new();
        if let Some(description) = description.filter(|value| !value.trim().is_empty()) {
            data.insert(
                "description".to_string(),
                Value::String(description.to_string()),
            );
        }
        self.push_trace(
            PlanTraceKind::EnteredPlanMode,
            source,
            "Entered plan mode",
            if data.is_empty() {
                None
            } else {
                Some(Value::Object(data))
            },
        );
    }

    pub fn request_approval(&mut self, source: &str, plan_text: Option<String>) {
        self.status = PlanWorkflowStatus::PendingApproval;
        self.approval_state = PlanApprovalState::Pending;
        if plan_text
            .as_deref()
            .is_some_and(|text| !text.trim().is_empty())
        {
            self.plan_text = plan_text;
        }
        self.push_trace(
            PlanTraceKind::ApprovalRequested,
            source,
            "Plan approval requested",
            None,
        );
    }

    pub fn approve(&mut self, source: &str) {
        self.status = PlanWorkflowStatus::Approved;
        self.approval_state = PlanApprovalState::Approved;
        self.push_trace(
            PlanTraceKind::ApprovalApproved,
            source,
            "Plan approved",
            None,
        );
    }

    pub fn reject(&mut self, source: &str, feedback: Option<String>) {
        self.status = PlanWorkflowStatus::Rejected;
        self.approval_state = PlanApprovalState::Rejected;
        self.push_trace(
            PlanTraceKind::ApprovalRejected,
            source,
            "Plan rejected",
            feedback.map(|value| serde_json::json!({ "feedback": value })),
        );
    }

    pub fn link_task(&mut self, source: &str, task_id: String, summary: Option<String>) {
        if !self.linked_task_ids.iter().any(|id| id == &task_id) {
            self.linked_task_ids.push(task_id.clone());
        }
        if self.status == PlanWorkflowStatus::Approved {
            self.status = PlanWorkflowStatus::Implementing;
        }
        self.push_trace(
            PlanTraceKind::ImplementationLinked,
            source,
            "Implementation evidence linked",
            Some(serde_json::json!({
                "task_id": task_id,
                "summary": summary,
            })),
        );
    }

    pub fn complete(&mut self, source: &str, summary: Option<String>) {
        self.status = PlanWorkflowStatus::Completed;
        self.push_trace(
            PlanTraceKind::Completed,
            source,
            "Plan workflow completed",
            summary.map(|value| serde_json::json!({ "summary": value })),
        );
    }

    fn push_trace(
        &mut self,
        kind: PlanTraceKind,
        source: &str,
        summary: &str,
        data: Option<Value>,
    ) {
        let now = timestamp();
        self.updated_at = now.clone();
        self.trace.push(PlanWorkflowTraceEvent {
            id: format!("ptrace_{}", Uuid::new_v4().as_simple()),
            kind,
            at: now,
            source: source.to_string(),
            summary: summary.to_string(),
            data,
        });
    }
}

pub fn summarize(record: &PlanWorkflowRecord) -> String {
    format!(
        "Plan workflow {}: status={:?}, approval={:?}, file={}, linked_tasks={}",
        record.id,
        record.status,
        record.approval_state,
        record.file_path,
        record.linked_task_ids.len()
    )
}

pub fn event_payload(
    record: &PlanWorkflowRecord,
    event: &str,
    summary: &str,
) -> PlanWorkflowEventPayload {
    PlanWorkflowEventPayload::new(event, summary, record.clone())
}

pub fn timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_flow_records_trace() {
        let mut record = PlanWorkflowRecord::new("/tmp/plan.md", Some("main".into()), "test");

        record.enter_plan_mode(
            "classifier",
            Some("refactor safely"),
            Some("explicit request"),
        );
        record.request_approval("tool", Some("1. Test\n2. Implement".into()));
        record.approve("permission");
        record.link_task("task", "task-1".into(), Some("tests passed".into()));

        assert_eq!(record.approval_state, PlanApprovalState::Approved);
        assert_eq!(record.status, PlanWorkflowStatus::Implementing);
        assert_eq!(record.linked_task_ids, vec!["task-1"]);
        assert!(record
            .trace
            .iter()
            .any(|event| event.kind == PlanTraceKind::ClassifierEntered));
        assert!(record
            .trace
            .iter()
            .any(|event| event.kind == PlanTraceKind::ApprovalApproved));
    }
}
