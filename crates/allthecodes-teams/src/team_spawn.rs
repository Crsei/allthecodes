//! TeamSpawn tool - creates and runs a new in-process teammate.
//!
//! This is the conversation-facing entry point for agent teams: the model
//! calls `TeamSpawn` to bring up a named teammate with its own prompt,
//! model, and color. If no team exists yet, an implicit team is created
//! named after the current session and the calling agent becomes team lead.
//!
//! Once spawned, the teammate runs as a tokio task in the same process,
//! reads mailbox messages for its name, and can be addressed through the
//! existing `SendMessage` tool.

use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::info;

use crate::tool_specs as team_tool_specs;

use crate::backend::TeammateExecutor;
use crate::types::{BackendType, TeamContext, TeamMember, TeammateInfo, TeammateSpawnConfig};
use crate::{backend, constants, helpers, identity, in_process::InProcessBackend};
use allthecodes_tools::tool::*;
use allthecodes_types::message::AssistantMessage;

/// TeamSpawn tool.
pub struct TeamSpawnTool;
pub struct SpawnAgentAliasTool;

fn preview_tool_result(data: Value, preview: impl Into<String>) -> ToolResult {
    let preview = preview.into();
    ToolResult {
        data,
        model_content: None,
        display_preview: Some(preview),
        new_messages: vec![],
        ..Default::default()
    }
}

#[derive(Deserialize)]
struct TeamSpawnInput {
    /// Unique teammate name (used as mailbox name + agent id).
    name: String,
    /// Initial prompt passed to the teammate's QueryEngine.
    prompt: String,
    /// Optional model override (defaults to parent model).
    #[serde(default)]
    model: Option<String>,
    /// Optional UI color (red/blue/green/yellow/purple/orange/pink/cyan).
    #[serde(default)]
    color: Option<String>,
    /// Optional team name; if omitted, uses current team or creates one.
    #[serde(default)]
    team: Option<String>,
    /// Optional description for an implicitly-created team.
    #[serde(default)]
    description: Option<String>,
    /// Optional backend. allthecodes supports only in-process.
    #[serde(default)]
    backend: Option<BackendType>,
    /// Optional permission mode for the teammate. `plan` requires plan approval.
    #[serde(default)]
    mode: Option<String>,
    /// Optional agent definition used for system prompt/tool policy.
    #[serde(default)]
    agent_type: Option<String>,
}

#[async_trait]
impl Tool for TeamSpawnTool {
    fn name(&self) -> &str {
        team_tool_specs::TEAM_SPAWN_TOOL_NAME
    }

    async fn description(&self, _input: &Value) -> String {
        team_tool_specs::team_spawn_description()
    }

    fn input_json_schema(&self) -> Value {
        team_tool_specs::team_spawn_schema()
    }

    fn is_enabled(&self) -> bool {
        crate::teams_tooling_enabled()
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        team_tool_specs::validate_team_spawn(input, |raw_backend| {
            match raw_backend.parse::<BackendType>() {
                Ok(backend_type) if backend::is_backend_supported(backend_type) => Ok(()),
                Ok(backend_type) => Err(backend::unsupported_backend_message(backend_type)),
                Err(e) => Err(e),
            }
        })
        .map(|_| ValidationResult::Ok)
        .unwrap_or_else(|message| ValidationResult::Error {
            message,
            error_code: 400,
        })
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let params: TeamSpawnInput = serde_json::from_value(input)?;
        let backend_type = params.backend.unwrap_or_else(backend::default_backend_type);
        backend::ensure_backend_supported(backend_type)?;

        let app_state = (ctx.get_app_state)();
        let cwd = ctx
            .messages
            .first()
            .map(|_| String::new())
            .unwrap_or_default();
        let cwd = if cwd.is_empty() {
            std::env::current_dir()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|_| ".".into())
        } else {
            cwd
        };

        // Resolve team name: explicit > active context > session-derived.
        let (team_name, freshly_created) = match params.team.clone() {
            Some(t) if !t.trim().is_empty() => (t, false),
            _ => match app_state.team_context.as_ref() {
                Some(tc) if !tc.team_name.is_empty() => (tc.team_name.clone(), false),
                _ => {
                    let base = format!(
                        "session-{}",
                        &ctx.session_id.chars().take(8).collect::<String>()
                    );
                    let tf = helpers::create_team(
                        &base,
                        params.description.clone(),
                        Some(ctx.session_id.clone()),
                        &cwd,
                    )?;
                    info!(team = %tf.name, "implicit team created via TeamSpawn");
                    (tf.name, true)
                }
            },
        };

        // Load (or re-load after creation) the TeamFile to assign a color.
        let mut team_file = helpers::read_team_file(&team_name)?;

        // Reject duplicate member names.
        if team_file.members.iter().any(|m| m.name == params.name) {
            return Ok(preview_tool_result(
                json!({
                    "error": format!("teammate '{}' already exists in team '{}'", params.name, team_name),
                }),
                format!(
                    "TeamSpawn failed: teammate '{}' already exists in team '{}'",
                    params.name, team_name
                ),
            ));
        }

        let agent_id = identity::format_agent_id(&params.name, &team_name);
        let color = params
            .color
            .clone()
            .unwrap_or_else(|| helpers::assign_color_for_teammate(&team_file, &agent_id));
        let now = chrono::Utc::now().timestamp();
        let plan_mode_required = team_spawn_plan_mode_required(params.mode.as_deref());
        let agent_type = resolve_team_spawn_agent_type(params.agent_type.as_deref());
        let system_prompt = agent_type
            .as_deref()
            .and_then(allthecodes_engine::agent_runtime::builtin_agent_prompt);
        let system_prompt_mode = system_prompt
            .as_ref()
            .map(|_| crate::types::SystemPromptMode::Append);

        let new_member = TeamMember {
            agent_id: agent_id.clone(),
            name: params.name.clone(),
            agent_type: agent_type.clone(),
            model: params.model.clone(),
            prompt: Some(params.prompt.clone()),
            color: Some(color.clone()),
            plan_mode_required: plan_mode_required.then_some(true),
            joined_at: now,
            tmux_pane_id: String::new(),
            cwd: cwd.clone(),
            worktree_path: None,
            session_id: None,
            task_id: None,
            task_path: None,
            subscriptions: vec![],
            backend_type: Some(BackendType::InProcess),
            is_active: Some(true),
            mode: params.mode.clone(),
            close_state: None,
            close_requested_at: None,
        };
        team_file.members.push(new_member.clone());
        helpers::write_team_file(&team_name, &team_file)?;

        let backend = InProcessBackend::new();
        let task_list_id =
            allthecodes_tasks::task_list_id_from_parts(allthecodes_tasks::TaskListScope {
                explicit_task_list_id: None,
                scoped_team_name: Some(team_name.clone()),
                app_team_name: app_state
                    .team_context
                    .as_ref()
                    .map(|team_context| team_context.team_name.clone()),
                session_id: Some(ctx.session_id.clone()),
            });
        let spawn_result = backend
            .spawn(TeammateSpawnConfig {
                name: params.name.clone(),
                team_name: team_name.clone(),
                task_list_id,
                color: Some(color.clone()),
                plan_mode_required,
                prompt: params.prompt.clone(),
                agent_type: agent_type.clone(),
                cwd: cwd.clone(),
                model: params.model.clone(),
                system_prompt,
                system_prompt_mode,
                worktree_path: None,
                parent_session_id: ctx.session_id.clone(),
                permissions: vec![],
                allow_permission_prompts: false,
                tool_permission_context: (ctx.get_app_state)().tool_permission_context.clone(),
                hooks: app_state.hooks.clone(),
                hook_runner: Some(ctx.hook_runner.clone()),
            })
            .await?;
        if !spawn_result.success {
            let _ = helpers::set_member_active(&team_name, &agent_id, false);
            return Ok(preview_tool_result(
                json!({
                    "spawned": false,
                    "error": spawn_result.error.unwrap_or_else(|| "failed to spawn teammate".into()),
                    "team": team_name,
                    "name": params.name,
                }),
                format!(
                    "TeamSpawn failed to spawn agent {} in team {}",
                    params.name, team_name
                ),
            ));
        }
        let task_id = spawn_result.task_id.unwrap_or_default();
        let task_path = if task_id.is_empty() {
            None
        } else {
            Some(
                helpers::set_member_task(&team_name, &agent_id, &task_id)?
                    .to_string_lossy()
                    .into_owned(),
            )
        };

        // Update app_state.team_context so the session has a live view.
        let tc_team_name = team_name.clone();
        let tc_agent_id = agent_id.clone();
        let tc_agent_name = params.name.clone();
        let tc_color = color.clone();
        let tc_cwd = cwd.clone();
        let tc_task_id = task_id.clone();
        let tc_description = params.description.clone();
        let tc_freshly_created = freshly_created;
        let tc_agent_type = agent_type.clone();
        (ctx.set_app_state)(Box::new(move |mut state| {
            let tc = state.team_context.get_or_insert_with(|| TeamContext {
                team_name: tc_team_name.clone(),
                team_file_path: helpers::team_config_path(&tc_team_name)
                    .to_string_lossy()
                    .into_owned(),
                lead_agent_id: identity::lead_agent_id(&tc_team_name),
                self_agent_id: Some(identity::lead_agent_id(&tc_team_name)),
                self_agent_name: Some(constants::TEAM_LEAD_NAME.into()),
                is_leader: Some(true),
                self_agent_color: None,
                teammates: Default::default(),
            });
            // If the previous team was different (or freshly created), reset.
            if tc.team_name != tc_team_name || tc_freshly_created {
                tc.team_name = tc_team_name.clone();
                tc.team_file_path = helpers::team_config_path(&tc_team_name)
                    .to_string_lossy()
                    .into_owned();
                tc.lead_agent_id = identity::lead_agent_id(&tc_team_name);
                tc.self_agent_id = Some(tc.lead_agent_id.clone());
                tc.self_agent_name = Some(constants::TEAM_LEAD_NAME.into());
                tc.is_leader = Some(true);
                tc.teammates.clear();
            }
            tc.teammates.insert(
                tc_agent_id.clone(),
                TeammateInfo {
                    name: tc_agent_name.clone(),
                    agent_type: tc_agent_type.clone(),
                    color: Some(tc_color.clone()),
                    tmux_session_name: String::new(),
                    tmux_pane_id: String::new(),
                    cwd: tc_cwd.clone(),
                    worktree_path: None,
                    spawned_at: now,
                },
            );
            // Stash task id alongside so later /team kill can find it.
            let _ = tc_task_id;
            let _ = tc_description;
            state
        }));

        info!(
            team = %team_name,
            agent_id = %agent_id,
            task_id = %task_id,
            "teammate spawned via TeamSpawn"
        );

        Ok(preview_tool_result(
            json!({
                "spawned": true,
                "team": team_name,
                "agent_id": agent_id,
                "task_id": task_id,
                "task_path": task_path,
                "name": params.name,
                "color": color,
                "backend": backend_type.to_string(),
                "plan_mode_required": plan_mode_required,
                "agent_type": agent_type,
                "implicitly_created_team": freshly_created,
            }),
            format!(
                "Spawned agent {} in team {}; task_id={}",
                params.name,
                team_name,
                if task_id.is_empty() {
                    "<none>"
                } else {
                    task_id.as_str()
                }
            ),
        ))
    }

    async fn prompt(&self) -> String {
        team_tool_specs::team_spawn_prompt()
    }

    fn user_facing_name(&self, input: Option<&Value>) -> String {
        team_tool_specs::team_spawn_user_facing_name(input)
    }
}

#[async_trait]
impl Tool for SpawnAgentAliasTool {
    fn name(&self) -> &str {
        "spawn_agent"
    }

    async fn description(&self, input: &Value) -> String {
        TeamSpawnTool.description(input).await
    }

    fn input_json_schema(&self) -> Value {
        TeamSpawnTool.input_json_schema()
    }

    fn is_enabled(&self) -> bool {
        TeamSpawnTool.is_enabled()
    }

    async fn validate_input(&self, input: &Value, ctx: &ToolUseContext) -> ValidationResult {
        TeamSpawnTool.validate_input(input, ctx).await
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolUseContext,
        parent: &AssistantMessage,
        on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        TeamSpawnTool.call(input, ctx, parent, on_progress).await
    }

    async fn prompt(&self) -> String {
        TeamSpawnTool.prompt().await
    }

    fn user_facing_name(&self, input: Option<&Value>) -> String {
        TeamSpawnTool.user_facing_name(input)
    }
}

fn team_spawn_plan_mode_required(mode: Option<&str>) -> bool {
    team_tool_specs::team_spawn_plan_mode_required(mode)
}

fn resolve_team_spawn_agent_type(explicit: Option<&str>) -> Option<String> {
    team_tool_specs::resolve_team_spawn_agent_type(
        explicit,
        crate::coordinator::default_teammate_agent_type(),
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_config::features::{self, FeatureFlags};
    use std::sync::Arc;

    struct FeatureOverrideGuard;

    impl Drop for FeatureOverrideGuard {
        fn drop(&mut self) {
            features::clear_runtime_override();
        }
    }

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    #[test]
    fn input_json_schema_requires_name_and_prompt() {
        let tool = TeamSpawnTool;
        let schema = tool.input_json_schema();
        let required = schema.get("required").and_then(|v| v.as_array()).unwrap();
        let names: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();
        assert!(names.contains(&"name"));
        assert!(names.contains(&"prompt"));
    }

    #[test]
    #[serial_test::serial]
    fn tool_is_enabled_when_agent_teams_is_enabled() {
        let _guard = FeatureOverrideGuard;
        let mut flags = FeatureFlags::all_disabled();
        flags.agent_teams = true;
        features::set_runtime_override(flags);

        assert!(TeamSpawnTool.is_enabled());
    }

    #[test]
    #[serial_test::serial]
    fn tool_is_hidden_when_agent_teams_and_coordinator_are_disabled() {
        let _guard = FeatureOverrideGuard;
        features::set_runtime_override(FeatureFlags::all_disabled());

        assert!(!TeamSpawnTool.is_enabled());
        assert!(!SpawnAgentAliasTool.is_enabled());
    }

    #[test]
    #[serial_test::serial]
    fn agent_teams_feature_enables_tooling() {
        let _guard = FeatureOverrideGuard;
        let mut flags = FeatureFlags::all_disabled();
        flags.agent_teams = true;
        features::set_runtime_override(flags);

        assert!(TeamSpawnTool.is_enabled());
        assert!(SpawnAgentAliasTool.is_enabled());
    }

    #[test]
    #[serial_test::serial]
    fn coordinator_feature_enables_tooling() {
        let _guard = FeatureOverrideGuard;
        let mut flags = FeatureFlags::all_disabled();
        flags.coordinator = true;
        features::set_runtime_override(flags);

        assert!(TeamSpawnTool.is_enabled());
        assert!(SpawnAgentAliasTool.is_enabled());
    }

    #[test]
    fn tool_name_is_team_spawn() {
        assert_eq!(TeamSpawnTool.name(), "TeamSpawn");
    }

    #[test]
    fn input_json_schema_exposes_plan_mode() {
        let schema = TeamSpawnTool.input_json_schema();
        let mode_enum = schema["properties"]["mode"]["enum"].as_array().unwrap();
        let variants: Vec<&str> = mode_enum.iter().filter_map(|v| v.as_str()).collect();
        assert!(variants.contains(&"plan"));
    }

    #[test]
    fn plan_mode_flag_only_accepts_plan_mode() {
        assert!(team_spawn_plan_mode_required(Some("plan")));
        assert!(team_spawn_plan_mode_required(Some("read-only")));
        assert!(!team_spawn_plan_mode_required(None));
        assert!(!team_spawn_plan_mode_required(Some("acceptEdits")));
    }

    #[test]
    #[serial_test::serial]
    fn agent_type_defaults_to_worker_only_in_coordinator_mode() {
        let _guard = FeatureOverrideGuard;
        features::set_runtime_override(FeatureFlags::all_disabled());
        assert_eq!(
            resolve_team_spawn_agent_type(None),
            Some("teammate".to_string())
        );
        assert_eq!(
            resolve_team_spawn_agent_type(Some(" reviewer ")),
            Some("reviewer".to_string())
        );

        let mut flags = FeatureFlags::all_disabled();
        flags.coordinator = true;
        features::set_runtime_override(flags);
        assert_eq!(
            resolve_team_spawn_agent_type(None),
            Some("worker".to_string())
        );
    }

    #[tokio::test]
    async fn validate_rejects_unsupported_backend() {
        let tool = TeamSpawnTool;
        let ctx = create_test_context();
        let input = json!({
            "name": "worker",
            "prompt": "Investigate the issue",
            "backend": "tmux",
        });

        match tool.validate_input(&input, &ctx).await {
            ValidationResult::Error {
                message,
                error_code,
            } => {
                assert_eq!(error_code, 400);
                assert!(message.contains("tmux"));
                assert!(message.contains("not supported"));
                assert!(message.contains("in-process"));
            }
            other => panic!("expected unsupported backend error, got {other:?}"),
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn duplicate_teammate_returns_display_preview() {
        let tmp = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", tmp.path().to_str().unwrap());
        let team = helpers::create_team("team-spawn-preview", None, Some("session".into()), ".")
            .expect("create team");
        let agent_id = identity::format_agent_id("worker", &team.name);
        helpers::add_member(
            &team.name,
            TeamMember {
                agent_id,
                name: "worker".to_string(),
                agent_type: Some("teammate".to_string()),
                model: None,
                prompt: Some("existing worker".to_string()),
                color: Some("blue".to_string()),
                plan_mode_required: None,
                joined_at: chrono::Utc::now().timestamp(),
                tmux_pane_id: String::new(),
                cwd: ".".to_string(),
                worktree_path: None,
                session_id: None,
                task_id: None,
                task_path: None,
                subscriptions: vec![],
                backend_type: Some(BackendType::InProcess),
                is_active: Some(true),
                mode: None,
                close_state: None,
                close_requested_at: None,
            },
        )
        .expect("add member");

        let result = TeamSpawnTool
            .call(
                json!({
                    "team": team.name,
                    "name": "worker",
                    "prompt": "duplicate worker"
                }),
                &create_test_context(),
                &AssistantMessage {
                    uuid: uuid::Uuid::new_v4(),
                    timestamp: 0,
                    role: "assistant".to_string(),
                    content: vec![],
                    usage: None,
                    stop_reason: None,
                    is_api_error_message: false,
                    api_error: None,
                    cost_usd: 0.0,
                },
                None,
            )
            .await
            .unwrap();

        assert!(result.data["error"]
            .as_str()
            .unwrap()
            .contains("already exists"));
        assert!(result
            .display_preview
            .as_deref()
            .unwrap_or("")
            .contains("TeamSpawn failed"));
    }

    fn create_test_context() -> ToolUseContext {
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
            read_file_state: FileStateCache::default(),
            get_app_state: Arc::new(ToolAppState::default),
            set_app_state: Arc::new(|_| {}),
            session_id: "test-session".to_string(),
            langfuse_session_id: "test-session".to_string(),
            messages: vec![],
            agent_id: None,
            agent_type: None,
            query_tracking: None,
            permission_callback: None,
            permission_event_callback: None,
            ask_user_callback: None,
            bg_agent_tx: None,
            hook_runner: Arc::new(allthecodes_types::hooks::NoopHookRunner::new()),
            command_dispatcher: Arc::new(allthecodes_types::commands::NoopCommandDispatcher::new()),
            available_tools: vec![],
            execute_deferred_tool: None,
        }
    }
}
