use std::collections::HashSet;
use std::sync::{Arc, LazyLock};

use crate::exec::SleepTool;
use crate::interaction::{AskUserQuestionTool, SendUserMessageTool, StructuredOutputTool};
use crate::metadata::ToolMetadata;
use crate::plan_mode::{EnterPlanModeTool, ExitPlanModeTool};
use crate::runtime::{BriefTool, ConfigTool, SystemStatusTool, ToolSearchTool};
use crate::tool::{Tool, Tools};
use allthecodes_config::features::{self, Feature, FeatureFlags};
use parking_lot::RwLock;

/// Tool provider used to inject tools owned by crates that cannot be depended
/// on from `cc-tools` without creating dependency cycles.
pub type ToolProvider = Arc<dyn Fn() -> Tools + Send + Sync + 'static>;

/// External tool providers for the shared registry.
///
/// `base_tool_providers` are part of the normal built-in tool set. Use this for
/// tools still owned by `cc-engine`, `cc-lsp-service`, `cc-teams`,
/// `cc-worktree`, or the root crate.
///
/// `runtime_tool_providers` are appended after built-ins with duplicate-name
/// protection. Use this for plugin-contributed runtime tools.
#[derive(Clone, Default)]
pub struct ToolRegistryProviders {
    pub base_tool_providers: Vec<ToolProvider>,
    pub runtime_tool_providers: Vec<ToolProvider>,
}

impl ToolRegistryProviders {
    pub fn empty() -> Self {
        Self::default()
    }
}

static INSTALLED_PROVIDERS: LazyLock<RwLock<ToolRegistryProviders>> =
    LazyLock::new(|| RwLock::new(ToolRegistryProviders::empty()));

/// Install process-wide external providers used by [`get_all_tools`].
pub fn install_tool_registry_providers(providers: ToolRegistryProviders) {
    *INSTALLED_PROVIDERS.write() = providers;
}

/// Snapshot the currently installed external providers.
pub fn installed_tool_registry_providers() -> ToolRegistryProviders {
    INSTALLED_PROVIDERS.read().clone()
}

/// Runtime tool visibility profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolPolicy {
    /// Default interactive/session tool pool.
    DefaultAgent,
    /// Coordinator lead tool pool.
    Coordinator,
    /// Dedicated coordinator worker pool.
    CoordinatorWorker,
    /// Generic in-process teammate pool.
    InProcessTeammate,
}

/// Per-turn session visibility gates layered on top of feature/model gates.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ToolSessionGates {
    /// Non-interactive clients such as `-p`, JSON SDK mode, autonomous ticks,
    /// and subagents cannot safely block on direct user-prompt tools.
    pub non_interactive: bool,
    /// Subagents must not recursively spawn new agents through their visible
    /// schema or deferred hidden-tool catalog unless a narrower agent
    /// definition explicitly supplies a separate runtime.
    pub subagent: bool,
}

pub fn tool_allowed(policy: ToolPolicy, name: &str) -> bool {
    metadata_allowed_for_policy(ToolMetadata::from_tool_name(name), policy)
}

/// Keep the first tool for each name and drop later duplicates.
///
/// Tool names are part of the provider request contract. Providers reject
/// duplicate names, so this function is intentionally conservative: built-ins
/// or earlier providers win over later runtime/MCP/plugin tools.
pub fn dedupe_tools_by_name(tools: Tools) -> Tools {
    let mut seen = HashSet::new();
    let mut deduped = Tools::with_capacity(tools.len());

    for tool in tools {
        let name = tool.name().to_string();
        if seen.insert(name.clone()) {
            deduped.push(tool);
        } else {
            tracing::warn!(tool = %name, "skipping tool with duplicate name");
        }
    }

    deduped
}

const GOAL_TOOL_NAMES: &[&str] = &[
    "GetGoal",
    "get_goal",
    "CreateGoal",
    "create_goal",
    "UpdateGoal",
    "update_goal",
];

const WORKFLOW_TOOL_NAMES: &[&str] = &["Workflow", "workflow", "DynamicWorkflow"];

const MULTI_AGENT_V2_TOOL_NAMES: &[&str] = &[
    "ListAgents",
    "list_agents",
    "FollowupTask",
    "followup_task",
    "WaitAgent",
    "wait_agent",
    "CloseAgent",
    "close_agent",
    "TeamSpawn",
    "spawn_agent",
    "SendMessage",
    "send_message",
];

fn tool_enabled_by_feature_gates(name: &str, flags: &FeatureFlags) -> bool {
    if GOAL_TOOL_NAMES.contains(&name) {
        return flags.is_enabled(Feature::GoalTools);
    }
    if WORKFLOW_TOOL_NAMES.contains(&name) {
        return flags.is_enabled(Feature::WorkflowScripts);
    }
    if MULTI_AGENT_V2_TOOL_NAMES.contains(&name) {
        return flags.is_enabled(Feature::MultiAgentV2);
    }
    true
}

fn tool_enabled_by_session_gates(tool: &dyn Tool, gates: ToolSessionGates) -> bool {
    let metadata = tool.metadata();
    if gates.non_interactive && !metadata.visibility.allow_non_interactive {
        return false;
    }
    if gates.subagent && metadata.capabilities.spawn_agents {
        return false;
    }
    true
}

/// Filter tools controlled by runtime feature gates.
///
/// These gates default to enabled in the full-build branch. The filter is
/// still applied centrally so an explicit runtime/environment disable removes
/// tools from both API schemas and system prompt visible tool lists.
pub fn filter_tools_for_feature_gates(tools: Tools) -> Tools {
    let flags = features::current();
    filter_tools_for_feature_gates_with_flags(tools, &flags)
}

/// Filter tools using an explicit flag snapshot.
///
/// This is useful for tests and for callers that already have a resolved
/// session-local feature snapshot.
pub fn filter_tools_for_feature_gates_with_flags(tools: Tools, flags: &FeatureFlags) -> Tools {
    tools
        .into_iter()
        .filter(|tool| tool_enabled_by_feature_gates(tool.name(), flags))
        .collect()
}

/// Filter tools for the current execution session.
///
/// This is intentionally separate from [`ToolPolicy`]: policies describe a
/// configured role's positive allow-list, while session gates remove tools that
/// are unsafe for the current turn shape regardless of role.
pub fn filter_tools_for_session_gates(tools: Tools, gates: ToolSessionGates) -> Tools {
    tools
        .into_iter()
        .filter(|tool| tool_enabled_by_session_gates(tool.as_ref(), gates))
        .collect()
}

/// Filter tools controlled directly by runtime settings.
pub fn filter_tools_for_runtime_settings(
    tools: Tools,
    settings: &allthecodes_config::runtime_settings::SettingsJson,
) -> Tools {
    let hashline_enabled = settings.hashline_mode.unwrap_or(false);
    tools
        .into_iter()
        .filter(|tool| hashline_enabled || tool.name() != "HashEdit")
        .collect()
}

/// Get all tools owned directly by `cc-tools`.
///
/// Heavier tools that are still owned by dependency-cycle parents must be
/// supplied through [`ToolRegistryProviders`].
pub fn allthecodes_tools_base_tools() -> Tools {
    let mut tools = Tools::new();

    tools.extend(crate::fs::tools());
    tools.push(Arc::new(SleepTool));
    tools.extend(crate::tasks::tools());
    tools.extend(crate::deferred_tools::tools());
    tools.extend(crate::product::tools());
    tools.extend(crate::skills::tools());
    tools.extend(crate::media::tools());
    tools.extend(crate::goals::tools());
    tools.extend(crate::workflow::tools());
    tools.extend(crate::workflow_dynamic::tools());
    tools.extend(crate::memory::tools());
    tools.extend(crate::network::tools());
    tools.extend(crate::notifications::tools());

    tools.extend([
        Arc::new(AskUserQuestionTool) as _,
        Arc::new(ConfigTool) as _,
        Arc::new(StructuredOutputTool) as _,
        Arc::new(SendUserMessageTool) as _,
        Arc::new(EnterPlanModeTool) as _,
        Arc::new(ExitPlanModeTool) as _,
        Arc::new(BriefTool) as _,
        Arc::new(SystemStatusTool) as _,
        Arc::new(ToolSearchTool) as _,
    ]);

    filter_tools_for_feature_gates(tools.into_iter().filter(|tool| tool.is_enabled()).collect())
}

/// Get all built-in tools using the supplied external providers.
pub fn base_tools_with_providers(providers: &ToolRegistryProviders) -> Tools {
    let mut tools = allthecodes_tools_base_tools();
    for provider in &providers.base_tool_providers {
        tools.extend((provider)().into_iter().filter(|tool| tool.is_enabled()));
    }
    dedupe_tools_by_name(filter_tools_for_feature_gates(tools))
}

/// Get all runtime tools using the supplied external providers.
pub fn get_all_tools_with_providers(providers: &ToolRegistryProviders) -> Tools {
    let mut tools = base_tools_with_providers(providers);
    let mut seen: HashSet<String> = tools.iter().map(|tool| tool.name().to_string()).collect();

    for provider in &providers.runtime_tool_providers {
        for tool in (provider)().into_iter().filter(|tool| tool.is_enabled()) {
            let name = tool.name().to_string();
            if seen.insert(name.clone()) {
                tools.push(tool);
            } else {
                tracing::warn!(tool = %name, "skipping registry tool with duplicate name");
            }
        }
    }

    dedupe_tools_by_name(filter_tools_for_feature_gates(tools))
}

/// Get all runtime tools currently owned directly by `cc-tools`.
pub fn get_all_tools() -> Tools {
    get_all_tools_with_providers(&installed_tool_registry_providers())
}

/// Compatibility entry point for process-default runtime service adapters.
///
/// New engine construction should pass an explicit tool registry service. This
/// wrapper keeps remaining legacy callers visibly tied to installed globals.
pub fn process_default_active_tools() -> Tools {
    get_all_tools()
}

/// Get tools for a concrete runtime policy using the supplied providers.
pub fn get_tools_for_policy_with_providers(
    providers: &ToolRegistryProviders,
    policy: ToolPolicy,
) -> Tools {
    filter_tools_for_policy(get_all_tools_with_providers(providers), policy)
}

/// Get tools for a concrete runtime policy using installed providers.
pub fn get_tools_for_policy(policy: ToolPolicy) -> Tools {
    get_tools_for_policy_with_providers(&installed_tool_registry_providers(), policy)
}

/// Filter an existing tool set for a runtime policy.
pub fn filter_tools_for_policy(tools: Tools, policy: ToolPolicy) -> Tools {
    tools
        .into_iter()
        .filter(|tool| metadata_allowed_for_policy(tool.metadata(), policy))
        .collect()
}

fn metadata_allowed_for_policy(metadata: ToolMetadata, policy: ToolPolicy) -> bool {
    match policy {
        ToolPolicy::DefaultAgent => true,
        ToolPolicy::Coordinator => metadata.visibility.coordinator,
        ToolPolicy::CoordinatorWorker => metadata.visibility.coordinator_worker,
        ToolPolicy::InProcessTeammate => metadata.visibility.in_process_teammate,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::{ToolResult, ToolUseContext, ValidationResult};
    use async_trait::async_trait;
    use serde_json::{json, Value};

    struct NamedTestTool(&'static str);

    #[async_trait]
    impl Tool for NamedTestTool {
        fn name(&self) -> &str {
            self.0
        }

        async fn description(&self, _input: &Value) -> String {
            format!("{} description", self.0)
        }

        fn input_json_schema(&self) -> Value {
            json!({"type": "object", "properties": {}})
        }

        async fn validate_input(&self, _input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
            ValidationResult::Ok
        }

        async fn call(
            &self,
            _input: Value,
            _ctx: &ToolUseContext,
            _parent_message: &allthecodes_types::message::AssistantMessage,
            _on_progress: Option<Box<dyn Fn(crate::tool::ToolProgress) + Send + Sync>>,
        ) -> anyhow::Result<ToolResult> {
            Ok(ToolResult::default())
        }

        async fn prompt(&self) -> String {
            self.0.to_string()
        }
    }

    #[test]
    fn default_policy_allows_all_tools() {
        assert!(tool_allowed(ToolPolicy::DefaultAgent, "Bash"));
        assert!(tool_allowed(ToolPolicy::DefaultAgent, "Agent"));
        assert!(tool_allowed(ToolPolicy::DefaultAgent, "Task"));
    }

    #[test]
    fn coordinator_policy_is_lead_only() {
        assert!(tool_allowed(ToolPolicy::Coordinator, "Agent"));
        assert!(tool_allowed(ToolPolicy::Coordinator, "Task"));
        assert!(tool_allowed(ToolPolicy::Coordinator, "TaskStop"));
        assert!(!tool_allowed(ToolPolicy::Coordinator, "Bash"));
    }

    #[test]
    fn allthecodes_tools_registry_has_builtin_tools() {
        let names = get_all_tools()
            .into_iter()
            .map(|tool| tool.name().to_string())
            .collect::<Vec<_>>();

        assert!(names.contains(&"Read".to_string()));
        assert!(names.contains(&"TodoWrite".to_string()));
        assert!(names.contains(&"WebFetch".to_string()));
    }

    #[test]
    fn semantic_feature_gates_remove_goal_and_workflow_tools() {
        let mut flags = allthecodes_config::features::FeatureFlags::all_enabled();
        flags.goal_tools = false;
        flags.workflow_scripts = false;

        let names = filter_tools_for_feature_gates_with_flags(get_all_tools(), &flags)
            .into_iter()
            .map(|tool| tool.name().to_string())
            .collect::<Vec<_>>();

        for hidden in [
            "GetGoal",
            "get_goal",
            "CreateGoal",
            "create_goal",
            "UpdateGoal",
            "update_goal",
            "Workflow",
            "workflow",
        ] {
            assert!(
                !names.contains(&hidden.to_string()),
                "{hidden} should be hidden when its feature gate is disabled"
            );
        }
        assert!(names.contains(&"PushNotification".to_string()));
        assert!(names.contains(&"SearchExtraTools".to_string()));
    }

    #[test]
    fn session_gates_remove_prompt_and_recursive_tools() {
        let names = filter_tools_for_session_gates(
            get_all_tools(),
            ToolSessionGates {
                non_interactive: true,
                subagent: true,
            },
        )
        .into_iter()
        .map(|tool| tool.name().to_string())
        .collect::<Vec<_>>();

        for hidden in [
            "AskUserQuestion",
            "Agent",
            "Task",
            "TeamSpawn",
            "spawn_agent",
            "FollowupTask",
            "followup_task",
        ] {
            assert!(
                !names.contains(&hidden.to_string()),
                "{hidden} should be hidden by session gates"
            );
        }
        assert!(names.contains(&"Read".to_string()));
        assert!(names.contains(&"SearchExtraTools".to_string()));
        assert!(names.contains(&"ExecuteExtraTool".to_string()));
    }

    #[test]
    fn runtime_settings_gate_hash_edit_tool() {
        let mut settings = allthecodes_config::runtime_settings::SettingsJson::default();
        let names = filter_tools_for_runtime_settings(get_all_tools(), &settings)
            .into_iter()
            .map(|tool| tool.name().to_string())
            .collect::<Vec<_>>();
        assert!(!names.contains(&"HashEdit".to_string()));

        settings.hashline_mode = Some(true);
        let names = filter_tools_for_runtime_settings(get_all_tools(), &settings)
            .into_iter()
            .map(|tool| tool.name().to_string())
            .collect::<Vec<_>>();
        assert!(names.contains(&"HashEdit".to_string()));
    }

    #[test]
    fn dedupe_tools_by_name_keeps_first_duplicate() {
        let first: Arc<dyn Tool> = Arc::new(NamedTestTool("Duplicate"));
        let second: Arc<dyn Tool> = Arc::new(NamedTestTool("Duplicate"));
        let unique: Arc<dyn Tool> = Arc::new(NamedTestTool("Unique"));

        let tools = dedupe_tools_by_name(vec![first.clone(), second, unique.clone()]);

        assert_eq!(tools.len(), 2);
        assert!(Arc::ptr_eq(&tools[0], &first));
        assert!(Arc::ptr_eq(&tools[1], &unique));
    }

    #[test]
    fn tool_metadata_drives_session_gates() {
        assert!(
            !crate::metadata::ToolMetadata::from_tool_name("AskUserQuestion")
                .visibility
                .allow_non_interactive
        );
        assert!(
            crate::metadata::ToolMetadata::from_tool_name("Agent")
                .capabilities
                .spawn_agents
        );
        assert!(
            crate::metadata::ToolMetadata::from_tool_name("TeamSpawn")
                .capabilities
                .spawn_agents
        );
        assert!(
            crate::metadata::ToolMetadata::from_tool_name("FollowupTask")
                .capabilities
                .spawn_agents
        );

        let tools: Tools = vec![
            Arc::new(NamedTestTool("AskUserQuestion")),
            Arc::new(NamedTestTool("Agent")),
            Arc::new(NamedTestTool("Read")),
        ];
        let names = filter_tools_for_session_gates(
            tools,
            ToolSessionGates {
                non_interactive: true,
                subagent: true,
            },
        )
        .into_iter()
        .map(|tool| tool.name().to_string())
        .collect::<Vec<_>>();

        assert_eq!(names, vec!["Read".to_string()]);
    }

    #[test]
    fn tool_metadata_drives_policy_filters() {
        let tools: Tools = vec![
            Arc::new(NamedTestTool("Agent")),
            Arc::new(NamedTestTool("Bash")),
            Arc::new(NamedTestTool("Read")),
            Arc::new(NamedTestTool("TaskOutput")),
            Arc::new(NamedTestTool("TaskStop")),
        ];

        let coordinator = filter_tools_for_policy(tools.clone(), ToolPolicy::Coordinator)
            .into_iter()
            .map(|tool| tool.name().to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            coordinator,
            vec!["Agent".to_string(), "TaskStop".to_string()]
        );

        let worker = filter_tools_for_policy(tools.clone(), ToolPolicy::CoordinatorWorker)
            .into_iter()
            .map(|tool| tool.name().to_string())
            .collect::<Vec<_>>();
        assert_eq!(worker, vec!["Bash".to_string(), "Read".to_string()]);

        let teammate = filter_tools_for_policy(tools, ToolPolicy::InProcessTeammate)
            .into_iter()
            .map(|tool| tool.name().to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            teammate,
            vec![
                "Bash".to_string(),
                "Read".to_string(),
                "TaskOutput".to_string()
            ]
        );
    }
}
