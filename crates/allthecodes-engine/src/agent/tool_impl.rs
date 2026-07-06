//! Tool trait implementation for AgentTool.

use anyhow::{bail, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use tracing::{info, warn};
use uuid::Uuid;

use crate::types::message::AssistantMessage;
use crate::types::tool::*;

use super::{resolve_model_alias, AgentInput, AgentTool, TaskAgentTool, MAX_AGENT_DEPTH};

#[async_trait]
impl Tool for AgentTool {
    fn name(&self) -> &str {
        "Agent"
    }

    async fn description(&self, _input: &Value) -> String {
        "Launch a new agent to handle complex, multi-step tasks autonomously.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "The task for the agent to perform"
                },
                "description": {
                    "type": "string",
                    "description": "A short (3-5 word) description of the task"
                },
                "subagent_type": {
                    "type": "string",
                    "description": "The type of specialized agent to use"
                },
                "model": {
                    "type": "string",
                    "description": "Optional model override for this agent. Recommended public aliases: SOTA, MOTA, FOTA; full model IDs are also accepted."
                },
                "run_in_background": {
                    "type": "boolean",
                    "default": false,
                    "description": "Set to true to run this agent in the background"
                },
                "name": {
                    "type": "string",
                    "description": "Name for the spawned teammate. When set, AgentTool routes to Agent Teams and the teammate can be messaged via SendMessage."
                },
                "team_name": {
                    "type": "string",
                    "description": "Team name for spawning a named teammate. Defaults to the active team context; if omitted with no active team, an implicit session team is created."
                },
                "mode": {
                    "type": "string",
                    "enum": ["default", "auto", "bypass", "plan", "acceptEdits", "dontAsk"],
                    "description": "Optional permission mode for the named teammate. Use \"plan\" to require plan approval."
                },
                "isolation": {
                    "type": "string",
                    "enum": ["worktree"],
                    "description": "Isolation mode for the agent"
                },
                "max_turns": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Optional hard cap on the child agent turn count"
                }
            },
            "required": ["prompt", "description"]
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let mut params: AgentInput = serde_json::from_value(input)?;

        // Check recursion depth
        let current_depth = ctx.query_tracking.as_ref().map(|t| t.depth).unwrap_or(0);

        if current_depth >= MAX_AGENT_DEPTH {
            bail!(
                "Agent recursion depth limit reached ({}/{}). \
                 Cannot spawn further subagents.",
                current_depth,
                MAX_AGENT_DEPTH
            );
        }

        let cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| ".".to_string());
        let subagent_type_owned = params
            .subagent_type
            .clone()
            .unwrap_or_else(|| "general-purpose".to_string());
        let agent_definition =
            super::active_agent_definition(std::path::Path::new(&cwd), &subagent_type_owned);
        apply_agent_definition_defaults(&mut params, agent_definition.as_ref(), ctx);

        let description = params.description.as_deref().unwrap_or("unnamed task");
        let subagent_type = params.subagent_type.as_deref().unwrap_or("general-purpose");

        // Resolve model for the subagent.
        // Priority: explicit model param, agent definition, CLAUDE_MODEL env,
        // then parent model. Custom agent definitions stay authoritative while
        // preserving the existing environment fallback.
        let parent_model = ctx.options.main_loop_model.clone();
        let env_model = std::env::var("CLAUDE_MODEL").ok().filter(|s| !s.is_empty());
        let agent_model = match params.model.as_deref() {
            Some(model) => resolve_model_alias(model, &parent_model)?,
            None => match env_model {
                Some(model) => resolve_model_alias(&model, &parent_model)?,
                None => parent_model.clone(),
            },
        };

        if let Some(spawn_request) = teammate_spawn_request(&params, &(ctx.get_app_state)())? {
            let teammate_model = match params.model.as_deref() {
                Some(model) => resolve_optional_teammate_model(model, &parent_model)?,
                None => agent_definition
                    .as_ref()
                    .and_then(|definition| definition.model.as_deref())
                    .map(|model| resolve_optional_teammate_model(model, &parent_model))
                    .transpose()?
                    .flatten(),
            };
            let spawn_input = json!({
                "name": spawn_request.name,
                "prompt": params.prompt.clone(),
                "description": description,
                "team": spawn_request.team_name,
                "agent_type": params.subagent_type.clone(),
                "model": teammate_model,
                "color": agent_definition.as_ref().and_then(|definition| definition.color.clone()),
                "mode": params.mode.clone(),
                "backend": "in-process",
            });
            let mut result =
                crate::agent_runtime::spawn_teammate(spawn_input, ctx, _parent, _on_progress)
                    .await?;
            annotate_agent_teammate_result(&mut result, &params.prompt);
            return Ok(result);
        }

        let agent_id = Uuid::new_v4().to_string();

        // Determine isolation mode before logging (borrow params.isolation)
        let use_worktree = params
            .isolation
            .as_deref()
            .map(|s| s.eq_ignore_ascii_case("worktree"))
            .unwrap_or(false);

        info!(
            agent_id = %agent_id,
            description = %description,
            subagent_type = %subagent_type,
            model = %agent_model,
            depth = current_depth + 1,
            isolation = ?params.isolation,
            "spawning subagent"
        );
        let _ = crate::agent_runtime::emit_subagent_event(
            "spawn",
            &agent_id,
            ctx.agent_id.as_deref(),
            Some(description),
            Some(&agent_model),
            current_depth + 1,
            params.run_in_background,
            Some(json!({
                "subagent_type": subagent_type,
                "isolation": params.isolation,
            })),
        );

        // Load hook configs once (used by both background and synchronous paths)
        let start_configs = {
            let app_state = (ctx.get_app_state)();
            ctx.hook_runner
                .load_hook_configs(&app_state.hooks, "SubagentStart")
        };
        let stop_configs = {
            let app_state = (ctx.get_app_state)();
            ctx.hook_runner
                .load_hook_configs(&app_state.hooks, "SubagentStop")
        };

        // -- Background path
        if params.run_in_background {
            let Some(bg_tx) = ctx.bg_agent_tx.clone() else {
                warn!(
                    agent_id = %agent_id,
                    "run_in_background requested but no bg_agent_tx — running synchronously"
                );
                let _ = crate::agent_runtime::emit_subagent_event(
                    "warning",
                    &agent_id,
                    ctx.agent_id.as_deref(),
                    Some(description),
                    Some(&agent_model),
                    current_depth + 1,
                    true,
                    Some(json!({
                        "message": "run_in_background requested but no completion channel was configured; running synchronously",
                    })),
                );
                // Fall through to synchronous dispatch below
                return self
                    .run_agent_dispatch(
                        use_worktree,
                        &params,
                        ctx,
                        &agent_id,
                        &agent_model,
                        &parent_model,
                        current_depth,
                        description,
                        &start_configs,
                        &stop_configs,
                        false,
                    )
                    .await;
            };

            let bg_description = description.to_string();
            let bg_subagent_type = subagent_type.to_string();

            let launch = super::supervisor::spawn_background_agent(
                params,
                ctx,
                agent_id.clone(),
                bg_description.clone(),
                bg_subagent_type,
                agent_model.clone(),
                parent_model.clone(),
                current_depth,
                use_worktree,
                bg_tx,
                start_configs,
                stop_configs,
            )
            .await?;

            return Ok(ToolResult {
                data: json!(format!(
                    "Agent '{}' launched in background (id: {}, task: {}). You will be notified when it completes.",
                    bg_description, agent_id, launch.task_id
                )),
                new_messages: vec![],
                ..Default::default()
            });
        }

        // -- Synchronous path
        self.run_agent_dispatch(
            use_worktree,
            &params,
            ctx,
            &agent_id,
            &agent_model,
            &parent_model,
            current_depth,
            description,
            &start_configs,
            &stop_configs,
            false,
        )
        .await
    }

    async fn prompt(&self) -> String {
        "Launch a new agent to handle complex, multi-step tasks autonomously.\n\n\
The Agent tool launches specialized agents (subprocesses) that autonomously handle complex tasks. \
Each agent type has specific capabilities and tools available to it.\n\n\
Usage notes:\n\
- Always include a short description (3-5 words) summarizing what the agent will do\n\
- Launch multiple agents concurrently whenever possible, to maximize performance; \
to do that, use a single message with multiple tool uses\n\
- When the agent is done, it will return a single message back to you. \
The result returned by the agent is not visible to the user. \
To show the user the result, you should send a text message back to the user \
with a concise summary of the result.\n\
- Provide clear, detailed prompts so the agent can work autonomously \
and return exactly the information you need.\n\
- The agent's outputs should generally be trusted\n\
- Clearly tell the agent whether you expect it to write code or just to do research \
(search, file reads, web fetches, etc.), since it is not aware of the user's intent"
            .to_string()
    }

    fn user_facing_name(&self, input: Option<&Value>) -> String {
        if let Some(desc) = input
            .and_then(|v| v.get("description"))
            .and_then(|v| v.as_str())
        {
            format!("Agent({})", desc)
        } else {
            "Agent".to_string()
        }
    }

    fn max_result_size_chars(&self) -> usize {
        200_000
    }
}

#[async_trait]
impl Tool for TaskAgentTool {
    fn name(&self) -> &str {
        "Task"
    }

    async fn description(&self, input: &Value) -> String {
        AgentTool.description(input).await
    }

    fn input_json_schema(&self) -> Value {
        AgentTool.input_json_schema()
    }

    fn is_concurrency_safe(&self, input: &Value) -> bool {
        AgentTool.is_concurrency_safe(input)
    }

    async fn validate_input(&self, input: &Value, ctx: &ToolUseContext) -> ValidationResult {
        AgentTool.validate_input(input, ctx).await
    }

    async fn check_permissions(&self, input: &Value, ctx: &ToolUseContext) -> PermissionResult {
        AgentTool.check_permissions(input, ctx).await
    }

    fn backfill_observable_input(&self, input: &mut serde_json::Map<String, Value>) {
        AgentTool.backfill_observable_input(input);
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolUseContext,
        parent: &AssistantMessage,
        on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        AgentTool.call(input, ctx, parent, on_progress).await
    }

    async fn prompt(&self) -> String {
        AgentTool.prompt().await
    }

    fn user_facing_name(&self, input: Option<&Value>) -> String {
        if let Some(desc) = input
            .and_then(|v| v.get("description"))
            .and_then(|v| v.as_str())
        {
            format!("Task({desc})")
        } else {
            "Task".to_string()
        }
    }

    fn max_result_size_chars(&self) -> usize {
        AgentTool.max_result_size_chars()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TeammateSpawnRequest {
    name: String,
    team_name: Option<String>,
}

fn teammate_spawn_request(
    params: &AgentInput,
    app_state: &allthecodes_tools::tool::ToolAppState,
) -> Result<Option<TeammateSpawnRequest>> {
    let Some(raw_name) = params.name.as_deref() else {
        return Ok(None);
    };
    let name = raw_name.trim();
    if name.is_empty() {
        bail!("'name' is required for Agent teammate spawn");
    }

    if let Some(team_context) = app_state
        .team_context
        .as_ref()
        .filter(|context| !context.team_name.is_empty())
    {
        if !allthecodes_types::teams::is_team_lead(Some(team_context)) {
            bail!(
                "Teammates cannot spawn other teammates; omit `name` to create a normal subagent"
            );
        }
    }

    let team_name = params
        .team_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            app_state
                .team_context
                .as_ref()
                .map(|context| context.team_name.trim())
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        });

    Ok(Some(TeammateSpawnRequest {
        name: name.to_string(),
        team_name,
    }))
}

fn resolve_optional_teammate_model(raw_model: &str, parent_model: &str) -> Result<Option<String>> {
    let model = raw_model.trim();
    if model.is_empty() {
        return Ok(None);
    }
    if model.eq_ignore_ascii_case("inherit") {
        return Ok(Some(parent_model.to_string()));
    }
    resolve_model_alias(model, parent_model).map(Some)
}

fn apply_agent_definition_defaults(
    params: &mut AgentInput,
    definition: Option<&allthecodes_ipc_protocol::subsystem_types::AgentDefinitionEntry>,
    ctx: &ToolUseContext,
) {
    let Some(definition) = definition else {
        return;
    };

    if option_empty(params.model.as_deref()) {
        params.model = definition.model.as_ref().and_then(|model| {
            let model = model.trim();
            (!model.is_empty()).then(|| model.to_string())
        });
    }

    if !params.run_in_background && definition.background {
        params.run_in_background = true;
    }

    if option_empty(params.isolation.as_deref()) {
        params.isolation = runtime_isolation(definition.isolation.as_deref());
    }

    if option_empty(params.mode.as_deref()) {
        if let Some(requested) = super::agent_definition_permission_mode(Some(definition)) {
            let parent_mode = (ctx.get_app_state)().tool_permission_context.mode;
            let effective = super::compose_agent_permission_mode(&parent_mode, Some(requested));
            params.mode = Some(effective.as_str().to_string());
        }
    }
}

fn option_empty(value: Option<&str>) -> bool {
    value.map(str::trim).unwrap_or("").is_empty()
}

fn runtime_isolation(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    if value.eq_ignore_ascii_case("worktree") {
        Some("worktree".to_string())
    } else {
        None
    }
}

fn annotate_agent_teammate_result(result: &mut ToolResult, prompt: &str) {
    let Some(object) = result.data.as_object_mut() else {
        return;
    };
    if object.get("spawned").and_then(Value::as_bool) != Some(true) {
        return;
    }

    object.insert("status".into(), json!("teammate_spawned"));
    object.insert("prompt".into(), json!(prompt));
    if let Some(agent_id) = object.get("agent_id").cloned() {
        object.entry("teammate_id").or_insert(agent_id);
    }
    if let Some(team_name) = object.get("team").cloned() {
        object.entry("team_name").or_insert(team_name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // -----------------------------------------------------------------------
    // MAX_AGENT_DEPTH constant
    // -----------------------------------------------------------------------

    #[test]
    fn test_max_agent_depth_value() {
        assert_eq!(MAX_AGENT_DEPTH, 5);
    }

    // -----------------------------------------------------------------------
    // use_worktree flag — isolation field case-insensitivity
    // -----------------------------------------------------------------------

    #[test]
    fn test_use_worktree_lowercase() {
        let input: AgentInput = serde_json::from_value(json!({
            "prompt": "task",
            "isolation": "worktree"
        }))
        .unwrap();
        let use_worktree = input
            .isolation
            .as_deref()
            .map(|s| s.eq_ignore_ascii_case("worktree"))
            .unwrap_or(false);
        assert!(use_worktree);
    }

    #[test]
    fn test_use_worktree_uppercase() {
        let input: AgentInput = serde_json::from_value(json!({
            "prompt": "task",
            "isolation": "WORKTREE"
        }))
        .unwrap();
        let use_worktree = input
            .isolation
            .as_deref()
            .map(|s| s.eq_ignore_ascii_case("worktree"))
            .unwrap_or(false);
        assert!(use_worktree);
    }

    #[test]
    fn test_use_worktree_mixed_case() {
        let input: AgentInput = serde_json::from_value(json!({
            "prompt": "task",
            "isolation": "WorkTree"
        }))
        .unwrap();
        let use_worktree = input
            .isolation
            .as_deref()
            .map(|s| s.eq_ignore_ascii_case("worktree"))
            .unwrap_or(false);
        assert!(use_worktree);
    }

    #[test]
    fn test_use_worktree_none_when_no_isolation() {
        let input: AgentInput = serde_json::from_value(json!({
            "prompt": "task"
        }))
        .unwrap();
        let use_worktree = input
            .isolation
            .as_deref()
            .map(|s| s.eq_ignore_ascii_case("worktree"))
            .unwrap_or(false);
        assert!(!use_worktree);
    }

    #[test]
    fn test_use_worktree_false_for_unknown_mode() {
        let input: AgentInput = serde_json::from_value(json!({
            "prompt": "task",
            "isolation": "sandbox"
        }))
        .unwrap();
        let use_worktree = input
            .isolation
            .as_deref()
            .map(|s| s.eq_ignore_ascii_case("worktree"))
            .unwrap_or(false);
        assert!(!use_worktree);
    }

    // -----------------------------------------------------------------------
    // description / subagent_type defaults used in call()
    // -----------------------------------------------------------------------

    #[test]
    fn test_description_default_fallback() {
        let input: AgentInput = serde_json::from_value(json!({
            "prompt": "do something"
        }))
        .unwrap();
        let description = input.description.as_deref().unwrap_or("unnamed task");
        assert_eq!(description, "unnamed task");
    }

    #[test]
    fn test_description_provided() {
        let input: AgentInput = serde_json::from_value(json!({
            "prompt": "do something",
            "description": "search codebase"
        }))
        .unwrap();
        let description = input.description.as_deref().unwrap_or("unnamed task");
        assert_eq!(description, "search codebase");
    }

    #[test]
    fn test_subagent_type_default_fallback() {
        let input: AgentInput = serde_json::from_value(json!({
            "prompt": "do something"
        }))
        .unwrap();
        let subagent_type = input.subagent_type.as_deref().unwrap_or("general-purpose");
        assert_eq!(subagent_type, "general-purpose");
    }

    #[test]
    fn test_subagent_type_provided() {
        let input: AgentInput = serde_json::from_value(json!({
            "prompt": "explore",
            "subagent_type": "Explore"
        }))
        .unwrap();
        let subagent_type = input.subagent_type.as_deref().unwrap_or("general-purpose");
        assert_eq!(subagent_type, "Explore");
    }

    // -----------------------------------------------------------------------
    // Tool trait flags — is_read_only and is_destructive (defaults)
    // -----------------------------------------------------------------------

    #[test]
    fn test_agent_tool_not_read_only() {
        let tool = AgentTool;
        assert!(!tool.is_read_only(&json!({})));
    }

    #[test]
    fn test_agent_tool_not_destructive() {
        let tool = AgentTool;
        assert!(!tool.is_destructive(&json!({})));
    }

    // -----------------------------------------------------------------------
    // prompt() content — sanity check key phrases
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_prompt_contains_key_guidance() {
        let tool = AgentTool;
        let prompt = tool.prompt().await;
        assert!(prompt.contains("description"));
        assert!(prompt.contains("concurrently"));
        assert!(prompt.contains("autonomously"));
    }

    // -----------------------------------------------------------------------
    // schema — required fields
    // -----------------------------------------------------------------------

    #[test]
    fn test_schema_required_fields() {
        let tool = AgentTool;
        let schema = tool.input_json_schema();
        let required = schema["required"].as_array().unwrap();
        let required_names: Vec<&str> = required.iter().map(|v| v.as_str().unwrap()).collect();
        assert!(required_names.contains(&"prompt"));
        assert!(required_names.contains(&"description"));
    }

    #[test]
    fn test_schema_model_enum() {
        let tool = AgentTool;
        let schema = tool.input_json_schema();
        assert!(schema["properties"]["model"]["enum"].is_null());
        let description = schema["properties"]["model"]["description"]
            .as_str()
            .unwrap();
        assert!(description.contains("SOTA"));
        assert!(description.contains("MOTA"));
        assert!(description.contains("FOTA"));
    }

    #[test]
    fn test_schema_isolation_enum() {
        let tool = AgentTool;
        let schema = tool.input_json_schema();
        let isolation_enum = schema["properties"]["isolation"]["enum"]
            .as_array()
            .unwrap();
        let variants: Vec<&str> = isolation_enum.iter().map(|v| v.as_str().unwrap()).collect();
        assert!(variants.contains(&"worktree"));
    }

    #[test]
    fn test_schema_run_in_background_default_false() {
        let tool = AgentTool;
        let schema = tool.input_json_schema();
        let default_val = &schema["properties"]["run_in_background"]["default"];
        assert_eq!(default_val, &serde_json::Value::Bool(false));
    }

    #[test]
    fn test_schema_exposes_multi_agent_spawn_fields() {
        let tool = AgentTool;
        let schema = tool.input_json_schema();
        let props = schema["properties"].as_object().unwrap();
        assert!(props.contains_key("name"));
        assert!(props.contains_key("team_name"));
        assert!(props.contains_key("mode"));
        assert!(props.contains_key("max_turns"));

        let mode_enum = schema["properties"]["mode"]["enum"].as_array().unwrap();
        let variants: Vec<&str> = mode_enum.iter().filter_map(|v| v.as_str()).collect();
        assert!(variants.contains(&"plan"));
    }

    #[test]
    fn test_teammate_spawn_request_uses_explicit_or_active_team() {
        let mut state = crate::types::app_state::AppState::default();
        let lead_id = allthecodes_types::teams::lead_agent_id("alpha");
        state.team_context = Some(allthecodes_types::teams::TeamContext {
            team_name: "alpha".into(),
            lead_agent_id: lead_id.clone(),
            self_agent_id: Some(lead_id),
            ..Default::default()
        });

        let params: AgentInput = serde_json::from_value(json!({
            "prompt": "review",
            "description": "review code",
            "name": " reviewer ",
            "team_name": " beta "
        }))
        .unwrap();
        let request = teammate_spawn_request(&params, &state.to_tool_app_state())
            .unwrap()
            .unwrap();
        assert_eq!(request.name, "reviewer");
        assert_eq!(request.team_name.as_deref(), Some("beta"));

        let params: AgentInput = serde_json::from_value(json!({
            "prompt": "review",
            "description": "review code",
            "name": "reviewer"
        }))
        .unwrap();
        let request = teammate_spawn_request(&params, &state.to_tool_app_state())
            .unwrap()
            .unwrap();
        assert_eq!(request.team_name.as_deref(), Some("alpha"));
    }

    #[test]
    fn test_teammate_spawn_request_allows_implicit_team_without_context() {
        let state = crate::types::app_state::AppState::default();
        let params: AgentInput = serde_json::from_value(json!({
            "prompt": "review",
            "description": "review code",
            "name": "reviewer"
        }))
        .unwrap();
        let request = teammate_spawn_request(&params, &state.to_tool_app_state())
            .unwrap()
            .unwrap();
        assert_eq!(request.name, "reviewer");
        assert!(request.team_name.is_none());
    }

    #[test]
    fn test_teammate_spawn_request_rejects_nested_teammate_spawn() {
        let state = crate::types::app_state::AppState {
            team_context: Some(allthecodes_types::teams::TeamContext {
                team_name: "alpha".into(),
                lead_agent_id: allthecodes_types::teams::lead_agent_id("alpha"),
                self_agent_id: Some(allthecodes_types::teams::format_agent_id("worker", "alpha")),
                ..Default::default()
            }),
            ..Default::default()
        };
        let params: AgentInput = serde_json::from_value(json!({
            "prompt": "review",
            "description": "review code",
            "name": "reviewer"
        }))
        .unwrap();
        let error = teammate_spawn_request(&params, &state.to_tool_app_state()).unwrap_err();
        assert!(error.to_string().contains("Teammates cannot spawn"));
    }

    #[test]
    fn test_agent_teammate_result_annotation() {
        let mut result = ToolResult {
            data: json!({
                "spawned": true,
                "agent_id": "reviewer@alpha",
                "team": "alpha"
            }),
            ..Default::default()
        };
        annotate_agent_teammate_result(&mut result, "review code");
        assert_eq!(result.data["status"], json!("teammate_spawned"));
        assert_eq!(result.data["prompt"], json!("review code"));
        assert_eq!(result.data["teammate_id"], json!("reviewer@alpha"));
        assert_eq!(result.data["team_name"], json!("alpha"));
    }

    // -----------------------------------------------------------------------
    // user_facing_name — edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_user_facing_name_empty_string_description() {
        let tool = AgentTool;
        // Empty string is still a string — shows Agent()
        let input = json!({"description": ""});
        assert_eq!(tool.user_facing_name(Some(&input)), "Agent()");
    }

    #[test]
    fn test_user_facing_name_non_string_description_falls_back() {
        let tool = AgentTool;
        // description is a number, not a string — falls back to "Agent"
        let input = json!({"description": 42});
        assert_eq!(tool.user_facing_name(Some(&input)), "Agent");
    }
}
