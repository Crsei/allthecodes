//! Slash-command contract and low-coupling command implementations.

pub mod add_dir;
pub mod advisor;
pub mod agents_cmd;
pub mod assistant;
pub mod audit_export;
pub mod branch;
pub mod brief;
pub mod browser;
pub mod btw;
pub mod channels;
pub mod chrome_cmd;
pub mod clear;
pub mod commit;
pub mod compact;
pub mod config_cmd;
pub mod context;
pub mod coordinator;
pub mod copy;
pub mod cost;
pub mod daemon_cmd;
pub mod debug_cmd;
pub mod diff;
pub mod doctor;
pub mod dream;
pub mod dynamic_registry;
pub mod effort;
pub mod exit;
pub mod experimental;
pub mod export;
pub mod extra_usage;
pub mod fast;
pub mod feedback;
pub mod files;
pub mod gbranch;
pub mod goal;
pub mod help;
pub mod hooks_cmd;
pub mod ide_cmd;
pub mod init;
pub mod insights;
pub mod keybindings_cmd;
pub mod login;
pub mod login_code;
pub mod logout;
pub mod loop_cmd;
pub mod lsp_cmd;
pub mod mcp;
pub mod memory;
pub mod model;
pub mod model_add;
pub mod notify;
pub mod permissions_cmd;
pub mod plan;
pub mod plan_workflow;
pub mod plugin_cmd;
pub mod plugin_commands;
pub mod rate_limit;
pub mod recap;
pub mod reload_plugins_cmd;
pub mod remote_cmd;
pub mod rename;
pub mod resume;
pub mod review;
pub mod rewind;
pub mod sandbox_cmd;
pub mod schedule;
pub mod security_review;
pub mod session;
pub mod session_export;
pub mod simplify;
pub mod skills_cmd;
pub mod sleep_cmd;
pub mod status;
pub mod statusline_cmd;
pub mod tasks_cmd;
pub mod team_cmd;
pub mod team_onboarding;
pub mod terminal_env;
pub mod terminal_setup;
pub mod version;
pub mod voice_cmd;

use std::path::PathBuf;
use std::sync::LazyLock;

use anyhow::Result;
use async_trait::async_trait;

use allthecodes_bootstrap::SessionId;
use allthecodes_engine::{command_runtime, types::app_state::AppState};
use allthecodes_types::message::Message;

pub mod runtime {
    use std::future::Future;
    use std::path::PathBuf;
    use std::pin::Pin;
    use std::sync::{OnceLock, RwLock};

    use allthecodes_engine::status_line::payload::WorktreeStatus;
    use allthecodes_ipc_protocol::subsystem_types::{LspRecommendationSettings, LspServerInfo};
    use allthecodes_tasks::TaskEntry;
    use allthecodes_tools::tool::Tools;
    use allthecodes_types::message::Message;

    use crate::{CommandContext, CommandMetadata};

    type Installer = fn();
    type LspServersProvider = fn() -> Vec<LspServerInfo>;
    type LspRecommendationSettingsProvider = fn() -> LspRecommendationSettings;
    type LspRecommendationsProvider = fn() -> Vec<LspPluginRecommendationInfo>;
    type BuiltinAgentsProvider = fn() -> Vec<BuiltinAgentEntry>;
    type BuiltinAgentPromptProvider = fn(&str) -> Option<String>;
    type TaskListProvider = fn() -> Vec<TaskEntry>;
    type TaskGetProvider = fn(&str) -> Option<TaskEntry>;
    type TaskMutateProvider = fn(&str) -> Result<Option<TaskEntry>, String>;
    type TeamTaskSnapshotProvider = fn() -> Vec<TeamTaskSnapshot>;
    type CommandMetadataProvider = fn() -> Vec<CommandMetadata>;
    type WorktreeStatusProvider = fn() -> Option<WorktreeStatus>;
    type RemoteDaemonStatusProvider =
        fn() -> Result<crate::remote_cmd::LocalGatewayDaemonStatus, String>;
    type RemoteTokenPathProvider = fn() -> PathBuf;
    type ToolPolicyNamesProvider = fn(CommandToolPolicy) -> Vec<String>;
    type ToolListProvider = fn() -> Tools;
    type TeamContextForSessionProvider = fn(&str) -> Option<allthecodes_types::teams::TeamContext>;
    type ForkRunner = fn(
        CommandForkParams,
    ) -> Pin<
        Box<dyn Future<Output = anyhow::Result<CommandForkOutcome>> + Send + 'static>,
    >;
    type TeamCommandExecutor = for<'a> fn(
        &'a str,
        &'a mut CommandContext,
    ) -> Pin<Box<dyn Future<Output = String> + Send + 'a>>;

    #[derive(Debug, Clone)]
    pub struct BuiltinAgentEntry {
        pub name: String,
        pub description: String,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum TeamTaskStatus {
        Running,
        Stopped,
        Completed,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum CommandToolPolicy {
        DefaultAgent,
        Coordinator,
    }

    #[derive(Clone)]
    pub struct CommandForkParams {
        pub prompt: String,
        pub cwd: String,
        pub model: String,
        pub fallback_model: Option<String>,
        pub tools: Tools,
        pub max_turns: Option<usize>,
        pub parent_messages: Option<Vec<Message>>,
        pub append_system_prompt: Option<String>,
        pub custom_system_prompt: Option<String>,
    }

    #[derive(Debug, Clone)]
    pub struct CommandForkOutcome {
        pub text: String,
        pub had_error: bool,
        pub duration_ms: u64,
        pub agent_id: String,
    }

    #[derive(Debug, Clone)]
    pub struct TeamTaskSnapshot {
        pub id: String,
        pub agent_id: String,
        pub agent_name: String,
        pub team_name: String,
        pub status: TeamTaskStatus,
        pub is_idle: bool,
        pub has_error: bool,
        pub error_message: Option<String>,
        pub prompt: String,
        pub model: Option<String>,
        pub awaiting_plan_approval: bool,
        pub permission_mode: String,
    }

    /// Lightweight recommendation info returned by the LSP recommendation provider.
    #[derive(Debug, Clone)]
    pub struct LspPluginRecommendationInfo {
        pub plugin_id: String,
        pub plugin_name: String,
        pub description: String,
        pub languages: Vec<String>,
        pub confidence: f64,
        pub is_already_installed: bool,
        pub is_dismissed: bool,
    }

    static INSTALLER: OnceLock<RwLock<Option<Installer>>> = OnceLock::new();
    static LSP_SERVERS_PROVIDER: OnceLock<RwLock<Option<LspServersProvider>>> = OnceLock::new();
    static LSP_SETTINGS_PROVIDER: OnceLock<RwLock<Option<LspRecommendationSettingsProvider>>> =
        OnceLock::new();
    static LSP_RECOMMENDATIONS_PROVIDER: OnceLock<RwLock<Option<LspRecommendationsProvider>>> =
        OnceLock::new();
    static BUILTIN_AGENTS_PROVIDER: OnceLock<RwLock<Option<BuiltinAgentsProvider>>> =
        OnceLock::new();
    static BUILTIN_AGENT_PROMPT_PROVIDER: OnceLock<RwLock<Option<BuiltinAgentPromptProvider>>> =
        OnceLock::new();
    static TASK_LIST_PROVIDER: OnceLock<RwLock<Option<TaskListProvider>>> = OnceLock::new();
    static TASK_GET_PROVIDER: OnceLock<RwLock<Option<TaskGetProvider>>> = OnceLock::new();
    static TASK_STOP_PROVIDER: OnceLock<RwLock<Option<TaskMutateProvider>>> = OnceLock::new();
    static TASK_DELETE_PROVIDER: OnceLock<RwLock<Option<TaskMutateProvider>>> = OnceLock::new();
    static TEAM_TASK_SNAPSHOT_PROVIDER: OnceLock<RwLock<Option<TeamTaskSnapshotProvider>>> =
        OnceLock::new();
    static COMMAND_METADATA_PROVIDER: OnceLock<RwLock<Option<CommandMetadataProvider>>> =
        OnceLock::new();
    static WORKTREE_STATUS_PROVIDER: OnceLock<RwLock<Option<WorktreeStatusProvider>>> =
        OnceLock::new();
    static REMOTE_DAEMON_STATUS_PROVIDER: OnceLock<RwLock<Option<RemoteDaemonStatusProvider>>> =
        OnceLock::new();
    static REMOTE_TOKEN_PATH_PROVIDER: OnceLock<RwLock<Option<RemoteTokenPathProvider>>> =
        OnceLock::new();
    static TOOL_POLICY_NAMES_PROVIDER: OnceLock<RwLock<Option<ToolPolicyNamesProvider>>> =
        OnceLock::new();
    static TOOL_LIST_PROVIDER: OnceLock<RwLock<Option<ToolListProvider>>> = OnceLock::new();
    static TEAM_CONTEXT_FOR_SESSION_PROVIDER: OnceLock<
        RwLock<Option<TeamContextForSessionProvider>>,
    > = OnceLock::new();
    static FORK_RUNNER: OnceLock<RwLock<Option<ForkRunner>>> = OnceLock::new();
    static TEAM_COMMAND_EXECUTOR: OnceLock<RwLock<Option<TeamCommandExecutor>>> = OnceLock::new();

    pub fn set_runtime_installer(installer: Installer) {
        let slot = INSTALLER.get_or_init(|| RwLock::new(None));
        if let Ok(mut guard) = slot.write() {
            *guard = Some(installer);
        }
    }

    pub fn set_lsp_runtime_providers(
        servers: LspServersProvider,
        settings: LspRecommendationSettingsProvider,
    ) {
        let servers_slot = LSP_SERVERS_PROVIDER.get_or_init(|| RwLock::new(None));
        if let Ok(mut guard) = servers_slot.write() {
            *guard = Some(servers);
        }

        let settings_slot = LSP_SETTINGS_PROVIDER.get_or_init(|| RwLock::new(None));
        if let Ok(mut guard) = settings_slot.write() {
            *guard = Some(settings);
        }
    }

    pub fn set_lsp_recommendations_provider(provider: LspRecommendationsProvider) {
        let slot = LSP_RECOMMENDATIONS_PROVIDER.get_or_init(|| RwLock::new(None));
        if let Ok(mut guard) = slot.write() {
            *guard = Some(provider);
        }
    }

    pub fn set_agent_runtime_providers(
        builtins: BuiltinAgentsProvider,
        prompt: BuiltinAgentPromptProvider,
    ) {
        set_provider(&BUILTIN_AGENTS_PROVIDER, builtins);
        set_provider(&BUILTIN_AGENT_PROMPT_PROVIDER, prompt);
    }

    pub fn set_task_runtime_providers(
        list: TaskListProvider,
        get: TaskGetProvider,
        stop: TaskMutateProvider,
        delete: TaskMutateProvider,
        team_snapshots: TeamTaskSnapshotProvider,
    ) {
        set_provider(&TASK_LIST_PROVIDER, list);
        set_provider(&TASK_GET_PROVIDER, get);
        set_provider(&TASK_STOP_PROVIDER, stop);
        set_provider(&TASK_DELETE_PROVIDER, delete);
        set_provider(&TEAM_TASK_SNAPSHOT_PROVIDER, team_snapshots);
    }

    pub fn set_team_command_executor(executor: TeamCommandExecutor) {
        set_provider(&TEAM_COMMAND_EXECUTOR, executor);
    }

    pub fn set_command_metadata_provider(provider: CommandMetadataProvider) {
        set_provider(&COMMAND_METADATA_PROVIDER, provider);
    }

    pub fn set_worktree_status_provider(provider: WorktreeStatusProvider) {
        set_provider(&WORKTREE_STATUS_PROVIDER, provider);
    }

    pub fn set_remote_daemon_status_provider(provider: RemoteDaemonStatusProvider) {
        set_provider(&REMOTE_DAEMON_STATUS_PROVIDER, provider);
    }

    pub fn set_remote_token_path_provider(provider: RemoteTokenPathProvider) {
        set_provider(&REMOTE_TOKEN_PATH_PROVIDER, provider);
    }

    pub fn set_tool_policy_names_provider(provider: ToolPolicyNamesProvider) {
        set_provider(&TOOL_POLICY_NAMES_PROVIDER, provider);
    }

    pub fn set_tool_list_provider(provider: ToolListProvider) {
        set_provider(&TOOL_LIST_PROVIDER, provider);
    }

    pub fn set_team_context_for_session_provider(provider: TeamContextForSessionProvider) {
        set_provider(&TEAM_CONTEXT_FOR_SESSION_PROVIDER, provider);
    }

    pub fn set_fork_runner(runner: ForkRunner) {
        set_provider(&FORK_RUNNER, runner);
    }

    fn set_provider<T: Copy>(slot: &OnceLock<RwLock<Option<T>>>, provider: T) {
        let slot = slot.get_or_init(|| RwLock::new(None));
        if let Ok(mut guard) = slot.write() {
            *guard = Some(provider);
        }
    }

    pub(crate) fn ensure_runtime_installed() {
        let Some(slot) = INSTALLER.get() else {
            return;
        };
        let Ok(guard) = slot.read() else {
            return;
        };
        if let Some(installer) = *guard {
            installer();
        }
    }

    pub(crate) fn lsp_server_info_list() -> Vec<LspServerInfo> {
        ensure_runtime_installed();
        LSP_SERVERS_PROVIDER
            .get()
            .and_then(|slot| slot.read().ok().and_then(|guard| *guard))
            .map(|provider| provider())
            .unwrap_or_default()
    }

    pub(crate) fn lsp_recommendation_settings() -> LspRecommendationSettings {
        ensure_runtime_installed();
        LSP_SETTINGS_PROVIDER
            .get()
            .and_then(|slot| slot.read().ok().and_then(|guard| *guard))
            .map(|provider| provider())
            .unwrap_or_default()
    }

    pub(crate) fn lsp_recommendations() -> Vec<LspPluginRecommendationInfo> {
        ensure_runtime_installed();
        LSP_RECOMMENDATIONS_PROVIDER
            .get()
            .and_then(|slot| slot.read().ok().and_then(|guard| *guard))
            .map(|provider| provider())
            .unwrap_or_default()
    }

    pub(crate) fn builtin_agent_entries() -> Vec<BuiltinAgentEntry> {
        ensure_runtime_installed();
        get_provider(&BUILTIN_AGENTS_PROVIDER)
            .map(|provider| provider())
            .unwrap_or_default()
    }

    pub(crate) fn builtin_agent_prompt(name: &str) -> Option<String> {
        ensure_runtime_installed();
        get_provider(&BUILTIN_AGENT_PROMPT_PROVIDER).and_then(|provider| provider(name))
    }

    pub(crate) fn tool_tasks() -> Vec<TaskEntry> {
        ensure_runtime_installed();
        get_provider(&TASK_LIST_PROVIDER)
            .map(|provider| provider())
            .unwrap_or_default()
    }

    pub(crate) fn get_tool_task(id: &str) -> Option<TaskEntry> {
        ensure_runtime_installed();
        get_provider(&TASK_GET_PROVIDER).and_then(|provider| provider(id))
    }

    pub(crate) fn stop_tool_task(id: &str) -> Result<Option<TaskEntry>, String> {
        ensure_runtime_installed();
        get_provider(&TASK_STOP_PROVIDER)
            .map(|provider| provider(id))
            .unwrap_or(Ok(None))
    }

    pub(crate) fn delete_tool_task(id: &str) -> Result<Option<TaskEntry>, String> {
        ensure_runtime_installed();
        get_provider(&TASK_DELETE_PROVIDER)
            .map(|provider| provider(id))
            .unwrap_or(Ok(None))
    }

    pub(crate) fn team_task_snapshots() -> Vec<TeamTaskSnapshot> {
        ensure_runtime_installed();
        get_provider(&TEAM_TASK_SNAPSHOT_PROVIDER)
            .map(|provider| provider())
            .unwrap_or_default()
    }

    pub fn command_metadata_snapshot() -> Vec<CommandMetadata> {
        ensure_runtime_installed();
        get_provider(&COMMAND_METADATA_PROVIDER)
            .map(|provider| provider())
            .unwrap_or_else(|| crate::command_metadata(&crate::get_all_commands()))
    }

    pub(crate) fn current_worktree_status() -> Option<WorktreeStatus> {
        ensure_runtime_installed();
        get_provider(&WORKTREE_STATUS_PROVIDER).and_then(|provider| provider())
    }

    pub(crate) fn remote_daemon_status(
    ) -> Result<crate::remote_cmd::LocalGatewayDaemonStatus, String> {
        ensure_runtime_installed();
        get_provider(&REMOTE_DAEMON_STATUS_PROVIDER)
            .map(|provider| provider())
            .unwrap_or(Ok(crate::remote_cmd::LocalGatewayDaemonStatus::Stopped))
    }

    pub(crate) fn remote_token_path() -> PathBuf {
        ensure_runtime_installed();
        get_provider(&REMOTE_TOKEN_PATH_PROVIDER)
            .map(|provider| provider())
            .unwrap_or_else(|| allthecodes_config::paths::daemon_dir().join("control-token.json"))
    }

    pub(crate) fn tool_policy_names(policy: CommandToolPolicy) -> Vec<String> {
        ensure_runtime_installed();
        get_provider(&TOOL_POLICY_NAMES_PROVIDER)
            .map(|provider| provider(policy))
            .unwrap_or_default()
    }

    pub(crate) fn available_tools() -> Tools {
        ensure_runtime_installed();
        get_provider(&TOOL_LIST_PROVIDER)
            .map(|provider| provider())
            .unwrap_or_default()
    }

    pub(crate) fn team_context_for_session(
        session_id: &str,
    ) -> Option<allthecodes_types::teams::TeamContext> {
        ensure_runtime_installed();
        get_provider(&TEAM_CONTEXT_FOR_SESSION_PROVIDER).and_then(|provider| provider(session_id))
    }

    pub(crate) async fn run_fork(params: CommandForkParams) -> anyhow::Result<CommandForkOutcome> {
        ensure_runtime_installed();
        let Some(runner) = get_provider(&FORK_RUNNER) else {
            anyhow::bail!("fork runtime adapter is unavailable");
        };
        runner(params).await
    }

    pub(crate) async fn execute_team_command(args: &str, ctx: &mut CommandContext) -> String {
        ensure_runtime_installed();
        match get_provider(&TEAM_COMMAND_EXECUTOR) {
            Some(executor) => executor(args, ctx).await,
            None => "Team command runtime is unavailable.".to_string(),
        }
    }

    fn get_provider<T: Copy>(slot: &OnceLock<RwLock<Option<T>>>) -> Option<T> {
        slot.get()
            .and_then(|slot| slot.read().ok().and_then(|guard| *guard))
    }
}

/// A registered slash command.
pub struct Command {
    /// Primary command name (e.g. "help").
    pub name: String,
    /// Alternative names (e.g. ["h", "?"]).
    pub aliases: Vec<String>,
    /// Short description shown in /help output.
    pub description: String,
    /// The handler that executes this command.
    pub handler: Box<dyn CommandHandler>,
}

impl Command {
    pub fn metadata(&self) -> CommandMetadata {
        CommandMetadata {
            name: self.name.clone(),
            aliases: self.aliases.clone(),
            description: self.description.clone(),
        }
    }
}

/// Command metadata used by parsers and dispatchers without handler ownership.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandMetadata {
    pub name: String,
    pub aliases: Vec<String>,
    pub description: String,
}

/// Trait implemented by every slash command.
#[async_trait]
pub trait CommandHandler: Send + Sync {
    /// Execute the command with the given arguments and context.
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult>;
}

/// Execution context passed to command handlers.
pub struct CommandContext {
    /// Current conversation messages.
    pub messages: Vec<Message>,
    /// Current working directory.
    pub cwd: PathBuf,
    /// Application state snapshot.
    pub app_state: AppState,
    /// Current session ID.
    pub session_id: SessionId,
}

/// Result of executing a command.
pub enum CommandResult {
    /// Output text to display to the user (not sent to the model).
    Output(String),
    /// Messages to add to the conversation and then send to the model.
    Query(Vec<Message>),
    /// Switch the active runtime session to `session_id` with the provided
    /// visible/runtime transcript and display `notice` to the user.
    SwitchSession {
        session_id: SessionId,
        messages: Vec<Message>,
        notice: String,
    },
    /// Clear the visible conversation by starting a fresh session.
    Clear,
    /// Exit the REPL with a goodbye message.
    Exit(String),
    /// No visible output.
    None,
}

pub fn command<H>(name: &str, aliases: &[&str], description: &str, handler: H) -> Command
where
    H: CommandHandler + 'static,
{
    Command {
        name: name.to_string(),
        aliases: aliases.iter().map(|alias| (*alias).to_string()).collect(),
        description: description.to_string(),
        handler: Box::new(handler),
    }
}

pub fn sort_commands_for_display(commands: &mut [Command]) {
    commands.sort_by(|a, b| match (a.name.as_str(), b.name.as_str()) {
        ("init", "init") => std::cmp::Ordering::Equal,
        ("init", _) => std::cmp::Ordering::Less,
        (_, "init") => std::cmp::Ordering::Greater,
        _ => a.name.cmp(&b.name),
    });
}

/// Returns `true` if a builtin command should be hidden from the command
/// palette and `/help` listing.  Hidden commands are still executable when
/// typed directly.
pub fn is_hidden_command(name: &str) -> bool {
    matches!(
        name,
        "advisor"
            | "brief"
            | "extra-usage"
            | "rate-limit-options"
            | "model-add"
            | "login-code"
            | "voice"
    )
}

/// Global dynamic command registry shared across the application.
///
/// This registry stores dynamically-registered commands from user, project,
/// plugin, and skill sources alongside builtin commands.
pub static DYNAMIC_REGISTRY: LazyLock<parking_lot::Mutex<dynamic_registry::DynamicRegistry>> =
    LazyLock::new(|| parking_lot::Mutex::new(dynamic_registry::DynamicRegistry::new()));

/// Get merged command metadata from both builtin commands and the dynamic
/// registry. This is the primary lookup source for command resolution.
pub fn get_dynamic_metadata() -> Vec<CommandMetadata> {
    let mut metadata = command_metadata(&get_all_commands());

    let registry = DYNAMIC_REGISTRY.lock();
    for entry in registry.list_all() {
        // Avoid adding entries that shadow builtin with the same name
        if !metadata.iter().any(|m| m.name == entry.name) {
            metadata.push(CommandMetadata {
                name: entry.name.clone(),
                aliases: entry.aliases.clone(),
                description: entry.description.clone(),
            });
        }
    }

    metadata
}

pub fn command_metadata(commands: &[Command]) -> Vec<CommandMetadata> {
    commands.iter().map(Command::metadata).collect()
}

/// Build the full list of available commands.
///
/// Runtime-dependent commands use provider hooks in [`runtime`] and in their
/// local adapter modules. The binary crate is still responsible for installing
/// those providers before command execution.
pub fn get_all_commands() -> Vec<Command> {
    let mut commands = vec![
        command(
            "help",
            &["h", "?"],
            "Help V2: commands, quick surfaces, keys, and diagnostics hints",
            help::HelpHandler,
        ),
        command(
            "clear",
            &[],
            "Clear the conversation history",
            clear::ClearHandler,
        ),
        command(
            "config",
            &["settings"],
            "Show or modify configuration settings",
            config_cmd::ConfigHandler,
        ),
        command(
            "diff",
            &[],
            "Show git diff of current changes",
            diff::DiffHandler,
        ),
        command(
            "debug",
            &[],
            "Export or inspect debug information",
            debug_cmd::DebugHandler,
        ),
        command(
            "exit",
            &["quit", "q"],
            "Exit the REPL via the normal exit flow",
            exit::ExitHandler,
        ),
        command(
            "version",
            &["v"],
            "Show the current version",
            version::VersionHandler,
        ),
        command(
            "feedback",
            &[],
            "Open the allthecodes product feedback page",
            feedback::FeedbackHandler,
        ),
        command(
            "model",
            &[],
            "Show or switch the active model",
            model::ModelHandler,
        ),
        command(
            "cost",
            &["usage"],
            "Show token usage and cost for the current session",
            cost::CostHandler,
        ),
        command(
            "session",
            &[],
            "Show current session info or list saved sessions",
            session::SessionHandler,
        ),
        command(
            "resume",
            &["sessions", "preview"],
            "Resume or preview a previous saved session",
            resume::ResumeHandler,
        ),
        command(
            "rename",
            &[],
            "Set or clear the custom title for the current session",
            rename::RenameHandler,
        ),
        command(
            "rewind",
            &[],
            "Rewind the conversation to an earlier user turn",
            rewind::RewindHandler,
        ),
        command(
            "insights",
            &[],
            "Session history analytics (cross-session statistics)",
            insights::InsightsHandler,
        ),
        command(
            "files",
            &[],
            "List files referenced in the current conversation",
            files::FilesHandler,
        ),
        command(
            "context",
            &["ctx"],
            "Show context usage information",
            context::ContextHandler,
        ),
        command(
            "coordinator",
            &["coord"],
            "Enable or inspect coordinator mode for Agent Teams",
            coordinator::CoordinatorHandler,
        ),
        command(
            "permissions",
            &["perms"],
            "View or modify tool permission settings",
            permissions_cmd::PermissionsHandler,
        ),
        command(
            "plan",
            &[],
            "Enter plan mode and show or edit the plan file",
            plan::PlanHandler,
        ),
        command(
            "login",
            &[],
            "Authenticate (Claude Code / Anthropic-compatible, OpenAI Codex, OpenAI API, Bedrock, Vertex)",
            login::LoginHandler,
        ),
        command(
            "login-code",
            &[],
            "Complete OAuth login with authorization code",
            login_code::LoginCodeHandler,
        ),
        command(
            "logout",
            &[],
            "Clear stored authentication credentials",
            logout::LogoutHandler,
        ),
        command(
            "commit",
            &[],
            "Create a git commit from current changes",
            commit::CommitHandler,
        ),
        command(
            "branch",
            &["br"],
            "Fork the current conversation into a new branch",
            branch::BranchHandler,
        ),
        command(
            "gbranch",
            &["gitbranch"],
            "Show or switch git branches",
            gbranch::GitBranchHandler,
        ),
        command(
            "goal",
            &[],
            "Set or inspect the active session goal",
            goal::GoalHandler,
        ),
        command(
            "effort",
            &[],
            "Set the thinking effort level (low/medium/high)",
            effort::EffortHandler,
        ),
        command("fast", &[], "Toggle fast mode on/off", fast::FastHandler),
        command(
            "memory",
            &["mem", "global-search", "quick-open"],
            "View, search, and quick-open memory/project instructions",
            memory::MemoryHandler,
        ),
        command(
            "skills",
            &[],
            "List available skills",
            skills_cmd::SkillsHandler,
        ),
        command(
            "init",
            &[],
            "Initialize project config and AGENTS.md",
            init::InitHandler,
        ),
        command(
            "copy",
            &["cp"],
            "Copy the last assistant response to clipboard",
            copy::CopyHandler,
        ),
        command("status", &[], "Show session status", status::StatusHandler),
        command(
            "export",
            &["markdown-export"],
            "Export conversation to Markdown (.md)",
            export::ExportHandler,
        ),
        command(
            "experimental",
            &["experiments", "exp"],
            "Inspect or override experimental feature gates",
            experimental::ExperimentalHandler,
        ),
        command(
            "audit-export",
            &["audit"],
            "Export session as verifiable audit record (.audit.json)",
            audit_export::AuditExportHandler,
        ),
        command(
            "session-export",
            &["sexport", "structured-export"],
            "Export session as structured JSON data package (.session.json)",
            session_export::SessionExportHandler,
        ),
        command(
            "extra-usage",
            &["eu"],
            "Show extended token usage and cost analysis",
            extra_usage::ExtraUsageHandler,
        ),
        command(
            "rate-limit-options",
            &["rlo", "rate-limit"],
            "Show rate limit information for the current model",
            rate_limit::RateLimitHandler,
        ),
        command(
            "compact",
            &[],
            "Compact conversation to reduce token usage",
            compact::CompactHandler,
        ),
        command(
            "mcp",
            &[],
            "MCP server management (list, status, add, edit, remove, approve, reject, connect)",
            mcp::McpHandler,
        ),
        command(
            "ide",
            &[],
            "Detect, select, or reconnect the IDE MCP bridge",
            ide_cmd::IdeHandler,
        ),
        command(
            "lsp",
            &[],
            "Show LSP server cards and recommendation settings",
            lsp_cmd::LspHandler,
        ),
        command(
            "chrome",
            &[],
            "Claude in Chrome (first-party integration) status + reconnect",
            chrome_cmd::ChromeHandler,
        ),
        command(
            "plugin",
            &["plugins"],
            "Plugin management (list, status, enable, disable)",
            plugin_cmd::PluginHandler,
        ),
        command(
            "reload-plugins",
            &[],
            "Hot-refresh the plugin registry",
            reload_plugins_cmd::ReloadPluginsHandler,
        ),
        command(
            "model-add",
            &["ma"],
            "Add a model with token pricing to .env",
            model_add::ModelAddHandler,
        ),
        command(
            "sleep",
            &[],
            "Set proactive sleep duration",
            sleep_cmd::SleepCmdHandler,
        ),
        command(
            "assistant",
            &["kairos"],
            "View assistant mode status",
            assistant::AssistantHandler,
        ),
        command(
            "daemon",
            &[],
            "View/control daemon process",
            daemon_cmd::DaemonCmdHandler,
        ),
        command(
            "notify",
            &[],
            "Push notification settings",
            notify::NotifyHandler,
        ),
        command(
            "remote",
            &[],
            "Inspect and control the local remote-control gateway",
            remote_cmd::RemoteHandler,
        ),
        command(
            "channels",
            &[],
            "View connected channels",
            channels::ChannelsHandler,
        ),
        command(
            "dream",
            &["logs"],
            "Distill daily logs into memory",
            dream::DreamHandler,
        ),
        command(
            "add-dir",
            &[],
            "Add a new working directory",
            add_dir::AddDirHandler,
        ),
        command(
            "sandbox",
            &[],
            "View or toggle sandbox + network access settings",
            sandbox_cmd::SandboxHandler,
        ),
        command(
            "keybindings",
            &["keys", "shortcuts"],
            "View, edit, or reload keybindings.json",
            keybindings_cmd::KeybindingsHandler,
        ),
        command(
            "statusline",
            &["status-line"],
            "View, edit, or test the scriptable status line",
            statusline_cmd::StatusLineHandler,
        ),
        command(
            "terminal-setup",
            &["term-setup", "terminal"],
            "Diagnose terminal env + print Shift+Enter / tmux / notification tips",
            terminal_setup::TerminalSetupHandler,
        ),
        command(
            "voice",
            &["dictation"],
            "Inspect compatibility-only voice settings (runtime voice unsupported)",
            voice_cmd::VoiceHandler,
        ),
        command(
            "team",
            &["teams"],
            "Manage Agent Teams (create, list, spawn, send, kill, leave, delete)",
            team_cmd::TeamHandler,
        ),
        command(
            "review",
            &[],
            "Review a pull request using a local gh pr workflow",
            review::ReviewHandler,
        ),
        command(
            "security-review",
            &["secreview"],
            "Run a focused security review of the current branch diff",
            security_review::SecurityReviewHandler,
        ),
        command(
            "recap",
            &[],
            "Summarize the current session (short | long)",
            recap::RecapHandler,
        ),
        command(
            "hooks",
            &[],
            "Read-only merged hook tree (managed + user + project + local)",
            hooks_cmd::HooksHandler,
        ),
        command(
            "agents",
            &[],
            "Browse agent definitions with source + override visibility",
            agents_cmd::AgentsHandler,
        ),
        command(
            "doctor",
            &["diagnostics", "diag"],
            "Aggregated diagnostics (install, auth, settings, MCP, keybindings, terminal)",
            doctor::DoctorHandler,
        ),
        command(
            "tasks",
            &[],
            "List and drill into background tasks (tool + team)",
            tasks_cmd::TasksHandler,
        ),
        command(
            "btw",
            &[],
            "Ask a side question in a forked agent",
            btw::BtwHandler,
        ),
        command(
            "simplify",
            &[],
            "Run a multi-agent simplification review of recent changes",
            simplify::SimplifyHandler,
        ),
        command(
            "advisor",
            &[],
            "Show, set, or clear the advisor model",
            advisor::AdvisorHandler,
        ),
        command(
            "loop",
            &[],
            "Register a recurring local task or slash command and run it once",
            loop_cmd::LoopHandler,
        ),
        command(
            "schedule",
            &["cron"],
            "Manage local cron tasks (add, list, pause, trigger, remove)",
            schedule::ScheduleHandler,
        ),
        command(
            "team-onboarding",
            &["teamonboarding"],
            "Generate a teammate onboarding guide from project and team state",
            team_onboarding::TeamOnboardingHandler,
        ),
    ];
    sort_commands_for_display(&mut commands);
    commands
}

/// Find a command by name or alias from user input.
pub fn find_command_in(input: &str, commands: &[CommandMetadata]) -> Option<usize> {
    let cmd_name = input.split_whitespace().next().unwrap_or("");

    commands
        .iter()
        .position(|c| c.name == cmd_name || c.aliases.iter().any(|a| a == cmd_name))
}

/// Parse user input into (command_index, args) if it starts with `/`.
pub fn parse_command_input_in(
    input: &str,
    commands: &[CommandMetadata],
) -> Option<(usize, String)> {
    let trimmed = input.trim();
    if !trimmed.starts_with('/') {
        return None;
    }

    let without_slash = &trimmed[1..];
    let cmd_name = without_slash.split_whitespace().next().unwrap_or("");
    let args = without_slash
        .strip_prefix(cmd_name)
        .unwrap_or("")
        .trim()
        .to_string();

    find_command_in(without_slash, commands).map(|idx| (idx, args))
}

/// Find a command by name or alias in the full registry.
pub fn find_command(input: &str) -> Option<usize> {
    find_command_in(input, &get_dynamic_metadata())
}

/// Parse user input against the full registry.
pub fn parse_command_input(input: &str) -> Option<(usize, String)> {
    parse_command_input_in(input, &get_dynamic_metadata())
}

/// Concrete [`allthecodes_types::commands::CommandDispatcher`] backed by command metadata.
pub struct DefaultCommandDispatcher {
    commands: Vec<CommandMetadata>,
}

impl DefaultCommandDispatcher {
    pub fn new(commands: Vec<CommandMetadata>) -> Self {
        Self { commands }
    }

    pub fn for_full_registry() -> Self {
        Self::new(get_dynamic_metadata())
    }

    pub fn from_commands(commands: &[Command]) -> Self {
        Self::new(command_metadata(commands))
    }
}

impl allthecodes_types::commands::CommandDispatcher for DefaultCommandDispatcher {
    fn parse_command_input(
        &self,
        input: &str,
    ) -> Option<allthecodes_types::commands::ParsedCommand> {
        parse_command_input_in(input, &self.commands)
            .map(|(index, args)| allthecodes_types::commands::ParsedCommand { index, args })
    }

    fn command_name(&self, index: usize) -> Option<String> {
        self.commands.get(index).map(|cmd| cmd.name.clone())
    }
}

pub struct EngineCommandExecutor;

#[async_trait]
impl command_runtime::CommandExecutor for EngineCommandExecutor {
    async fn execute(
        &self,
        parsed: allthecodes_types::commands::ParsedCommand,
        command_name: String,
        ctx: &mut command_runtime::CommandContext,
    ) -> anyhow::Result<command_runtime::CommandResult> {
        let mut commands = get_all_commands();
        let mut command_ctx = CommandContext {
            messages: ctx.messages.clone(),
            cwd: ctx.cwd.clone(),
            app_state: ctx.app_state.clone(),
            session_id: ctx.session_id.clone(),
        };

        let result = if let Some(command) = commands.get_mut(parsed.index) {
            command
                .handler
                .execute(&parsed.args, &mut command_ctx)
                .await?
        } else {
            let entry = DYNAMIC_REGISTRY
                .lock()
                .find(&command_name)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("Unknown command: /{}", command_name))?;
            execute_dynamic_command(&entry, &parsed.args, &mut command_ctx).await?
        };
        ctx.messages = command_ctx.messages;
        ctx.cwd = command_ctx.cwd;
        ctx.app_state = command_ctx.app_state;
        ctx.session_id = command_ctx.session_id;

        Ok(match result {
            CommandResult::Output(text) => command_runtime::CommandResult::Output(text),
            CommandResult::Query(messages) => command_runtime::CommandResult::Query(messages),
            CommandResult::SwitchSession {
                session_id,
                messages,
                notice,
            } => command_runtime::CommandResult::SwitchSession {
                session_id,
                messages,
                notice,
            },
            CommandResult::Clear => command_runtime::CommandResult::Clear,
            CommandResult::Exit(text) => command_runtime::CommandResult::Exit(text),
            CommandResult::None => command_runtime::CommandResult::None,
        })
    }
}

pub fn install_engine_command_executor() {
    command_runtime::set_global_command_executor(std::sync::Arc::new(EngineCommandExecutor));
}

async fn execute_dynamic_command(
    entry: &dynamic_registry::DynamicCommandEntry,
    args: &str,
    ctx: &mut CommandContext,
) -> anyhow::Result<CommandResult> {
    match entry.execution_strategy {
        dynamic_registry::ExecutionStrategy::Skill | dynamic_registry::ExecutionStrategy::Fork => {
            let Some(skill) = allthecodes_skills::find_skill(&entry.name) else {
                anyhow::bail!("Skill command /{} is not loaded", entry.name);
            };
            let prepared = allthecodes_skills::invocation::prepare_skill_invocation(
                &skill,
                args,
                &ctx.app_state.main_loop_model,
                Some(ctx.session_id.as_str()),
            );
            let messages = match prepared {
                allthecodes_skills::invocation::PreparedSkillInvocation::Inline {
                    new_messages,
                    ..
                } => new_messages,
                allthecodes_skills::invocation::PreparedSkillInvocation::Fork { .. } => {
                    vec![allthecodes_skills::invocation::make_skill_message(
                        &skill,
                        args,
                        Some(ctx.session_id.as_str()),
                    )]
                }
            };
            Ok(CommandResult::Query(messages))
        }
        dynamic_registry::ExecutionStrategy::Plugin => {
            let owner = entry.plugin_id.as_deref().unwrap_or("unknown plugin");
            Ok(CommandResult::Output(format!(
                "Plugin command /{} is registered by {}. Plugin command execution is routed through the dynamic command registry.",
                entry.name, owner
            )))
        }
        dynamic_registry::ExecutionStrategy::Mcp { ref server_name } => {
            Ok(CommandResult::Output(format!(
                "MCP command /{} is registered for server {}.",
                entry.name, server_name
            )))
        }
        dynamic_registry::ExecutionStrategy::Inline => {
            anyhow::bail!("Dynamic inline command /{} has no handler", entry.name)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_types::commands::CommandDispatcher;

    fn sample_commands() -> Vec<Command> {
        let mut commands = vec![
            command("help", &["h", "?"], "Show help", clear::ClearHandler),
            command("init", &[], "Init", clear::ClearHandler),
            command("config", &["settings"], "Configure", clear::ClearHandler),
        ];
        sort_commands_for_display(&mut commands);
        commands
    }

    #[test]
    fn parser_resolves_names_and_aliases() {
        let metadata = command_metadata(&sample_commands());

        assert_eq!(find_command_in("help", &metadata), Some(2));
        assert_eq!(find_command_in("settings", &metadata), Some(1));
        assert_eq!(find_command_in("missing", &metadata), None);
        assert_eq!(
            parse_command_input_in("/config set model SOTA", &metadata),
            Some((1, "set model SOTA".to_string()))
        );
        assert_eq!(parse_command_input_in("not a command", &metadata), None);
    }

    #[test]
    fn dispatcher_uses_stable_metadata_snapshot() {
        let dispatcher = DefaultCommandDispatcher::from_commands(&sample_commands());

        let parsed = dispatcher.parse_command_input("/h").unwrap();
        assert_eq!(parsed.index, 2);
        assert_eq!(parsed.args, "");
        assert_eq!(dispatcher.command_name(0).as_deref(), Some("init"));
    }
}
