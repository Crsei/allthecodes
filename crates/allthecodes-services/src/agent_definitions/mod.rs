//! Agent-definition settings — loader, writer, and IPC handler.
//!
//! Backs the `/agents` settings dialog. Parallels
//! [`super::subsystem_handlers`]' MCP config editor but for agent-definition
//! markdown files (`{scope}/agents/<name>.md`).
//!
//! File format: YAML frontmatter + markdown body. Full Build parity with
//! upstream [`AgentDefinition`][upstream] covers these frontmatter keys:
//! `name`, `description`, `tools`, `disallowedTools`, `model`, `color`,
//! `permissionMode`, `memory`, `maxTurns`, `effort`, `background`,
//! `isolation`, `skills`, `hooks`, `mcpServers`, `initialPrompt`.
//!
//! ```text
//! ---
//! name: reviewer
//! description: "Reviews code against the plan"
//! tools: Read, Grep
//! model: MOTA
//! color: blue
//! permissionMode: acceptEdits
//! memory: project
//! ---
//!
//! You are a thorough reviewer. -//! ```
//!
//! Sources:
//!   * `Builtin` — engine-provided, loaded from [`super::builtin_agents`]
//!   * `User`    — `~/.allthecodes/agents/*.md`, editable
//!   * `Project` — `{cwd}/.allthecodes/agents/*.md`, editable
//!   * `Plugin`  — contributed by plugins, read-only (future hook)
//!
//! [upstream]: https://github.com/anthropics/claude-code/blob/main/src/tools/AgentTool/loadAgentsDir.ts

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

pub mod builtin;
pub mod generate;

use self::builtin::builtin_agent_entries;
use allthecodes_ipc_protocol::subsystem_events::{AgentSettingsCommand, AgentSettingsEvent};
use allthecodes_ipc_protocol::subsystem_types::{
    AgentDefinitionEntry, AgentDefinitionSource, AgentMemoryScope, AgentPermissionMode,
    AgentToolInfo, McpServerStatusInfo,
};
use allthecodes_ipc_protocol::BackendMessage;

pub trait AgentDefinitionsRuntimeHost: Send + Sync + 'static {
    fn build_mcp_server_info_list(&self) -> Vec<McpServerStatusInfo>;
}

static HOST: OnceLock<Arc<dyn AgentDefinitionsRuntimeHost>> = OnceLock::new();

pub fn set_runtime_host(host: Arc<dyn AgentDefinitionsRuntimeHost>) {
    let _ = HOST.set(host);
}

fn build_mcp_server_info_list() -> Vec<McpServerStatusInfo> {
    HOST.get()
        .map(|host| host.build_mcp_server_info_list())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Public handler
// ---------------------------------------------------------------------------

/// Handle an `AgentSettingsCommand` from the frontend.
pub fn handle(cmd: AgentSettingsCommand) -> Vec<BackendMessage> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    match cmd {
        AgentSettingsCommand::QueryList => {
            let entries = list_all_agents(&cwd);
            vec![BackendMessage::AgentSettingsEvent {
                event: AgentSettingsEvent::List { entries },
            }]
        }
        AgentSettingsCommand::Upsert { entry } => match upsert_agent(&cwd, *entry) {
            Ok(saved) => vec![BackendMessage::AgentSettingsEvent {
                event: AgentSettingsEvent::Changed {
                    name: saved.name.clone(),
                    entry: Some(Box::new(saved)),
                },
            }],
            Err((name, error)) => {
                tracing::warn!(agent = %name, %error, "agent upsert rejected");
                vec![BackendMessage::AgentSettingsEvent {
                    event: AgentSettingsEvent::Error { name, error },
                }]
            }
        },
        AgentSettingsCommand::Delete { name, source } => match delete_agent(&cwd, &name, &source) {
            Ok(()) => vec![BackendMessage::AgentSettingsEvent {
                event: AgentSettingsEvent::Changed { name, entry: None },
            }],
            Err(error) => {
                tracing::warn!(agent = %name, %error, "agent delete rejected");
                vec![BackendMessage::AgentSettingsEvent {
                    event: AgentSettingsEvent::Error { name, error },
                }]
            }
        },
        AgentSettingsCommand::QueryTools => {
            let tools = available_tools();
            vec![BackendMessage::AgentSettingsEvent {
                event: AgentSettingsEvent::ToolList { tools },
            }]
        }
        AgentSettingsCommand::OpenInEditor { file_path } => match open_in_editor(&file_path) {
            Ok(()) => vec![BackendMessage::AgentSettingsEvent {
                event: AgentSettingsEvent::EditorOpened { file_path },
            }],
            Err(error) => {
                tracing::warn!(%file_path, %error, "editor launch failed");
                vec![BackendMessage::AgentSettingsEvent {
                    event: AgentSettingsEvent::Error {
                        name: file_path,
                        error,
                    },
                }]
            }
        },
        AgentSettingsCommand::Generate {
            user_prompt,
            mut existing_names,
        } => {
            // Preseed with built-in names so the model can't pick an
            // identifier that would collide with an engine default.
            for name in builtin::builtin_agent_names() {
                if !existing_names.contains(&name) {
                    existing_names.push(name);
                }
            }
            // Dispatch the generation asynchronously so the handler returns
            // quickly with `GenerateStarted`. The spawned task posts
            // `Generated` / `Error` onto the event bus when the model
            // returns (see [`super::runtime::HeadlessRuntime::run`]).
            let events = vec![BackendMessage::AgentSettingsEvent {
                event: AgentSettingsEvent::GenerateStarted,
            }];
            generate::spawn_generation(user_prompt, existing_names);
            events
        }
    }
}

// ---------------------------------------------------------------------------
// Discovery — list every definition across all sources
// ---------------------------------------------------------------------------

/// Return every agent the UI should know about: built-ins + user + project.
pub fn list_all_agents(cwd: &Path) -> Vec<AgentDefinitionEntry> {
    let mut out: Vec<AgentDefinitionEntry> = builtin_agent_entries();

    let user_dir = allthecodes_config::paths::data_root().join("agents");
    out.extend(load_agents_from_dir(&user_dir, AgentDefinitionSource::User));

    let project_dir = cwd.join(".allthecodes").join("agents");
    out.extend(load_agents_from_dir(
        &project_dir,
        AgentDefinitionSource::Project,
    ));

    out
}

fn load_agents_from_dir(dir: &Path, source: AgentDefinitionSource) -> Vec<AgentDefinitionEntry> {
    let Ok(read_dir) = fs::read_dir(dir) else {
        return vec![];
    };
    let mut out = Vec::new();
    for entry in read_dir.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let Ok(raw) = fs::read_to_string(&path) else {
            continue;
        };
        if let Some(parsed) = parse_agent_file(&path, &raw, source.clone()) {
            out.push(parsed);
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

// ---------------------------------------------------------------------------
// Frontmatter parsing / rendering
// ---------------------------------------------------------------------------

/// Parse a single agent markdown file. Returns `None` if the file is
/// malformed beyond recovery (no name resolvable).
pub fn parse_agent_file(
    path: &Path,
    content: &str,
    source: AgentDefinitionSource,
) -> Option<AgentDefinitionEntry> {
    let (fm, body) = parse_frontmatter(content);
    let file_stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string());
    let name = fm_get(&fm, "name").or_else(|| file_stem.clone())?;
    let description = fm_get(&fm, "description").unwrap_or_default();
    let tools = normalize_tools(
        fm_get(&fm, "tools")
            .map(|v| split_list(&v))
            .unwrap_or_default(),
    );
    let disallowed_tools = normalize_disallowed_tools(
        fm_get(&fm, "disallowedTools")
            .or_else(|| fm_get(&fm, "disallowed_tools"))
            .map(|v| split_list(&v))
            .unwrap_or_default(),
    );
    let model = fm_get(&fm, "model").filter(|s| !s.is_empty());
    let color = fm_get(&fm, "color").filter(|s| !s.is_empty());
    let permission_mode = fm_get(&fm, "permissionMode")
        .or_else(|| fm_get(&fm, "permission_mode"))
        .and_then(parse_permission_mode);
    let memory = fm_get(&fm, "memory").and_then(parse_memory_scope);
    let max_turns = fm_get(&fm, "maxTurns")
        .or_else(|| fm_get(&fm, "max_turns"))
        .and_then(|v| v.parse::<u32>().ok());
    let effort = fm_get(&fm, "effort").filter(|s| !s.is_empty());
    let background = fm_get(&fm, "background")
        .map(|v| matches!(v.to_ascii_lowercase().as_str(), "true" | "yes" | "1"))
        .unwrap_or(false);
    let isolation = normalize_isolation(fm_get(&fm, "isolation"));
    let skills = fm_get(&fm, "skills")
        .map(|v| split_list(&v))
        .unwrap_or_default();
    let initial_prompt = fm_get(&fm, "initialPrompt")
        .or_else(|| fm_get(&fm, "initial_prompt"))
        .filter(|s| !s.is_empty());

    // Hooks and mcpServers may be multiline JSON blocks; our minimal parser
    // only exposes single-line values. We accept them as raw JSON strings in
    // the frontmatter or ignore if missing — editors can reshape later.
    let hooks = fm_get(&fm, "hooks")
        .and_then(|v| serde_json::from_str(&v).ok())
        .unwrap_or(serde_json::Value::Null);
    let mcp_servers = fm_get(&fm, "mcpServers")
        .or_else(|| fm_get(&fm, "mcp_servers"))
        .and_then(|v| serde_json::from_str::<serde_json::Value>(&v).ok())
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default();

    let filename = file_stem.filter(|s| s != &name);

    Some(AgentDefinitionEntry {
        name,
        description,
        system_prompt: body.trim_start_matches('\n').to_string(),
        tools,
        disallowed_tools,
        model,
        color,
        permission_mode,
        memory,
        max_turns,
        effort,
        background,
        isolation,
        skills,
        hooks,
        mcp_servers,
        initial_prompt,
        filename,
        source,
        file_path: Some(path.to_string_lossy().to_string()),
    })
}

/// Render an `AgentDefinitionEntry` back to on-disk markdown.
fn render_agent_file(entry: &AgentDefinitionEntry) -> String {
    let mut out = String::new();
    out.push_str("---\n");
    out.push_str(&format!("name: {}\n", entry.name));
    if !entry.description.is_empty() {
        out.push_str(&format!(
            "description: \"{}\"\n",
            yaml_escape(&entry.description)
        ));
    }
    if !entry.tools.is_empty() {
        out.push_str(&format!("tools: {}\n", entry.tools.join(", ")));
    }
    if !entry.disallowed_tools.is_empty() {
        out.push_str(&format!(
            "disallowedTools: {}\n",
            entry.disallowed_tools.join(", ")
        ));
    }
    if let Some(model) = &entry.model {
        if !model.is_empty() {
            out.push_str(&format!("model: {}\n", model));
        }
    }
    if let Some(color) = &entry.color {
        if !color.is_empty() {
            out.push_str(&format!("color: {}\n", color));
        }
    }
    if let Some(mode) = entry.permission_mode {
        out.push_str(&format!("permissionMode: {}\n", permission_mode_str(mode)));
    }
    if let Some(scope) = entry.memory {
        out.push_str(&format!("memory: {}\n", memory_scope_str(scope)));
    }
    if let Some(mt) = entry.max_turns {
        out.push_str(&format!("maxTurns: {}\n", mt));
    }
    if let Some(effort) = &entry.effort {
        if !effort.is_empty() {
            out.push_str(&format!("effort: {}\n", effort));
        }
    }
    if entry.background {
        out.push_str("background: true\n");
    }
    if let Some(iso) = &entry.isolation {
        if !iso.is_empty() {
            out.push_str(&format!("isolation: {}\n", iso));
        }
    }
    if !entry.skills.is_empty() {
        out.push_str(&format!("skills: {}\n", entry.skills.join(", ")));
    }
    if !entry.hooks.is_null() {
        out.push_str(&format!(
            "hooks: {}\n",
            serde_json::to_string(&entry.hooks).unwrap_or_else(|_| "null".into())
        ));
    }
    if !entry.mcp_servers.is_empty() {
        out.push_str(&format!(
            "mcpServers: {}\n",
            serde_json::to_string(&entry.mcp_servers).unwrap_or_else(|_| "[]".into())
        ));
    }
    if let Some(ip) = &entry.initial_prompt {
        if !ip.is_empty() {
            out.push_str(&format!("initialPrompt: \"{}\"\n", yaml_escape(ip)));
        }
    }
    out.push_str("---\n\n");
    out.push_str(entry.system_prompt.trim_start_matches('\n'));
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Minimal YAML-frontmatter parser (single-line `key: value` pairs plus
/// optional leading/trailing `---`). Case-preserving.
fn parse_frontmatter(content: &str) -> (Vec<(String, String)>, String) {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return (vec![], content.to_string());
    }
    let after_open = &trimmed[3..];
    let Some(close_pos) = after_open.find("\n---") else {
        return (vec![], content.to_string());
    };
    let yaml_block = &after_open[..close_pos];
    let body = &after_open[close_pos + 4..];
    let mut out = Vec::new();
    for line in yaml_block.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim().to_string();
        let value = value.trim();
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .unwrap_or(value)
            .to_string();
        out.push((key, value));
    }
    (out, body.to_string())
}

fn fm_get(fm: &[(String, String)], key: &str) -> Option<String> {
    fm.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
}

fn split_list(value: &str) -> Vec<String> {
    value
        .split([',', ' '])
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

fn normalize_list(values: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for value in values {
        let value = value.trim();
        if value.is_empty() || out.iter().any(|existing| existing == value) {
            continue;
        }
        out.push(value.to_string());
    }
    out
}

/// Normalize the `tools` list: `["*"]` becomes `[]` (meaning "all tools"), so
/// the rest of the code only has to check for an empty vec.
fn normalize_tools(tools: Vec<String>) -> Vec<String> {
    let tools = normalize_list(tools);
    if tools.len() == 1 && tools[0] == "*" {
        vec![]
    } else {
        tools.into_iter().filter(|tool| tool != "*").collect()
    }
}

fn normalize_disallowed_tools(tools: Vec<String>) -> Vec<String> {
    normalize_list(tools)
}

fn normalize_isolation(value: Option<String>) -> Option<String> {
    let value = value?.trim().to_string();
    if value.is_empty() {
        return None;
    }
    if value.eq_ignore_ascii_case("worktree") {
        Some("worktree".to_string())
    } else {
        Some(value)
    }
}

fn parse_permission_mode(value: String) -> Option<AgentPermissionMode> {
    match value.trim() {
        "default" => Some(AgentPermissionMode::Default),
        "acceptEdits" | "accept_edits" => Some(AgentPermissionMode::AcceptEdits),
        "bypassPermissions" | "bypass_permissions" => Some(AgentPermissionMode::BypassPermissions),
        "plan" => Some(AgentPermissionMode::Plan),
        _ => None,
    }
}

fn permission_mode_str(mode: AgentPermissionMode) -> &'static str {
    match mode {
        AgentPermissionMode::Default => "default",
        AgentPermissionMode::AcceptEdits => "acceptEdits",
        AgentPermissionMode::BypassPermissions => "bypassPermissions",
        AgentPermissionMode::Plan => "plan",
    }
}

fn parse_memory_scope(value: String) -> Option<AgentMemoryScope> {
    match value.as_str() {
        "user" => Some(AgentMemoryScope::User),
        "project" => Some(AgentMemoryScope::Project),
        "local" => Some(AgentMemoryScope::Local),
        _ => None,
    }
}

fn memory_scope_str(scope: AgentMemoryScope) -> &'static str {
    match scope {
        AgentMemoryScope::User => "user",
        AgentMemoryScope::Project => "project",
        AgentMemoryScope::Local => "local",
    }
}

/// Escape a string for use inside a double-quoted YAML value. Matches the
/// upstream `formatAgentAsMarkdown` encoding (backslash first, then quotes
/// and newlines) so a round-trip through the editor produces identical YAML.
fn yaml_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\\\n")
}

// ---------------------------------------------------------------------------
// Persistence: upsert + delete
// ---------------------------------------------------------------------------

fn upsert_agent(
    cwd: &Path,
    mut entry: AgentDefinitionEntry,
) -> Result<AgentDefinitionEntry, (String, String)> {
    if !entry.source.is_editable() {
        return Err((
            entry.name.clone(),
            format!(
                "source `{}` is read-only — cannot edit agent",
                entry.source.label()
            ),
        ));
    }
    validate_name(&entry.name).map_err(|e| (entry.name.clone(), e))?;
    entry.tools = normalize_tools(entry.tools);
    entry.disallowed_tools = normalize_disallowed_tools(entry.disallowed_tools);
    entry.isolation = normalize_isolation(entry.isolation);
    validate_security_fields(&entry).map_err(|e| (entry.name.clone(), e))?;

    let dir = agents_dir_for_source(cwd, &entry.source);
    fs::create_dir_all(&dir).map_err(|e| {
        (
            entry.name.clone(),
            format!("failed to create {}: {}", dir.display(), e),
        )
    })?;

    let path = dir.join(format!("{}.md", entry.name));
    let rendered = render_agent_file(&entry);
    let tmp = path.with_extension("md.tmp");
    fs::write(&tmp, &rendered).map_err(|e| {
        (
            entry.name.clone(),
            format!("failed to write {}: {}", tmp.display(), e),
        )
    })?;
    fs::rename(&tmp, &path).map_err(|e| {
        (
            entry.name.clone(),
            format!(
                "failed to rename {} -> {}: {}",
                tmp.display(),
                path.display(),
                e
            ),
        )
    })?;

    entry.file_path = Some(path.to_string_lossy().to_string());
    entry.filename = None;
    Ok(entry)
}

fn validate_security_fields(entry: &AgentDefinitionEntry) -> Result<(), String> {
    if let Some(isolation) = entry.isolation.as_deref() {
        if !isolation.eq_ignore_ascii_case("worktree") {
            return Err(format!(
                "unsupported isolation `{}`; only `worktree` is supported",
                isolation
            ));
        }
    }
    if entry.max_turns == Some(0) {
        return Err("maxTurns must be greater than 0".to_string());
    }
    Ok(())
}

fn delete_agent(cwd: &Path, name: &str, source: &AgentDefinitionSource) -> Result<(), String> {
    if !source.is_editable() {
        return Err(format!(
            "source `{}` is read-only — cannot delete agent",
            source.label()
        ));
    }
    validate_name(name)?;
    let path = agents_dir_for_source(cwd, source).join(format!("{}.md", name));
    if !path.exists() {
        return Err(format!(
            "no agent file at {} — nothing to delete",
            path.display()
        ));
    }
    fs::remove_file(&path).map_err(|e| format!("failed to remove {}: {}", path.display(), e))?;
    Ok(())
}

fn agents_dir_for_source(cwd: &Path, source: &AgentDefinitionSource) -> PathBuf {
    match source {
        AgentDefinitionSource::User => allthecodes_config::paths::data_root().join("agents"),
        AgentDefinitionSource::Project => cwd.join(".allthecodes").join("agents"),
        // Read-only scopes are rejected earlier, but return something harmless.
        AgentDefinitionSource::Builtin | AgentDefinitionSource::Plugin { .. } => cwd.join(""),
    }
}

/// Agent names become filenames on disk — reject anything that would escape
/// the directory or produce an ambiguous path. Matches upstream
/// `validateAgentType` regex plus a length cap.
fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("agent name must not be empty".to_string());
    }
    if name.len() > 64 {
        return Err("agent name must be 64 characters or fewer".to_string());
    }
    let ok = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !ok {
        return Err("agent name may only contain letters, digits, `-`, and `_`".to_string());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Available tools — categorized for the ToolSelector UI
// ---------------------------------------------------------------------------

/// Return the flat list of tools agents can be restricted to, tagged with a
/// bucket category for the `ToolSelector` UI. Matches the bucket mapping
/// from upstream `components/agents/ToolSelector.tsx`:
///   * read_only — Glob, Grep, Read, WebFetch, WebSearch, TodoWrite, ExitPlanMode
///   * edit      — Edit, Write, NotebookEdit
///   * execution — Bash
///   * other     — Agent (Task), Skill, LSP, MCP lifecycle, etc.
///
/// MCP-provided tools are appended with category `"mcp"` and their
/// originating server recorded in `mcp_server`.
pub fn available_tools() -> Vec<AgentToolInfo> {
    // Base list matches the shared tool registry names — duplicated
    // here so the IPC doesn't depend on the tool registry's lazy_static at
    // call time. The registry is the source of truth at runtime; if it
    // diverges, add a doctest or a cross-check (see tests below).
    const READ_ONLY: &[&str] = &[
        "Glob",
        "Grep",
        "Read",
        "TodoWrite",
        "WebFetch",
        "WebSearch",
        "ExitPlanMode",
        "Thinking",
    ];
    const EDIT: &[&str] = &["Edit", "Write", "NotebookEdit"];
    const EXECUTION: &[&str] = &["Bash"];
    const OTHER: &[&str] = &[
        "Agent",
        "Skill",
        "ToolSearch",
        "LSP",
        "Sleep",
        "Brief",
        "AskUserQuestion",
        "Monitor",
        "SendMessage",
        "TeamSpawn",
    ];

    let mut out: Vec<AgentToolInfo> = Vec::new();
    for name in READ_ONLY {
        out.push(AgentToolInfo {
            name: (*name).to_string(),
            category: "read_only".into(),
            description: None,
            mcp_server: None,
        });
    }
    for name in EDIT {
        out.push(AgentToolInfo {
            name: (*name).to_string(),
            category: "edit".into(),
            description: None,
            mcp_server: None,
        });
    }
    for name in EXECUTION {
        out.push(AgentToolInfo {
            name: (*name).to_string(),
            category: "execution".into(),
            description: None,
            mcp_server: None,
        });
    }
    for name in OTHER {
        out.push(AgentToolInfo {
            name: (*name).to_string(),
            category: "other".into(),
            description: None,
            mcp_server: None,
        });
    }

    // MCP tools — pull from the subsystem snapshot so the editor can restrict
    // agents to specific MCP servers. Names are pre-prefixed with
    // `mcp__{server}__{tool}` to match how they appear at tool-use time.
    let mcp_servers = build_mcp_server_info_list();
    for server in mcp_servers {
        out.push(AgentToolInfo {
            name: format!("mcp__{}__*", server.name),
            category: "mcp".into(),
            description: Some(format!(
                "{} tools ({} registered)",
                server.name, server.tools_count
            )),
            mcp_server: Some(server.name),
        });
    }

    out
}

// ---------------------------------------------------------------------------
// External editor launch
// ---------------------------------------------------------------------------

/// Spawn `$EDITOR` (or the platform default) against the given path and wait
/// for it to return. Matches the upstream `editFileInEditor` semantics: the
/// editor blocks while the user edits, and we return when it exits.
///
/// On Windows the fallback is `notepad`; elsewhere `vi`. We explicitly refuse
/// paths outside the editable agent directories so a malicious IPC payload
/// cannot trigger an editor on arbitrary system files.
fn open_in_editor(file_path: &str) -> Result<(), String> {
    let path = PathBuf::from(file_path);
    if !path.is_absolute() {
        return Err("file path must be absolute".into());
    }
    if !is_inside_agents_dir(&path) {
        return Err("path is outside any known agents directory".into());
    }

    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| default_editor().into());

    // Split on spaces to honour "code --wait" style settings.
    let mut parts = editor.split_whitespace();
    let program = parts.next().ok_or_else(|| "empty EDITOR".to_string())?;
    let args: Vec<&str> = parts.collect();

    let status = std::process::Command::new(program)
        .args(&args)
        .arg(&path)
        .status()
        .map_err(|e| format!("failed to spawn editor `{}`: {}", program, e))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "editor `{}` exited with status {}",
            program, status
        ))
    }
}

fn default_editor() -> &'static str {
    if cfg!(windows) {
        "notepad"
    } else {
        "vi"
    }
}

fn is_inside_agents_dir(path: &Path) -> bool {
    let user_dir = allthecodes_config::paths::data_root().join("agents");
    let Ok(user_dir) = user_dir.canonicalize() else {
        return path_prefixed_by(path, &user_dir);
    };
    if path_prefixed_by(path, &user_dir) {
        return true;
    }
    // Project dir is cwd-relative; canonicalize to catch symlinks.
    let Ok(cwd) = std::env::current_dir() else {
        return false;
    };
    let project_dir = cwd.join(".allthecodes").join("agents");
    let project_dir = project_dir.canonicalize().unwrap_or(project_dir);
    path_prefixed_by(path, &project_dir)
}

fn path_prefixed_by(path: &Path, prefix: &Path) -> bool {
    let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    resolved.starts_with(prefix)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_entry(name: &str, source: AgentDefinitionSource) -> AgentDefinitionEntry {
        AgentDefinitionEntry {
            name: name.to_string(),
            description: format!("Agent {} description", name),
            system_prompt: "You are a helpful agent.\n\nDo your best work.\n".to_string(),
            tools: vec!["Read".to_string(), "Grep".to_string()],
            disallowed_tools: vec![],
            model: Some("MOTA".to_string()),
            color: Some("blue".to_string()),
            permission_mode: Some(AgentPermissionMode::AcceptEdits),
            memory: Some(AgentMemoryScope::Project),
            max_turns: Some(10),
            effort: Some("medium".to_string()),
            background: false,
            isolation: None,
            skills: vec![],
            hooks: serde_json::Value::Null,
            mcp_servers: vec![],
            initial_prompt: None,
            filename: None,
            source,
            file_path: None,
        }
    }

    #[test]
    fn validate_name_rejects_traversal_and_weirdness() {
        assert!(validate_name("").is_err());
        assert!(validate_name("../escape").is_err());
        assert!(validate_name("slash/inside").is_err());
        assert!(validate_name("dot.inside").is_err());
        assert!(validate_name("good_name-1").is_ok());
    }

    #[test]
    fn upsert_and_reload_round_trips_project_scope() {
        let tmp = tempdir().unwrap();
        let entry = make_entry("reviewer", AgentDefinitionSource::Project);

        let saved = upsert_agent(tmp.path(), entry.clone()).unwrap();
        assert_eq!(saved.name, "reviewer");
        let expected = tmp
            .path()
            .join(".allthecodes")
            .join("agents")
            .join("reviewer.md");
        assert!(expected.exists(), "file should have been written");

        let loaded = load_agents_from_dir(
            &tmp.path().join(".allthecodes").join("agents"),
            AgentDefinitionSource::Project,
        );
        assert_eq!(loaded.len(), 1);
        let back = &loaded[0];
        assert_eq!(back.name, "reviewer");
        assert_eq!(back.description, "Agent reviewer description");
        assert_eq!(back.tools, vec!["Read".to_string(), "Grep".to_string()]);
        assert_eq!(back.model.as_deref(), Some("MOTA"));
        assert_eq!(back.color.as_deref(), Some("blue"));
        assert_eq!(back.permission_mode, Some(AgentPermissionMode::AcceptEdits));
        assert_eq!(back.memory, Some(AgentMemoryScope::Project));
        assert_eq!(back.max_turns, Some(10));
        assert_eq!(back.effort.as_deref(), Some("medium"));
        assert!(back.system_prompt.contains("You are a helpful agent."));
    }

    #[test]
    fn upsert_rejects_builtin_source() {
        let tmp = tempdir().unwrap();
        let entry = make_entry("general-purpose", AgentDefinitionSource::Builtin);
        let err = upsert_agent(tmp.path(), entry).unwrap_err();
        assert!(err.1.contains("read-only"));
    }

    #[test]
    fn delete_round_trips_and_is_rejected_for_builtin() {
        let tmp = tempdir().unwrap();
        let entry = make_entry("temp", AgentDefinitionSource::Project);
        upsert_agent(tmp.path(), entry).unwrap();

        let path = tmp
            .path()
            .join(".allthecodes")
            .join("agents")
            .join("temp.md");
        assert!(path.exists());

        delete_agent(tmp.path(), "temp", &AgentDefinitionSource::Project).unwrap();
        assert!(!path.exists());

        let err = delete_agent(
            tmp.path(),
            "general-purpose",
            &AgentDefinitionSource::Builtin,
        )
        .unwrap_err();
        assert!(err.contains("read-only"));
    }

    #[test]
    fn parse_agent_file_extracts_all_frontmatter_fields() {
        let raw = "---\nname: foo\ndescription: \"A foo agent\"\ntools: Read, Bash\nmodel: SOTA\ncolor: red\npermissionMode: plan\nmemory: user\nmaxTurns: 20\neffort: high\nbackground: true\nisolation: worktree\nskills: simplify, loop\n---\n\nBody text here.\n";
        let parsed = parse_agent_file(
            &PathBuf::from("/tmp/foo.md"),
            raw,
            AgentDefinitionSource::User,
        )
        .expect("should parse");
        assert_eq!(parsed.name, "foo");
        assert_eq!(parsed.description, "A foo agent");
        assert_eq!(parsed.tools, vec!["Read".to_string(), "Bash".to_string()]);
        assert_eq!(parsed.model.as_deref(), Some("SOTA"));
        assert_eq!(parsed.color.as_deref(), Some("red"));
        assert_eq!(parsed.permission_mode, Some(AgentPermissionMode::Plan));
        assert_eq!(parsed.memory, Some(AgentMemoryScope::User));
        assert_eq!(parsed.max_turns, Some(20));
        assert_eq!(parsed.effort.as_deref(), Some("high"));
        assert!(parsed.background);
        assert_eq!(parsed.isolation.as_deref(), Some("worktree"));
        assert_eq!(
            parsed.skills,
            vec!["simplify".to_string(), "loop".to_string()]
        );
        assert!(parsed.system_prompt.starts_with("Body text here."));
    }

    #[test]
    fn tools_wildcard_is_normalized_to_empty() {
        let raw = "---\nname: any\ntools: \"*\"\n---\nBody\n";
        let parsed = parse_agent_file(
            &PathBuf::from("/tmp/any.md"),
            raw,
            AgentDefinitionSource::User,
        )
        .expect("should parse");
        assert!(parsed.tools.is_empty(), "`*` should normalize to empty");
    }

    #[test]
    fn tool_security_lists_are_normalized() {
        let raw = "---\nname: bounded\ntools: \"*, Read, Read\"\ndisallowedTools: \"Write, Write, *\"\nisolation: WorkTree\n---\nBody\n";
        let parsed = parse_agent_file(
            &PathBuf::from("/tmp/bounded.md"),
            raw,
            AgentDefinitionSource::User,
        )
        .expect("should parse");

        assert_eq!(parsed.tools, vec!["Read".to_string()]);
        assert_eq!(
            parsed.disallowed_tools,
            vec!["Write".to_string(), "*".to_string()]
        );
        assert_eq!(parsed.isolation.as_deref(), Some("worktree"));
    }

    #[test]
    fn upsert_rejects_unknown_isolation() {
        let tmp = tempdir().unwrap();
        let mut entry = make_entry("sandboxed", AgentDefinitionSource::Project);
        entry.isolation = Some("sandbox".to_string());

        let err = upsert_agent(tmp.path(), entry).unwrap_err();

        assert!(err.1.contains("unsupported isolation"));
    }

    #[test]
    fn upsert_rejects_zero_max_turns() {
        let tmp = tempdir().unwrap();
        let mut entry = make_entry("zero-turns", AgentDefinitionSource::Project);
        entry.max_turns = Some(0);

        let err = upsert_agent(tmp.path(), entry).unwrap_err();

        assert!(err.1.contains("maxTurns"));
    }

    #[test]
    fn filename_populated_when_file_stem_differs_from_name() {
        let raw = "---\nname: canonical-name\n---\nBody\n";
        let parsed = parse_agent_file(
            &PathBuf::from("/tmp/on-disk-name.md"),
            raw,
            AgentDefinitionSource::User,
        )
        .expect("should parse");
        assert_eq!(parsed.name, "canonical-name");
        assert_eq!(parsed.filename.as_deref(), Some("on-disk-name"));
    }

    #[test]
    fn list_all_agents_always_includes_builtins() {
        let tmp = tempdir().unwrap();
        let list = list_all_agents(tmp.path());
        let names: Vec<&str> = list.iter().map(|e| e.name.as_str()).collect();
        // Match the registry defined in builtin_agents.rs
        assert!(names.contains(&"general-purpose"));
        assert!(names.contains(&"Explore"));
        assert!(names.contains(&"Plan"));
        assert!(names.contains(&"code-reviewer"));
        assert!(names.contains(&"statusline-setup"));
    }

    #[test]
    fn render_agent_file_includes_all_populated_fields() {
        let entry = make_entry("rtrip", AgentDefinitionSource::User);
        let rendered = render_agent_file(&entry);
        assert!(rendered.starts_with("---\n"));
        assert!(rendered.contains("name: rtrip"));
        assert!(rendered.contains("tools: Read, Grep"));
        assert!(rendered.contains("model: MOTA"));
        assert!(rendered.contains("color: blue"));
        assert!(rendered.contains("permissionMode: acceptEdits"));
        assert!(rendered.contains("memory: project"));
        assert!(rendered.contains("maxTurns: 10"));
        assert!(rendered.contains("effort: medium"));
        assert!(rendered.contains("You are a helpful agent."));
    }

    #[test]
    fn agent_source_editable_flag() {
        assert!(!AgentDefinitionSource::Builtin.is_editable());
        assert!(AgentDefinitionSource::User.is_editable());
        assert!(AgentDefinitionSource::Project.is_editable());
        assert!(!AgentDefinitionSource::Plugin { id: "p".into() }.is_editable());
    }

    #[test]
    fn available_tools_covers_every_bucket() {
        let tools = available_tools();
        let cats: std::collections::HashSet<_> =
            tools.iter().map(|t| t.category.as_str()).collect();
        assert!(cats.contains("read_only"));
        assert!(cats.contains("edit"));
        assert!(cats.contains("execution"));
        // `other` is always populated; `mcp` only appears when servers are
        // configured, so skip it.
        assert!(cats.contains("other"));
        assert!(tools.iter().any(|t| t.name == "Read"));
        assert!(tools.iter().any(|t| t.name == "Bash"));
    }

    #[test]
    fn open_in_editor_rejects_paths_outside_agents_dirs() {
        let err = open_in_editor("/etc/passwd").unwrap_err();
        assert!(err.contains("outside") || err.contains("absolute"));
    }

    #[test]
    fn open_in_editor_rejects_relative_paths() {
        let err = open_in_editor("foo.md").unwrap_err();
        assert!(err.contains("absolute"));
    }

    #[test]
    fn yaml_escape_matches_upstream_rules() {
        assert_eq!(yaml_escape("hello"), "hello");
        assert_eq!(yaml_escape("a \"b\" c"), "a \\\"b\\\" c");
        assert_eq!(yaml_escape("multi\nline"), "multi\\\\nline");
        assert_eq!(yaml_escape("back\\slash"), "back\\\\slash");
    }

    #[test]
    fn permission_mode_round_trips_through_string() {
        for mode in [
            AgentPermissionMode::Default,
            AgentPermissionMode::AcceptEdits,
            AgentPermissionMode::BypassPermissions,
            AgentPermissionMode::Plan,
        ] {
            let s = permission_mode_str(mode);
            assert_eq!(parse_permission_mode(s.to_string()), Some(mode));
        }
    }
}
