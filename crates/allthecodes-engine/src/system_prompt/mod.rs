//! System prompt construction.
//!
//! Corresponds to TypeScript:
//!   `constants/prompts.ts` — `getSystemPrompt()` + static section functions
//!   `utils/systemPrompt.ts` — `buildEffectiveSystemPrompt()`
//!
//! Assembly order:
//!   1. Static sections (cacheable before DYNAMIC_BOUNDARY)
//!   2. DYNAMIC_BOUNDARY marker
//!   3. Dynamic sections (session-specific, via prompt_sections registry)
//!   4. AGENTS.md (or CLAUDE.md fallback) context injection
//!   5. Memory context injection
//!   6. Append prompt (if any)

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use tracing::debug;

use crate::config::claude_md;
use crate::prompt_sections::{self, cached_section, uncached_section, DYNAMIC_BOUNDARY};
use crate::types::tool::Tool;

mod dynamic_sections;
mod static_sections;

use dynamic_sections::*;
use static_sections::*;

// ═══════════════════════════════════════════════════════════════════════════
// Assembly functions
// ═══════════════════════════════════════════════════════════════════════════

/// Build the default system prompt parts.
///
/// Corresponds to TS: `getSystemPrompt(tools, model, dirs, mcpClients)`
///
/// `language` and `output_style` come from the resolved settings layer:
/// when present, they extend the dynamic section list with a
/// `# Language` and `# Output Style: <name>` section respectively.
///
/// Returns `(system_prompt_parts, user_context, system_context)`.
#[cfg(test)]
#[expect(clippy::too_many_arguments)]
pub fn build_system_prompt(
    custom_prompt: Option<&str>,
    append_prompt: Option<&str>,
    tools: &[Arc<dyn Tool>],
    model: &str,
    cwd: &str,
    language: Option<&str>,
    output_style: Option<&str>,
    include_auto_memory: bool,
) -> (
    Vec<String>,
    HashMap<String, String>,
    HashMap<String, String>,
) {
    build_system_prompt_with_session_memory(
        custom_prompt,
        append_prompt,
        tools,
        model,
        cwd,
        language,
        output_style,
        include_auto_memory,
        None,
    )
}

/// Build the default system prompt parts with optional session insights.
#[expect(
    clippy::too_many_arguments,
    reason = "public prompt builder keeps upstream-compatible prompt inputs explicit"
)]
pub fn build_system_prompt_with_session_memory(
    custom_prompt: Option<&str>,
    append_prompt: Option<&str>,
    tools: &[Arc<dyn Tool>],
    model: &str,
    cwd: &str,
    language: Option<&str>,
    output_style: Option<&str>,
    include_auto_memory: bool,
    session_memory_context: Option<&str>,
) -> (
    Vec<String>,
    HashMap<String, String>,
    HashMap<String, String>,
) {
    build_system_prompt_with_memory_contexts(
        custom_prompt,
        append_prompt,
        tools,
        model,
        cwd,
        language,
        output_style,
        include_auto_memory,
        None,
        session_memory_context,
    )
}

/// Build the default system prompt parts with optional prebuilt memory context
/// and session insights.
#[expect(
    clippy::too_many_arguments,
    reason = "public prompt builder keeps memory context inputs explicit"
)]
pub fn build_system_prompt_with_memory_contexts(
    custom_prompt: Option<&str>,
    append_prompt: Option<&str>,
    tools: &[Arc<dyn Tool>],
    model: &str,
    cwd: &str,
    language: Option<&str>,
    output_style: Option<&str>,
    include_auto_memory: bool,
    memory_context_override: Option<&str>,
    session_memory_context: Option<&str>,
) -> (
    Vec<String>,
    HashMap<String, String>,
    HashMap<String, String>,
) {
    let mut parts: Vec<String> = Vec::new();

    if let Some(custom) = custom_prompt {
        // Custom prompt replaces all static sections.
        parts.push(custom.to_string());
    } else {
        // ── Static sections (cacheable) ──
        parts.push(intro_section());
        parts.push(system_section());
        parts.push(doing_tasks_section());
        parts.push(actions_section().to_string());

        let enabled_tools: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        parts.push(using_tools_section(&enabled_tools));
        parts.push(tone_and_style_section());
        parts.push(output_efficiency_section().to_string());

        // ── Cache boundary ──
        parts.push(DYNAMIC_BOUNDARY.to_string());

        // ── Dynamic sections ──
        let model_owned = model.to_string();
        let cwd_owned = cwd.to_string();

        let cwd_for_git = cwd.to_string();
        let language_owned = language.map(|s| s.to_string());
        let output_style_owned = output_style.map(|s| s.to_string());
        let cwd_for_style = std::path::PathBuf::from(cwd);
        let dynamic_sections = vec![
            cached_section("env_info_simple", move || {
                Some(env_info_section(&model_owned, &cwd_owned))
            }),
            cached_section("git_status", move || git_status_section(&cwd_for_git)),
            uncached_section(
                "language",
                move || language_section(language_owned.as_deref()),
                "language is a runtime setting that may change between sessions",
            ),
            uncached_section(
                "output_style",
                move || {
                    let name = output_style_owned.as_deref()?;
                    let resolution =
                        crate::output_style::resolve_with_diagnostic(name, &cwd_for_style);
                    crate::output_style::resolution_section(&resolution)
                },
                "output style files are read from disk per session",
            ),
            cached_section("summarize_tool_results", || {
                Some(SUMMARIZE_TOOL_RESULTS.to_string())
            }),
            uncached_section(
                "mcp_instructions",
                mcp_instructions_section,
                "MCP servers connect/disconnect between turns",
            ),
            cached_section("brief_mode", brief_mode_section),
            cached_section("proactive_mode", proactive_mode_section),
            cached_section("external_channels", external_channels_section),
            uncached_section(
                "coordinator_mode",
                coordinator_prompt_section,
                "coordinator mode can be toggled for the current session",
            ),
            cached_section("subsystem_status", build_subsystem_status_reminder),
        ];

        let resolved = prompt_sections::resolve_sections(&dynamic_sections);
        parts.extend(resolved);

        // ── Computer Use system prompt (when CU tools are detected) ──
        if let Some(cu_prompt) = computer_use_system_prompt(tools) {
            parts.push(cu_prompt);
        }

        // ── Browser MCP system prompt (when browser MCP tools are detected) ──
        // Looks up the set of browser MCP server names installed during MCP
        // startup; if any tool matches (by heuristic or by config flag) we
        // emit a dedicated "# Browser Automation" section so the model knows
        // it can drive a browser and how to do so safely.
        let browser_servers = allthecodes_browser::detection::browser_servers_snapshot();
        if let Some(browser_prompt) = browser_system_prompt(tools, &browser_servers) {
            parts.push(browser_prompt);
        }

        // ── Tool descriptions ──
        let enabled: Vec<&Arc<dyn Tool>> = tools.iter().filter(|t| t.is_enabled()).collect();
        if !enabled.is_empty() {
            let mut tool_section = String::from("\n# Available tools\n");
            for tool in &enabled {
                tool_section.push_str(&format!("\n## {}\n", tool.name()));
                let schema = tool.input_json_schema();
                tool_section.push_str(&format!(
                    "Input schema: {}\n",
                    serde_json::to_string(&schema).unwrap_or_default()
                ));
            }
            parts.push(tool_section);
        }
    }

    // ── AGENTS.md context injection (always, even with custom prompt) ──
    let cwd_path = Path::new(cwd);
    match claude_md::build_agents_md_context(cwd_path) {
        Ok(context) if !context.is_empty() => {
            debug!(
                cwd = cwd,
                context_len = context.len(),
                "injecting AGENTS.md context into system prompt"
            );
            parts.push(format!(
                "# Project Instructions (AGENTS.md)\n\n\
                 IMPORTANT: These instructions OVERRIDE any default behavior \
                 and you MUST follow them exactly as written.\n\n\
                 {}",
                context
            ));
        }
        Ok(_) => {
            debug!(cwd = cwd, "no AGENTS.md files found");
        }
        Err(e) => {
            debug!(
                cwd = cwd,
                error = %e,
                "failed to load AGENTS.md context, continuing without it"
            );
        }
    }

    // ── Memory context injection ──
    let mut memory_context_parts = Vec::new();
    let memory_context_result = memory_context_override
        .map(|context| Ok(context.to_string()))
        .unwrap_or_else(|| {
            allthecodes_session::memdir::build_memory_context_with(cwd_path, include_auto_memory)
        });
    match memory_context_result {
        Ok(context) if !context.is_empty() => {
            debug!(
                cwd = cwd,
                context_len = context.len(),
                "injecting memory context into system prompt"
            );
            memory_context_parts.push(context);
        }
        Ok(_) => {
            debug!(cwd = cwd, "no memory context found");
        }
        Err(e) => {
            debug!(
                cwd = cwd,
                error = %e,
                "failed to load memory context, continuing without it"
            );
        }
    }
    if let Some(context) = session_memory_context
        .map(str::trim)
        .filter(|c| !c.is_empty())
    {
        debug!(
            cwd = cwd,
            context_len = context.len(),
            "injecting session insight context into system prompt"
        );
        memory_context_parts.push(context.to_string());
    }
    if !memory_context_parts.is_empty() {
        parts.push(format!(
            "# Memory Context\n\n\
             The following memories may contain user preferences, project facts, \
             and durable context from previous work. Use them when relevant, but \
             prefer newer conversation context when there is a conflict.\n\n\
             {}",
            memory_context_parts.join("\n\n")
        ));
    }

    // ── Append prompt ──
    if let Some(append) = append_prompt {
        parts.push(append.to_string());
    }

    // ── User context ──
    let mut user_context = HashMap::new();
    user_context.insert("cwd".to_string(), cwd.to_string());
    user_context.insert(
        "date".to_string(),
        chrono::Utc::now().format("%Y-%m-%d").to_string(),
    );
    user_context.insert("platform".to_string(), std::env::consts::OS.to_string());
    user_context.insert("model".to_string(), model.to_string());

    let system_context: HashMap<String, String> = HashMap::new();

    (parts, user_context, system_context)
}

/// Build the effective system prompt with priority-based variant selection.
///
/// Corresponds to TS: `buildEffectiveSystemPrompt({...})`
///
/// Priority:
///   0. override_prompt — replaces everything
///   1. agent_prompt — replaces default
///   2. custom_prompt — replaces default
///   3. default_prompt — standard prompt
///   + append_prompt always added at end (unless override)
#[cfg(test)]
pub fn build_effective_system_prompt(
    default_prompt: Vec<String>,
    custom_prompt: Option<&str>,
    append_prompt: Option<&str>,
    override_prompt: Option<&str>,
    agent_prompt: Option<&str>,
) -> Vec<String> {
    // Priority 0: override
    if let Some(ov) = override_prompt {
        return vec![ov.to_string()];
    }

    // Priority 1: agent replaces default
    // Priority 2: custom replaces default
    // Priority 3: default
    let mut base = if let Some(agent) = agent_prompt {
        vec![agent.to_string()]
    } else if let Some(custom) = custom_prompt {
        vec![custom.to_string()]
    } else {
        default_prompt
    };

    // Append always added (unless override, which returned early)
    if let Some(append) = append_prompt {
        base.push(append.to_string());
    }

    base
}

#[cfg(test)]
mod tests;
