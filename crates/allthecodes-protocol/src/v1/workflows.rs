#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct WorkflowDefinitionsQuery {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct WorkflowDefinitionParams {
    pub workflow: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct WorkflowDefinitionsResponse {
    pub definitions: Vec<WorkflowDefinitionSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct WorkflowDefinitionSummary {
    pub workflow: String,
    pub workflow_file: String,
    pub step_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parse_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct WorkflowDefinitionDetail {
    pub workflow: String,
    pub workflow_file: String,
    pub steps: Vec<WorkflowStepDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct WorkflowStepDefinition {
    pub index: usize,
    pub name: String,
    pub prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum WorkflowRunStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum WorkflowRunStepStatus {
    Pending,
    Running,
    Ready,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum WorkflowAdvanceStatus {
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct WorkflowRunListQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<WorkflowRunStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct WorkflowRunParams {
    pub run_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct WorkflowStartRequest {
    pub request_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct WorkflowRunStartParams {
    pub workflow: String,
    pub request_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct WorkflowAdvanceRequest {
    pub request_id: String,
    pub expected_revision: u64,
    pub applied_status: WorkflowAdvanceStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct WorkflowRunAdvanceParams {
    pub run_id: String,
    pub request_id: String,
    pub expected_revision: u64,
    pub applied_status: WorkflowAdvanceStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct WorkflowCancelRequest {
    pub request_id: String,
    pub expected_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct WorkflowRunCancelParams {
    pub run_id: String,
    pub request_id: String,
    pub expected_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct WorkflowRunSummary {
    pub run_id: String,
    pub workflow: String,
    pub workflow_file: String,
    pub status: WorkflowRunStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_step_index: Option<usize>,
    pub completed_steps: usize,
    pub total_steps: usize,
    pub revision: u64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct WorkflowRunStep {
    pub index: usize,
    pub name: String,
    pub prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
    pub status: WorkflowRunStepStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct WorkflowRun {
    pub run_id: String,
    pub workflow: String,
    pub workflow_file: String,
    pub status: WorkflowRunStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_step_index: Option<usize>,
    pub steps: Vec<WorkflowRunStep>,
    pub completed_steps: usize,
    pub total_steps: usize,
    pub has_args: bool,
    pub revision: u64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct WorkflowRunPage {
    pub runs: Vec<WorkflowRunSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    pub truncated: bool,
    pub corrupt_entry_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct WorkflowMutationMetadata {
    pub request_id: String,
    pub replayed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projection_warning: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct WorkflowRunResponse {
    pub run: WorkflowRun,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mutation: Option<WorkflowMutationMetadata>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workflow_mutation_contract_round_trips() {
        let value = serde_json::json!({
            "runId": "workflow-run-12345678-1234-1234-1234-123456789abc",
            "requestId": "request-1",
            "expectedRevision": 3,
            "appliedStatus": "completed"
        });
        let params: WorkflowRunAdvanceParams =
            serde_json::from_value(value.clone()).expect("advance params deserialize");
        assert_eq!(params.applied_status, WorkflowAdvanceStatus::Completed);
        assert_eq!(
            serde_json::to_value(params).expect("advance params serialize"),
            value
        );
    }

    #[test]
    fn workflow_run_response_exposes_no_paths_or_arguments() {
        let response = WorkflowRunResponse {
            run: WorkflowRun {
                run_id: "workflow-run-12345678-1234-1234-1234-123456789abc".to_string(),
                workflow: "release".to_string(),
                workflow_file: "release.md".to_string(),
                status: WorkflowRunStatus::Running,
                current_step_index: Some(0),
                steps: Vec::new(),
                completed_steps: 0,
                total_steps: 1,
                has_args: true,
                revision: 1,
                created_at: "2026-07-16T00:00:00Z".to_string(),
                updated_at: "2026-07-16T00:00:00Z".to_string(),
            },
            mutation: None,
        };
        let value = serde_json::to_value(response).expect("response serializes");
        assert!(value["run"].get("workflowPath").is_none());
        assert!(value["run"].get("runPath").is_none());
        assert!(value["run"].get("args").is_none());
        assert_eq!(value["run"]["hasArgs"], true);
    }
}
