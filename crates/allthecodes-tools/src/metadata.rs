#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolMetadata {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub capabilities: ToolCapabilities,
    pub risk: ToolRisk,
    pub concurrency: ToolConcurrency,
    pub visibility: ToolVisibility,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ToolCapabilities {
    pub read_files: bool,
    pub write_files: bool,
    pub run_processes: bool,
    pub spawn_agents: bool,
    pub use_network: bool,
    pub control_browser: bool,
    pub control_desktop: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ToolRisk {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolConcurrency {
    Unknown,
    Safe,
    Exclusive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolVisibility {
    pub allow_non_interactive: bool,
    pub coordinator: bool,
    pub coordinator_worker: bool,
    pub in_process_teammate: bool,
}

impl ToolMetadata {
    pub fn from_tool_name(name: &str) -> Self {
        let mut metadata = Self::unknown();
        metadata.name = canonical_tool_name(name);
        metadata.aliases = aliases_for(metadata.name);

        apply_capability_seed(name, &mut metadata);
        apply_risk_seed(name, &mut metadata);
        apply_visibility_seed(name, &mut metadata);

        metadata
    }

    fn unknown() -> Self {
        Self {
            name: "Unknown",
            aliases: &[],
            capabilities: ToolCapabilities::default(),
            risk: ToolRisk::Medium,
            concurrency: ToolConcurrency::Unknown,
            visibility: ToolVisibility::default(),
        }
    }
}

impl Default for ToolVisibility {
    fn default() -> Self {
        Self {
            allow_non_interactive: true,
            coordinator: false,
            coordinator_worker: false,
            in_process_teammate: false,
        }
    }
}

fn canonical_tool_name(name: &str) -> &'static str {
    match name {
        "Agent" | "Task" => "Agent",
        "SendMessage" | "send_message" => "SendMessage",
        "ListAgents" | "list_agents" => "ListAgents",
        "FollowupTask" | "followup_task" => "FollowupTask",
        "WaitAgent" | "wait_agent" => "WaitAgent",
        "CloseAgent" | "close_agent" => "CloseAgent",
        "TeamSpawn" | "spawn_agent" => "TeamSpawn",
        "Bash" | "bash" => "Bash",
        "PowerShell" | "powershell" | "Pwsh" | "pwsh" => "PowerShell",
        "Read" | "read" => "Read",
        "Grep" | "grep" => "Grep",
        "Glob" | "glob" => "Glob",
        "WebSearch" | "web_search" => "WebSearch",
        "WebFetch" | "web_fetch" => "WebFetch",
        "LSP" | "lsp" => "LSP",
        "Sleep" | "sleep" => "Sleep",
        "TaskCreate" | "TaskUpdate" | "TaskGet" | "TaskList" | "TaskStop" | "TaskOutput"
        | "TodoWrite" | "Edit" | "Write" | "Plan" | "AskUserQuestion" => match name {
            "TaskCreate" => "TaskCreate",
            "TaskUpdate" => "TaskUpdate",
            "TaskGet" => "TaskGet",
            "TaskList" => "TaskList",
            "TaskStop" => "TaskStop",
            "TaskOutput" => "TaskOutput",
            "TodoWrite" => "TodoWrite",
            "Edit" => "Edit",
            "Write" => "Write",
            "Plan" => "Plan",
            "AskUserQuestion" => "AskUserQuestion",
            _ => "Unknown",
        },
        _ => "Unknown",
    }
}

fn aliases_for(name: &str) -> &'static [&'static str] {
    match name {
        "Agent" => &["Task"],
        "SendMessage" => &["send_message"],
        "ListAgents" => &["list_agents"],
        "FollowupTask" => &["followup_task"],
        "WaitAgent" => &["wait_agent"],
        "CloseAgent" => &["close_agent"],
        "TeamSpawn" => &["spawn_agent"],
        "Bash" => &["bash"],
        "PowerShell" => &["powershell", "Pwsh", "pwsh"],
        "Read" => &["read"],
        "Grep" => &["grep"],
        "Glob" => &["glob"],
        "WebSearch" => &["web_search"],
        "WebFetch" => &["web_fetch"],
        "LSP" => &["lsp"],
        "Sleep" => &["sleep"],
        _ => &[],
    }
}

fn apply_capability_seed(name: &str, metadata: &mut ToolMetadata) {
    let capabilities = &mut metadata.capabilities;

    match name {
        "Read" | "read" | "Grep" | "grep" | "Glob" | "glob" | "LSP" | "lsp" => {
            capabilities.read_files = true;
        }
        "Edit" | "Write" | "TodoWrite" => {
            capabilities.write_files = true;
        }
        "Bash" | "bash" | "PowerShell" | "powershell" | "Pwsh" | "pwsh" => {
            capabilities.run_processes = true;
        }
        "Agent" | "Task" | "TeamSpawn" | "spawn_agent" | "FollowupTask" | "followup_task" => {
            capabilities.spawn_agents = true;
        }
        "WebSearch" | "web_search" | "WebFetch" | "web_fetch" => {
            capabilities.use_network = true;
        }
        _ => {}
    }
}

fn apply_risk_seed(name: &str, metadata: &mut ToolMetadata) {
    metadata.risk = match name {
        "Read" | "read" | "Grep" | "grep" | "Glob" | "glob" | "LSP" | "lsp" | "Sleep" | "sleep"
        | "TaskCreate" | "TaskUpdate" | "TaskGet" | "TaskList" | "Plan" | "WebSearch"
        | "web_search" | "WebFetch" | "web_fetch" => ToolRisk::Low,
        "Bash" | "bash" | "PowerShell" | "powershell" | "Pwsh" | "pwsh" | "Agent" | "Task"
        | "TeamSpawn" | "spawn_agent" | "FollowupTask" | "followup_task" | "Edit" | "Write" => {
            ToolRisk::High
        }
        _ => metadata.risk,
    };
}

fn apply_visibility_seed(name: &str, metadata: &mut ToolMetadata) {
    if matches!(name, "AskUserQuestion") {
        metadata.visibility.allow_non_interactive = false;
    }

    if matches!(
        name,
        "Agent"
            | "Task"
            | "SendMessage"
            | "send_message"
            | "ListAgents"
            | "list_agents"
            | "FollowupTask"
            | "followup_task"
            | "WaitAgent"
            | "wait_agent"
            | "CloseAgent"
            | "close_agent"
            | "TaskList"
            | "TaskStop"
            | "subscribe_pr_activity"
            | "unsubscribe_pr_activity"
    ) {
        metadata.visibility.coordinator = true;
    }

    if matches!(
        name,
        "Glob"
            | "Grep"
            | "Read"
            | "Bash"
            | "Edit"
            | "Write"
            | "TodoWrite"
            | "TaskList"
            | "TaskUpdate"
            | "SendMessage"
            | "send_message"
    ) {
        metadata.visibility.coordinator_worker = true;
    }

    if matches!(
        name,
        "Glob"
            | "Grep"
            | "Read"
            | "Bash"
            | "Edit"
            | "Write"
            | "TodoWrite"
            | "TaskList"
            | "TaskUpdate"
            | "TaskOutput"
            | "SendMessage"
            | "send_message"
    ) {
        metadata.visibility.in_process_teammate = true;
    }
}
