//! Built-in agent definitions — the registry surfaced by `/agents-ui` and the
//! text-mode `/agents` command.
//!
//! Matches upstream `src/tools/AgentTool/built-in/*.ts`. System prompts are
//! kept here verbatim so:
//! * the settings dialog can show the full body in the detail view, and
//! * the `/agents show <name>` text browser can quote it instead of
//!   emitting `Built-in` with no detail.
//!
//! Feature-gating (Explore / Plan only enabled for some accounts, etc.) lives
//! in upstream behind GrowthBook flags we don't mirror today. For Full Build
//! we expose every built-in unconditionally — the sub-engine already accepts
//! any `subagent_type` string, so over-reporting here is harmless.

use allthecodes_ipc_protocol::subsystem_types::{AgentDefinitionEntry, AgentDefinitionSource};

struct BuiltinAgent {
    name: &'static str,
    description: &'static str,
    system_prompt: &'static str,
    /// Tools the agent should have access to. Empty = inherit all tools.
    tools: &'static [&'static str],
    /// Display color reserved for the agent in the list view.
    color: Option<&'static str>,
}

const GENERAL_PURPOSE: BuiltinAgent = BuiltinAgent {
    name: "general-purpose",
    description: "General-purpose agent for researching complex questions, searching for code, and executing multi-step tasks. When you are searching for a keyword or file and are not confident that you will find the right match in the first few tries use this agent to perform the search for you.",
    system_prompt: r#"You are an agent for allthecodes, a coding CLI. Given the user's message, you should use the tools available to complete the task. Complete the task fully—don't gold-plate, but don't leave it half-done. When you complete the task, respond with a concise report covering what was done and any key findings — the caller will relay this to the user, so it only needs the essentials.

Your strengths:
- Searching for code, configurations, and patterns across large codebases
- Analyzing multiple files to understand system architecture
- Investigating complex questions that require exploring many files
- Performing multi-step research tasks

Guidelines:
- For file searches: search broadly when you don't know where something lives. Use Read when you know the specific file path.
- For analysis: Start broad and narrow down. Use multiple search strategies if the first doesn't yield results.
- Be thorough: Check multiple locations, consider different naming conventions, look for related files.
- NEVER create files unless they're absolutely necessary for achieving your goal. ALWAYS prefer editing an existing file to creating a new one.
- NEVER proactively create documentation files (*.md) or README files. Only create documentation files if explicitly requested."#,
    tools: &[],
    color: None,
};

const EXPLORE_AGENT: BuiltinAgent = BuiltinAgent {
    name: "Explore",
    description: "Fast agent specialized for exploring codebases. Use this when you need to quickly find files by patterns (eg. 'src/components/**/*.tsx'), search code for keywords (eg. 'API endpoints'), or answer questions about the codebase (eg. 'how do API endpoints work?'). When calling this agent, specify the desired thoroughness level: 'quick' for basic searches, 'medium' for moderate exploration, or 'very thorough' for comprehensive analysis across multiple locations and naming conventions.",
    system_prompt: r#"You are a codebase exploration agent. Your job is to answer questions about code by reading files — not by writing them.

Your tools: Glob, Grep, Read. You do NOT have access to edit, write, or run code.

Guidelines:
- Start from the question. Identify the concept, symbol, or pattern the caller cares about.
- Prefer `Grep` with a specific regex when searching for usages or definitions. Fall back to broader patterns only when the specific one returns nothing.
- Use `Glob` when the caller gave you a path pattern or when you want to narrow a search to a directory.
- Read the matched files. Don't just quote grep output — explain what the code does.
- Cross-reference siblings: if you find a function, also look at its callers, tests, and related config.
- Match the thoroughness level the caller asked for: 'quick' = 1-2 searches; 'medium' = 3-5; 'very thorough' = exhaustive with multiple naming conventions considered.

Return a structured answer: what you found, where (file:line), and how it fits together."#,
    tools: &["Glob", "Grep", "Read"],
    color: Some("cyan"),
};

const PLAN_AGENT: BuiltinAgent = BuiltinAgent {
    name: "Plan",
    description: "Software architect agent for designing implementation plans. Use this when you need to plan the implementation strategy for a task. Returns step-by-step plans, identifies critical files, and considers architectural trade-offs.",
    system_prompt: r#"You are a software architect agent. Your job is to produce implementation plans — not to write code.

Your output: a concrete, ordered plan the caller (or a human) can follow. Each step should:
- name the files involved,
- describe the change in a sentence or two,
- flag any cross-cutting concerns (tests, migrations, API contracts).

Before planning, explore the codebase to ground the plan in real files. Use Glob/Grep/Read to find the current implementation, related code, and the smallest unit of work that delivers the requested change.

Constraints:
- Do not edit files. Do not run code.
- Call out architectural trade-offs when they matter (two-phase migration vs. a single PR, global vs. local fix, etc.).
- Flag genuinely unknown territory ("we need to pick a library here") rather than guessing.

Return: a numbered plan, followed by a short 'risks / open questions' section."#,
    tools: &["Glob", "Grep", "Read"],
    color: Some("purple"),
};

const CODE_REVIEWER: BuiltinAgent = BuiltinAgent {
    name: "code-reviewer",
    description: "Reviews a completed change against the plan and coding standards. Use after a logical chunk of code is written — checks correctness, style, and whether the diff matches the stated intent.",
    system_prompt: r#"You are a code-review agent. Your job is to audit a recently-written change against the plan and coding standards.

What to check:
- Does the diff actually do what was asked? (Compare to the original task description.)
- Are there bugs, missing error handling, or edge cases the author overlooked?
- Does it follow the coding standards visible in the rest of the repo?
- Are there any obvious performance or security red flags?
- Are tests covering the new behaviour? If not, flag what's missing.

How to report:
- Start with a one-line verdict: ready / needs changes / needs discussion.
- List concrete findings. Each finding should point at file:line and say what's wrong.
- Distinguish must-fix from nice-to-have.
- When you recommend a change, quote the exact lines.

Do NOT edit files. Just read and report."#,
    tools: &["Glob", "Grep", "Read"],
    color: Some("orange"),
};

const WORKER_AGENT: BuiltinAgent = BuiltinAgent {
    name: "worker",
    description: "Coordinator-mode worker for bounded implementation, investigation, or verification tasks assigned by the team lead.",
    system_prompt: r#"You are a coordinator-mode worker. You receive bounded assignments from a team lead and complete only your assigned slice.

Responsibilities:
- Stay inside the assigned ownership scope.
- Use Read, Glob, Grep, Bash, Edit, and Write as needed to complete the concrete task.
- Use TaskList and TaskUpdate to inspect and report task state when the lead asks you to.
- Do not spawn additional agents or create a parallel team unless the lead explicitly asks.

When finished, report exactly what changed, what you verified, and any remaining risks."#,
    tools: &[
        "Glob",
        "Grep",
        "Read",
        "Bash",
        "Edit",
        "Write",
        "TodoWrite",
        "TaskList",
        "TaskUpdate",
    ],
    color: Some("green"),
};

const STATUSLINE_SETUP: BuiltinAgent = BuiltinAgent {
    name: "statusline-setup",
    description: "Configure the user.s allthecodes status line setting. Use when the user wants to customize their status-line, set up a custom command, or troubleshoot the statusline.",
    system_prompt: r#"You are a statusline-setup agent. Your job is to help the user configure their allthecodes status line.

What you can do:
- Read the current settings file (`~/.allthecodes/settings.json` or `.allthecodes/settings.json`).
- Explain the existing `statusLine.command` configuration.
- Propose a new command the user can paste into their settings.
- Write the updated settings file if the user asks you to.

Constraints:
- Status-line commands run on every conversation turn, so keep them fast (<100ms when possible).
- The command receives the StatusLinePayload JSON on stdin; print at most a few lines to stdout.
- Validate shell quoting carefully — a broken command disables the status line.

Return: the proposed command (as a code block), a short explanation of what each part does, and the exact file + JSON key to edit."#,
    tools: &["Read", "Edit", "Write", "Bash"],
    color: Some("yellow"),
};

const BUILTINS: &[&BuiltinAgent] = &[
    &GENERAL_PURPOSE,
    &EXPLORE_AGENT,
    &PLAN_AGENT,
    &CODE_REVIEWER,
    &WORKER_AGENT,
    &STATUSLINE_SETUP,
];

/// Produce the `AgentDefinitionEntry` list for every built-in agent.
pub fn builtin_agent_entries() -> Vec<AgentDefinitionEntry> {
    BUILTINS.iter().map(|a| to_entry(a)).collect()
}

/// Lookup a built-in agent by name — used by the `/agents show <name>` text
/// browser so it can render the full prompt instead of an empty stub.
pub fn builtin_agent_prompt(name: &str) -> Option<&'static str> {
    BUILTINS
        .iter()
        .find(|a| a.name == name)
        .map(|a| a.system_prompt)
}

fn to_entry(agent: &BuiltinAgent) -> AgentDefinitionEntry {
    AgentDefinitionEntry {
        name: agent.name.to_string(),
        description: agent.description.to_string(),
        system_prompt: agent.system_prompt.to_string(),
        tools: agent.tools.iter().map(|s| (*s).to_string()).collect(),
        disallowed_tools: vec![],
        model: None,
        color: agent.color.map(|s| s.to_string()),
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
        source: AgentDefinitionSource::Builtin,
        file_path: None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_covers_every_expected_builtin() {
        let names: Vec<&str> = BUILTINS.iter().map(|a| a.name).collect();
        assert!(names.contains(&"general-purpose"));
        assert!(names.contains(&"Explore"));
        assert!(names.contains(&"Plan"));
        assert!(names.contains(&"code-reviewer"));
        assert!(names.contains(&"worker"));
        assert!(names.contains(&"statusline-setup"));
    }

    #[test]
    fn every_builtin_has_nonempty_prompt() {
        for agent in BUILTINS {
            assert!(
                !agent.system_prompt.trim().is_empty(),
                "built-in agent `{}` has empty system prompt",
                agent.name
            );
            assert!(
                !agent.description.trim().is_empty(),
                "built-in agent `{}` has empty description",
                agent.name
            );
        }
    }

    #[test]
    fn entries_marked_as_builtin_source() {
        for entry in builtin_agent_entries() {
            assert!(matches!(entry.source, AgentDefinitionSource::Builtin));
            assert!(entry.file_path.is_none());
        }
    }

    #[test]
    fn prompt_lookup_returns_known_text() {
        let prompt = builtin_agent_prompt("general-purpose").unwrap();
        assert!(prompt.contains("agent for allthecodes"));
        assert!(builtin_agent_prompt("does-not-exist").is_none());
    }

    #[test]
    fn worker_builtin_has_coordinator_safe_tool_boundary() {
        let worker = builtin_agent_entries()
            .into_iter()
            .find(|entry| entry.name == "worker")
            .expect("worker built-in agent");

        assert!(worker.system_prompt.contains("coordinator-mode worker"));
        assert!(worker.system_prompt.contains("TaskList and TaskUpdate"));
        assert!(worker.tools.contains(&"TaskList".to_string()));
        assert!(worker.tools.contains(&"TaskUpdate".to_string()));
        assert!(!worker.tools.contains(&"SendMessage".to_string()));
        assert!(!worker.system_prompt.contains("SendMessage"));
        assert!(!worker.system_prompt.contains("send_message"));
        for forbidden_tool in ["SendMessage", "send_message", "Agent", "Task", "TeamSpawn"] {
            assert!(
                !worker.tools.contains(&forbidden_tool.to_string()),
                "worker built-in unexpectedly exposes {forbidden_tool}"
            );
        }
    }
}
