//! WorkflowContext: runtime orchestration for DynamicWorkflow stages.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
use futures::future::join_all;
use serde_json::{json, Value};
use tokio::sync::Semaphore;
use tracing::{debug, info, warn};

use crate::tool::{DeferredToolExecutionRequest, ToolResult, ToolUseContext};
use allthecodes_types::message::ToolResultContent;

use super::executor::{WorkflowPlan, WorkflowStage};

const MAX_REDUCE_INPUT_CHARS: usize = 12_000;
const TRUNCATION_MARKER: &str = "\n... [truncated workflow intermediate results]";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageError {
    pub stage_id: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct WorkflowExecutionReport {
    pub stage_results: HashMap<String, String>,
    pub final_stage_id: Option<String>,
    pub final_result: Option<String>,
    pub total_stages: usize,
    pub errors: Vec<StageError>,
}

impl WorkflowExecutionReport {
    pub fn completed_stages(&self) -> usize {
        self.stage_results.len()
    }

    pub fn is_success(&self) -> bool {
        self.errors.is_empty() && self.completed_stages() == self.total_stages
    }

    pub fn error_message(&self) -> Option<String> {
        if self.errors.is_empty() {
            return None;
        }
        Some(
            self.errors
                .iter()
                .map(|err| format!("{}: {}", err.stage_id, err.message))
                .collect::<Vec<_>>()
                .join("; "),
        )
    }
}

struct StageExecution {
    stage_id: String,
    output: String,
}

pub struct WorkflowContext<'a> {
    name: String,
    default_subagent: String,
    max_concurrency: u8,
    semaphore: Arc<Semaphore>,
    ctx: &'a ToolUseContext,
}

impl<'a> WorkflowContext<'a> {
    pub fn new(
        name: String,
        default_subagent: String,
        max_concurrency: u8,
        ctx: &'a ToolUseContext,
    ) -> Self {
        let max_concurrency = max_concurrency.clamp(1, 64);
        Self {
            name,
            default_subagent,
            max_concurrency,
            semaphore: Arc::new(Semaphore::new(max_concurrency as usize)),
            ctx,
        }
    }

    pub async fn execute_plan(&self, plan: WorkflowPlan) -> WorkflowExecutionReport {
        info!(
            workflow = %self.name,
            plan_name = %plan.name,
            stages = plan.stages.len(),
            max_concurrency = self.max_concurrency,
            "executing dynamic workflow plan"
        );

        let total_stages = plan.stages.len();
        let terminal_stage_ids = terminal_stage_ids(&plan.stages);
        let mut stage_results: HashMap<String, String> = HashMap::new();
        let mut remaining: HashMap<String, WorkflowStage> = plan
            .stages
            .iter()
            .cloned()
            .map(|stage| (stage.id.clone(), stage))
            .collect();
        let mut errors = Vec::new();

        while !remaining.is_empty() {
            let ready: Vec<WorkflowStage> = remaining
                .values()
                .filter(|stage| {
                    stage
                        .depends_on
                        .iter()
                        .all(|dep| stage_results.contains_key(dep))
                })
                .cloned()
                .collect();

            if ready.is_empty() {
                let stuck = remaining.keys().cloned().collect::<Vec<_>>();
                warn!(
                    workflow = %self.name,
                    stuck = ?stuck,
                    "workflow DAG deadlocked: no ready stages"
                );
                errors.push(StageError {
                    stage_id: "<workflow>".to_string(),
                    message: format!("workflow DAG deadlocked with remaining stages: {stuck:?}"),
                });
                break;
            }

            let futures = ready
                .into_iter()
                .map(|stage| self.execute_stage(stage, &stage_results));
            let layer_results = join_all(futures).await;

            for result in layer_results {
                match result {
                    Ok(execution) => {
                        debug!(
                            workflow = %self.name,
                            stage = %execution.stage_id,
                            len = execution.output.len(),
                            "workflow stage completed"
                        );
                        remaining.remove(&execution.stage_id);
                        stage_results.insert(execution.stage_id, execution.output);
                    }
                    Err(error) => {
                        let stage_id = error.stage_id;
                        warn!(
                            workflow = %self.name,
                            stage = %stage_id,
                            error = %error.message,
                            "workflow stage failed"
                        );
                        remaining.remove(&stage_id);
                        errors.push(StageError {
                            stage_id,
                            message: error.message,
                        });
                    }
                }
            }

            if !errors.is_empty() {
                break;
            }
        }

        let final_stage_id = if errors.is_empty() {
            terminal_stage_ids
                .iter()
                .rev()
                .find(|stage_id| stage_results.contains_key(stage_id.as_str()))
                .cloned()
        } else {
            None
        };
        let final_result = final_stage_id
            .as_deref()
            .and_then(|stage_id| stage_results.get(stage_id))
            .cloned();

        WorkflowExecutionReport {
            stage_results,
            final_stage_id,
            final_result,
            total_stages,
            errors,
        }
    }

    async fn execute_stage(
        &self,
        stage: WorkflowStage,
        stage_results: &HashMap<String, String>,
    ) -> std::result::Result<StageExecution, StageError> {
        let stage_id = stage.id.clone();
        let output = match stage.kind.as_str() {
            "agent" => {
                let input = self.agent_input(&stage, stage.prompt.clone());
                self.call_agent(&stage_id, input).await
            }
            "map" => self.map_agents(&stage).await,
            "reduce" => {
                let prompt = self.reduce_prompt(&stage, stage_results);
                let input = self.agent_input(&stage, prompt);
                self.call_agent(&stage_id, input).await
            }
            other => Err(anyhow!("unsupported stage kind: {other}")),
        };

        output
            .map(|output| StageExecution { stage_id, output })
            .map_err(|err| StageError {
                stage_id: stage.id,
                message: err.to_string(),
            })
    }

    async fn map_agents(&self, stage: &WorkflowStage) -> Result<String> {
        let items = stage
            .items
            .as_ref()
            .filter(|items| !items.is_empty())
            .ok_or_else(|| anyhow!("map stage requires non-empty items"))?;

        let futures = items.iter().enumerate().map(|(index, item)| {
            let prompt = render_item_prompt(&stage.prompt, item);
            let mut item_stage = stage.clone();
            item_stage.id = format!("{}[{index}]", stage.id);
            let input = self.agent_input(&item_stage, prompt);
            async move {
                let output = self.call_agent(&item_stage.id, input).await?;
                Ok::<_, anyhow::Error>((index, item.clone(), output))
            }
        });

        let item_results = join_all(futures).await;
        let mut parts = Vec::with_capacity(item_results.len());
        for result in item_results {
            let (index, item, output) = result?;
            parts.push(format!("[{index}] {item}\n{output}"));
        }
        Ok(parts.join("\n\n---\n\n"))
    }

    fn reduce_prompt(
        &self,
        stage: &WorkflowStage,
        stage_results: &HashMap<String, String>,
    ) -> String {
        let reduce_inputs: Vec<String> = stage
            .reduce_input
            .as_ref()
            .map(|input_str| {
                input_str
                    .split(',')
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned)
                    .collect()
            })
            .unwrap_or_default();

        let mut context_parts = Vec::new();
        if reduce_inputs.is_empty() {
            let mut ids = stage_results.keys().cloned().collect::<Vec<_>>();
            ids.sort();
            for sid in ids {
                if let Some(result) = stage_results.get(&sid) {
                    context_parts.push(format!("[{sid}]\n{result}"));
                }
            }
        } else {
            for sid in &reduce_inputs {
                if let Some(result) = stage_results.get(sid) {
                    context_parts.push(format!("[{sid}]\n{result}"));
                }
            }
        }

        let context_str = self.truncate_context(&context_parts.join("\n\n---\n\n"));
        format!("{}\n\nContext:\n{}", stage.prompt, context_str)
    }

    fn agent_input(&self, stage: &WorkflowStage, prompt: String) -> Value {
        let subagent_type = stage
            .subagent_type
            .as_deref()
            .unwrap_or(&self.default_subagent);
        json!({
            "prompt": prompt,
            "description": format!("wf:{}", stage.id),
            "subagent_type": subagent_type,
        })
    }

    fn truncate_context(&self, text: &str) -> String {
        if text.len() <= MAX_REDUCE_INPUT_CHARS {
            return text.to_string();
        }
        let end = floor_char_boundary(text, MAX_REDUCE_INPUT_CHARS);
        format!("{}{}", &text[..end], TRUNCATION_MARKER)
    }

    async fn call_agent(&self, stage_id: &str, input: Value) -> Result<String> {
        let execute = self
            .ctx
            .execute_deferred_tool
            .as_ref()
            .ok_or_else(|| anyhow!("Agent dispatch is unavailable in this tool context"))?;

        let _permit = self
            .semaphore
            .clone()
            .acquire_owned()
            .await
            .context("workflow semaphore closed")?;

        let request = DeferredToolExecutionRequest {
            tool_use_id: format!("dynamic-workflow-{}-{stage_id}", sanitize_id(&self.name)),
            tool_name: "Agent".to_string(),
            input,
        };
        let result = execute(request).await?;
        let text = tool_result_text(&result.result);
        if result.is_error {
            bail!("Agent tool failed: {text}");
        }
        Ok(text)
    }
}

fn render_item_prompt(template: &str, item: &str) -> String {
    if template.contains("{item}") {
        template.replace("{item}", item)
    } else {
        format!("{template}\n\nItem:\n{item}")
    }
}

fn terminal_stage_ids(stages: &[WorkflowStage]) -> Vec<String> {
    let has_dependents: HashSet<&str> = stages
        .iter()
        .flat_map(|stage| stage.depends_on.iter().map(String::as_str))
        .collect();
    stages
        .iter()
        .filter(|stage| !has_dependents.contains(stage.id.as_str()))
        .map(|stage| stage.id.clone())
        .collect()
}

fn tool_result_text(result: &ToolResult) -> String {
    if let Some(ToolResultContent::Text(text)) = &result.model_content {
        return text.clone();
    }
    if let Some(text) = result.data.as_str() {
        return text.to_string();
    }
    if !result.data.is_null() {
        return result.data.to_string();
    }
    result.display_preview.clone().unwrap_or_default()
}

fn floor_char_boundary(value: &str, mut index: usize) -> usize {
    index = index.min(value.len());
    while index > 0 && !value.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn sanitize_id(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();
    sanitized.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    use crate::tool::{
        DeferredToolExecutionResult, FileStateCache, ToolAppState, ToolUseContext, ToolUseOptions,
    };
    use allthecodes_types::message::Message;

    fn test_context(calls: Arc<Mutex<Vec<Value>>>, fail_stage: Option<&str>) -> ToolUseContext {
        let (_tx, rx) = tokio::sync::watch::channel(false);
        let fail_stage = fail_stage.map(ToOwned::to_owned);
        ToolUseContext {
            options: ToolUseOptions {
                debug: false,
                main_loop_model: "test-model".into(),
                verbose: false,
                is_non_interactive_session: false,
                custom_system_prompt: None,
                append_system_prompt: None,
                max_budget_usd: None,
            },
            abort_signal: rx,
            read_file_state: FileStateCache::default(),
            get_app_state: Arc::new(ToolAppState::default),
            set_app_state: Arc::new(|_| {}),
            session_id: "workflow-test".into(),
            langfuse_session_id: "workflow-test".into(),
            messages: Vec::<Message>::new(),
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
            execute_deferred_tool: Some(Arc::new(move |request| {
                let calls = calls.clone();
                let fail_stage = fail_stage.clone();
                Box::pin(async move {
                    calls.lock().unwrap().push(request.input.clone());
                    let description = request
                        .input
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .trim_start_matches("wf:")
                        .to_string();
                    let prompt = request
                        .input
                        .get("prompt")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    let should_fail = fail_stage
                        .as_deref()
                        .is_some_and(|stage| stage == description);
                    Ok(DeferredToolExecutionResult {
                        tool_use_id: request.tool_use_id,
                        tool_name: request.tool_name,
                        result: ToolResult {
                            data: json!(if should_fail {
                                format!("failed:{description}")
                            } else {
                                format!("result:{prompt}")
                            }),
                            ..Default::default()
                        },
                        is_error: should_fail,
                    })
                })
            })),
        }
    }

    fn stage(id: &str, kind: &str, prompt: &str) -> WorkflowStage {
        WorkflowStage {
            id: id.into(),
            kind: kind.into(),
            prompt: prompt.into(),
            subagent_type: None,
            depends_on: vec![],
            items: None,
            reduce_input: None,
        }
    }

    #[test]
    fn truncate_short_text() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let ctx = test_context(calls, None);
        let workflow = WorkflowContext::new("test".into(), "general-purpose".into(), 8, &ctx);
        assert_eq!(workflow.truncate_context("short"), "short");
    }

    #[test]
    fn truncate_long_text_keeps_char_boundary() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let ctx = test_context(calls, None);
        let workflow = WorkflowContext::new("test".into(), "general-purpose".into(), 8, &ctx);
        let long = "é".repeat(MAX_REDUCE_INPUT_CHARS);
        let truncated = workflow.truncate_context(&long);
        assert!(truncated.ends_with(TRUNCATION_MARKER));
        let prefix_len = truncated.len() - TRUNCATION_MARKER.len();
        assert!(truncated.is_char_boundary(prefix_len));
    }

    #[test]
    fn agent_input_for_agent_stage() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let ctx = test_context(calls, None);
        let workflow = WorkflowContext::new("test".into(), "general-purpose".into(), 8, &ctx);
        let input =
            workflow.agent_input(&stage("s1", "agent", "do something"), "do something".into());
        assert_eq!(input["prompt"], "do something");
        assert_eq!(input["subagent_type"], "general-purpose");
        assert_eq!(input["description"], "wf:s1");
    }

    #[tokio::test]
    async fn map_stage_fans_out_one_agent_per_item() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let ctx = test_context(calls.clone(), None);
        let workflow = WorkflowContext::new("test".into(), "general-purpose".into(), 8, &ctx);
        let mut map_stage = stage("m1", "map", "review {item}");
        map_stage.items = Some(vec!["alpha".into(), "beta".into()]);
        let report = workflow
            .execute_plan(WorkflowPlan {
                name: "test".into(),
                max_concurrency: 8,
                stages: vec![map_stage],
            })
            .await;

        assert!(report.is_success());
        assert_eq!(calls.lock().unwrap().len(), 2);
        let prompts = calls
            .lock()
            .unwrap()
            .iter()
            .map(|input| input["prompt"].as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        assert_eq!(prompts, vec!["review alpha", "review beta"]);
        assert!(report.stage_results["m1"].contains("[0] alpha"));
        assert!(report.stage_results["m1"].contains("[1] beta"));
    }

    #[tokio::test]
    async fn map_stage_empty_items_returns_error() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let ctx = test_context(calls, None);
        let workflow = WorkflowContext::new("test".into(), "general-purpose".into(), 8, &ctx);
        let mut map_stage = stage("m1", "map", "review {item}");
        // Empty items — the executor should fail this stage
        map_stage.items = Some(vec![]);
        let report = workflow
            .execute_plan(WorkflowPlan {
                name: "test".into(),
                max_concurrency: 8,
                stages: vec![map_stage],
            })
            .await;

        assert!(!report.is_success());
        assert!(!report.errors.is_empty());
        assert_eq!(report.errors[0].stage_id, "m1");
    }

    #[tokio::test]
    async fn failed_stage_stops_dependents_and_reports_error() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let ctx = test_context(calls.clone(), Some("a"));
        let workflow = WorkflowContext::new("test".into(), "general-purpose".into(), 8, &ctx);
        let mut reduce = stage("c", "reduce", "summarize");
        reduce.depends_on = vec!["a".into()];
        let report = workflow
            .execute_plan(WorkflowPlan {
                name: "test".into(),
                max_concurrency: 8,
                stages: vec![stage("a", "agent", "do a"), reduce],
            })
            .await;

        assert!(!report.is_success());
        assert_eq!(report.errors[0].stage_id, "a");
        assert_eq!(calls.lock().unwrap().len(), 1);
        assert!(!report.stage_results.contains_key("c"));
    }

    #[tokio::test]
    async fn final_result_uses_terminal_stage_not_lexicographic_max() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let ctx = test_context(calls, None);
        let workflow = WorkflowContext::new("test".into(), "general-purpose".into(), 8, &ctx);
        let root = stage("z_root", "agent", "root");
        let mut reduce = stage("a_reduce", "reduce", "reduce");
        reduce.depends_on = vec!["z_root".into()];
        let report = workflow
            .execute_plan(WorkflowPlan {
                name: "test".into(),
                max_concurrency: 8,
                stages: vec![root, reduce],
            })
            .await;

        assert!(report.is_success());
        assert_eq!(report.final_stage_id.as_deref(), Some("a_reduce"));
        assert_eq!(
            report.final_result.as_deref(),
            report.stage_results.get("a_reduce").map(String::as_str)
        );
    }

    // ---------------------------------------------------------------------------
    // reduce with no prior results
    // ---------------------------------------------------------------------------

    #[tokio::test]
    async fn reduce_with_no_prior_results_produces_empty_context() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let ctx = test_context(calls.clone(), None);
        let workflow = WorkflowContext::new("test".into(), "general-purpose".into(), 8, &ctx);
        let reduce = stage("r1", "reduce", "synthesize");
        let report = workflow
            .execute_plan(WorkflowPlan {
                name: "test".into(),
                max_concurrency: 8,
                stages: vec![reduce],
            })
            .await;

        assert!(report.is_success());
        // The reduce stage should have run with empty context
        let calls_guard = calls.lock().unwrap();
        let prompt = calls_guard.first().unwrap()["prompt"].as_str().unwrap();
        assert!(prompt.starts_with("synthesize"));
        // Since there are no prior results, context should be empty after the prompt
        assert!(prompt.contains("Context:\n") || !prompt.contains("Context:"));
    }

    // ---------------------------------------------------------------------------
    // Run a single agent through WorkflowContext via execute_plan
    // ---------------------------------------------------------------------------

    #[tokio::test]
    async fn context_run_single_agent_returns_result() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let ctx = test_context(calls.clone(), None);
        let workflow = WorkflowContext::new("test".into(), "general-purpose".into(), 8, &ctx);
        let report = workflow
            .execute_plan(WorkflowPlan {
                name: "test".into(),
                max_concurrency: 8,
                stages: vec![stage("a1", "agent", "do something")],
            })
            .await;

        assert!(report.is_success());
        assert_eq!(report.completed_stages(), 1);
        assert!(report.stage_results.contains_key("a1"));
        assert_eq!(calls.lock().unwrap().len(), 1);
        let input = calls.lock().unwrap().first().unwrap().clone();
        assert_eq!(input["prompt"], "do something");
        assert_eq!(input["description"], "wf:a1");
        assert_eq!(input["subagent_type"], "general-purpose");
    }

    // ---------------------------------------------------------------------------
    // Result truncation when context exceeds MAX_REDUCE_INPUT_CHARS
    // ---------------------------------------------------------------------------

    #[test]
    fn result_truncated_if_too_large() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let ctx = test_context(calls, None);
        let workflow = WorkflowContext::new("test".into(), "general-purpose".into(), 8, &ctx);

        // Build a string that exceeds MAX_REDUCE_INPUT_CHARS
        let oversized = "x".repeat(MAX_REDUCE_INPUT_CHARS + 100);
        let truncated = workflow.truncate_context(&oversized);

        assert!(truncated.len() < oversized.len());
        assert!(truncated.ends_with(TRUNCATION_MARKER));
        // The text portion should be at most MAX_REDUCE_INPUT_CHARS
        let text_part = &truncated[..truncated.len() - TRUNCATION_MARKER.len()];
        assert!(text_part.len() <= MAX_REDUCE_INPUT_CHARS);
    }

    #[test]
    fn truncate_at_char_boundary_multibyte() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let ctx = test_context(calls, None);
        let workflow = WorkflowContext::new("test".into(), "general-purpose".into(), 8, &ctx);

        // Multi-byte characters (3 bytes each) at the boundary
        let long: String = (0..MAX_REDUCE_INPUT_CHARS / 2)
            .map(|_| "\u{1F600}")
            .collect();
        let truncated = workflow.truncate_context(&long);
        assert!(truncated.ends_with(TRUNCATION_MARKER));
        let prefix_len = truncated.len() - TRUNCATION_MARKER.len();
        assert!(truncated.is_char_boundary(prefix_len));
    }
}
