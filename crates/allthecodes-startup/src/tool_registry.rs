use std::sync::Arc;

use allthecodes_engine::tools::exec;
use allthecodes_engine::types::tool::Tools;

use allthecodes_lsp_service::tool::LspTool;
use allthecodes_teams::pr_activity::{
    SubscribePrActivityTool, SubscribePrTool, UnsubscribePrActivityTool,
};
use allthecodes_teams::send_message::{SendMessageAliasTool, SendMessageTool};
use allthecodes_teams::team_spawn::{SpawnAgentAliasTool, TeamSpawnTool};
pub use allthecodes_tools::registry::ToolPolicy;
use allthecodes_tools::registry::ToolRegistryProviders;
use allthecodes_worktree::tool::{EnterWorktreeTool, ExitWorktreeTool};

fn root_owned_base_tools() -> Tools {
    let mut tools: Tools = Tools::new();

    tools.extend(exec::tools());
    tools.extend(allthecodes_engine::browser_tool::tools());
    tools.extend(allthecodes_engine::mcp_resource_tools::tools());
    tools.extend(allthecodes_services::scheduler_tools::tools());
    tools.extend(allthecodes_teams::multi_agent_v2::tools());
    tools.extend([
        Arc::new(allthecodes_engine::agent::AgentTool) as _,
        Arc::new(allthecodes_engine::agent::TaskAgentTool) as _,
        Arc::new(allthecodes_engine::skill_tool::SkillTool) as _,
        Arc::new(EnterWorktreeTool) as _,
        Arc::new(ExitWorktreeTool) as _,
        Arc::new(LspTool) as _,
        Arc::new(SendMessageTool) as _,
        Arc::new(SendMessageAliasTool) as _,
        Arc::new(SubscribePrTool) as _,
        Arc::new(SubscribePrActivityTool) as _,
        Arc::new(UnsubscribePrActivityTool) as _,
        Arc::new(TeamSpawnTool) as _,
        Arc::new(SpawnAgentAliasTool) as _,
    ]);

    tools.into_iter().filter(|t| t.is_enabled()).collect()
}

/// Registry providers for tools that still live outside `cc-tools`.
///
/// This is intentionally local to the root crate until startup/main can install
/// providers directly into `allthecodes_tools::registry` and stop importing
/// `crate::tools`.
pub fn root_tool_registry_providers() -> ToolRegistryProviders {
    ToolRegistryProviders {
        base_tool_providers: vec![Arc::new(root_owned_base_tools)],
        runtime_tool_providers: vec![Arc::new(allthecodes_plugins::discover_plugin_tools)],
    }
}

/// Get all runtime tools, including plugin-contributed tools that expose an
/// executable runtime in their plugin manifest.
pub fn get_all_tools() -> Tools {
    allthecodes_tools::registry::get_all_tools_with_providers(&root_tool_registry_providers())
}

/// Get tools for the active top-level session.
pub fn get_tools_for_active_session() -> Tools {
    if allthecodes_teams::coordinator::is_coordinator_mode_enabled() {
        get_tools_for_policy(ToolPolicy::Coordinator)
    } else {
        get_tools_for_policy(ToolPolicy::DefaultAgent)
    }
}

/// Get tools filtered for a concrete runtime policy.
pub fn get_tools_for_policy(policy: ToolPolicy) -> Tools {
    allthecodes_tools::registry::get_tools_for_policy_with_providers(
        &root_tool_registry_providers(),
        policy,
    )
}

/// Filter an existing tool set for a runtime policy.
pub fn filter_tools_for_policy(tools: Tools, policy: ToolPolicy) -> Tools {
    allthecodes_tools::registry::filter_tools_for_policy(tools, policy)
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_config::features::FeatureFlags;

    fn tool_names(tools: Tools) -> Vec<String> {
        tools
            .into_iter()
            .map(|tool| tool.name().to_string())
            .collect()
    }

    #[test]
    fn test_get_all_tools_not_empty() {
        let tools = get_all_tools();
        assert!(!tools.is_empty(), "should have at least one tool");
    }

    #[test]
    fn test_find_tool_by_name() {
        let tools = get_all_tools();

        let bash = tools.iter().find(|t| t.name() == "Bash");
        assert!(bash.is_some(), "should find Bash tool");
        assert_eq!(bash.unwrap().name(), "Bash");

        let read = tools.iter().find(|t| t.name() == "Read");
        assert!(read.is_some(), "should find Read tool");

        let todo_write = tools.iter().find(|t| t.name() == "TodoWrite");
        assert!(todo_write.is_some(), "should find TodoWrite tool");

        let notebook_edit = tools.iter().find(|t| t.name() == "NotebookEdit");
        assert!(notebook_edit.is_some(), "should find NotebookEdit P0 tool");

        let list_mcp_resources = tools.iter().find(|t| t.name() == "ListMcpResources");
        assert!(
            list_mcp_resources.is_some(),
            "should find ListMcpResources P0 tool"
        );

        let read_mcp_resource = tools.iter().find(|t| t.name() == "ReadMcpResource");
        assert!(
            read_mcp_resource.is_some(),
            "should find ReadMcpResource P0 tool"
        );

        for name in ["CronCreate", "CronDelete", "CronList", "WebBrowser"] {
            assert!(
                tools.iter().any(|t| t.name() == name),
                "should find {name} Phase 2 tool"
            );
        }

        for name in ["SearchExtraTools", "ExecuteExtraTool"] {
            assert!(
                tools.iter().any(|t| t.name() == name),
                "should find {name} deferred tool system tool"
            );
        }

        for name in [
            "DiscoverSkills",
            "ViewImage",
            "view_image",
            "GetGoal",
            "get_goal",
            "CreateGoal",
            "create_goal",
            "UpdateGoal",
            "update_goal",
            "VerifyPlanExecution",
            "Workflow",
            "workflow",
            "ApplyPatch",
            "apply_patch",
            "LocalMemoryRecall",
            "VaultHttpFetch",
            "PushNotification",
            "ListAgents",
            "list_agents",
            "FollowupTask",
            "followup_task",
            "WaitAgent",
            "wait_agent",
            "CloseAgent",
            "close_agent",
            "SendMessage",
            "send_message",
            "TeamSpawn",
            "spawn_agent",
        ] {
            assert!(
                tools.iter().any(|t| t.name() == name),
                "should find {name} Phase 5 tool"
            );
        }

        for name in [
            "SubscribePR",
            "Monitor",
            "SendUserFile",
            "TerminalCapture",
            "ReviewArtifact",
            "Snip",
            "CtxInspect",
            "RemoteTrigger",
            "ListPeers",
        ] {
            assert!(
                tools.iter().any(|t| t.name() == name),
                "should find {name} P1/P2 product tool"
            );
        }

        let task = tools.iter().find(|t| t.name() == "Task");
        assert!(task.is_some(), "should find Task compatibility tool");

        let nonexistent = tools.iter().find(|t| t.name() == "NonExistentTool");
        assert!(nonexistent.is_none(), "should not find nonexistent tool");
    }

    #[test]
    fn feature_gate_removes_multi_agent_v2_tools_from_root_registry() {
        let mut flags = FeatureFlags::all_enabled();
        flags.multi_agent_v2 = false;

        let names = tool_names(
            allthecodes_tools::registry::filter_tools_for_feature_gates_with_flags(
                get_all_tools(),
                &flags,
            ),
        );
        for hidden in [
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
        ] {
            assert!(
                !names.contains(&hidden.to_string()),
                "{hidden} should be hidden when multi-agent v2 is disabled"
            );
        }
        assert!(names.contains(&"Agent".to_string()));
        assert!(names.contains(&"Task".to_string()));
    }

    #[test]
    fn test_all_tools_have_unique_names() {
        let tools = get_all_tools();
        let mut names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        let original_len = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), original_len, "all tool names should be unique");
    }

    #[test]
    fn test_all_tools_have_schema() {
        let tools = get_all_tools();
        for tool in &tools {
            let schema = tool.input_json_schema();
            assert!(
                schema.is_object(),
                "tool {} schema should be an object",
                tool.name()
            );
            assert!(
                schema.get("properties").is_some(),
                "tool {} schema should have properties",
                tool.name()
            );
        }
    }

    #[test]
    fn coordinator_policy_exposes_only_lead_orchestration_tools() {
        let names = tool_names(get_tools_for_policy(ToolPolicy::Coordinator));

        assert!(names.contains(&"Agent".to_string()));
        assert!(names.contains(&"Task".to_string()));
        assert!(names.contains(&"SendMessage".to_string()));
        assert!(names.contains(&"send_message".to_string()));
        assert!(names.contains(&"list_agents".to_string()));
        assert!(names.contains(&"TaskList".to_string()));
        assert!(names.contains(&"TaskStop".to_string()));
        assert!(!names.contains(&"Bash".to_string()));
        assert!(!names.contains(&"Write".to_string()));
        assert!(!names.contains(&"TeamSpawn".to_string()));
    }

    #[test]
    fn worker_policy_removes_internal_orchestration_tools() {
        let names = tool_names(get_tools_for_policy(ToolPolicy::CoordinatorWorker));

        assert!(names.contains(&"Read".to_string()));
        assert!(names.contains(&"Bash".to_string()));
        assert!(names.contains(&"Write".to_string()));
        assert!(names.contains(&"SendMessage".to_string()));
        assert!(names.contains(&"send_message".to_string()));
        assert!(names.contains(&"TaskUpdate".to_string()));
        assert!(!names.contains(&"Agent".to_string()));
        assert!(!names.contains(&"Task".to_string()));
        assert!(!names.contains(&"TeamSpawn".to_string()));
        assert!(!names.contains(&"TaskStop".to_string()));
    }

    #[test]
    fn in_process_teammate_policy_can_report_but_not_spawn_agents() {
        let names = tool_names(get_tools_for_policy(ToolPolicy::InProcessTeammate));

        assert!(names.contains(&"SendMessage".to_string()));
        assert!(names.contains(&"send_message".to_string()));
        assert!(names.contains(&"TaskList".to_string()));
        assert!(names.contains(&"TaskUpdate".to_string()));
        assert!(names.contains(&"TaskOutput".to_string()));
        assert!(!names.contains(&"Agent".to_string()));
        assert!(!names.contains(&"Task".to_string()));
        assert!(!names.contains(&"TeamSpawn".to_string()));
    }
}
