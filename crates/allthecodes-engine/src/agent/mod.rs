//! Agent tool — spawns a sub-QueryEngine to handle complex tasks.
//!
//! Corresponds to TypeScript: tools/AgentTool/
//!
//! The Agent tool creates a child QueryEngine with its own conversation context,
//! runs the provided prompt through it, and returns the result to the parent.
//! This enables delegation of complex, multi-step tasks to specialized subagents.

pub(crate) mod builtin_agents;
mod dispatch;
pub mod fork;
pub mod supervisor;
mod tool_impl;
mod worktree;

#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Result};
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use crate::permissions::dangerous::set_permission_mode_with_auto_mode_safety;
use crate::types::config::{AgentContext, QueryEngineConfig};
use crate::types::tool::*;
use allthecodes_config::features::{self, Feature};
use allthecodes_tools::registry::{filter_tools_for_policy, ToolPolicy};

use allthecodes_ipc_protocol::subsystem_types::{
    AgentDefinitionEntry, AgentDefinitionSource, AgentPermissionMode,
};

/// AgentTool — spawns subagent instances to handle complex tasks.
pub struct AgentTool;

/// Upstream-compatible alias for [`AgentTool`].
///
/// Claude Code's TypeScript surface exposes this capability as `Task`; allthecodes
/// historically exposed it as `Agent`. Keeping both names lets providers that
/// emit the upstream name still execute the same subagent runtime.
pub struct TaskAgentTool;

#[derive(Deserialize)]
struct AgentInput {
    /// The task/prompt for the subagent to execute.
    prompt: String,
    /// Short (3-5 word) description of the task.
    description: Option<String>,
    /// Type of specialized subagent (e.g., "general-purpose", "Explore", "Plan").
    #[serde(default)]
    subagent_type: Option<String>,
    /// Optional model override for the subagent.
    #[serde(default)]
    model: Option<String>,
    /// Whether to run the agent in the background.
    #[serde(default)]
    run_in_background: bool,
    /// Optional teammate name. When set, AgentTool routes to Agent Teams spawn.
    #[serde(default)]
    name: Option<String>,
    /// Optional Agent Teams team name. Defaults to active team context.
    #[serde(default)]
    team_name: Option<String>,
    /// Optional teammate permission mode. `plan` requires plan approval.
    #[serde(default)]
    mode: Option<String>,
    /// Isolation mode ("worktree" for git worktree isolation).
    #[serde(default)]
    isolation: Option<String>,
    /// Optional hard cap on the child agent turn count.
    #[serde(default)]
    max_turns: Option<usize>,
}

/// Maximum depth for nested agent spawning to prevent infinite recursion.
const MAX_AGENT_DEPTH: usize = 5;

/// Resolve a public model alias to a full model ID.
fn resolve_model_alias(alias: &str, fallback: &str) -> Result<String> {
    let trimmed = alias.trim();
    if trimmed.eq_ignore_ascii_case("inherit") {
        return Ok(fallback.to_string());
    }
    if allthecodes_types::models::is_removed_legacy_model_alias(trimmed) {
        bail!(allthecodes_types::models::removed_legacy_model_alias_error(
            trimmed
        ));
    }
    Ok(allthecodes_types::models::resolve_model_alias(trimmed))
}

// ---------------------------------------------------------------------------
// Worktree isolation helpers
// ---------------------------------------------------------------------------

/// Find the git root directory from a working directory.
async fn find_git_root(cwd: &Path) -> Result<PathBuf> {
    let output = tokio::process::Command::new("git")
        .args(["-C", &cwd.to_string_lossy(), "rev-parse", "--show-toplevel"])
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("Not a git repository (or git not found): {}", stderr.trim());
    }

    let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(PathBuf::from(root))
}

/// Get the HEAD commit SHA at a given path.
async fn get_head_sha(cwd: &Path) -> Option<String> {
    let output = tokio::process::Command::new("git")
        .args(["-C", &cwd.to_string_lossy(), "rev-parse", "HEAD"])
        .output()
        .await
        .ok()?;

    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

/// Count uncommitted changes and new commits in a worktree relative to a
/// baseline commit.  Returns `(changed_files, new_commits)`.
async fn count_worktree_changes(
    worktree_path: &Path,
    original_head: Option<&str>,
) -> Option<(usize, usize)> {
    let status = tokio::process::Command::new("git")
        .args([
            "-C",
            &worktree_path.to_string_lossy(),
            "status",
            "--porcelain",
        ])
        .output()
        .await
        .ok()?;

    if !status.status.success() {
        return None;
    }

    let changed_files = String::from_utf8_lossy(&status.stdout)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .count();

    let Some(orig_head) = original_head else {
        return Some((changed_files, 0));
    };

    let rev_list = tokio::process::Command::new("git")
        .args([
            "-C",
            &worktree_path.to_string_lossy(),
            "rev-list",
            "--count",
            &format!("{}..HEAD", orig_head),
        ])
        .output()
        .await
        .ok()?;

    if !rev_list.status.success() {
        return None;
    }

    let commits = String::from_utf8_lossy(&rev_list.stdout)
        .trim()
        .parse::<usize>()
        .unwrap_or(0);

    Some((changed_files, commits))
}

#[allow(clippy::too_many_arguments)]
fn persist_agent_worktree_session_record(
    session_id: &str,
    agent_id: &str,
    original_cwd: &Path,
    git_root: &Path,
    worktree_path: &Path,
    branch_name: &str,
    original_head: Option<String>,
    source: allthecodes_session::worktree_sessions::WorktreeSessionSource,
) {
    let repo_id = allthecodes_session::storage::workspace_key(git_root);
    let record = allthecodes_session::worktree_sessions::WorktreeSessionRecord::new_active(
        session_id.to_string(),
        repo_id,
        worktree_path.to_path_buf(),
        branch_name.to_string(),
        original_head,
        active_goal_id_for_session_best_effort(session_id, agent_id),
        allthecodes_session::worktree_sessions::WorktreeSessionCreator::Agent,
        git_root.to_path_buf(),
        Some(original_cwd.to_path_buf()),
        source,
    );

    if let Err(err) = allthecodes_session::worktree_sessions::upsert_worktree_session(&record) {
        tracing::warn!(
            session_id,
            agent_id,
            worktree_path = %worktree_path.display(),
            error = %err,
            "failed to persist agent worktree session record"
        );
    }
}

fn active_goal_id_for_session_best_effort(session_id: &str, agent_id: &str) -> Option<String> {
    match allthecodes_tools::goals::active_goal_id_for_session(session_id) {
        Ok(goal_id) => goal_id,
        Err(err) => {
            tracing::warn!(
                session_id,
                agent_id,
                error = %err,
                "failed to load active goal for agent worktree session"
            );
            None
        }
    }
}

fn mark_agent_worktree_session_kept(session_id: &str, agent_id: &str, worktree_path: &Path) {
    if let Err(err) = allthecodes_session::worktree_sessions::mark_worktree_session_kept(
        session_id,
        worktree_path,
    ) {
        tracing::warn!(
            session_id,
            agent_id,
            worktree_path = %worktree_path.display(),
            error = %err,
            "failed to mark agent worktree session kept"
        );
    }
}

fn mark_agent_worktree_session_removed(session_id: &str, agent_id: &str, worktree_path: &Path) {
    if let Err(err) = allthecodes_session::worktree_sessions::mark_worktree_session_removed(
        session_id,
        worktree_path,
    ) {
        tracing::warn!(
            session_id,
            agent_id,
            worktree_path = %worktree_path.display(),
            error = %err,
            "failed to mark agent worktree session removed"
        );
    }
}

fn mark_agent_worktree_session_cleanup_failed(
    session_id: &str,
    agent_id: &str,
    worktree_path: &Path,
) {
    if let Err(err) = allthecodes_session::worktree_sessions::mark_worktree_session_cleanup_failed(
        session_id,
        worktree_path,
    ) {
        tracing::warn!(
            session_id,
            agent_id,
            worktree_path = %worktree_path.display(),
            error = %err,
            "failed to mark agent worktree session cleanup failed"
        );
    }
}

// ---------------------------------------------------------------------------
// Helper: convert SdkMessage ->AgentEvent for IPC forwarding
// ---------------------------------------------------------------------------

/// Convert an SdkMessage to an AgentEvent for IPC forwarding.
/// Returns None for messages that don't map to agent events.
pub(crate) fn sdk_to_agent_event(
    sdk_msg: &allthecodes_types::sdk::SdkMessage,
    agent_id: &str,
) -> Option<allthecodes_types::agent_events::AgentEvent> {
    use crate::types::message::{ContentBlock, StreamEvent, ToolResultContent};
    use allthecodes_types::agent_events::AgentEvent;
    use allthecodes_types::sdk::SdkMessage;

    match sdk_msg {
        SdkMessage::StreamEvent(evt) => match &evt.event {
            StreamEvent::ContentBlockDelta { delta, .. } => {
                if stream_delta_type_matches(delta, "text_delta") {
                    if let Some(text) = delta.get("text").and_then(|v| v.as_str()) {
                        return Some(AgentEvent::StreamDelta {
                            agent_id: agent_id.to_string(),
                            text: text.to_string(),
                        });
                    }
                }
                if stream_delta_type_matches(delta, "thinking_delta") {
                    if let Some(thinking) = delta.get("thinking").and_then(|v| v.as_str()) {
                        return Some(AgentEvent::ThinkingDelta {
                            agent_id: agent_id.to_string(),
                            thinking: thinking.to_string(),
                        });
                    }
                }
                None
            }
            _ => None,
        },
        SdkMessage::Assistant(a) => {
            for block in &a.message.content {
                if let ContentBlock::ToolUse { id, name, input } = block {
                    return Some(AgentEvent::ToolUse {
                        agent_id: agent_id.to_string(),
                        tool_use_id: id.clone(),
                        tool_name: name.clone(),
                        input: input.clone(),
                    });
                }
            }
            None
        }
        SdkMessage::UserReplay(replay) => {
            if let Some(blocks) = &replay.content_blocks {
                for block in blocks {
                    if let ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                    } = block
                    {
                        let output = match content {
                            ToolResultContent::Text(t) => t.clone(),
                            ToolResultContent::Blocks(_) => "[complex output]".to_string(),
                        };
                        return Some(AgentEvent::ToolResult {
                            agent_id: agent_id.to_string(),
                            tool_use_id: tool_use_id.clone(),
                            output,
                            is_error: *is_error,
                        });
                    }
                }
            }
            None
        }
        _ => None,
    }
}

fn stream_delta_type_matches(delta: &serde_json::Value, expected: &str) -> bool {
    delta
        .get("type")
        .and_then(|v| v.as_str())
        .is_none_or(|actual| actual == expected)
}

// ---------------------------------------------------------------------------
// Helper: build a child QueryEngineConfig
// ---------------------------------------------------------------------------

fn build_child_config(
    cwd: String,
    ctx: &ToolUseContext,
    agent_id: &str,
    child_agent_type: Option<&str>,
    agent_model: &str,
    parent_model: &str,
    current_depth: usize,
) -> QueryEngineConfig {
    let agent_type = child_agent_type.or(Some("general-purpose"));
    let definition = agent_type.and_then(|agent_type| {
        let child_cwd = Path::new(&cwd);
        active_agent_definition(child_cwd, agent_type).or_else(|| {
            std::env::current_dir().ok().and_then(|parent_cwd| {
                (parent_cwd != child_cwd)
                    .then(|| active_agent_definition(&parent_cwd, agent_type))?
            })
        })
    });
    let child_mcp_context = crate::mcp_tool_adapter::mcp_binding_context_for_engine(
        &cwd,
        &ctx.session_id,
        Some(&AgentContext {
            agent_id: agent_id.to_string(),
            parent_agent_id: ctx.agent_id.clone(),
            query_tracking: QueryChainTracking {
                chain_id: ctx
                    .query_tracking
                    .as_ref()
                    .map(|tracking| tracking.chain_id.clone())
                    .unwrap_or_default(),
                depth: current_depth + 1,
            },
            langfuse_session_id: ctx.langfuse_session_id.clone(),
            agent_type: child_agent_type.map(|value| value.to_string()),
            team_context: None,
            tool_permission_context: None,
        }),
    );
    let child_tools = filter_coordinator_worker_child_tools(
        filter_tools_for_optional_definition(
            crate::agent_runtime::tools_for_mcp_context(&child_mcp_context),
            definition.as_ref(),
        ),
        child_agent_type,
    );
    let chain_id = ctx
        .query_tracking
        .as_ref()
        .map(|t| t.chain_id.clone())
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let app_state = (ctx.get_app_state)();
    let mut permission_context =
        child_agent_permission_context(app_state.tool_permission_context, definition.as_ref());
    let team_context = app_state.team_context.clone();
    let mut append_system_prompt = ctx.options.append_system_prompt.clone();
    if let Some(scratchpad) = child_scratchpad_context_for_coordinator_worker(
        child_agent_type,
        team_context.as_ref(),
        ctx,
    ) {
        apply_child_scratchpad_permissions(&mut permission_context, &scratchpad.path);
        append_system_prompt = Some(append_prompt_section(
            append_system_prompt,
            scratchpad.prompt,
        ));
    }

    QueryEngineConfig {
        cwd,
        tools: child_tools,
        custom_system_prompt: ctx.options.custom_system_prompt.clone(),
        append_system_prompt,
        user_specified_model: Some(agent_model.to_string()),
        fallback_model: Some(parent_model.to_string()),
        max_turns: definition
            .as_ref()
            .and_then(|entry| entry.max_turns)
            .filter(|turns| *turns > 0)
            .map(|turns| turns as usize)
            .or(None),
        max_budget_usd: ctx.options.max_budget_usd,
        task_budget: None,
        verbose: ctx.options.verbose,
        initial_messages: None,
        commands: vec![],
        thinking_config: None,
        json_schema: None,
        replay_user_messages: false,
        persist_session: false,
        resolved_model: None,
        auto_save_session: false,
        agent_context: Some(AgentContext {
            agent_id: agent_id.to_string(),
            parent_agent_id: ctx.agent_id.clone(),
            query_tracking: QueryChainTracking {
                chain_id,
                depth: current_depth + 1,
            },
            langfuse_session_id: ctx.langfuse_session_id.clone(),
            agent_type: child_agent_type.map(|value| value.to_string()),
            team_context,
            tool_permission_context: Some(permission_context),
        }),
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct AgentRunUsage {
    pub total_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub tool_uses: Option<u64>,
}

pub(super) fn agent_run_usage_from_tracking(
    tracking: &allthecodes_types::sdk::UsageTracking,
    tool_uses: u64,
) -> AgentRunUsage {
    let total_tokens = tracking.total_input_tokens
        + tracking.total_output_tokens
        + tracking.total_cache_read_tokens
        + tracking.total_cache_creation_tokens
        + tracking.total_reasoning_output_tokens;
    AgentRunUsage {
        total_tokens: (total_tokens > 0).then_some(total_tokens),
        output_tokens: (tracking.total_output_tokens > 0).then_some(tracking.total_output_tokens),
        tool_uses: (tool_uses > 0).then_some(tool_uses),
    }
}

pub(super) fn assistant_tool_use_count(
    assistant_msg: &allthecodes_types::sdk::SdkAssistantMessage,
) -> u64 {
    assistant_msg
        .message
        .content
        .iter()
        .filter(|block| matches!(block, crate::types::message::ContentBlock::ToolUse { .. }))
        .count() as u64
}

#[derive(Debug, Clone)]
struct ChildScratchpadContext {
    path: PathBuf,
    prompt: String,
}

fn child_scratchpad_context_for_coordinator_worker(
    child_agent_type: Option<&str>,
    team_context: Option<&allthecodes_types::teams::TeamContext>,
    ctx: &ToolUseContext,
) -> Option<ChildScratchpadContext> {
    if !is_coordinator_worker_agent_type(child_agent_type) || !scratchpad_enabled() {
        return None;
    }
    let team_name = team_context?.team_name.trim();
    if team_name.is_empty() || ctx.session_id.trim().is_empty() {
        return None;
    }

    let name = format!(
        "{}-{}",
        sanitize_scratchpad_segment(team_name, "team"),
        sanitize_scratchpad_segment(&ctx.session_id, "session")
    );
    let path = allthecodes_config::paths::data_root()
        .join("scratchpads")
        .join(name);
    if let Err(err) = std::fs::create_dir_all(&path) {
        tracing::warn!(
            path = %path.display(),
            error = %err,
            "failed to create coordinator scratchpad directory for child agent"
        );
        return None;
    }

    let prompt = format!(
        "Scratchpad path: {}\n\
         Use this directory only for durable cross-worker findings that another worker may need. \
         Do not use Scratchpad as a substitute for final synthesis.",
        path.display()
    );
    Some(ChildScratchpadContext { path, prompt })
}

fn is_coordinator_worker_agent_type(agent_type: Option<&str>) -> bool {
    agent_type
        .map(str::trim)
        .is_some_and(|value| value.eq_ignore_ascii_case("worker"))
}

fn filter_coordinator_worker_child_tools(tools: Tools, child_agent_type: Option<&str>) -> Tools {
    if !features::enabled(Feature::Coordinator)
        || !is_coordinator_worker_agent_type(child_agent_type)
    {
        return tools;
    }

    filter_tools_for_policy(tools, coordinator_worker_tool_policy())
}

fn coordinator_worker_tool_policy() -> ToolPolicy {
    if coordinator_worker_simple_enabled() {
        ToolPolicy::CoordinatorWorkerSimple
    } else {
        ToolPolicy::CoordinatorWorker
    }
}

fn coordinator_worker_simple_enabled() -> bool {
    ["ALLTHECODES_COORDINATOR_SIMPLE", "CLAUDE_CODE_SIMPLE"]
        .iter()
        .any(|key| std::env::var(key).ok().is_some_and(|value| truthy(&value)))
}

fn scratchpad_enabled() -> bool {
    [
        "ALLTHECODES_COORDINATOR_SCRATCHPAD",
        "TENGU_SCRATCH",
        "CLAUDE_CODE_TENGU_SCRATCH",
    ]
    .iter()
    .any(|key| std::env::var(key).ok().is_some_and(|value| truthy(&value)))
}

fn truthy(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn sanitize_scratchpad_segment(value: &str, fallback: &str) -> String {
    let mut out = String::new();
    let mut last_was_separator = false;

    for ch in value.chars() {
        let next = if ch.is_ascii_alphanumeric() {
            last_was_separator = false;
            Some(ch.to_ascii_lowercase())
        } else if ch == '-' || ch == '_' {
            last_was_separator = true;
            Some(ch)
        } else if !last_was_separator {
            last_was_separator = true;
            Some('-')
        } else {
            None
        };

        if let Some(ch) = next {
            out.push(ch);
        }
    }

    let trimmed = out.trim_matches(['-', '_']).to_string();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed
    }
}

fn apply_child_scratchpad_permissions(context: &mut ToolPermissionContext, path: &Path) {
    let path = path.to_string_lossy().to_string();
    context.additional_working_directories.insert(
        path.clone(),
        AdditionalWorkingDirectory {
            path,
            read_only: false,
        },
    );
}

fn append_prompt_section(existing: Option<String>, section: String) -> String {
    match existing.filter(|value| !value.trim().is_empty()) {
        Some(existing) => format!("{existing}\n\n{section}"),
        None => section,
    }
}

fn active_agent_definition(
    _cwd: &Path,
    agent_type: &str,
) -> Option<allthecodes_ipc_protocol::subsystem_types::AgentDefinitionEntry> {
    crate::agent_runtime::builtin_agent_entries()
        .into_iter()
        .filter(|entry| entry.name == agent_type)
        .next_back()
}

#[cfg(test)]
fn filter_tools_for_agent_definition(tools: Tools, definition: &AgentDefinitionEntry) -> Tools {
    filter_tools_for_optional_definition(tools, Some(definition))
}

fn filter_tools_for_optional_definition(
    tools: Tools,
    definition: Option<&AgentDefinitionEntry>,
) -> Tools {
    let tools = dedupe_tools_by_name(tools);
    let Some(definition) = definition else {
        return tools;
    };

    let available: Tools = tools
        .into_iter()
        .filter(|tool| {
            if !agent_definition_allows_mcp_tool(tool.as_ref(), definition) {
                return false;
            }
            !definition
                .disallowed_tools
                .iter()
                .any(|spec| tool_matches_spec(tool.name(), spec))
        })
        .collect();

    if definition.tools.is_empty() {
        return available;
    }

    let mut resolved = Tools::new();
    let mut seen = HashSet::new();

    for spec in &definition.tools {
        for tool in &available {
            if !tool_matches_spec(tool.name(), spec) {
                continue;
            }
            if seen.insert(tool.name().to_string()) {
                resolved.push(Arc::clone(tool));
            }
        }
    }

    resolved
}

fn agent_definition_allows_mcp_tool(tool: &dyn Tool, definition: &AgentDefinitionEntry) -> bool {
    let Some(server_name) = tool.mcp_server_name() else {
        return true;
    };
    let allowlisted_servers = mcp_server_allowlist(definition);
    if !allowlisted_servers.is_empty() && !allowlisted_servers.contains(server_name) {
        return false;
    }
    if matches!(definition.source, AgentDefinitionSource::Builtin)
        && allowlisted_servers.is_empty()
        && !definition
            .tools
            .iter()
            .any(|spec| tool_name_from_spec(spec).starts_with("mcp__"))
    {
        return false;
    }
    true
}

fn mcp_server_allowlist(definition: &AgentDefinitionEntry) -> HashSet<String> {
    definition
        .mcp_servers
        .iter()
        .filter_map(mcp_server_name_from_value)
        .collect()
}

fn mcp_server_name_from_value(value: &Value) -> Option<String> {
    if let Some(name) = value.as_str() {
        return non_empty_string(name);
    }
    let object = value.as_object()?;
    if let Some(name) = object.get("name").and_then(Value::as_str) {
        return non_empty_string(name);
    }
    if object.len() == 1 {
        return object.keys().next().and_then(|name| non_empty_string(name));
    }
    None
}

fn non_empty_string(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub(super) fn agent_definition_permission_mode(
    definition: Option<&AgentDefinitionEntry>,
) -> Option<AgentPermissionMode> {
    let definition = definition?;
    if matches!(definition.source, AgentDefinitionSource::Plugin { .. }) {
        return None;
    }
    definition.permission_mode
}

pub(super) fn compose_agent_permission_mode(
    parent: &PermissionMode,
    requested: Option<AgentPermissionMode>,
) -> PermissionMode {
    match parent {
        PermissionMode::Auto
        | PermissionMode::Bypass
        | PermissionMode::AcceptEdits
        | PermissionMode::Plan
        | PermissionMode::DontAsk => parent.clone(),
        PermissionMode::Default => requested
            .map(agent_permission_mode_to_runtime)
            .unwrap_or(PermissionMode::Default),
    }
}

fn child_agent_permission_context(
    mut context: ToolPermissionContext,
    definition: Option<&AgentDefinitionEntry>,
) -> ToolPermissionContext {
    let parent_mode = context.mode.clone();
    let requested = agent_definition_permission_mode(definition);
    let effective = compose_agent_permission_mode(&parent_mode, requested);
    set_permission_mode_with_auto_mode_safety(&mut context, effective);
    context.pre_plan_mode = child_pre_plan_mode(&parent_mode, requested, &context.mode);
    context
}

fn child_pre_plan_mode(
    parent: &PermissionMode,
    requested: Option<AgentPermissionMode>,
    effective: &PermissionMode,
) -> Option<PermissionMode> {
    if effective != &PermissionMode::Plan {
        return None;
    }
    if parent == &PermissionMode::Default && requested == Some(AgentPermissionMode::Plan) {
        Some(PermissionMode::Default)
    } else {
        Some(PermissionMode::Plan)
    }
}

fn agent_permission_mode_to_runtime(mode: AgentPermissionMode) -> PermissionMode {
    match mode {
        AgentPermissionMode::Default => PermissionMode::Default,
        AgentPermissionMode::AcceptEdits => PermissionMode::AcceptEdits,
        AgentPermissionMode::BypassPermissions => PermissionMode::Bypass,
        AgentPermissionMode::Plan => PermissionMode::Plan,
    }
}

fn dedupe_tools_by_name(tools: Tools) -> Tools {
    let mut seen = HashSet::new();
    tools
        .into_iter()
        .filter(|tool| seen.insert(tool.name().to_string()))
        .collect()
}

fn tool_name_from_spec(spec: &str) -> &str {
    let trimmed = spec.trim();
    trimmed
        .split_once('(')
        .map(|(name, _)| name.trim())
        .unwrap_or(trimmed)
}

fn tool_matches_spec(tool_name: &str, spec: &str) -> bool {
    let name = tool_name_from_spec(spec);
    if name == "*" {
        return true;
    }
    if let Some(prefix) = name.strip_suffix('*') {
        return !prefix.is_empty() && tool_name.starts_with(prefix);
    }
    tool_name == name
}

// ---------------------------------------------------------------------------
// Helper: consume a child engine stream and collect text result
// ---------------------------------------------------------------------------

/// Consume a child engine stream, collecting text output.
///
/// When `ipc` is provided (sender + agent_id), intermediate streaming events
/// are forwarded through the agent IPC channel via [`sdk_to_agent_event`].
async fn collect_stream_result(
    stream: std::pin::Pin<
        Box<dyn futures::Stream<Item = allthecodes_types::sdk::SdkMessage> + Send>,
    >,
    ipc: Option<(&allthecodes_types::agent_channel::AgentSender, &str)>,
) -> (String, bool, AgentRunUsage) {
    use allthecodes_types::sdk::SdkMessage;
    use futures::StreamExt;

    let mut stream = stream;
    let mut result_text = String::new();
    let mut had_error = false;
    let mut tool_uses = 0;
    let mut usage = AgentRunUsage::default();

    while let Some(msg) = stream.next().await {
        match msg {
            SdkMessage::Assistant(ref assistant_msg) => {
                tool_uses += assistant_tool_use_count(assistant_msg);
                for block in &assistant_msg.message.content {
                    if let crate::types::message::ContentBlock::Text { text } = block {
                        if !result_text.is_empty() {
                            result_text.push('\n');
                        }
                        result_text.push_str(text);
                    }
                }
            }
            SdkMessage::Result(ref sdk_result) => {
                usage = agent_run_usage_from_tracking(&sdk_result.usage, tool_uses);
                if sdk_result.is_error {
                    had_error = true;
                    if !sdk_result.result.is_empty() {
                        result_text = sdk_result.result.clone();
                    }
                } else if result_text.is_empty() && !sdk_result.result.is_empty() {
                    result_text = sdk_result.result.clone();
                }
            }
            _ => {}
        }

        // Forward intermediate events to IPC when a sender is available
        if let Some((tx, agent_id)) = ipc {
            if let Some(agent_event) = sdk_to_agent_event(&msg, agent_id) {
                let _ = tx.send(allthecodes_types::agent_channel::AgentIpcEvent::Agent(
                    agent_event,
                ));
            }
        }
    }

    if result_text.is_empty() {
        result_text = "(Agent completed with no text output)".to_string();
    }
    if usage.tool_uses.is_none() && tool_uses > 0 {
        usage.tool_uses = Some(tool_uses);
    }

    (result_text, had_error, usage)
}

#[cfg(test)]
mod child_tool_boundary_tests {
    use super::*;
    use crate::agent_runtime::{self, AgentRuntimeAdapters, AgentToolRegistry};
    use allthecodes_config::features::{self, FeatureFlags};
    use allthecodes_ipc_protocol::subsystem_types::{AgentDefinitionEntry, AgentDefinitionSource};
    use serde_json::json;

    struct NamedTool(&'static str);

    #[async_trait::async_trait]
    impl Tool for NamedTool {
        fn name(&self) -> &str {
            self.0
        }

        async fn description(&self, _input: &serde_json::Value) -> String {
            format!("{} test tool", self.0)
        }

        fn input_json_schema(&self) -> serde_json::Value {
            json!({"type": "object"})
        }

        async fn call(
            &self,
            _input: serde_json::Value,
            _ctx: &ToolUseContext,
            _parent_message: &allthecodes_types::message::AssistantMessage,
            _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
        ) -> Result<ToolResult> {
            Ok(ToolResult::default())
        }

        async fn prompt(&self) -> String {
            String::new()
        }
    }

    fn tool_names(tools: &Tools) -> Vec<String> {
        tools.iter().map(|tool| tool.name().to_string()).collect()
    }

    fn test_tools() -> Tools {
        [
            "Glob",
            "Grep",
            "Read",
            "Bash",
            "Edit",
            "Write",
            "TodoWrite",
            "TaskList",
            "TaskUpdate",
            "Task",
            "Agent",
            "SendMessage",
            "send_message",
            "TeamSpawn",
        ]
        .into_iter()
        .map(|name| Arc::new(NamedTool(name)) as Arc<dyn Tool>)
        .collect()
    }

    fn test_definition(tools: Vec<&str>, disallowed_tools: Vec<&str>) -> AgentDefinitionEntry {
        AgentDefinitionEntry {
            name: "limited".to_string(),
            description: "Limited test agent".to_string(),
            system_prompt: "Use the listed tools only.".to_string(),
            tools: tools.into_iter().map(|name| name.to_string()).collect(),
            disallowed_tools: disallowed_tools
                .into_iter()
                .map(|name| name.to_string())
                .collect(),
            model: None,
            color: None,
            permission_mode: None,
            memory: None,
            max_turns: None,
            effort: None,
            background: false,
            isolation: None,
            skills: vec![],
            hooks: serde_json::Value::Null,
            mcp_servers: vec![],
            initial_prompt: None,
            filename: None,
            source: AgentDefinitionSource::Project,
            file_path: None,
        }
    }

    struct EnvGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set_path(key: &'static str, value: &Path) -> Self {
            let previous = std::env::var_os(key);
            unsafe {
                std::env::set_var(key, value);
            }
            Self { key, previous }
        }

        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var_os(key);
            unsafe {
                std::env::set_var(key, value);
            }
            Self { key, previous }
        }

        fn remove(key: &'static str) -> Self {
            let previous = std::env::var_os(key);
            unsafe {
                std::env::remove_var(key);
            }
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            unsafe {
                match &self.previous {
                    Some(value) => std::env::set_var(self.key, value),
                    None => std::env::remove_var(self.key),
                }
            }
        }
    }

    struct FeatureOverrideGuard;

    impl Drop for FeatureOverrideGuard {
        fn drop(&mut self) {
            features::clear_runtime_override();
        }
    }

    #[derive(Clone)]
    struct TestAgentToolRegistry(Tools);

    impl AgentToolRegistry for TestAgentToolRegistry {
        fn get_all_tools(&self) -> Vec<Arc<dyn Tool>> {
            self.0.clone()
        }
    }

    struct AgentRuntimeAdaptersGuard {
        previous: AgentRuntimeAdapters,
    }

    impl AgentRuntimeAdaptersGuard {
        fn with_tools(tools: Tools) -> Self {
            let previous = agent_runtime::agent_runtime_adapters();
            let mut updated = previous.clone();
            updated.tools = Arc::new(TestAgentToolRegistry(tools));
            agent_runtime::set_agent_runtime_adapters(updated);
            Self { previous }
        }
    }

    impl Drop for AgentRuntimeAdaptersGuard {
        fn drop(&mut self) {
            agent_runtime::set_agent_runtime_adapters(self.previous.clone());
        }
    }

    fn test_tool_context(app_state: ToolAppState, session_id: &str) -> ToolUseContext {
        let (_tx, rx) = tokio::sync::watch::channel(false);
        ToolUseContext {
            options: ToolUseOptions {
                debug: false,
                main_loop_model: "parent-model".to_string(),
                verbose: false,
                is_non_interactive_session: false,
                custom_system_prompt: None,
                append_system_prompt: None,
                max_budget_usd: None,
            },
            abort_signal: rx,
            read_file_state: FileStateCache::default(),
            get_app_state: Arc::new(move || app_state.clone()),
            set_app_state: Arc::new(|_| {}),
            session_id: session_id.to_string(),
            langfuse_session_id: session_id.to_string(),
            messages: vec![],
            agent_id: None,
            agent_type: None,
            query_tracking: None,
            permission_callback: None,
            ask_user_callback: None,
            permission_event_callback: None,
            bg_agent_tx: None,
            hook_runner: Arc::new(allthecodes_types::hooks::NoopHookRunner::new()),
            command_dispatcher: Arc::new(allthecodes_types::commands::NoopCommandDispatcher::new()),
            available_tools: vec![],
            execute_deferred_tool: None,
        }
    }

    #[test]
    fn builtin_explore_agent_receives_only_read_only_tools() {
        let definition = crate::agent_runtime::builtin_agent_entries()
            .into_iter()
            .find(|entry| entry.name == "Explore")
            .expect("Explore built-in agent");
        let tools = filter_tools_for_agent_definition(test_tools(), &definition);
        let names = tool_names(&tools);

        assert_eq!(names, vec!["Glob", "Grep", "Read"]);
        assert!(!names.contains(&"Write".to_string()));
        assert!(!names.contains(&"Bash".to_string()));
        assert!(!names.contains(&"Agent".to_string()));
    }

    #[test]
    fn custom_agent_tools_apply_allow_deny_specs_and_dedupe() {
        let definition = test_definition(
            vec!["Read", "Write", "Read", "Bash(git status)"],
            vec!["Write"],
        );
        let tools = filter_tools_for_agent_definition(test_tools(), &definition);
        let names = tool_names(&tools);

        assert_eq!(names, vec!["Read", "Bash"]);
        assert_eq!(names.iter().filter(|name| *name == "Read").count(), 1);
    }

    #[test]
    fn custom_agent_disallowed_wildcard_hides_every_tool() {
        let definition = test_definition(vec![], vec!["*"]);
        let tools = filter_tools_for_agent_definition(test_tools(), &definition);

        assert!(tools.is_empty(), "disallowedTools: * must deny all tools");
    }

    #[test]
    fn custom_agent_tool_wildcards_match_mcp_prefixes() {
        assert!(tool_matches_spec("mcp__demo__safe", "mcp__demo__*"));
        assert!(tool_matches_spec("mcp__demo__safe", "mcp__demo__safe"));
        assert!(!tool_matches_spec("mcp__other__safe", "mcp__demo__*"));
        assert!(tool_matches_spec("Bash", "Bash(git status)"));
    }

    #[test]
    fn custom_agent_permission_mode_applies_only_from_default_parent() {
        assert_eq!(
            compose_agent_permission_mode(
                &PermissionMode::Default,
                Some(AgentPermissionMode::AcceptEdits)
            ),
            PermissionMode::AcceptEdits
        );
        assert_eq!(
            compose_agent_permission_mode(&PermissionMode::Auto, Some(AgentPermissionMode::Plan)),
            PermissionMode::Auto
        );
        assert_eq!(
            compose_agent_permission_mode(
                &PermissionMode::Plan,
                Some(AgentPermissionMode::BypassPermissions)
            ),
            PermissionMode::Plan
        );
        assert_eq!(
            compose_agent_permission_mode(
                &PermissionMode::DontAsk,
                Some(AgentPermissionMode::AcceptEdits)
            ),
            PermissionMode::DontAsk
        );
    }

    #[test]
    fn plugin_agent_permission_mode_is_ignored() {
        let mut definition = test_definition(vec![], vec![]);
        definition.source = AgentDefinitionSource::Plugin {
            id: "plugin-a".to_string(),
        };
        definition.permission_mode = Some(AgentPermissionMode::BypassPermissions);

        assert_eq!(agent_definition_permission_mode(Some(&definition)), None);
    }

    #[tokio::test]
    async fn worktree_change_count_returns_none_for_unverifiable_path() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("missing-worktree");

        let changes = count_worktree_changes(&missing, Some("abc123")).await;

        assert!(changes.is_none());
    }

    #[test]
    #[serial_test::serial]
    fn scratchpad_permission_is_added_for_coordinator_worker_child() {
        let temp = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set_path("ALLTHECODES_HOME", temp.path());
        let _gate = EnvGuard::set("ALLTHECODES_COORDINATOR_SCRATCHPAD", "1");
        let _tengu = EnvGuard::remove("TENGU_SCRATCH");
        let _claude = EnvGuard::remove("CLAUDE_CODE_TENGU_SCRATCH");

        let mut app_state = ToolAppState::default();
        app_state.team_context = Some(allthecodes_types::teams::TeamContext {
            team_name: "Team A".to_string(),
            ..Default::default()
        });
        let ctx = test_tool_context(app_state, "session-123");

        let child = build_child_config(
            "/tmp".to_string(),
            &ctx,
            "worker-1",
            Some("worker"),
            "child-model",
            "parent-model",
            0,
        );

        let permission_context = child
            .agent_context
            .expect("agent context")
            .tool_permission_context
            .expect("permission context");
        let scratchpad_path = temp
            .path()
            .join("scratchpads")
            .join("team-a-session-123")
            .to_string_lossy()
            .to_string();
        let dir = permission_context
            .additional_working_directories
            .get(&scratchpad_path)
            .expect("scratchpad directory grant");
        assert_eq!(dir.path, scratchpad_path);
        assert!(!dir.read_only);
        assert!(child
            .append_system_prompt
            .as_deref()
            .unwrap_or_default()
            .contains(&scratchpad_path));
    }

    #[test]
    #[serial_test::serial]
    fn scratchpad_permission_is_not_added_for_normal_teammate_child() {
        let temp = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set_path("ALLTHECODES_HOME", temp.path());
        let _gate = EnvGuard::set("ALLTHECODES_COORDINATOR_SCRATCHPAD", "1");
        let _tengu = EnvGuard::remove("TENGU_SCRATCH");
        let _claude = EnvGuard::remove("CLAUDE_CODE_TENGU_SCRATCH");

        let mut app_state = ToolAppState::default();
        app_state.team_context = Some(allthecodes_types::teams::TeamContext {
            team_name: "Team A".to_string(),
            ..Default::default()
        });
        let ctx = test_tool_context(app_state, "session-123");

        let child = build_child_config(
            "/tmp".to_string(),
            &ctx,
            "teammate-1",
            Some("teammate"),
            "child-model",
            "parent-model",
            0,
        );

        let permission_context = child
            .agent_context
            .expect("agent context")
            .tool_permission_context
            .expect("permission context");
        assert!(permission_context.additional_working_directories.is_empty());
        assert!(child.append_system_prompt.is_none());
    }

    #[test]
    #[serial_test::serial]
    fn coordinator_worker_child_uses_strict_policy_in_normal_mode() {
        let _feature_guard = FeatureOverrideGuard;
        let _adapters_guard = AgentRuntimeAdaptersGuard::with_tools(test_tools());
        let _simple = EnvGuard::remove("ALLTHECODES_COORDINATOR_SIMPLE");
        let _claude_simple = EnvGuard::remove("CLAUDE_CODE_SIMPLE");
        let mut flags = FeatureFlags::all_disabled();
        flags.coordinator = true;
        flags.agent_teams = true;
        features::set_runtime_override(flags);

        let ctx = test_tool_context(ToolAppState::default(), "session-123");
        let child = build_child_config(
            "/tmp".to_string(),
            &ctx,
            "worker-1",
            Some("worker"),
            "child-model",
            "parent-model",
            0,
        );
        let names = tool_names(&child.tools);

        for allowed in [
            "Read",
            "Grep",
            "Glob",
            "Bash",
            "Edit",
            "Write",
            "TodoWrite",
            "TaskList",
            "TaskUpdate",
        ] {
            assert!(
                names.contains(&allowed.to_string()),
                "{allowed} should be visible to coordinator worker children"
            );
        }

        for forbidden in ["SendMessage", "send_message", "Agent", "Task", "TeamSpawn"] {
            assert!(
                !names.contains(&forbidden.to_string()),
                "{forbidden} should be hidden from coordinator worker children"
            );
        }
    }

    #[test]
    #[serial_test::serial]
    fn coordinator_worker_child_uses_simple_policy_when_enabled() {
        let _feature_guard = FeatureOverrideGuard;
        let _adapters_guard = AgentRuntimeAdaptersGuard::with_tools(test_tools());
        let _simple = EnvGuard::set("ALLTHECODES_COORDINATOR_SIMPLE", "on");
        let _claude_simple = EnvGuard::remove("CLAUDE_CODE_SIMPLE");
        let mut flags = FeatureFlags::all_disabled();
        flags.coordinator = true;
        flags.agent_teams = true;
        features::set_runtime_override(flags);

        let ctx = test_tool_context(ToolAppState::default(), "session-123");
        let child = build_child_config(
            "/tmp".to_string(),
            &ctx,
            "worker-1",
            Some("worker"),
            "child-model",
            "parent-model",
            0,
        );
        let names = tool_names(&child.tools);

        assert_eq!(names, vec!["Read", "Bash", "Edit"]);
        assert!(!names.contains(&"TaskList".to_string()));
        assert!(!names.contains(&"SendMessage".to_string()));
        assert!(!names.contains(&"Write".to_string()));
    }
}
