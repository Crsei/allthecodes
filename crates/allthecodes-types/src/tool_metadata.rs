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

        apply_capability_seed(metadata.name, &mut metadata);
        apply_risk_seed(metadata.name, &mut metadata);
        apply_visibility_seed(metadata.name, &mut metadata);

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
    if name.eq_ignore_ascii_case("Agent") || name == "Task" {
        return "Agent";
    }
    if name == "SendMessage" || name == "send_message" {
        return "SendMessage";
    }
    if name == "ListAgents" || name == "list_agents" {
        return "ListAgents";
    }
    if name == "FollowupTask" || name == "followup_task" {
        return "FollowupTask";
    }
    if name == "WaitAgent" || name == "wait_agent" {
        return "WaitAgent";
    }
    if name == "CloseAgent" || name == "close_agent" {
        return "CloseAgent";
    }
    if name == "TeamSpawn" || name == "spawn_agent" {
        return "TeamSpawn";
    }
    if name.eq_ignore_ascii_case("Bash") {
        return "Bash";
    }
    if name.eq_ignore_ascii_case("PowerShell") || name.eq_ignore_ascii_case("Pwsh") {
        return "PowerShell";
    }
    if name.eq_ignore_ascii_case("Read") {
        return "Read";
    }
    if name.eq_ignore_ascii_case("Grep") {
        return "Grep";
    }
    if name.eq_ignore_ascii_case("Glob") {
        return "Glob";
    }
    if name == "WebSearch" || name == "web_search" {
        return "WebSearch";
    }
    if name == "WebFetch" || name == "web_fetch" {
        return "WebFetch";
    }
    if name.eq_ignore_ascii_case("LSP") {
        return "LSP";
    }
    if name.eq_ignore_ascii_case("Sleep") {
        return "Sleep";
    }

    match name {
        "TaskCreate" => "TaskCreate",
        "TaskUpdate" => "TaskUpdate",
        "TaskGet" => "TaskGet",
        "TaskList" => "TaskList",
        "TaskStop" => "TaskStop",
        "TaskOutput" => "TaskOutput",
        "DelegateTask" | "delegate_task" => "DelegateTask",
        "TodoWrite" => "TodoWrite",
        "Edit" => "Edit",
        "Write" => "Write",
        "Plan" => "Plan",
        "AskUserQuestion" => "AskUserQuestion",
        "subscribe_pr_activity" => "subscribe_pr_activity",
        "unsubscribe_pr_activity" => "unsubscribe_pr_activity",
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
        "DelegateTask" => &["delegate_task"],
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
        "Read" | "Grep" | "Glob" | "LSP" => {
            capabilities.read_files = true;
        }
        "Edit" | "Write" | "TodoWrite" => {
            capabilities.write_files = true;
        }
        "Bash" | "PowerShell" => {
            capabilities.run_processes = true;
        }
        "Agent" | "TeamSpawn" | "FollowupTask" | "DelegateTask" => {
            capabilities.spawn_agents = true;
        }
        "WebSearch" | "WebFetch" => {
            capabilities.use_network = true;
        }
        _ => {}
    }
}

fn apply_risk_seed(name: &str, metadata: &mut ToolMetadata) {
    metadata.risk = match name {
        "Read" | "Grep" | "Glob" | "LSP" | "Sleep" | "TaskCreate" | "TaskUpdate" | "TaskGet"
        | "TaskList" | "Plan" | "WebSearch" | "WebFetch" => ToolRisk::Low,
        "Bash" | "PowerShell" | "Agent" | "TeamSpawn" | "FollowupTask" | "DelegateTask"
        | "Edit" | "Write" => ToolRisk::High,
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
            | "SendMessage"
            | "ListAgents"
            | "FollowupTask"
            | "WaitAgent"
            | "CloseAgent"
            | "DelegateTask"
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
    ) {
        metadata.visibility.in_process_teammate = true;
    }
}
