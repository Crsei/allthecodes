use std::sync::Arc;

use crate::command_runtime::{CommandContext, CommandExecutor, CommandResult};
use crate::lifecycle::*;
use crate::runtime_services::{
    CommandDispatcherService, HookRunnerService, ModelClientFactoryService,
    PermissionMessageResolver, RuntimeServices, ToolRegistryService,
};
use crate::types::config::{AgentContext, QueryEngineConfig, QuerySource};
use crate::types::message::{
    AssistantMessage, ContentBlock, Message, MessageContent, Usage, UserMessage,
};
use crate::types::tool::{PermissionMode, PermissionResult, ToolResult};
use allthecodes_types::agent_runtime_record::AgentRuntimePermissionDecision;
use allthecodes_types::callbacks::{PermissionCallback, PermissionResponsePayload};
use allthecodes_types::hooks::{
    HookEventConfig, HookOutput, HookRunner, HooksMap, PermissionOverride, PostToolHookResult,
    PreToolHookResult,
};
use allthecodes_types::sdk::*;
use serde_json::{json, Value};
use tempfile::tempdir;

struct EnvGuard {
    key: &'static str,
    previous: Option<String>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &std::path::Path) -> Self {
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

struct TestCommandDispatcher;

impl allthecodes_types::commands::CommandDispatcher for TestCommandDispatcher {
    fn parse_command_input(
        &self,
        input: &str,
    ) -> Option<allthecodes_types::commands::ParsedCommand> {
        let trimmed = input.trim();
        if trimmed == "/clear" {
            return Some(allthecodes_types::commands::ParsedCommand {
                index: 0,
                args: String::new(),
            });
        }
        if let Some(rest) = trimmed.strip_prefix("/help") {
            return Some(allthecodes_types::commands::ParsedCommand {
                index: 1,
                args: rest.trim().to_string(),
            });
        }
        if let Some(rest) = trimmed.strip_prefix("/review") {
            return Some(allthecodes_types::commands::ParsedCommand {
                index: 2,
                args: rest.trim().to_string(),
            });
        }
        None
    }

    fn command_name(&self, index: usize) -> Option<String> {
        match index {
            0 => Some("clear".to_string()),
            1 => Some("help".to_string()),
            2 => Some("review".to_string()),
            _ => None,
        }
    }
}

struct TestCommandExecutor;

#[async_trait::async_trait]
impl CommandExecutor for TestCommandExecutor {
    async fn execute(
        &self,
        parsed: allthecodes_types::commands::ParsedCommand,
        _command_name: String,
        ctx: &mut CommandContext,
    ) -> anyhow::Result<CommandResult> {
        Ok(match parsed.index {
            0 => CommandResult::Clear,
            1 => CommandResult::Output("See /clear to clear the conversation".to_string()),
            2 => {
                ctx.messages.push(Message::User(UserMessage {
                    uuid: uuid::Uuid::new_v4(),
                    timestamp: 1,
                    role: "user".into(),
                    content: MessageContent::Text(format!("Review pull request `{}`", parsed.args)),
                    is_meta: false,
                    tool_use_result: None,
                    source_tool_assistant_uuid: None,
                }));
                CommandResult::Query(ctx.messages.clone())
            }
            _ => CommandResult::None,
        })
    }
}

struct TestTool;

#[async_trait::async_trait]
impl crate::types::tool::Tool for TestTool {
    fn name(&self) -> &str {
        "TestTool"
    }

    async fn description(&self, _input: &serde_json::Value) -> String {
        "test tool".to_string()
    }

    fn input_json_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object"})
    }

    async fn call(
        &self,
        _input: serde_json::Value,
        _ctx: &crate::types::tool::ToolUseContext,
        _parent_message: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(crate::types::tool::ToolProgress) + Send + Sync>>,
    ) -> anyhow::Result<crate::types::tool::ToolResult> {
        Ok(crate::types::tool::ToolResult::default())
    }

    async fn prompt(&self) -> String {
        String::new()
    }
}

struct NamedServiceTool(&'static str);

#[async_trait::async_trait]
impl crate::types::tool::Tool for NamedServiceTool {
    fn name(&self) -> &str {
        self.0
    }

    async fn description(&self, _input: &serde_json::Value) -> String {
        format!("{} service tool", self.0)
    }

    fn input_json_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object"})
    }

    async fn call(
        &self,
        _input: serde_json::Value,
        _ctx: &crate::types::tool::ToolUseContext,
        _parent_message: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(crate::types::tool::ToolProgress) + Send + Sync>>,
    ) -> anyhow::Result<crate::types::tool::ToolResult> {
        Ok(crate::types::tool::ToolResult::default())
    }

    async fn prompt(&self) -> String {
        String::new()
    }
}

struct TestRuntimeToolRegistry {
    tools: crate::types::tool::Tools,
}

impl ToolRegistryService for TestRuntimeToolRegistry {
    fn active_tools(&self) -> crate::types::tool::Tools {
        self.tools.clone()
    }
}

struct TestPermissionMessageResolver {
    message: &'static str,
}

impl PermissionMessageResolver for TestPermissionMessageResolver {
    fn resolve_permission_message(&self, _tool_name: &str) -> Option<String> {
        Some(self.message.to_string())
    }
}

struct TestNoPermissionMessageResolver;

impl PermissionMessageResolver for TestNoPermissionMessageResolver {
    fn resolve_permission_message(&self, _tool_name: &str) -> Option<String> {
        None
    }
}

struct TestHookRunnerService;

impl HookRunnerService for TestHookRunnerService {
    fn hook_runner(&self) -> Arc<dyn HookRunner> {
        Arc::new(allthecodes_types::hooks::NoopHookRunner::new())
    }
}

struct TestCommandDispatcherService;

impl CommandDispatcherService for TestCommandDispatcherService {
    fn command_dispatcher(&self) -> Arc<dyn allthecodes_types::commands::CommandDispatcher> {
        Arc::new(allthecodes_types::commands::NoopCommandDispatcher::new())
    }
}

struct TestModelClientFactoryService;

impl ModelClientFactoryService for TestModelClientFactoryService {
    fn client_for_backend(
        &self,
        _backend_name: Option<&str>,
    ) -> Option<Arc<allthecodes_api::api::client::ApiClient>> {
        None
    }
}

fn test_runtime_services(
    tool_name: &'static str,
    permission_message: &'static str,
) -> Arc<RuntimeServices> {
    Arc::new(RuntimeServices {
        tool_registry: Arc::new(TestRuntimeToolRegistry {
            tools: vec![Arc::new(NamedServiceTool(tool_name))],
        }),
        permission_message_resolver: Arc::new(TestPermissionMessageResolver {
            message: permission_message,
        }),
        hook_runner: Arc::new(TestHookRunnerService),
        command_dispatcher: Arc::new(TestCommandDispatcherService),
        model_client_factory: Arc::new(TestModelClientFactoryService),
    })
}

fn test_runtime_services_without_permission_message(
    tool_name: &'static str,
) -> Arc<RuntimeServices> {
    Arc::new(RuntimeServices {
        tool_registry: Arc::new(TestRuntimeToolRegistry {
            tools: vec![Arc::new(NamedServiceTool(tool_name))],
        }),
        permission_message_resolver: Arc::new(TestNoPermissionMessageResolver),
        hook_runner: Arc::new(TestHookRunnerService),
        command_dispatcher: Arc::new(TestCommandDispatcherService),
        model_client_factory: Arc::new(TestModelClientFactoryService),
    })
}

struct DeferredTargetTool {
    name: &'static str,
    deny: bool,
}

#[derive(Clone, Copy)]
enum MatrixToolPermission {
    Allow,
    Ask,
}

struct PermissionMatrixTool {
    permission: MatrixToolPermission,
}

#[async_trait::async_trait]
impl crate::types::tool::Tool for PermissionMatrixTool {
    fn name(&self) -> &str {
        "PermissionMatrix"
    }

    async fn description(&self, _input: &Value) -> String {
        "permission matrix tool".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({"type": "object"})
    }

    async fn check_permissions(
        &self,
        input: &Value,
        _ctx: &crate::types::tool::ToolUseContext,
    ) -> PermissionResult {
        match self.permission {
            MatrixToolPermission::Allow => PermissionResult::Allow {
                updated_input: input.clone(),
            },
            MatrixToolPermission::Ask => PermissionResult::Ask {
                message: "Allow PermissionMatrix?".to_string(),
            },
        }
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &crate::types::tool::ToolUseContext,
        _parent_message: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(crate::types::tool::ToolProgress) + Send + Sync>>,
    ) -> anyhow::Result<ToolResult> {
        Ok(ToolResult {
            data: json!({ "ok": true, "input": input }),
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        String::new()
    }
}

#[derive(Default)]
struct PermissionMatrixHookRunner {
    pre_override: Option<PermissionOverride>,
}

#[async_trait::async_trait]
impl HookRunner for PermissionMatrixHookRunner {
    fn load_hook_configs(&self, _hooks_value: &HooksMap, event_name: &str) -> Vec<HookEventConfig> {
        if event_name == "PreToolUse" && self.pre_override.is_some() {
            vec![HookEventConfig {
                matcher: Some("*".to_string()),
                critical: false,
                hooks: vec![],
            }]
        } else {
            vec![]
        }
    }

    async fn run_pre_tool_hooks(
        &self,
        _tool_name: &str,
        _input: &Value,
        _hook_configs: &[HookEventConfig],
    ) -> anyhow::Result<PreToolHookResult> {
        Ok(PreToolHookResult::Continue {
            updated_input: None,
            permission_override: self.pre_override.clone(),
        })
    }

    async fn run_post_tool_hooks(
        &self,
        _tool_name: &str,
        _input: &Value,
        _tool_result_data: &Value,
        _hook_configs: &[HookEventConfig],
    ) -> anyhow::Result<PostToolHookResult> {
        Ok(PostToolHookResult::Continue)
    }

    async fn run_post_tool_failure_hooks(
        &self,
        _tool_name: &str,
        _input: &Value,
        _error: &str,
        _hook_configs: &[HookEventConfig],
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn run_event_hooks(
        &self,
        _event_name: &str,
        _payload: &Value,
        _hook_configs: &[HookEventConfig],
    ) -> anyhow::Result<HookOutput> {
        Ok(HookOutput::default())
    }

    async fn run_stop_hooks(
        &self,
        _hook_configs: &[HookEventConfig],
    ) -> anyhow::Result<PostToolHookResult> {
        Ok(PostToolHookResult::Continue)
    }
}

#[async_trait::async_trait]
impl crate::types::tool::Tool for DeferredTargetTool {
    fn name(&self) -> &str {
        self.name
    }

    async fn description(&self, _input: &Value) -> String {
        format!("{} deferred test tool", self.name)
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "value": {"type": "integer"}
            }
        })
    }

    async fn check_permissions(
        &self,
        input: &Value,
        _ctx: &crate::types::tool::ToolUseContext,
    ) -> PermissionResult {
        if self.deny {
            PermissionResult::Deny {
                message: "blocked target".to_string(),
            }
        } else {
            PermissionResult::Allow {
                updated_input: input.clone(),
            }
        }
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &crate::types::tool::ToolUseContext,
        _parent_message: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(crate::types::tool::ToolProgress) + Send + Sync>>,
    ) -> anyhow::Result<ToolResult> {
        Ok(ToolResult {
            data: json!({
                "target": self.name,
                "input": input,
            }),
            display_preview: Some(format!("{} executed", self.name)),
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        format!("{} deferred prompt", self.name)
    }
}

#[derive(Default)]
struct RecordingToolHookRunner {
    pre_tool_names: parking_lot::Mutex<Vec<String>>,
    post_tool_names: parking_lot::Mutex<Vec<String>>,
}

#[async_trait::async_trait]
impl HookRunner for RecordingToolHookRunner {
    fn load_hook_configs(&self, _hooks_value: &HooksMap, event_name: &str) -> Vec<HookEventConfig> {
        if matches!(
            event_name,
            "PreToolUse" | "PostToolUse" | "PostToolUseFailure"
        ) {
            vec![HookEventConfig {
                matcher: Some("*".to_string()),
                critical: false,
                hooks: vec![],
            }]
        } else {
            vec![]
        }
    }

    async fn run_pre_tool_hooks(
        &self,
        tool_name: &str,
        _input: &Value,
        _hook_configs: &[HookEventConfig],
    ) -> anyhow::Result<PreToolHookResult> {
        self.pre_tool_names.lock().push(tool_name.to_string());
        Ok(PreToolHookResult::Continue {
            updated_input: None,
            permission_override: None,
        })
    }

    async fn run_post_tool_hooks(
        &self,
        tool_name: &str,
        _input: &Value,
        _tool_result_data: &Value,
        _hook_configs: &[HookEventConfig],
    ) -> anyhow::Result<PostToolHookResult> {
        self.post_tool_names.lock().push(tool_name.to_string());
        Ok(PostToolHookResult::Continue)
    }

    async fn run_post_tool_failure_hooks(
        &self,
        tool_name: &str,
        _input: &Value,
        _error: &str,
        _hook_configs: &[HookEventConfig],
    ) -> anyhow::Result<()> {
        self.post_tool_names
            .lock()
            .push(format!("{tool_name}:failure"));
        Ok(())
    }

    async fn run_event_hooks(
        &self,
        _event_name: &str,
        _payload: &Value,
        _hook_configs: &[HookEventConfig],
    ) -> anyhow::Result<HookOutput> {
        Ok(HookOutput::default())
    }

    async fn run_stop_hooks(
        &self,
        _hook_configs: &[HookEventConfig],
    ) -> anyhow::Result<PostToolHookResult> {
        Ok(PostToolHookResult::Continue)
    }
}

fn make_config() -> QueryEngineConfig {
    QueryEngineConfig {
        cwd: "/tmp".to_string(),
        tools: vec![],
        custom_system_prompt: None,
        append_system_prompt: None,
        user_specified_model: None,
        fallback_model: None,
        max_turns: None,
        max_budget_usd: None,
        task_budget: None,
        verbose: false,
        initial_messages: None,
        commands: vec![],
        thinking_config: None,
        json_schema: None,
        replay_user_messages: false,
        persist_session: false,
        resolved_model: None,
        auto_save_session: false,
        agent_context: None,
    }
}

fn permission_callback(decision: &'static str) -> PermissionCallback {
    Arc::new(move |_| Box::pin(async move { PermissionResponsePayload::decision(decision) }))
}

fn make_lifecycle_deps(
    engine: &QueryEngine,
    hook_runner: Arc<dyn HookRunner>,
    permission_callback: Option<PermissionCallback>,
) -> super::deps::QueryEngineDeps {
    super::deps::QueryEngineDeps {
        aborted: engine.aborted.clone(),
        state: engine.state.clone(),
        runtime_services: engine.runtime_services.clone(),
        cwd: "/tmp".to_string(),
        session_id: "permission-matrix".to_string(),
        query_source: crate::types::config::QuerySource::ReplMainThread,
        audit_ctx: crate::observability::AuditContext::noop("permission-matrix"),
        langfuse_trace: None,
        api_client: None,
        session_recorder: engine.session_recorder.clone(),
        agent_context: None,
        permission_callback,
        bg_agent_tx: None,
        permission_event_callback: None,
        tool_progress_callback: None,
        pending_bg_results: engine.pending_bg_results.clone(),
        active_steer_state: engine.active_steer_state.clone(),
        hook_runner,
        command_dispatcher: Arc::new(allthecodes_types::commands::NoopCommandDispatcher::new()),
        auto_classifier_fn: None,
        submit_overrides: crate::types::config::SubmitMessageOverrides::default(),
        submit_tools: None,
    }
}

async fn execute_permission_matrix_case<F>(
    permission: MatrixToolPermission,
    configure_permissions: F,
    hook_runner: Arc<dyn HookRunner>,
    permission_callback: Option<PermissionCallback>,
) -> crate::query::deps::ToolExecResult
where
    F: FnOnce(&mut allthecodes_types::permissions::ToolPermissionContext),
{
    let tools: crate::types::tool::Tools = vec![Arc::new(PermissionMatrixTool { permission })];
    let mut config = make_config();
    config.tools = tools.clone();
    let engine = QueryEngine::new(config);
    {
        let mut state = engine.state.write();
        configure_permissions(&mut state.app_state.tool_permission_context);
    }
    let deps = make_lifecycle_deps(&engine, hook_runner, permission_callback);
    let Message::Assistant(parent) = assistant_message("permission parent") else {
        unreachable!("assistant_message returns an assistant message");
    };

    deps.execute_tool_impl(
        crate::query::deps::ToolExecRequest {
            tool_use_id: "matrix-call".to_string(),
            tool_name: "PermissionMatrix".to_string(),
            input: json!({"value": 1}),
            langfuse_batch_span: None,
        },
        &tools,
        &parent,
        None,
    )
    .await
    .expect("execute permission matrix case")
}

#[test]
fn test_query_engine_creation() {
    let engine = QueryEngine::new(make_config());
    assert_eq!(engine.messages().len(), 0);
    assert_eq!(engine.total_turn_count(), 0);
    assert!(engine.usage().total_cost_usd == 0.0);
    assert!(!engine.session_id.as_str().is_empty());
    assert_eq!(engine.current_session_id(), engine.session_id);
}

#[test]
fn runtime_services_tool_registry_is_per_engine() {
    let engine_a =
        QueryEngine::new_with_services(make_config(), test_runtime_services("ServiceToolA", "A"));
    let engine_b =
        QueryEngine::new_with_services(make_config(), test_runtime_services("ServiceToolB", "B"));

    assert_eq!(engine_a.tool_names(), vec!["ServiceToolA".to_string()]);
    assert_eq!(engine_b.tool_names(), vec!["ServiceToolB".to_string()]);
}

#[test]
fn runtime_services_permission_resolver_is_per_engine() {
    let engine_a =
        QueryEngine::new_with_services(make_config(), test_runtime_services("ToolA", "message A"));
    let engine_b =
        QueryEngine::new_with_services(make_config(), test_runtime_services("ToolB", "message B"));

    let decision_a = super::deps::central_permission_decision_for_tool(
        "AnyTool",
        &Value::Null,
        &engine_a.app_state(),
        None,
        None,
        None,
        &engine_a.runtime_services,
    );
    let decision_b = super::deps::central_permission_decision_for_tool(
        "AnyTool",
        &Value::Null,
        &engine_b.app_state(),
        None,
        None,
        None,
        &engine_b.runtime_services,
    );

    assert_eq!(decision_a.message.as_deref(), Some("message A"));
    assert_eq!(decision_b.message.as_deref(), Some("message B"));
}

#[test]
fn runtime_services_permission_resolver_none_does_not_fall_back_to_process_callbacks() {
    allthecodes_permissions::decision::set_cu_message_callback(|tool_name| {
        (tool_name == "GlobalLeakTool").then(|| "process-global message".to_string())
    });

    let engine = QueryEngine::new_with_services(
        make_config(),
        test_runtime_services_without_permission_message("GlobalLeakTool"),
    );

    let decision = super::deps::central_permission_decision_for_tool(
        "GlobalLeakTool",
        &Value::Null,
        &engine.app_state(),
        None,
        None,
        None,
        &engine.runtime_services,
    );

    assert_eq!(
        decision.message.as_deref(),
        Some("Allow tool 'GlobalLeakTool'?")
    );
}

#[tokio::test]
async fn execution_record_permission_decision_matrix() {
    let default_hooks: Arc<dyn HookRunner> = Arc::new(PermissionMatrixHookRunner::default());

    let policy_allow = execute_permission_matrix_case(
        MatrixToolPermission::Allow,
        |ctx| {
            ctx.always_allow_rules
                .insert("policy".to_string(), vec!["PermissionMatrix".to_string()]);
        },
        default_hooks.clone(),
        None,
    )
    .await;
    assert!(!policy_allow.is_error);
    assert_eq!(
        policy_allow.permission_decision,
        Some(AgentRuntimePermissionDecision::AllowedByPolicy)
    );

    let user_allow = execute_permission_matrix_case(
        MatrixToolPermission::Ask,
        |_| {},
        default_hooks.clone(),
        Some(permission_callback("allow")),
    )
    .await;
    assert!(!user_allow.is_error);
    assert_eq!(
        user_allow.permission_decision,
        Some(AgentRuntimePermissionDecision::AllowedByUser)
    );

    let user_deny = execute_permission_matrix_case(
        MatrixToolPermission::Ask,
        |_| {},
        default_hooks.clone(),
        Some(permission_callback("deny")),
    )
    .await;
    assert!(user_deny.is_error);
    assert_eq!(
        user_deny.permission_decision,
        Some(AgentRuntimePermissionDecision::DeniedByUser)
    );

    let policy_deny = execute_permission_matrix_case(
        MatrixToolPermission::Allow,
        |ctx| {
            ctx.always_deny_rules
                .insert("policy".to_string(), vec!["PermissionMatrix".to_string()]);
        },
        default_hooks.clone(),
        None,
    )
    .await;
    assert!(policy_deny.is_error);
    assert_eq!(
        policy_deny.permission_decision,
        Some(AgentRuntimePermissionDecision::DeniedByPolicy)
    );

    let hook_allow = execute_permission_matrix_case(
        MatrixToolPermission::Allow,
        |_| {},
        Arc::new(PermissionMatrixHookRunner {
            pre_override: Some(PermissionOverride::Allow),
        }),
        None,
    )
    .await;
    assert!(!hook_allow.is_error);
    assert_eq!(
        hook_allow.permission_decision,
        Some(AgentRuntimePermissionDecision::AllowedByHook)
    );

    let hook_deny = execute_permission_matrix_case(
        MatrixToolPermission::Allow,
        |_| {},
        Arc::new(PermissionMatrixHookRunner {
            pre_override: Some(PermissionOverride::Deny {
                reason: "blocked by hook".to_string(),
            }),
        }),
        None,
    )
    .await;
    assert!(hook_deny.is_error);
    assert_eq!(
        hook_deny.permission_decision,
        Some(AgentRuntimePermissionDecision::DeniedByHook)
    );

    let not_required = execute_permission_matrix_case(
        MatrixToolPermission::Allow,
        |ctx| {
            ctx.mode = PermissionMode::Bypass;
        },
        default_hooks,
        None,
    )
    .await;
    assert!(!not_required.is_error);
    assert_eq!(
        not_required.permission_decision,
        Some(AgentRuntimePermissionDecision::NotRequired)
    );
}

#[tokio::test]
#[serial_test::serial]
async fn execute_extra_tool_reenters_canonical_target_boundary() {
    allthecodes_tools::deferred_tools::clear_discovered_tools_for_tests();
    allthecodes_tools::deferred_tools::mark_discovered_tools(
        "canonical-deferred",
        ["DeferredTarget".to_string(), "DeniedTarget".to_string()],
    );

    let tools: crate::types::tool::Tools = vec![
        Arc::new(allthecodes_tools::deferred_tools::ExecuteExtraToolTool),
        Arc::new(DeferredTargetTool {
            name: "DeferredTarget",
            deny: false,
        }),
        Arc::new(DeferredTargetTool {
            name: "DeniedTarget",
            deny: true,
        }),
    ];
    let mut config = make_config();
    config.tools = tools.clone();
    let engine = QueryEngine::new(config);
    {
        let mut state = engine.state.write();
        state
            .app_state
            .tool_permission_context
            .grant_session_allow("ExecuteExtraTool");
        state
            .app_state
            .tool_permission_context
            .grant_session_allow("DeferredTarget");
        state
            .app_state
            .tool_permission_context
            .grant_session_allow("DeniedTarget");
    }
    let hook_runner = Arc::new(RecordingToolHookRunner::default());
    let deps = super::deps::QueryEngineDeps {
        aborted: engine.aborted.clone(),
        state: engine.state.clone(),
        runtime_services: engine.runtime_services.clone(),
        cwd: "/tmp".to_string(),
        session_id: "canonical-deferred".to_string(),
        query_source: crate::types::config::QuerySource::ReplMainThread,
        audit_ctx: crate::observability::AuditContext::noop("canonical-deferred"),
        langfuse_trace: None,
        api_client: None,
        session_recorder: engine.session_recorder.clone(),
        agent_context: None,
        permission_callback: None,
        bg_agent_tx: None,
        permission_event_callback: None,
        tool_progress_callback: None,
        pending_bg_results: engine.pending_bg_results.clone(),
        active_steer_state: engine.active_steer_state.clone(),
        hook_runner: hook_runner.clone(),
        command_dispatcher: Arc::new(allthecodes_types::commands::NoopCommandDispatcher::new()),
        auto_classifier_fn: None,
        submit_overrides: crate::types::config::SubmitMessageOverrides::default(),
        submit_tools: None,
    };
    let Message::Assistant(parent) = assistant_message("tool parent") else {
        unreachable!("assistant_message returns an assistant message");
    };

    let result = deps
        .execute_tool_impl(
            crate::query::deps::ToolExecRequest {
                tool_use_id: "wrapper-call".to_string(),
                tool_name: "ExecuteExtraTool".to_string(),
                input: json!({
                    "tool_name": "DeferredTarget",
                    "params": {"value": 7}
                }),
                langfuse_batch_span: None,
            },
            &tools,
            &parent,
            None,
        )
        .await
        .unwrap();
    assert!(
        !result.is_error,
        "unexpected wrapper error: {:?}",
        result.result.data
    );
    assert_eq!(result.result.data["tool_name"], "DeferredTarget");
    assert_eq!(result.result.data["result"]["target"], "DeferredTarget");

    let pre_tool_names = hook_runner.pre_tool_names.lock().clone();
    assert!(pre_tool_names.contains(&"ExecuteExtraTool".to_string()));
    assert!(pre_tool_names.contains(&"DeferredTarget".to_string()));
    let post_tool_names = hook_runner.post_tool_names.lock().clone();
    assert!(post_tool_names.contains(&"DeferredTarget".to_string()));

    let denied = deps
        .execute_tool_impl(
            crate::query::deps::ToolExecRequest {
                tool_use_id: "wrapper-denied".to_string(),
                tool_name: "ExecuteExtraTool".to_string(),
                input: json!({
                    "tool_name": "DeniedTarget",
                    "params": {"value": 9}
                }),
                langfuse_batch_span: None,
            },
            &tools,
            &parent,
            None,
        )
        .await
        .unwrap();
    assert!(denied.is_error);
    assert!(denied
        .result
        .data
        .as_str()
        .is_some_and(|message| message.contains("blocked target")));
    assert!(hook_runner
        .pre_tool_names
        .lock()
        .contains(&"DeniedTarget".to_string()));
}

#[test]
fn test_query_engine_inherits_agent_team_context() {
    let mut config = make_config();
    config.agent_context = Some(AgentContext {
        agent_id: "researcher@alpha".to_string(),
        parent_agent_id: None,
        query_tracking: crate::types::tool::QueryChainTracking {
            chain_id: "chain-1".to_string(),
            depth: 1,
        },
        langfuse_session_id: "session-1".to_string(),
        agent_type: Some("Explore".to_string()),
        team_context: Some(allthecodes_types::teams::TeamContext {
            team_name: "alpha".to_string(),
            ..Default::default()
        }),
        tool_permission_context: None,
    });

    let engine = QueryEngine::new(config);
    let app_state = &engine.state.read().app_state;
    assert_eq!(
        app_state
            .team_context
            .as_ref()
            .map(|context| context.team_name.as_str()),
        Some("alpha")
    );
}

#[test]
fn test_query_engine_inherits_agent_permission_context() {
    let mut permission_context =
        crate::types::app_state::AppState::default().tool_permission_context;
    permission_context.mode = crate::types::tool::PermissionMode::Plan;
    permission_context.grant_session_allow("Read");

    let mut config = make_config();
    config.agent_context = Some(AgentContext {
        agent_id: "planner@alpha".to_string(),
        parent_agent_id: None,
        query_tracking: crate::types::tool::QueryChainTracking {
            chain_id: "chain-2".to_string(),
            depth: 1,
        },
        langfuse_session_id: "session-2".to_string(),
        agent_type: Some("Plan".to_string()),
        team_context: None,
        tool_permission_context: Some(permission_context),
    });

    let engine = QueryEngine::new(config);
    let app_state = &engine.state.read().app_state;
    assert_eq!(
        app_state.tool_permission_context.mode,
        crate::types::tool::PermissionMode::Plan
    );
    assert!(app_state.tool_permission_context.has_session_grant("Read"));
}

#[test]
fn test_start_new_session_rotates_active_id_and_clears_runtime_state() {
    let engine = QueryEngine::new(make_config());
    let original = engine.current_session_id();

    let next = engine.start_new_session();

    assert_ne!(next, original);
    assert_eq!(engine.current_session_id(), next);
    assert!(engine.messages().is_empty());
    let usage = engine.usage();
    assert_eq!(usage.total_input_tokens, 0);
    assert_eq!(usage.total_output_tokens, 0);
    assert_eq!(usage.total_cost_usd, 0.0);
}

#[test]
#[serial_test::serial]
fn test_start_new_session_saves_previous_messages() {
    let home = tempdir().unwrap();
    let _guard = EnvGuard::set("ALLTHECODES_HOME", home.path());
    let workspace = home.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();

    let mut config = make_config();
    config.cwd = workspace.to_string_lossy().to_string();
    config.auto_save_session = true;
    let engine = QueryEngine::new(config);
    let previous = engine.current_session_id();
    engine.replace_messages(vec![Message::User(UserMessage {
        uuid: uuid::Uuid::new_v4(),
        timestamp: 1,
        role: "user".into(),
        content: MessageContent::Text("old message".into()),
        is_meta: false,
        tool_use_result: None,
        source_tool_assistant_uuid: None,
    })]);

    let next = engine.start_new_session();

    assert_ne!(previous, next);
    assert!(engine.messages().is_empty());
    let saved = crate::session::storage::load_session(previous.as_str()).unwrap();
    assert_eq!(saved.len(), 1);
}

#[test]
fn test_query_engine_abort() {
    let engine = QueryEngine::new(make_config());
    assert!(!engine.is_aborted());
    assert!(engine.abort_reason().is_none());

    engine.abort();
    assert!(engine.is_aborted());
    assert!(matches!(
        engine.abort_reason(),
        Some(AbortReason::UserAbort)
    ));

    engine.reset_abort();
    assert!(!engine.is_aborted());
    assert!(engine.abort_reason().is_none());
}

#[test]
#[serial_test::serial]
fn test_query_engine_abort_pauses_active_goal() {
    let temp = tempdir().unwrap();
    let _home = EnvGuard::set("ALLTHECODES_HOME", temp.path());
    let engine = QueryEngine::new(make_config());
    let session_id = engine.current_session_id();
    let goal =
        allthecodes_tools::goals::create_goal_record("ship", None, chrono::Utc::now()).unwrap();
    allthecodes_tools::goals::save_goal_for_session(session_id.as_str(), &goal).unwrap();

    engine.abort();

    let stored = allthecodes_tools::goals::load_goal_for_session(session_id.as_str())
        .unwrap()
        .unwrap();
    assert_eq!(stored.status, allthecodes_tools::goals::GoalStatus::Paused);
    assert_eq!(
        stored.status_reason.as_deref(),
        Some("task aborted by user")
    );
    assert!(engine
        .state
        .read()
        .runtime
        .goal_runtime
        .active_goal_id
        .is_none());
}

#[test]
fn test_query_engine_sleep_control() {
    let engine = QueryEngine::new(make_config());
    assert!(!engine.is_sleeping());

    engine.set_sleep_until(std::time::Instant::now() + std::time::Duration::from_secs(60));
    assert!(engine.is_sleeping());

    engine.wake_up();
    assert!(!engine.is_sleeping());
}

#[test]
fn test_query_engine_app_state() {
    let engine = QueryEngine::new(make_config());
    let state = engine.app_state();
    assert!(!state.verbose);

    engine.update_app_state(|s| {
        s.verbose = true;
    });

    let state = engine.app_state();
    assert!(state.verbose);
}

#[test]
fn test_query_engine_permission_denial() {
    let engine = QueryEngine::new(make_config());
    assert_eq!(engine.permission_denials().len(), 0);

    engine.record_permission_denial(PermissionDenial {
        tool_name: "Bash".to_string(),
        tool_use_id: "tu_1".to_string(),
        reason: "user denied".to_string(),
        timestamp: 0,
    });

    assert_eq!(engine.permission_denials().len(), 1);
    assert_eq!(engine.permission_denials()[0].tool_name, "Bash");
}

#[test]
fn engine_shared_state_appends_messages_through_transcript_state() {
    let engine = QueryEngine::new(make_config());
    let message = Message::User(UserMessage {
        uuid: uuid::Uuid::new_v4(),
        timestamp: 1,
        role: "user".into(),
        content: MessageContent::Text("hello".to_string()),
        is_meta: false,
        tool_use_result: None,
        source_tool_assistant_uuid: None,
    });

    engine.state.write().append_message(message.clone());

    let state = engine.state.read();
    assert_eq!(state.transcript.messages.len(), 1);
    assert_eq!(state.transcript.messages[0].uuid(), message.uuid());
}

#[test]
fn engine_shared_state_updates_usage_through_transcript_state() {
    let engine = QueryEngine::new(make_config());
    let usage = Usage {
        input_tokens: 10,
        output_tokens: 4,
        reasoning_output_tokens: 3,
        cache_read_input_tokens: 2,
        cache_creation_input_tokens: 1,
    };

    engine.state.write().update_usage(&usage, 0.25);

    let state = engine.state.read();
    assert_eq!(state.transcript.usage.total_input_tokens, 10);
    assert_eq!(state.transcript.usage.total_output_tokens, 4);
    assert_eq!(state.transcript.usage.total_reasoning_output_tokens, 3);
    assert_eq!(state.transcript.usage.total_cache_read_tokens, 2);
    assert_eq!(state.transcript.usage.total_cache_creation_tokens, 1);
    assert_eq!(state.transcript.usage.api_call_count, 1);
    assert_eq!(state.transcript.usage.total_cost_usd, 0.25);
}

#[test]
fn engine_shared_state_tracks_permission_denials_through_permission_state() {
    let engine = QueryEngine::new(make_config());
    let denial = PermissionDenial {
        tool_name: "Bash".to_string(),
        tool_use_id: "tu_denied".to_string(),
        reason: "user denied".to_string(),
        timestamp: 42,
    };

    engine
        .state
        .write()
        .record_permission_denial(denial.clone());

    let state = engine.state.read();
    assert_eq!(state.permissions.denials.len(), 1);
    assert_eq!(state.permissions.denials[0].tool_use_id, denial.tool_use_id);
}

#[test]
fn engine_shared_state_replaces_tools_through_tool_runtime_state() {
    let engine = QueryEngine::new(make_config());
    let tools = vec![Arc::new(TestTool) as Arc<dyn crate::types::tool::Tool>];

    engine.state.write().set_tools(tools);

    let state = engine.state.read();
    assert_eq!(state.tools.registry.len(), 1);
    assert_eq!(state.tools.registry[0].name(), "TestTool");
}

#[test]
fn test_usage_tracking() {
    let mut usage = UsageTracking::default();
    let api_usage = Usage {
        input_tokens: 100,
        output_tokens: 50,
        reasoning_output_tokens: 0,
        cache_read_input_tokens: 10,
        cache_creation_input_tokens: 5,
    };
    usage = usage.with_added_usage(&api_usage, 0.001);
    assert_eq!(usage.total_input_tokens, 100);
    assert_eq!(usage.total_output_tokens, 50);
    assert_eq!(usage.total_cache_read_tokens, 10);
    assert_eq!(usage.total_cache_creation_tokens, 5);
    assert!((usage.total_cost_usd - 0.001).abs() < f64::EPSILON);
    assert_eq!(usage.api_call_count, 1);

    // Second call accumulates
    usage = usage.with_added_usage(&api_usage, 0.002);
    assert_eq!(usage.total_input_tokens, 200);
    assert_eq!(usage.api_call_count, 2);
}

#[test]
fn test_discovered_skill_names() {
    let engine = QueryEngine::new(make_config());
    assert!(engine.discovered_skill_names().is_empty());

    engine
        .state
        .write()
        .tools
        .discovered_skill_names
        .insert("test_skill".to_string());
    assert_eq!(engine.discovered_skill_names().len(), 1);
}

#[test]
fn test_loaded_nested_memory_paths() {
    let engine = QueryEngine::new(make_config());
    assert!(engine.loaded_nested_memory_paths().is_empty());
}

#[test]
#[serial_test::serial]
fn test_try_extract_session_memory_uses_structured_insight() {
    let home = tempdir().unwrap();
    let _guard = EnvGuard::set("ALLTHECODES_HOME", home.path());
    let workspace = home.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();

    let mut config = make_config();
    config.cwd = workspace.to_string_lossy().to_string();
    let engine = QueryEngine::new(config);
    engine.replace_messages(vec![
        user_message("Earlier request"),
        assistant_message("Earlier assistant answer that is long enough."),
        user_message("Please add MCP reconnect tests"),
        assistant_message("Intermediate assistant answer that is long enough."),
        assistant_message(
            "Implemented the manager reconnect path. cargo test -p cc-mcp manager passed.",
        ),
    ]);

    engine.try_extract_session_memory();

    let entries = engine
        .state
        .read()
        .runtime
        .session_memory
        .get_memory_context(1);
    assert_eq!(entries.len(), 1);
    assert!(entries[0]
        .content
        .contains("Request: Please add MCP reconnect tests"));
    assert!(entries[0]
        .content
        .contains("Insight: Implemented the manager reconnect path."));
    assert!(entries[0].tags.contains(&"implementation".to_string()));
    assert!(entries[0].tags.contains(&"testing".to_string()));
    assert!(entries[0].tags.contains(&"mcp".to_string()));
}

#[test]
fn test_set_tools() {
    let engine = QueryEngine::new(make_config());
    assert_eq!(engine.state.read().tools.registry.len(), 0);

    engine.set_tools(vec![Arc::new(TestTool)]);
    assert_eq!(engine.tool_names(), vec!["TestTool".to_string()]);
}

#[tokio::test]
async fn test_submit_local_command() {
    use futures::StreamExt;

    let mut engine = QueryEngine::new(make_config());
    let original_session = engine.current_session_id();
    engine.set_command_dispatcher(Arc::new(TestCommandDispatcher));
    engine.set_command_executor(Arc::new(TestCommandExecutor));
    let stream = engine.submit_message("/clear", QuerySource::Sdk);
    let mut stream = std::pin::pin!(stream);

    let mut items: Vec<SdkMessage> = Vec::new();
    while let Some(msg) = stream.next().await {
        items.push(msg);
    }

    // Should yield SystemInit + Result
    assert!(
        items.len() >= 2,
        "expected at least 2 items, got {}",
        items.len()
    );

    // First should be SystemInit
    assert!(
        matches!(items[0], SdkMessage::SystemInit(_)),
        "first item should be SystemInit"
    );

    // Last should be Result with success
    let last = items.last().unwrap();
    match last {
        SdkMessage::Result(ref result) => {
            assert_eq!(result.subtype, ResultSubtype::Success);
            assert!(!result.is_error);
            assert!(result.result.contains("clear"));
            assert_eq!(result.session_id, engine.current_session_id().to_string());
        }
        other => panic!("expected SdkMessage::Result, got {:?}", other),
    }
    assert_ne!(engine.current_session_id(), original_session);
    assert!(engine.messages().is_empty());
}

#[tokio::test]
async fn submit_system_init_filters_view_image_for_text_only_model() {
    use futures::StreamExt;

    let mut engine = QueryEngine::new(make_config());
    engine.set_tools(vec![
        Arc::new(allthecodes_tools::exec::SleepTool),
        Arc::new(allthecodes_tools::media::ViewImageTool),
        Arc::new(allthecodes_tools::media::ViewImageAliasTool),
    ]);
    {
        let mut state = engine.state.write();
        state.app_state.main_loop_model = "text-only".into();
        state.app_state.settings.model_capabilities.insert(
            "text-only".into(),
            allthecodes_config::settings::ModelCapabilitySettings {
                input_modalities: vec!["text".into()],
                supports_image_detail_original: false,
                ..Default::default()
            },
        );
    }
    engine.set_command_dispatcher(Arc::new(TestCommandDispatcher));
    engine.set_command_executor(Arc::new(TestCommandExecutor));
    let stream = engine.submit_message("/clear", QuerySource::Sdk);
    let mut stream = std::pin::pin!(stream);

    let first = stream.next().await.expect("system init");
    match first {
        SdkMessage::SystemInit(init) => {
            assert!(init.tools.contains(&"Sleep".to_string()));
            assert!(!init.tools.contains(&"ViewImage".to_string()));
            assert!(!init.tools.contains(&"view_image".to_string()));
        }
        other => panic!("expected SystemInit, got {other:?}"),
    }
}

#[tokio::test]
async fn test_submit_output_command_executes_handler() {
    use futures::StreamExt;

    let mut engine = QueryEngine::new(make_config());
    engine.set_command_dispatcher(Arc::new(TestCommandDispatcher));
    engine.set_command_executor(Arc::new(TestCommandExecutor));
    let stream = engine.submit_message("/help clear", QuerySource::Sdk);
    let mut stream = std::pin::pin!(stream);

    let mut items: Vec<SdkMessage> = Vec::new();
    while let Some(msg) = stream.next().await {
        items.push(msg);
    }

    let result = items
        .iter()
        .find_map(|item| {
            if let SdkMessage::Result(result) = item {
                Some(result)
            } else {
                None
            }
        })
        .expect("result message");

    assert_eq!(result.subtype, ResultSubtype::Success);
    assert!(!result.is_error);
    assert!(result.result.contains("/clear"));
    assert!(!result.result.contains("help clear"));
}

#[tokio::test]
async fn test_submit_query_command_injects_handler_messages_before_model_call() {
    use futures::StreamExt;

    let mut engine = QueryEngine::new(make_config());
    engine.set_command_dispatcher(Arc::new(TestCommandDispatcher));
    engine.set_command_executor(Arc::new(TestCommandExecutor));
    let stream = engine.submit_message("/review 123", QuerySource::Sdk);
    let mut stream = std::pin::pin!(stream);

    let first = stream.next().await.expect("system init");
    assert!(matches!(first, SdkMessage::SystemInit(_)));

    let messages = engine.messages();
    let review_prompt = messages.iter().find_map(|message| {
        if let Message::User(user) = message {
            if let MessageContent::Text(text) = &user.content {
                return Some(text.as_str());
            }
        }
        None
    });
    assert!(
        matches!(review_prompt, Some(text) if text.contains("Review pull request `123`")),
        "query command should inject a user prompt before the first model call"
    );
}

#[tokio::test]
async fn test_submit_message_yields_system_init() {
    use futures::StreamExt;

    let engine = QueryEngine::new(make_config());
    let stream = engine.submit_message("hello", QuerySource::ReplMainThread);
    let mut stream = std::pin::pin!(stream);

    // The first item should always be SystemInit
    if let Some(msg) = stream.next().await {
        match msg {
            SdkMessage::SystemInit(init) => {
                assert_eq!(init.session_id, engine.session_id.to_string());
                assert!(!init.model.is_empty());
            }
            other => panic!("expected SystemInit, got {:?}", other),
        }
    } else {
        panic!("stream was empty");
    }
}

fn user_message(text: &str) -> Message {
    Message::User(UserMessage {
        uuid: uuid::Uuid::new_v4(),
        timestamp: 1,
        role: "user".into(),
        content: MessageContent::Text(text.to_string()),
        is_meta: false,
        tool_use_result: None,
        source_tool_assistant_uuid: None,
    })
}

fn assistant_message(text: &str) -> Message {
    Message::Assistant(AssistantMessage {
        uuid: uuid::Uuid::new_v4(),
        timestamp: 1,
        role: "assistant".into(),
        content: vec![ContentBlock::Text {
            text: text.to_string(),
        }],
        usage: None,
        stop_reason: None,
        is_api_error_message: false,
        api_error: None,
        cost_usd: 0.0,
    })
}
