use std::path::Path;
use std::sync::Arc;

use crate::types::tool::Tool;

// ═══════════════════════════════════════════════════════════════════════════
// Dynamic section compute functions
// ═══════════════════════════════════════════════════════════════════════════

/// Corresponds to TS: `computeSimpleEnvInfo(model, dirs)`
pub(super) fn env_info_section(model: &str, cwd: &str) -> String {
    let platform = std::env::consts::OS;
    let is_git = crate::utils::git::is_git_repo(Path::new(cwd));

    let shell = "bash";

    let model_desc = format!(
        "You are powered by the model {}. The exact model ID is {}.",
        crate::config::constants::marketing_name_for_model(model).unwrap_or(model),
        model,
    );

    let cutoff = crate::config::constants::knowledge_cutoff(model);
    let cutoff_msg = cutoff
        .map(|c| format!("\n\nAssistant knowledge cutoff is {}.", c))
        .unwrap_or_default();

    format!(
        "Here is useful information about the environment you are running in:\n\
         <env>\n\
         Working directory: {cwd}\n\
         Is directory a git repo: {is_git}\n\
         Platform: {platform}\n\
         Shell: {shell}\n\
         </env>\n\
         {model_desc}{cutoff_msg}",
        cwd = cwd,
        is_git = if is_git { "Yes" } else { "No" },
        platform = platform,
        shell = shell,
        model_desc = model_desc,
        cutoff_msg = cutoff_msg,
    )
}

/// Build a git status snapshot for the system prompt.
///
/// Returns `None` if the directory is not a git repo or if any git
/// operation fails (fail-open: never block prompt construction).
///
/// Corresponds to TS: `gitStatus` section in system prompt.
pub(super) fn git_status_section(cwd: &str) -> Option<String> {
    use crate::utils::git;

    let path = Path::new(cwd);
    if !git::is_git_repo(path) {
        return None;
    }

    let branch = git::current_branch(path).ok()?;
    let default_br = git::default_branch(path).unwrap_or_else(|_| "main".into());

    // Git user name via git2 config
    let git_user = git::open_repo(path)
        .ok()
        .and_then(|repo| repo.config().ok())
        .and_then(|cfg| cfg.get_string("user.name").ok())
        .unwrap_or_default();

    // Status (porcelain-style, capped at 20 files)
    let status_text = match git::get_status(path) {
        Ok(status) => {
            let mut lines = Vec::new();
            for f in &status.staged {
                let prefix = match f.status {
                    git::FileStatusKind::Deleted => "D ",
                    git::FileStatusKind::Renamed => "R ",
                    git::FileStatusKind::StagedAndModified => "MM",
                    git::FileStatusKind::Conflicted => "UU",
                    git::FileStatusKind::Staged
                    | git::FileStatusKind::Unstaged
                    | git::FileStatusKind::Untracked => "M ",
                };
                lines.push(format!("{} {}", prefix, f.path));
            }
            for f in &status.unstaged {
                let prefix = match f.status {
                    git::FileStatusKind::Deleted => " D",
                    git::FileStatusKind::Renamed => " R",
                    _ => " M",
                };
                lines.push(format!("{} {}", prefix, f.path));
            }
            for f in &status.untracked {
                lines.push(format!("?? {}", f.path));
            }
            if lines.is_empty() {
                String::new()
            } else {
                let total = lines.len();
                let mut out: Vec<String> = lines.into_iter().take(20).collect();
                if total > 20 {
                    out.push(format!("... and {} more files", total - 20));
                }
                format!("\nStatus:\n{}", out.join("\n"))
            }
        }
        Err(_) => String::new(),
    };

    // Recent commits (up to 10)
    let commits_text = match git::get_log(path, 10) {
        Ok(log) if !log.is_empty() => {
            let lines: Vec<String> = log
                .iter()
                .map(|e| format!("{} {}", e.short_sha, e.summary))
                .collect();
            format!("\nRecent commits:\n{}", lines.join("\n"))
        }
        _ => String::new(),
    };

    Some(format!(
        "gitStatus: This is the git status at the start of the conversation. \
         Note that this status is a snapshot in time, and will not update during the conversation.\n\
         \n\
         Current branch: {branch}\n\
         \n\
         Main branch (you will usually use this for PRs): {default_br}\n\
         \n\
         Git user: {git_user}\
         {status_text}\
         {commits_text}",
    ))
}

/// Corresponds to TS: `getLanguageSection(language)`
pub(super) fn language_section(language: Option<&str>) -> Option<String> {
    language.map(|lang| {
        format!(
            "# Language\n\
             Always respond in {lang}. Use {lang} for all explanations, comments, and \
             communications with the user. Technical terms and code identifiers should \
             remain in their original form.",
            lang = lang,
        )
    })
}

/// Corresponds to TS: `getMcpInstructionsSection(mcpClients)`
pub(super) fn mcp_instructions_section() -> Option<String> {
    // MCP instructions would be injected from connected MCP servers.
    // Currently returns None since we don't have MCP server instructions at prompt build time.
    None
}

pub(super) fn coordinator_prompt_section() -> Option<String> {
    use allthecodes_config::features::{self, Feature};

    features::enabled(Feature::Coordinator).then(|| {
        concat!(
            "# Coordinator Mode\n\n",
            "You are coordinating an Agent Team. Treat yourself as the team lead: ",
            "break work into small, verifiable tasks, delegate only when parallel ",
            "work materially helps, and keep ownership of final integration and verification.\n\n",
            "## Worker Coordination\n",
            "- Spawn or address workers only for bounded tasks with clear ownership.\n",
            "- Use SendMessage for direct worker updates and concise handoffs.\n",
            "- Use TaskList to inspect active work before assigning more work.\n",
            "- Use TaskStop to cancel stale, duplicate, or unsafe worker tasks.\n"
        )
        .to_string()
    })
}

pub(super) fn brief_mode_section() -> Option<String> {
    use allthecodes_config::features::{self, Feature};

    features::enabled(Feature::KairosBrief).then(|| {
        "# Brief Mode\n\n\
         All user-facing communication MUST go through the Brief tool.\n\
         Do not produce plain text output intended for the user outside of this tool.\n\
         Plain text you emit will be treated as internal reasoning and may be hidden.\n\n\
         Use Brief for:\n\
         - Status updates and progress reports\n\
         - Questions that need user input\n\
         - Final results and summaries\n\
         - Proactive notifications (set status: \"proactive\")\n"
            .to_string()
    })
}

pub(super) fn proactive_mode_section() -> Option<String> {
    use allthecodes_config::features::{self, Feature};

    features::enabled(Feature::Proactive).then(|| {
        "# Proactive Mode\n\n\
         You receive periodic <tick_tag> messages containing the user's local time\n\
         and terminal focus state.\n\n\
         ## Rules\n\
         - First tick: Greet briefly, ask what to work on. Do NOT explore unprompted.\n\
         - Subsequent ticks: Look for useful work — investigate, verify, check, commit.\n\
         - No useful work: Call Sleep tool. Do NOT emit \"still waiting\" text.\n\
         - Don't spam the user. If you already asked a question, wait for their reply.\n\
         - Bias toward action: read files, search code, make changes, commit.\n\n\
         ## Terminal Focus\n\
         - `focus: false` (user away) → Highly autonomous, execute pending tasks\n\
         - `focus: true` (user watching) → More collaborative, ask before large changes\n\n\
         ## Output\n\
         All user-facing output MUST go through the Brief tool.\n"
            .to_string()
    })
}

pub(super) fn external_channels_section() -> Option<String> {
    use allthecodes_config::features::{self, Feature};

    features::enabled(Feature::KairosChannels).then(|| {
        "# External Channels\n\n\
         You may receive messages from external channels wrapped in <channel> tags.\n\
         These are real messages from external services (Slack, GitHub, etc.).\n\
         Respond to channel messages via Brief tool with appropriate context.\n\
         Do NOT fabricate channel messages or pretend to have received one.\n"
            .to_string()
    })
}

pub(super) fn computer_use_system_prompt(tools: &[Arc<dyn Tool>]) -> Option<String> {
    const COMPUTER_USE_SERVER: &str = "computer-use";
    const COMPUTER_USE_PREFIX: &str = "mcp__computer-use__";

    let tool_list = tools
        .iter()
        .filter_map(|tool| {
            let name = tool.user_facing_name(None);
            let action = name.strip_prefix(COMPUTER_USE_PREFIX)?;
            Some(format!("- `{}` ({})", name, action))
        })
        .collect::<Vec<_>>();

    if tool_list.is_empty() {
        return None;
    }

    Some(format!(
        "# Computer Use\n\n\
         You have access to Computer Use tools from the `{}` MCP server.\n\n\
         Available Computer Use tools:\n\
         {}\n\n\
         - Always take a screenshot first before acting.\n\
         - After an input action, observe again to verify the result.\n",
        COMPUTER_USE_SERVER,
        tool_list.join("\n")
    ))
}

#[derive(Debug, Clone)]
struct BrowserToolInfo {
    full_name: String,
    server_name: String,
    action: String,
}

pub(super) fn browser_system_prompt(
    tools: &[Arc<dyn Tool>],
    browser_server_names: &std::collections::HashSet<String>,
) -> Option<String> {
    let detected = detect_browser_tools(tools, browser_server_names);
    if detected.is_empty() && browser_server_names.is_empty() {
        return None;
    }

    let mut by_server: std::collections::BTreeMap<&str, Vec<&BrowserToolInfo>> =
        std::collections::BTreeMap::new();
    for info in &detected {
        by_server
            .entry(info.server_name.as_str())
            .or_default()
            .push(info);
    }
    for server in browser_server_names {
        by_server.entry(server.as_str()).or_default();
    }

    let mut sections = Vec::new();
    for (server, infos) in &by_server {
        let mut lines = Vec::new();
        lines.push(format!("Server `{}`:", server));
        if infos.is_empty() {
            lines.push(
                "  (configured as a browser MCP server; no tools have been reported yet)"
                    .to_string(),
            );
        } else {
            for info in infos {
                let cat = allthecodes_browser::permissions::classify_browser_action(&info.action);
                lines.push(format!(
                    "  - `{}` ({}, category: {})",
                    info.full_name,
                    info.action,
                    cat.label()
                ));
            }
        }
        sections.push(lines.join("\n"));
    }

    Some(format!(
        "# Browser Automation (via MCP)\n\n\
         One or more MCP servers in this session expose browser-automation tools.\n\n\
         ## Available browser tools\n\
         {servers}\n\n\
         ## Usage guidelines\n\
         - Start from a known state and re-observe after navigation, clicks, or form submissions.\n\
         - Prefer structured selectors over coordinates when both are available.\n\
         - Do not paste user secrets into forms unless explicitly requested.\n",
        servers = sections.join("\n\n"),
    ))
}

fn detect_browser_tools(
    tools: &[Arc<dyn Tool>],
    browser_server_names: &std::collections::HashSet<String>,
) -> Vec<BrowserToolInfo> {
    let mut out = Vec::new();
    for tool in tools {
        let full_name = tool.user_facing_name(None);
        let Some(rest) = full_name.strip_prefix(allthecodes_browser::detection::MCP_PREFIX) else {
            continue;
        };
        let Some((server, action)) = rest.split_once("__") else {
            continue;
        };
        let server = server.to_string();
        let action = action.to_string();
        let is_known_action =
            allthecodes_browser::detection::BROWSER_TOOL_BASENAMES.contains(&action.as_str());
        let is_flagged_server = browser_server_names.contains(&server);
        if is_known_action || is_flagged_server {
            out.push(BrowserToolInfo {
                full_name,
                server_name: server,
                action,
            });
        }
    }
    out
}

/// Corresponds to TS: `SUMMARIZE_TOOL_RESULTS_SECTION`
pub(super) const SUMMARIZE_TOOL_RESULTS: &str =
    "When working with tool results, write down any important information you might need later \
     in your response, as the original tool result may be cleared later.";

// ═══════════════════════════════════════════════════════════════════════════
// Subsystem status reminder
// ═══════════════════════════════════════════════════════════════════════════

/// Build a system-reminder with active subsystem counts.
///
/// Returns `None` when no subsystems are active beyond defaults.
pub(super) fn build_subsystem_status_reminder() -> Option<String> {
    let lsp_configs = allthecodes_lsp_service::default_server_configs().len();
    let (mcp_count, mcp_error) = match allthecodes_mcp::discovery::discover_mcp_servers(
        &std::env::current_dir().unwrap_or_default(),
    ) {
        Ok(servers) => (servers.len(), None),
        Err(err) => {
            tracing::warn!(error = %err, "MCP server discovery failed while building system prompt");
            (0, Some(format!("MCP discovery failed: {err:#}")))
        }
    };
    let plugin_count = 0;
    let skill_count = allthecodes_skills::get_all_skills().len();
    let agent_count = crate::agent_runtime::active_agent_count();

    if mcp_count + plugin_count + skill_count == 0 && agent_count == 0 && mcp_error.is_none() {
        return None;
    }

    let mut text = format!(
        "# Active Subsystems\n\
         - LSP: {} language(s) configured\n\
         - MCP: {} server(s) configured\n\
         - Plugins: {} enabled\n\
         - Skills: {} loaded\n",
        lsp_configs, mcp_count, plugin_count, skill_count
    );
    if agent_count > 0 {
        text.push_str(&format!("- Agents: {} active\n", agent_count));
    }
    if let Some(error) = mcp_error {
        text.push_str(&format!("- MCP diagnostic: {}\n", error));
    }
    text.push_str("Use the SystemStatus tool for detailed information.\n");
    Some(text)
}
