use std::collections::HashSet;
use std::sync::{Arc, LazyLock};

use crate::ask_user::AskUserQuestionTool;
use crate::brief::BriefTool;
use crate::config_tool::ConfigTool;
use crate::plan_mode::{EnterPlanModeTool, ExitPlanModeTool};
use crate::send_user_message::SendUserMessageTool;
use crate::sleep::SleepTool;
use crate::structured_output::StructuredOutputTool;
use crate::system_status::SystemStatusTool;
use crate::tool::Tools;
use crate::tool_search::ToolSearchTool;
use crate::web_fetch::WebFetchTool;
use crate::web_search::WebSearchTool;
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

/// Return the allow-list for policies that restrict visible tools.
pub fn allowed_tool_names(policy: ToolPolicy) -> Option<&'static [&'static str]> {
    match policy {
        ToolPolicy::DefaultAgent => None,
        ToolPolicy::Coordinator => Some(&[
            "Agent",
            "Task",
            "SendMessage",
            "TaskList",
            "TaskStop",
            "subscribe_pr_activity",
            "unsubscribe_pr_activity",
        ]),
        ToolPolicy::CoordinatorWorker => Some(&[
            "Glob",
            "Grep",
            "Read",
            "Bash",
            "Edit",
            "Write",
            "TodoWrite",
            "TaskList",
            "TaskUpdate",
            "SendMessage",
        ]),
        ToolPolicy::InProcessTeammate => Some(&[
            "Glob",
            "Grep",
            "Read",
            "Bash",
            "Edit",
            "Write",
            "TodoWrite",
            "TaskList",
            "TaskUpdate",
            "TaskOutput",
            "SendMessage",
        ]),
    }
}

pub fn tool_allowed(policy: ToolPolicy, name: &str) -> bool {
    allowed_tool_names(policy)
        .map(|allowed| allowed.contains(&name))
        .unwrap_or(true)
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
    tools.extend(crate::product_tools::tools());

    tools.extend([
        Arc::new(AskUserQuestionTool) as _,
        Arc::new(ConfigTool) as _,
        Arc::new(StructuredOutputTool) as _,
        Arc::new(SendUserMessageTool) as _,
        Arc::new(WebFetchTool) as _,
        Arc::new(WebSearchTool) as _,
        Arc::new(EnterPlanModeTool) as _,
        Arc::new(ExitPlanModeTool) as _,
        Arc::new(BriefTool) as _,
        Arc::new(SystemStatusTool) as _,
        Arc::new(ToolSearchTool) as _,
    ]);

    tools.into_iter().filter(|tool| tool.is_enabled()).collect()
}

/// Get all built-in tools using the supplied external providers.
pub fn base_tools_with_providers(providers: &ToolRegistryProviders) -> Tools {
    let mut tools = allthecodes_tools_base_tools();
    for provider in &providers.base_tool_providers {
        tools.extend((provider)().into_iter().filter(|tool| tool.is_enabled()));
    }
    tools
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

    tools
}

/// Get all runtime tools currently owned directly by `cc-tools`.
pub fn get_all_tools() -> Tools {
    get_all_tools_with_providers(&installed_tool_registry_providers())
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
    let Some(allowed) = allowed_tool_names(policy) else {
        return tools;
    };
    tools
        .into_iter()
        .filter(|tool| allowed.iter().any(|name| *name == tool.name()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
