//! Dynamic workflow tool definitions — Action, Observation, Tool trait impl.

use std::collections::HashMap;

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::tool::{
    PermissionResult, Tool, ToolProgress, ToolResult, ToolUseContext, ValidationResult,
};
use allthecodes_types::message::AssistantMessage;

use super::executor::{validate_subagent_type_name, validate_workflow_plan};
use super::{WorkflowContext, WorkflowExecutionReport};

// ---------------------------------------------------------------------------
// Action
// ---------------------------------------------------------------------------

/// Schema for running a dynamic workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicWorkflowAction {
    /// A short name for this workflow run.
    pub name: String,
    /// JSON DAG plan: either a top-level array of stages or an object
    /// `{name, description, max_concurrency, stages}`.
    pub plan: Value,
    /// Default subagent type for all stages.
    #[serde(default)]
    pub subagent_type: Option<String>,
    /// Maximum number of concurrent sub-agents.
    #[serde(default = "default_max_concurrency")]
    pub max_concurrency: u8,
}

const fn default_max_concurrency() -> u8 {
    8
}

// ---------------------------------------------------------------------------
// Observation
// ---------------------------------------------------------------------------

/// Observation from a dynamic workflow run.
#[derive(Debug, Clone, Serialize)]
pub struct DynamicWorkflowObservation {
    pub name: String,
    pub status: &'static str,
    /// Stage results: stage_id -> result text (only on success).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage_results: Option<HashMap<String, String>>,
    /// Final synthesized result (last stage if sequential, or reduce result).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_stage_id: Option<String>,
    /// Total number of stages in the plan.
    pub total_stages: usize,
    /// Number of completed stages.
    pub completed_stages: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl DynamicWorkflowObservation {
    fn success(name: String, report: WorkflowExecutionReport) -> Self {
        let completed = report.completed_stages();
        Self {
            name,
            status: "completed",
            stage_results: Some(report.stage_results),
            final_result: report.final_result,
            final_stage_id: report.final_stage_id,
            total_stages: report.total_stages,
            completed_stages: completed,
            error: None,
        }
    }

    fn error(name: String, msg: String, completed: usize, total: usize) -> Self {
        Self {
            name,
            status: "error",
            stage_results: None,
            final_result: None,
            final_stage_id: None,
            total_stages: total,
            completed_stages: completed,
            error: Some(msg),
        }
    }
}

impl From<DynamicWorkflowObservation> for ToolResult {
    fn from(obs: DynamicWorkflowObservation) -> Self {
        let is_err = obs.status == "error";
        let text = if is_err {
            format!(
                "DynamicWorkflow '{}' failed: {}",
                obs.name,
                obs.error.as_deref().unwrap_or("unknown error")
            )
        } else {
            format!(
                "DynamicWorkflow '{}' completed ({}/{})",
                obs.name, obs.completed_stages, obs.total_stages
            )
        };
        ToolResult {
            data: json!(obs),
            display_preview: Some(text.clone()),
            model_content: Some(allthecodes_types::message::ToolResultContent::Text(text)),
            ..Default::default()
        }
    }
}

// ---------------------------------------------------------------------------
// Tool descriptor
// ---------------------------------------------------------------------------

const DYNAMIC_WORKFLOW_DESCRIPTION: &str = r#"Run a dynamic workflow with DAG-based agent orchestration.

Use this tool for tasks that benefit from fan-out/fan-in with multiple sub-agents,
such as parallel code review, multi-area audit, or investigate-and-reduce patterns.

Provide a JSON plan with stages. Each stage is one of:
- "agent": run a single sub-agent
- "map": fan out one sub-agent per item
- "reduce": collect prior stage results and synthesize

Stages can declare `depends_on` to form a DAG; stages with no dependencies run
immediately. The executor runs stages layer-by-layer, respecting `max_concurrency`.

Example plan:
```json
{
  "name": "coverage-audit",
  "stages": [
    {"id": "a", "kind": "agent", "prompt": "Review module A", "depends_on": []},
    {"id": "b", "kind": "agent", "prompt": "Review module B", "depends_on": []},
    {"id": "c", "kind": "reduce", "prompt": "Synthesize A and B", "depends_on": ["a", "b"]}
  ]
}
```"#;

// ---------------------------------------------------------------------------
// Tool
// ---------------------------------------------------------------------------

pub struct DynamicWorkflowTool;

#[async_trait]
impl Tool for DynamicWorkflowTool {
    fn name(&self) -> &str {
        "DynamicWorkflow"
    }

    async fn description(&self, _input: &Value) -> String {
        DYNAMIC_WORKFLOW_DESCRIPTION.to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "A short name for this workflow run."
                },
                "plan": {
                    "type": "object",
                    "description": "JSON workflow plan — object with `name`, `stages`, optional `max_concurrency`.",
                    "properties": {
                        "name": {"type": "string"},
                        "max_concurrency": {
                            "type": "integer",
                            "minimum": 1,
                            "maximum": 64,
                            "default": 8
                        },
                        "stages": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "id": {"type": "string"},
                                    "kind": {
                                        "type": "string",
                                        "enum": ["agent", "map", "reduce"]
                                    },
                                    "prompt": {"type": "string"},
                                    "subagent_type": {"type": "string"},
                                    "depends_on": {
                                        "type": "array",
                                        "items": {"type": "string"}
                                    },
                                    "items": {
                                        "type": "array",
                                        "items": {"type": "string"},
                                        "description": "Items for map stages"
                                    },
                                    "reduce_input": {
                                        "type": "string",
                                        "description": "Comma-separated stage IDs whose results feed this reduce"
                                    }
                                },
                                "required": ["id", "kind", "prompt"]
                            }
                        }
                    },
                    "required": ["stages"]
                },
                "subagent_type": {
                    "type": "string",
                    "default": "general-purpose",
                    "description": "Default subagent type for all stages."
                },
                "max_concurrency": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 64,
                    "default": 8,
                    "description": "Maximum concurrent sub-agents."
                }
            },
            "required": ["name", "plan"]
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        false
    }

    fn is_destructive(&self, _input: &Value) -> bool {
        false
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        let name = input
            .get("name")
            .and_then(Value::as_str)
            .map(|s| s.trim())
            .unwrap_or("");
        if name.is_empty() {
            return ValidationResult::Error {
                message: "name is required for DynamicWorkflow".into(),
                error_code: 400,
            };
        }

        if let Some(max_concurrency) = input.get("max_concurrency") {
            if !valid_max_concurrency(max_concurrency) {
                return ValidationResult::Error {
                    message: "max_concurrency must be an integer between 1 and 64".into(),
                    error_code: 400,
                };
            }
        }

        if let Some(default_subagent) = input.get("subagent_type").and_then(Value::as_str) {
            if let Err(err) = validate_subagent_type_name(default_subagent) {
                return ValidationResult::Error {
                    message: err.to_string(),
                    error_code: 400,
                };
            }
        }

        let plan = match input.get("plan") {
            Some(Value::Object(m)) => m,
            _ => {
                return ValidationResult::Error {
                    message: "plan must be a JSON object with a `stages` array".into(),
                    error_code: 400,
                }
            }
        };

        if let Some(max_concurrency) = plan.get("max_concurrency") {
            if !valid_max_concurrency(max_concurrency) {
                return ValidationResult::Error {
                    message: "plan.max_concurrency must be an integer between 1 and 64".into(),
                    error_code: 400,
                };
            }
        }

        let stages = match plan.get("stages").and_then(Value::as_array) {
            Some(arr) if !arr.is_empty() => arr,
            Some(_) => {
                return ValidationResult::Error {
                    message: "plan.stages must not be empty".into(),
                    error_code: 400,
                }
            }
            _ => {
                return ValidationResult::Error {
                    message: "plan.stages is required".into(),
                    error_code: 400,
                }
            }
        };

        // Validate individual stage structure
        for (i, stage) in stages.iter().enumerate() {
            let id = stage
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("<missing>");
            if stage.get("id").and_then(Value::as_str).is_none() {
                return ValidationResult::Error {
                    message: format!("stage[{}] is missing 'id'", i),
                    error_code: 400,
                };
            }
            let kind = match stage.get("kind").and_then(Value::as_str) {
                Some(k) => k,
                None => {
                    return ValidationResult::Error {
                        message: format!("stage '{}' is missing 'kind'", id),
                        error_code: 400,
                    }
                }
            };
            if !matches!(kind, "agent" | "map" | "reduce") {
                return ValidationResult::Error {
                    message: format!(
                        "stage '{}' has invalid kind '{}'; must be agent/map/reduce",
                        id, kind
                    ),
                    error_code: 400,
                };
            }
            if stage
                .get("prompt")
                .and_then(Value::as_str)
                .is_none_or(|s| s.trim().is_empty())
            {
                return ValidationResult::Error {
                    message: format!("stage '{}' has empty or missing prompt", id),
                    error_code: 400,
                };
            }
            if let Some(stage_subagent) = stage.get("subagent_type").and_then(Value::as_str) {
                if let Err(err) = validate_subagent_type_name(stage_subagent) {
                    return ValidationResult::Error {
                        message: format!("stage '{}' {}", id, err),
                        error_code: 400,
                    };
                }
            }
            if kind == "map"
                && stage
                    .get("items")
                    .and_then(Value::as_array)
                    .is_none_or(|a| a.is_empty())
            {
                return ValidationResult::Error {
                    message: format!("map stage '{}' requires non-empty items", id),
                    error_code: 400,
                };
            }
            if let Some(items) = stage.get("items") {
                if !items
                    .as_array()
                    .is_some_and(|items| items.iter().all(|v| v.as_str().is_some()))
                {
                    return ValidationResult::Error {
                        message: format!("stage '{}' items must be an array of strings", id),
                        error_code: 400,
                    };
                }
            }
            // depends_on must be an array of strings if present
            if let Some(deps) = stage.get("depends_on") {
                if !deps
                    .as_array()
                    .is_some_and(|deps| deps.iter().all(|v| v.as_str().is_some()))
                {
                    return ValidationResult::Error {
                        message: format!("stage '{}' depends_on must be an array of strings", id),
                        error_code: 400,
                    };
                }
            }
        }

        if let Err(err) = validate_workflow_plan(&Value::Object(plan.clone())) {
            return ValidationResult::Error {
                message: err.to_string(),
                error_code: 400,
            };
        }

        ValidationResult::Ok
    }

    async fn check_permissions(&self, input: &Value, _ctx: &ToolUseContext) -> PermissionResult {
        let name = input
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("<unnamed>");
        let stage_count = input
            .get("plan")
            .and_then(|p| p.get("stages"))
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0);
        PermissionResult::Ask {
            message: format!(
                "Allow DynamicWorkflow '{}' with {} stage(s)?",
                name, stage_count
            ),
        }
    }

    fn backfill_observable_input(&self, input: &mut serde_json::Map<String, Value>) {
        // redact plan stages from observable input
        if let Some(plan) = input.get_mut("plan") {
            if let Some(stages) = plan.as_object_mut().and_then(|m| m.get_mut("stages")) {
                if let Some(arr) = stages.as_array_mut() {
                    for stage in arr.iter_mut() {
                        if let Some(prompt) =
                            stage.as_object_mut().and_then(|m| m.get_mut("prompt"))
                        {
                            if let Some(s) = prompt.as_str() {
                                if s.len() > 80 {
                                    *prompt = json!(format!("{}...", &s[..80]));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    fn to_auto_classifier_input(&self, input: &Value) -> Value {
        let name = input
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("<unnamed>");
        let stage_count = input
            .get("plan")
            .and_then(|p| p.get("stages"))
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0);
        json!({
            "operation": "dynamic_workflow",
            "name": name,
            "stage_count": stage_count,
        })
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let name = input
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("unnamed")
            .to_string();
        let plan_value = input
            .get("plan")
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("plan is required"))?;
        let default_subagent = input
            .get("subagent_type")
            .and_then(Value::as_str)
            .unwrap_or("general-purpose")
            .to_string();
        validate_subagent_type_name(&default_subagent)?;
        let top_level_max_concurrency = match input.get("max_concurrency") {
            Some(value) => Some(
                value
                    .as_u64()
                    .filter(|number| (1..=64).contains(number))
                    .map(|number| number as u8)
                    .ok_or_else(|| {
                        anyhow::anyhow!("max_concurrency must be an integer between 1 and 64")
                    })?,
            ),
            None => None,
        };

        // Parse the plan
        let plan = validate_workflow_plan(&plan_value).context("invalid workflow plan")?;
        let max_concurrency = top_level_max_concurrency.unwrap_or(plan.max_concurrency);

        let context = WorkflowContext::new(name.clone(), default_subagent, max_concurrency, _ctx);

        // Execute plan
        let report = context.execute_plan(plan).await;
        if !report.is_success() {
            let error_msg = report
                .error_message()
                .unwrap_or_else(|| "workflow did not complete all stages".to_string());
            let obs = DynamicWorkflowObservation::error(
                name,
                error_msg,
                report.completed_stages(),
                report.total_stages,
            );
            return Ok(ToolResult::from(obs));
        }

        let obs = DynamicWorkflowObservation::success(name, report);
        Ok(ToolResult::from(obs))
    }

    async fn prompt(&self) -> String {
        DYNAMIC_WORKFLOW_DESCRIPTION.to_string()
    }

    fn max_result_size_chars(&self) -> usize {
        200_000
    }
}

fn valid_max_concurrency(value: &Value) -> bool {
    value
        .as_u64()
        .is_some_and(|number| (1..=64).contains(&number))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::{ToolResult, ToolUseContext, ToolUseOptions, ValidationResult};
    use std::sync::Arc;

    // ---------------------------------------------------------------------------
    // DynamicWorkflowAction serde roundtrip
    // ---------------------------------------------------------------------------

    #[test]
    fn action_serde_roundtrip_minimal() {
        let action = DynamicWorkflowAction {
            name: "test".into(),
            plan: json!({
                "stages": [{
                    "id": "a1",
                    "kind": "agent",
                    "prompt": "review module A",
                    "subagent_type": "general-purpose",
                    "depends_on": []
                }]
            }),
            subagent_type: None,
            max_concurrency: 8,
        };
        let json = serde_json::to_string(&action).unwrap();
        let deserialized: DynamicWorkflowAction = serde_json::from_str(&json).unwrap();
        assert_eq!(action.name, deserialized.name);
        assert_eq!(action.max_concurrency, deserialized.max_concurrency);
        assert_eq!(action.subagent_type, deserialized.subagent_type);
        assert_eq!(
            serde_json::to_string(&action.plan).unwrap(),
            serde_json::to_string(&deserialized.plan).unwrap()
        );
    }

    #[test]
    fn action_serde_roundtrip_with_subagent_type() {
        let action = DynamicWorkflowAction {
            name: "audit".into(),
            plan: json!({
                "name": "audit-plan",
                "max_concurrency": 4,
                "stages": [
                    {"id": "a", "kind": "agent", "prompt": "do A"},
                    {"id": "b", "kind": "agent", "prompt": "do B", "depends_on": ["a"]}
                ]
            }),
            subagent_type: Some("Explore".into()),
            max_concurrency: 4,
        };
        let json = serde_json::to_string(&action).unwrap();
        let deserialized: DynamicWorkflowAction = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.subagent_type.as_deref(), Some("Explore"));
        assert_eq!(deserialized.max_concurrency, 4);
    }

    #[test]
    fn action_serde_default_max_concurrency() {
        let json =
            r#"{"name":"test","plan":{"stages":[{"id":"s1","kind":"agent","prompt":"do it"}]}}"#;
        let deserialized: DynamicWorkflowAction = serde_json::from_str(json).unwrap();
        assert_eq!(deserialized.max_concurrency, 8);
        assert!(deserialized.subagent_type.is_none());
    }

    // ---------------------------------------------------------------------------
    // DynamicWorkflowObservation serde
    // ---------------------------------------------------------------------------

    #[test]
    fn observation_serde_success() {
        let mut stage_results = HashMap::new();
        stage_results.insert("a".into(), "result A".into());
        stage_results.insert("b".into(), "result B".into());
        let obs = DynamicWorkflowObservation {
            name: "test".into(),
            status: "completed",
            stage_results: Some(stage_results),
            final_result: Some("synthesized".into()),
            final_stage_id: Some("c".into()),
            total_stages: 3,
            completed_stages: 3,
            error: None,
        };
        let json = serde_json::to_string(&obs).unwrap();
        let deserialized: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized["name"], "test");
        assert_eq!(deserialized["status"], "completed");
        assert_eq!(deserialized["total_stages"], 3);
        assert_eq!(deserialized["completed_stages"], 3);
        assert_eq!(deserialized["final_result"], "synthesized");
        assert_eq!(deserialized["final_stage_id"], "c");
        assert!(deserialized.get("stage_results").is_some());
        assert!(deserialized.get("error").is_none());
    }

    #[test]
    fn observation_serde_error_omits_stage_results() {
        let obs = DynamicWorkflowObservation {
            name: "fail".into(),
            status: "error",
            stage_results: None,
            final_result: None,
            final_stage_id: None,
            total_stages: 2,
            completed_stages: 1,
            error: Some("stage a failed".into()),
        };
        let json = serde_json::to_string(&obs).unwrap();
        let deserialized: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized["status"], "error");
        assert_eq!(deserialized["error"], "stage a failed");
        assert!(deserialized.get("stage_results").is_none());
        assert!(deserialized.get("final_result").is_none());
        assert!(deserialized.get("final_stage_id").is_none());
    }

    #[test]
    fn observation_error_formats_tool_result() {
        let obs = DynamicWorkflowObservation::error("test".into(), "something broke".into(), 1, 3);
        let result: ToolResult = obs.into();
        assert!(result.display_preview.unwrap().contains("failed"));
    }

    #[test]
    fn observation_success_formats_tool_result() {
        let report = WorkflowExecutionReport {
            stage_results: HashMap::new(),
            final_stage_id: None,
            final_result: None,
            total_stages: 2,
            errors: vec![],
        };
        let obs = DynamicWorkflowObservation::success("test".into(), report);
        let result: ToolResult = obs.into();
        assert!(result.display_preview.unwrap().contains("completed"));
    }

    // ---------------------------------------------------------------------------
    // Plan JSON with valid stages roundtrips through DynamicWorkflowAction
    // ---------------------------------------------------------------------------

    #[test]
    fn action_roundtrip_plan_with_all_stage_kinds() {
        let action = DynamicWorkflowAction {
            name: "full".into(),
            plan: json!({
                "name": "full-plan",
                "stages": [
                    {"id": "a", "kind": "agent", "prompt": "agent A", "depends_on": []},
                    {"id": "m1", "kind": "map", "prompt": "map {item}", "items": ["x","y"], "depends_on": []},
                    {"id": "r1", "kind": "reduce", "prompt": "reduce", "depends_on": ["a","m1"], "reduce_input": "a,m1"}
                ]
            }),
            subagent_type: None,
            max_concurrency: 8,
        };
        let json = serde_json::to_string(&action).unwrap();
        let deserialized: DynamicWorkflowAction = serde_json::from_str(&json).unwrap();
        let stages = deserialized.plan["stages"]
            .as_array()
            .expect("plan should have stages array");
        assert_eq!(stages.len(), 3);
        assert_eq!(stages[0]["kind"], "agent");
        assert_eq!(stages[1]["kind"], "map");
        assert_eq!(stages[2]["kind"], "reduce");
    }

    // ---------------------------------------------------------------------------
    // Validation: plan JSON with invalid stage kind returns error during validation
    // (not during serde — serde accepts any Value, validation rejects bad kinds)
    // ---------------------------------------------------------------------------

    fn make_ctx() -> ToolUseContext {
        let (_tx, rx) = tokio::sync::watch::channel(false);
        ToolUseContext {
            options: ToolUseOptions {
                debug: false,
                main_loop_model: "test".into(),
                verbose: false,
                is_non_interactive_session: false,
                custom_system_prompt: None,
                append_system_prompt: None,
                max_budget_usd: None,
            },
            abort_signal: rx,
            read_file_state: crate::tool::FileStateCache::default(),
            get_app_state: Arc::new(crate::tool::ToolAppState::default),
            set_app_state: Arc::new(|_| {}),
            session_id: "test".into(),
            langfuse_session_id: "test".into(),
            messages: vec![],
            agent_id: None,
            agent_type: None,
            query_tracking: None,
            permission_callback: None,
            ask_user_callback: None,
            permission_event_callback: None,
            bg_agent_tx: None,
            hook_runner: Arc::new(allthecodes_types::hooks::NoopHookRunner::new()),
            command_dispatcher: Arc::new(allthecodes_types::commands::NoopCommandDispatcher::new()),
            available_tools: vec![],
            execute_deferred_tool: None,
        }
    }

    #[tokio::test]
    async fn validate_rejects_invalid_stage_kind() {
        let tool = DynamicWorkflowTool;
        let input = json!({
            "name": "test",
            "plan": {
                "stages": [
                    {"id": "x", "kind": "INVALID", "prompt": "test"}
                ]
            }
        });
        let ctx = make_ctx();
        match tool.validate_input(&input, &ctx).await {
            ValidationResult::Error { message, .. } => {
                assert!(message.contains("invalid kind"), "got: {message}");
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn validate_rejects_map_stage_without_items() {
        let tool = DynamicWorkflowTool;
        let input = json!({
            "name": "test",
            "plan": {
                "stages": [
                    {"id": "m1", "kind": "map", "prompt": "review {item}", "depends_on": []}
                ]
            }
        });
        let ctx = make_ctx();
        match tool.validate_input(&input, &ctx).await {
            ValidationResult::Error { message, .. } => {
                assert!(
                    message.contains("requires non-empty items"),
                    "got: {message}"
                );
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn validate_accepts_valid_plan() {
        let tool = DynamicWorkflowTool;
        let input = json!({
            "name": "test",
            "plan": {
                "stages": [
                    {"id": "a", "kind": "agent", "prompt": "do A"},
                    {"id": "b", "kind": "agent", "prompt": "do B", "depends_on": ["a"]}
                ]
            }
        });
        let ctx = make_ctx();
        assert!(matches!(
            tool.validate_input(&input, &ctx).await,
            ValidationResult::Ok
        ));
    }
}
