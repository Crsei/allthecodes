use anyhow::{bail, Result};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

use allthecodes_tools::tool::*;
use allthecodes_types::message::AssistantMessage;

use crate::backend::TeammateExecutor;
use crate::constants::TEAM_LEAD_NAME;
use crate::helpers;
use crate::in_process::InProcessBackend;
use crate::types::{BackendType, TeamContext};

pub struct TeamCreateTool;
pub struct TeamDeleteTool;

#[derive(Debug, Deserialize)]
struct TeamCreateInput {
    name: String,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TeamDeleteInput {
    name: String,
}

#[async_trait]
impl Tool for TeamCreateTool {
    fn name(&self) -> &str {
        "TeamCreate"
    }

    async fn description(&self, _input: &Value) -> String {
        "Create an Agent Teams team and make it active for the current session.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Name for the team to create."
                },
                "description": {
                    "type": "string",
                    "description": "Optional short team description."
                }
            },
            "required": ["name"],
            "additionalProperties": false
        })
    }

    fn is_enabled(&self) -> bool {
        crate::teams_tooling_enabled()
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        match serde_json::from_value::<TeamCreateInput>(input.clone()) {
            Ok(params) if !params.name.trim().is_empty() => ValidationResult::Ok,
            Ok(_) => ValidationResult::Error {
                message: "TeamCreate requires a non-empty name".to_string(),
                error_code: 400,
            },
            Err(err) => ValidationResult::Error {
                message: err.to_string(),
                error_code: 400,
            },
        }
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let params: TeamCreateInput = serde_json::from_value(input)?;
        let name = params.name.trim();
        if name.is_empty() {
            bail!("TeamCreate requires a non-empty name");
        }
        if let Some(active_team_name) =
            (ctx.get_app_state)().team_context.and_then(|team_context| {
                let team_name = team_context.team_name.trim().to_string();
                (!team_name.is_empty()).then_some(team_name)
            })
        {
            bail!(
                "TeamCreate cannot create team '{}' while team '{}' is already active",
                name,
                active_team_name
            );
        }

        let description = params
            .description
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let cwd = std::env::current_dir()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|_| ".".to_string());
        let team_file = helpers::create_team(
            name,
            description.clone(),
            Some(ctx.session_id.clone()),
            &cwd,
        )?;
        let team_name = team_file.name.clone();
        let team_file_path = helpers::team_config_path(&team_name)
            .to_string_lossy()
            .into_owned();
        let team_context = TeamContext {
            team_name: team_name.clone(),
            team_file_path: team_file_path.clone(),
            lead_agent_id: team_file.lead_agent_id.clone(),
            self_agent_id: Some(team_file.lead_agent_id.clone()),
            self_agent_name: Some(TEAM_LEAD_NAME.to_string()),
            is_leader: Some(true),
            self_agent_color: None,
            teammates: Default::default(),
        };
        (ctx.set_app_state)(Box::new(move |mut state| {
            state.team_context = Some(team_context);
            state
        }));

        Ok(ToolResult {
            data: json!({
                "created": true,
                "team_name": team_name,
                "description": description,
                "lead_agent_id": team_file.lead_agent_id,
                "member_count": team_file.members.len(),
                "team_file_path": team_file_path,
            }),
            model_content: None,
            display_preview: Some(format!("Team '{}' created", team_file.name)),
            new_messages: vec![],
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "Create an Agent Teams team.".to_string()
    }
}

#[async_trait]
impl Tool for TeamDeleteTool {
    fn name(&self) -> &str {
        "TeamDelete"
    }

    async fn description(&self, _input: &Value) -> String {
        "Delete an Agent Teams team and clean up its persisted state.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Name of the team to delete."
                }
            },
            "required": ["name"],
            "additionalProperties": false
        })
    }

    fn is_enabled(&self) -> bool {
        crate::teams_tooling_enabled()
    }

    fn is_destructive(&self, _input: &Value) -> bool {
        true
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        match serde_json::from_value::<TeamDeleteInput>(input.clone()) {
            Ok(params) if !params.name.trim().is_empty() => ValidationResult::Ok,
            Ok(_) => ValidationResult::Error {
                message: "TeamDelete requires a non-empty name".to_string(),
                error_code: 400,
            },
            Err(err) => ValidationResult::Error {
                message: err.to_string(),
                error_code: 400,
            },
        }
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let params: TeamDeleteInput = serde_json::from_value(input)?;
        let team_name = params.name.trim().to_string();
        if team_name.is_empty() {
            bail!("TeamDelete requires a non-empty name");
        }
        if !helpers::team_exists(&team_name) {
            bail!("Team '{}' does not exist", team_name);
        }

        let team_file = helpers::read_team_file(&team_name)?;
        let removed_members: Vec<String> = helpers::get_non_lead_members(&team_file)
            .into_iter()
            .map(|member| member.name.clone())
            .collect();
        let backend = InProcessBackend::new();
        let mut unassigned_messages = Vec::new();
        for member in helpers::get_non_lead_members(&team_file) {
            if member.backend_type.unwrap_or(BackendType::InProcess) == BackendType::InProcess {
                let _ = backend.kill(&member.agent_id).await;
                let unassigned = allthecodes_engine::agent_runtime::unassign_teammate_tasks(
                    &team_name,
                    &member.agent_id,
                    &member.name,
                    allthecodes_tasks::TeammateTaskExitReason::Terminated,
                );
                if !unassigned.unassigned_tasks.is_empty() {
                    unassigned_messages.push(unassigned.notification_message);
                }
            }
        }

        helpers::cleanup_team_directories(&team_name)?;
        let deleted_team_name = team_name.clone();
        (ctx.set_app_state)(Box::new(move |mut state| {
            if state
                .team_context
                .as_ref()
                .is_some_and(|team_context| team_context.team_name == deleted_team_name)
            {
                state.team_context = None;
            }
            state
        }));

        Ok(ToolResult {
            data: json!({
                "deleted": true,
                "team_name": team_name,
                "removed_members": removed_members,
                "unassigned_task_messages": unassigned_messages,
            }),
            model_content: None,
            display_preview: Some("Team deleted".to_string()),
            new_messages: vec![],
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "Delete an Agent Teams team.".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_config::features::{self, FeatureFlags};
    use allthecodes_types::message::AssistantMessage;
    use serde_json::json;
    use std::sync::{Arc, Mutex};

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
    #[serial_test::serial]
    fn team_tools_are_hidden_when_agent_teams_and_coordinator_are_disabled() {
        let _guard = FeatureOverrideGuard;
        features::set_runtime_override(FeatureFlags::all_disabled());

        assert!(!TeamCreateTool.is_enabled());
        assert!(!TeamDeleteTool.is_enabled());
    }

    #[test]
    #[serial_test::serial]
    fn agent_teams_feature_enables_team_tools() {
        let _guard = FeatureOverrideGuard;
        let mut flags = FeatureFlags::all_disabled();
        flags.agent_teams = true;
        features::set_runtime_override(flags);

        assert!(TeamCreateTool.is_enabled());
        assert!(TeamDeleteTool.is_enabled());
    }

    #[test]
    #[serial_test::serial]
    fn coordinator_feature_enables_team_tools() {
        let _guard = FeatureOverrideGuard;
        let mut flags = FeatureFlags::all_disabled();
        flags.coordinator = true;
        features::set_runtime_override(flags);

        assert!(TeamCreateTool.is_enabled());
        assert!(TeamDeleteTool.is_enabled());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn team_create_returns_created_team_json() {
        let tmp = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", tmp.path().to_str().unwrap());

        let result = TeamCreateTool
            .call(
                json!({"name": "model-team", "description": "from model"}),
                &create_test_context(),
                &dummy_parent(),
                None,
            )
            .await
            .unwrap();

        assert_eq!(result.data["created"], true);
        assert_eq!(result.data["team_name"], "model-team");
        assert_eq!(result.data["description"], "from model");
        assert_eq!(result.data["member_count"], 1);
        assert!(result.data["team_file_path"]
            .as_str()
            .unwrap()
            .contains("model-team"));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn team_create_rejects_when_team_context_is_active() {
        let tmp = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", tmp.path().to_str().unwrap());

        let app_state = ToolAppState {
            team_context: Some(active_team_context("existing-team")),
            ..Default::default()
        };

        let err = TeamCreateTool
            .call(
                json!({"name": "second-team"}),
                &create_test_context_with_app_state(app_state),
                &dummy_parent(),
                None,
            )
            .await
            .unwrap_err();

        assert!(
            err.to_string().contains(
                "TeamCreate cannot create team 'second-team' while team 'existing-team' is already active"
            ),
            "unexpected error: {err}"
        );
        assert!(!helpers::team_exists("second-team"));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn team_delete_returns_removed_member_names() {
        let tmp = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", tmp.path().to_str().unwrap());
        let team =
            crate::helpers::create_team("delete-model-team", None, Some("session".into()), ".")
                .expect("create team");
        crate::helpers::add_member(
            &team.name,
            crate::types::TeamMember {
                agent_id: crate::identity::format_agent_id("worker", &team.name),
                name: "worker".to_string(),
                agent_type: Some("teammate".to_string()),
                model: None,
                prompt: Some("work".to_string()),
                color: None,
                plan_mode_required: None,
                joined_at: chrono::Utc::now().timestamp(),
                tmux_pane_id: String::new(),
                cwd: ".".to_string(),
                worktree_path: None,
                session_id: None,
                task_id: None,
                task_path: None,
                subscriptions: vec![],
                backend_type: Some(crate::types::BackendType::InProcess),
                is_active: Some(true),
                mode: None,
                close_state: None,
                close_requested_at: None,
            },
        )
        .expect("add member");

        let result = TeamDeleteTool
            .call(
                json!({"name": team.name}),
                &create_test_context(),
                &dummy_parent(),
                None,
            )
            .await
            .unwrap();

        assert_eq!(result.data["deleted"], true);
        assert_eq!(result.data["team_name"], "delete-model-team");
        assert_eq!(result.data["removed_members"], json!(["worker"]));
        assert!(!helpers::team_exists("delete-model-team"));
    }

    #[test]
    fn team_delete_is_destructive() {
        assert!(TeamDeleteTool.is_destructive(&json!({"name": "any-team"})));
    }

    fn create_test_context() -> ToolUseContext {
        create_test_context_with_app_state(ToolAppState::default())
    }

    fn create_test_context_with_app_state(app_state: ToolAppState) -> ToolUseContext {
        let (_tx, rx) = tokio::sync::watch::channel(false);
        let shared_state = Arc::new(Mutex::new(app_state));
        let get_app_state = {
            let shared_state = Arc::clone(&shared_state);
            Arc::new(move || shared_state.lock().unwrap().clone())
        };
        let set_app_state = {
            let shared_state = Arc::clone(&shared_state);
            Arc::new(move |updater: AppStateUpdater| {
                let mut state = shared_state.lock().unwrap();
                let current = state.clone();
                *state = updater(current);
            })
        };

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
            get_app_state,
            set_app_state,
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

    fn active_team_context(team_name: &str) -> TeamContext {
        TeamContext {
            team_name: team_name.to_string(),
            team_file_path: format!("/tmp/{team_name}/config.json"),
            lead_agent_id: format!("team-lead@{team_name}"),
            self_agent_id: Some(format!("team-lead@{team_name}")),
            self_agent_name: Some(TEAM_LEAD_NAME.to_string()),
            is_leader: Some(true),
            self_agent_color: None,
            teammates: Default::default(),
        }
    }

    fn dummy_parent() -> AssistantMessage {
        AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 0,
            role: "assistant".to_string(),
            content: vec![],
            usage: None,
            stop_reason: None,
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        }
    }
}
