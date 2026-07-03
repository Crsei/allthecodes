use std::collections::BTreeMap;
use std::sync::LazyLock;

pub const CORE_TOOL_NAMES: &[&str] = &[
    "Agent",
    "AskUserQuestion",
    "Bash",
    "Brief",
    "Config",
    "Edit",
    "EnterPlanMode",
    "ExitPlanMode",
    "Glob",
    "Grep",
    "LSP",
    "ListMcpResources",
    "NotebookEdit",
    "Read",
    "ReadMcpResource",
    "SearchExtraTools",
    "SendUserMessage",
    "Skill",
    "Sleep",
    "StructuredOutput",
    "SystemStatus",
    "Task",
    "TaskCreate",
    "TaskGet",
    "TaskList",
    "TaskOutput",
    "TaskStop",
    "TaskUpdate",
    "TodoWrite",
    "ToolSearch",
    "WebFetch",
    "WebSearch",
    "Write",
    "ExecuteExtraTool",
];

static CORE_REGISTRY: LazyLock<RuntimeCapabilityRegistry> =
    LazyLock::new(RuntimeCapabilityRegistry::core_seed);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeCapabilityKind {
    Command,
    Tool,
    DeferredTool,
    McpServerTool,
    DynamicWorkflow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeCapabilityVisibility {
    Visible,
    Hidden,
    Deferred,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeCapabilityProvider {
    Builtin,
    Dynamic,
    Mcp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeCapabilitySourceScope {
    Builtin,
    Session,
    Project,
    User,
    McpServer(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeCapability {
    pub name: String,
    pub aliases: Vec<String>,
    pub description: Option<String>,
    pub kind: RuntimeCapabilityKind,
    pub visibility: RuntimeCapabilityVisibility,
    pub permission_subject: Option<String>,
    pub provider: RuntimeCapabilityProvider,
    pub source_scope: RuntimeCapabilitySourceScope,
    pub discoverable: bool,
}

impl RuntimeCapability {
    pub fn builtin_tool(name: &str) -> Self {
        Self {
            name: name.to_string(),
            aliases: Vec::new(),
            description: None,
            kind: RuntimeCapabilityKind::Tool,
            visibility: RuntimeCapabilityVisibility::Visible,
            permission_subject: Some(name.to_string()),
            provider: RuntimeCapabilityProvider::Builtin,
            source_scope: RuntimeCapabilitySourceScope::Builtin,
            discoverable: true,
        }
    }

    pub fn mcp_tool(server: &str, tool: &str) -> Self {
        let name = format!("{server}/{tool}");
        Self {
            permission_subject: Some(format!("McpTool({name})")),
            name,
            aliases: Vec::new(),
            description: None,
            kind: RuntimeCapabilityKind::McpServerTool,
            visibility: RuntimeCapabilityVisibility::Deferred,
            provider: RuntimeCapabilityProvider::Mcp,
            source_scope: RuntimeCapabilitySourceScope::McpServer(server.to_string()),
            discoverable: true,
        }
    }

    pub fn builtin_command(
        name: String,
        aliases: Vec<String>,
        description: String,
        hidden: bool,
    ) -> Self {
        let visibility = if hidden {
            RuntimeCapabilityVisibility::Hidden
        } else {
            RuntimeCapabilityVisibility::Visible
        };
        Self {
            permission_subject: Some(format!("Command({name})")),
            name,
            aliases,
            description: Some(description),
            kind: RuntimeCapabilityKind::Command,
            visibility,
            provider: RuntimeCapabilityProvider::Builtin,
            source_scope: RuntimeCapabilitySourceScope::Builtin,
            discoverable: !hidden,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeCapabilityRegistry {
    capabilities: BTreeMap<String, RuntimeCapability>,
}

impl RuntimeCapabilityRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn core_seed() -> Self {
        let mut registry = Self::new();
        for name in CORE_TOOL_NAMES {
            registry.register(RuntimeCapability::builtin_tool(name));
        }
        registry
    }

    pub fn register(&mut self, capability: RuntimeCapability) -> Option<RuntimeCapability> {
        self.capabilities
            .insert(capability.name.clone(), capability)
    }

    pub fn get(&self, name: &str) -> Option<&RuntimeCapability> {
        self.capabilities.get(name)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.capabilities.contains_key(name)
    }

    pub fn is_deferred(&self, name: &str) -> bool {
        !self.contains(name)
    }
}

pub fn core_runtime_capabilities() -> &'static RuntimeCapabilityRegistry {
    &CORE_REGISTRY
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeded_registry_marks_core_tools_visible() {
        let registry = RuntimeCapabilityRegistry::core_seed();
        let bash = registry.get("Bash").expect("Bash capability");

        assert_eq!(bash.name, "Bash");
        assert_eq!(bash.kind, RuntimeCapabilityKind::Tool);
        assert_eq!(bash.visibility, RuntimeCapabilityVisibility::Visible);
        assert_eq!(bash.permission_subject.as_deref(), Some("Bash"));
        assert!(!registry.is_deferred("Bash"));
    }

    #[test]
    fn tools_outside_seed_are_deferred() {
        let registry = RuntimeCapabilityRegistry::core_seed();

        assert!(registry.is_deferred("CronCreate"));
        assert!(registry.is_deferred("WebBrowser"));
        assert!(!registry.is_deferred("SearchExtraTools"));
        assert!(!registry.is_deferred("ExecuteExtraTool"));
    }

    #[test]
    fn registered_mcp_tool_uses_same_metadata_shape() {
        let mut registry = RuntimeCapabilityRegistry::new();
        registry.register(RuntimeCapability::mcp_tool("filesystem", "read_file"));

        let capability = registry.get("filesystem/read_file").expect("mcp tool");

        assert_eq!(capability.kind, RuntimeCapabilityKind::McpServerTool);
        assert_eq!(capability.provider, RuntimeCapabilityProvider::Mcp);
        assert_eq!(
            capability.permission_subject.as_deref(),
            Some("McpTool(filesystem/read_file)")
        );
    }
}
