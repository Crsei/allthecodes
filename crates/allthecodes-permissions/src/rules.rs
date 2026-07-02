//! Permission rule matching engine.
//!
//! Evaluates tool permission rules in priority order, matching the
//! precedence documented in `docs/claude-code-configuration/permissions.md`:
//!
//!   1. **Deny** rules (highest priority, blocks even if allow matches)
//!   2. **Ask**  rules (force a prompt even when allow matches)
//!   3. **Allow** rules (pre-approve)
//!   4. Permission-mode fallback (Default → Ask, Auto → Allow, ...)
//!
//! `Bash` patterns are dispatched through [`crate::bash_matcher`]
//! so compound commands and process wrappers participate in matching.

use crate::bash_matcher;
use crate::path_validation;
use allthecodes_types::permissions::{
    PermissionMode, ToolPermissionContext, ToolPermissionRulesBySource,
};
use allthecodes_utils::bash::{parse_command, split_compound_command};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Result of a permission check against the rule engine.
///
/// Retained as a stable internal API for tests and for callers that want
/// the rule-engine result without the full hook + mode flow (which lives
/// in [`crate::decision`]).
#[derive(Debug, Clone)]
pub enum PermissionCheckResult {
    /// Tool execution is allowed.
    Allow,
    /// Tool execution is denied.
    Deny { reason: String },
    /// User must be asked for confirmation.
    Ask { message: String },
}

/// Tools whose execution counts as an "edit" for [`PermissionMode::AcceptEdits`].
const EDIT_TOOLS: &[&str] = &[
    "Write",
    "Edit",
    "MultiEdit",
    "FileWrite",
    "FileEdit",
    "FileMultiEdit",
    "NotebookEdit",
    "ApplyPatch",
    "ApplyDiff",
];

/// Read-like built-in tools that should honor `Read(...)` rules on a
/// best-effort basis.
const READ_LIKE_TOOLS: &[&str] = &["Read", "FileRead", "Glob", "Grep"];

/// Common workspace-local filesystem commands auto-approved by
/// `acceptEdits` when every referenced path stays inside the working
/// directory or `additionalDirectories`.
const ACCEPT_EDITS_BASH_COMMANDS: &[&str] = &["mkdir", "touch", "mv", "cp"];

/// Encode a literal permission specifier that must match exactly.
///
/// The permission grammar uses `Tool(pattern)` with `*` and prefixes for
/// matching. Storing exact strings as hex prevents shell wildcards, commas,
/// parentheses, and JSON punctuation from broadening the saved rule.
pub fn literal_specifier(value: &str) -> String {
    let mut encoded = String::with_capacity("exact-hex:".len() + value.len() * 2);
    encoded.push_str("exact-hex:");
    for byte in value.as_bytes() {
        encoded.push_str(&format!("{byte:02x}"));
    }
    encoded
}

/// True when a tool counts as a file-system edit for accept-edits semantics.
pub fn is_edit_tool(tool_name: &str) -> bool {
    EDIT_TOOLS.contains(&tool_name)
}

fn is_read_like_tool(tool_name: &str) -> bool {
    READ_LIKE_TOOLS.contains(&tool_name)
}

/// True when `acceptEdits` should auto-approve this tool call.
pub fn is_accept_edits_tool_call(
    tool_name: &str,
    input: &Value,
    ctx: &ToolPermissionContext,
) -> bool {
    if is_edit_tool(tool_name) {
        return true;
    }

    if tool_name != "Bash" {
        return false;
    }

    let command = input
        .get("command")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if command.is_empty() {
        return false;
    }

    let cwd = allthecodes_bootstrap::PROCESS_STATE
        .read()
        .original_cwd
        .clone();
    accept_edits_bash_command_is_safe(command, &cwd, ctx)
}

/// Check whether a tool invocation is permitted given the current context.
///
/// Returns the rule-based decision only — caller is responsible for the
/// hook overlay and mode fallback (see [`crate::decision`]).
pub fn check_tool_permission(
    tool_name: &str,
    input: &Value,
    ctx: &ToolPermissionContext,
) -> PermissionCheckResult {
    // 1. Deny rules
    if let Some(reason) = match_rules_any_source(tool_name, input, &ctx.always_deny_rules) {
        return PermissionCheckResult::Deny {
            reason: if reason.is_empty() {
                format!("Tool '{}' is denied by policy.", tool_name)
            } else {
                format!("Denied by rule: {}", reason)
            },
        };
    }

    // 2. Ask rules (forced prompt — wins over allow per the spec)
    if let Some(rule) = match_rules_any_source(tool_name, input, &ctx.always_ask_rules) {
        return PermissionCheckResult::Ask {
            message: if rule.is_empty() {
                format!("Permission required for tool '{}'.", tool_name)
            } else {
                format!("Ask rule matched: {}", rule)
            },
        };
    }

    // 3. Allow rules
    if match_rules_any_source(tool_name, input, &ctx.always_allow_rules).is_some() {
        return PermissionCheckResult::Allow;
    }

    // 4. Mode fallback
    match ctx.mode {
        PermissionMode::Default => PermissionCheckResult::Ask {
            message: format!("Allow tool '{}'?", tool_name),
        },
        PermissionMode::Auto => PermissionCheckResult::Allow,
        PermissionMode::Bypass => PermissionCheckResult::Allow,
        PermissionMode::Plan => PermissionCheckResult::Ask {
            message: format!(
                "Tool '{}' requires confirmation in plan mode (read-only).",
                tool_name
            ),
        },
        PermissionMode::AcceptEdits => {
            if is_accept_edits_tool_call(tool_name, input, ctx) {
                PermissionCheckResult::Allow
            } else {
                PermissionCheckResult::Ask {
                    message: format!("Allow tool '{}'?", tool_name),
                }
            }
        }
        PermissionMode::DontAsk => PermissionCheckResult::Deny {
            reason: format!(
                "Permission mode 'dontAsk' silently denies '{}': add an allow rule to permit it.",
                tool_name
            ),
        },
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Iterate all sources in a rule set and return Some(matched_rule) if any
/// rule matches (`tool_name`, `input`).
fn match_rules_any_source(
    tool_name: &str,
    input: &Value,
    rules_by_source: &ToolPermissionRulesBySource,
) -> Option<String> {
    for (_source, rules) in rules_by_source.iter() {
        for rule in rules {
            if rule_matches(tool_name, input, rule) {
                return Some(rule.clone());
            }
        }
    }
    None
}

/// Check if a single rule pattern matches a tool invocation.
///
/// Supports four shapes:
///   1. Exact tool name: `"Bash"` or `"Read"`.
///   2. Tool prefix: `"mcp__server"` matches `"mcp__server__tool"`.
///   3. Glob: `"mcp__*"` matches `"mcp__anything"`.
///   4. Specifier: `"Bash(pattern)"`, `"Read(/tmp/*)"`, etc. — the inner
///      pattern is dispatched per-tool via [`specifier_matches`].
pub(crate) fn rule_matches(tool_name: &str, input: &Value, rule: &str) -> bool {
    // Specifier form: ToolName(pattern)
    if let Some(open) = rule.find('(') {
        if rule.ends_with(')') {
            let rule_tool = &rule[..open];
            if !rule_tool_matches_invocation(rule_tool, tool_name) {
                return false;
            }
            let pattern = &rule[open + 1..rule.len() - 1];
            return specifier_matches(rule_tool, tool_name, input, pattern);
        }
    }

    rule_tool_matches_invocation(rule, tool_name)
}

/// Match a rule's tool-name component (no specifier) against a tool name.
fn tool_name_matches(tool_name: &str, rule: &str) -> bool {
    if tool_name == rule {
        return true;
    }
    if rule.contains('*') {
        return glob_match(tool_name, rule);
    }
    if let Some(rest) = tool_name.strip_prefix(rule) {
        if rest.is_empty() || rest.starts_with('_') || rest.starts_with('/') {
            return true;
        }
    }
    false
}

fn rule_tool_matches_invocation(rule_tool: &str, tool_name: &str) -> bool {
    if tool_name_matches(tool_name, rule_tool) {
        return true;
    }
    if tool_aliases(tool_name)
        .iter()
        .any(|alias| tool_name_matches(alias, rule_tool))
        || tool_aliases(rule_tool)
            .iter()
            .any(|alias| tool_name_matches(tool_name, alias))
    {
        return true;
    }

    match rule_tool {
        "Read" => is_read_like_tool(tool_name),
        "Edit" => is_edit_tool(tool_name),
        _ => false,
    }
}

fn tool_aliases(tool_name: &str) -> &'static [&'static str] {
    match tool_name {
        "Bash" => &["bash"],
        "bash" => &["Bash"],
        "ViewImage" => &["view_image"],
        "view_image" => &["ViewImage"],
        "GetGoal" => &["get_goal"],
        "get_goal" => &["GetGoal"],
        "CreateGoal" => &["create_goal"],
        "create_goal" => &["CreateGoal"],
        "UpdateGoal" => &["update_goal"],
        "update_goal" => &["UpdateGoal"],
        "Workflow" => &["workflow"],
        "workflow" => &["Workflow"],
        "ApplyPatch" => &["apply_patch"],
        "apply_patch" => &["ApplyPatch"],
        "PowerShell" => &["powershell", "pwsh", "Pwsh"],
        "powershell" | "pwsh" | "Pwsh" => &["PowerShell"],
        "PushNotification" => &["push_notification"],
        "push_notification" => &["PushNotification"],
        "ListAgents" => &["list_agents"],
        "list_agents" => &["ListAgents"],
        "FollowupTask" => &["followup_task"],
        "followup_task" => &["FollowupTask"],
        "WaitAgent" => &["wait_agent"],
        "wait_agent" => &["WaitAgent"],
        "CloseAgent" => &["close_agent"],
        "close_agent" => &["CloseAgent"],
        "SendMessage" => &["send_message"],
        "send_message" => &["SendMessage"],
        "TeamSpawn" => &["spawn_agent"],
        "spawn_agent" => &["TeamSpawn"],
        _ => &[],
    }
}

/// Match a `ToolName(pattern)` specifier against the tool's input.
fn specifier_matches(rule_tool: &str, tool_name: &str, input: &Value, pattern: &str) -> bool {
    match rule_tool {
        "Bash" if matches!(tool_name, "Bash" | "bash") => {
            let cmd = shell_command_input(input);
            if pattern_is_exact_literal(pattern) {
                text_specifier_matches(pattern, cmd)
            } else {
                bash_matcher::bash_pattern_matches(pattern, cmd)
            }
        }
        "PowerShell" if matches!(tool_name, "PowerShell" | "powershell" | "pwsh" | "Pwsh") => {
            let cmd = shell_command_input(input);
            text_specifier_matches(pattern, cmd)
        }
        "Read" if is_read_like_tool(tool_name) => read_specifier_matches(tool_name, input, pattern),
        "Edit" if is_edit_tool(tool_name) => file_path_specifier_matches(input, pattern),
        "Write" if matches!(tool_name, "Write" | "FileWrite") => {
            file_path_specifier_matches(input, pattern)
        }
        "MultiEdit" if matches!(tool_name, "MultiEdit" | "FileMultiEdit") => {
            file_path_specifier_matches(input, pattern)
        }
        "NotebookEdit" if tool_name == "NotebookEdit" => {
            file_path_specifier_matches(input, pattern)
        }
        "Read" | "Write" | "Edit" | "MultiEdit" | "NotebookEdit" => {
            file_path_specifier_matches(input, pattern)
        }
        "Glob" if tool_name == "Glob" => {
            let pat = input.get("pattern").and_then(|v| v.as_str()).unwrap_or("");
            text_specifier_matches(pattern, pat)
        }
        "WebFetch" if tool_name == "WebFetch" => {
            let url = input.get("url").and_then(|v| v.as_str()).unwrap_or("");
            url_specifier_matches(pattern, url)
        }
        "WebSearch" if tool_name == "WebSearch" => {
            let url = input.get("query").and_then(|v| v.as_str()).unwrap_or("");
            text_specifier_matches(pattern, url)
        }
        "Agent" if tool_name == "Agent" => {
            let agent_type = input
                .get("subagent_type")
                .or_else(|| input.get("agent_type"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            text_specifier_matches(pattern, agent_type)
        }
        _ if rule_tool.starts_with("mcp__") && tool_name.starts_with("mcp__") => {
            text_specifier_matches(pattern, tool_name)
        }
        _ => {
            // Generic specifier: stringify input and glob-match.
            let s = input.to_string();
            text_specifier_matches(pattern, &s)
        }
    }
}

fn file_path_specifier_matches(input: &Value, pattern: &str) -> bool {
    let path = input
        .get("file_path")
        .or_else(|| input.get("notebook_path"))
        .or_else(|| input.get("path"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    text_specifier_matches(pattern, path)
}

fn shell_command_input(input: &Value) -> &str {
    input
        .get("command")
        .or_else(|| input.get("cmd"))
        .or_else(|| input.get("script"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
}

fn read_specifier_matches(tool_name: &str, input: &Value, pattern: &str) -> bool {
    match tool_name {
        "Read" | "FileRead" => file_path_specifier_matches(input, pattern),
        "Glob" => {
            let path = input
                .get("path")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty());
            let glob = input
                .get("pattern")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty());

            path.is_some_and(|p| text_specifier_matches(pattern, p))
                || glob.is_some_and(|g| text_specifier_matches(pattern, g))
                || match (path, glob) {
                    (Some(base), Some(glob_pat)) => {
                        let joined = Path::new(base).join(glob_pat);
                        text_specifier_matches(pattern, &joined.to_string_lossy())
                    }
                    _ => false,
                }
        }
        "Grep" => input
            .get("path")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .is_some_and(|p| text_specifier_matches(pattern, p)),
        _ => false,
    }
}

fn url_specifier_matches(pattern: &str, value: &str) -> bool {
    let pat = pattern.trim();
    if let Some(domain) = pat.strip_prefix("domain:") {
        return url_matches_domain(value, domain.trim());
    }
    text_specifier_matches(pat, value)
}

fn url_matches_domain(url: &str, domain: &str) -> bool {
    if domain.is_empty() {
        return false;
    }

    if let Ok(parsed) = url::Url::parse(url) {
        if let Some(host) = parsed.host_str() {
            return host == domain || host.ends_with(&format!(".{}", domain));
        }
    }

    false
}

fn text_specifier_matches(pattern: &str, value: &str) -> bool {
    let pat = pattern.trim();
    if pat.is_empty() || pat == "*" {
        return true;
    }
    if let Some(exact) = decode_exact_literal(pat) {
        return exact == value;
    }
    if let Some(prefix) = pat.strip_prefix("prefix:") {
        return value.starts_with(prefix);
    }
    if pat.contains('*') {
        return glob_match(value, pat);
    }
    value == pat
}

fn pattern_is_exact_literal(pattern: &str) -> bool {
    pattern.trim().starts_with("exact-hex:")
}

fn decode_exact_literal(pattern: &str) -> Option<String> {
    let hex = pattern.strip_prefix("exact-hex:")?;
    if hex.len() % 2 != 0 {
        return None;
    }
    let bytes = (0..hex.len())
        .step_by(2)
        .map(|idx| u8::from_str_radix(&hex[idx..idx + 2], 16))
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    String::from_utf8(bytes).ok()
}

/// Minimal glob matching that supports `*` as a wildcard for any sequence
/// of characters. This is intentionally simple — we only need to support
/// the patterns used in Claude Code's permission rules.
///
/// Also exposed as `glob_match_public` for use in [`crate::bash_matcher`].
fn glob_match(text: &str, pattern: &str) -> bool {
    let segments: Vec<&str> = pattern.split('*').collect();

    if segments.is_empty() {
        return true;
    }

    let mut pos = 0usize;

    for (i, segment) in segments.iter().enumerate() {
        if segment.is_empty() {
            continue;
        }

        if let Some(found) = text[pos..].find(segment) {
            if i == 0 && !pattern.starts_with('*') && found != 0 {
                return false;
            }
            pos += found + segment.len();
        } else {
            return false;
        }
    }

    if !pattern.ends_with('*') {
        if let Some(last_seg) = segments.last() {
            if !last_seg.is_empty() && !text.ends_with(last_seg) {
                return false;
            }
        }
    }

    true
}

/// Public wrapper around `glob_match` for use in other permission modules.
pub fn glob_match_public(text: &str, pattern: &str) -> bool {
    glob_match(text, pattern)
}

fn accept_edits_bash_command_is_safe(
    command: &str,
    cwd: &Path,
    ctx: &ToolPermissionContext,
) -> bool {
    let subcommands = split_compound_command(command);
    if subcommands.is_empty() {
        return accept_edits_subcommand_is_safe(command.trim(), cwd, ctx);
    }

    subcommands
        .iter()
        .all(|subcmd| accept_edits_subcommand_is_safe(subcmd.trim(), cwd, ctx))
}

fn accept_edits_subcommand_is_safe(
    subcommand: &str,
    cwd: &Path,
    ctx: &ToolPermissionContext,
) -> bool {
    if subcommand.is_empty() {
        return false;
    }

    let words = match parse_command(subcommand) {
        Ok(words) => words,
        Err(_) => return false,
    };
    let Some((command_index, command_name)) = first_bash_command(&words) else {
        return false;
    };
    if !ACCEPT_EDITS_BASH_COMMANDS.contains(&command_name.as_str()) {
        return false;
    }

    let path_args = extract_command_path_args(&words[command_index + 1..]);
    if path_args.is_empty() {
        return false;
    }

    path_args
        .iter()
        .all(|raw| accept_edits_path_is_safe(raw, cwd, ctx))
}

fn first_bash_command(words: &[String]) -> Option<(usize, String)> {
    for (idx, word) in words.iter().enumerate() {
        if word.starts_with('-') {
            return None;
        }

        if let Some((name, _value)) = word.split_once('=') {
            if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                continue;
            }
        }

        let normalized = Path::new(word)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(word)
            .to_string();
        return Some((idx, normalized));
    }

    None
}

fn extract_command_path_args(words: &[String]) -> Vec<&str> {
    words
        .iter()
        .filter_map(|word| {
            if word.starts_with('-') || word.is_empty() {
                None
            } else {
                Some(word.as_str())
            }
        })
        .collect()
}

fn accept_edits_path_is_safe(path_arg: &str, cwd: &Path, ctx: &ToolPermissionContext) -> bool {
    if path_arg == "-" {
        return false;
    }

    let candidate = if Path::new(path_arg).is_absolute() {
        PathBuf::from(path_arg)
    } else {
        cwd.join(path_arg)
    };

    let Ok(validated) = path_validation::validate_file_path(&candidate.to_string_lossy()) else {
        return false;
    };

    accept_edits_path_within_allowed_directories(&validated, cwd, ctx)
}

fn accept_edits_path_within_allowed_directories(
    path: &Path,
    cwd: &Path,
    ctx: &ToolPermissionContext,
) -> bool {
    if accept_edits_path_is_in_directory(path, cwd) {
        return true;
    }

    ctx.additional_working_directories
        .values()
        .any(|dir| accept_edits_path_is_in_directory(path, Path::new(&dir.path)))
}

fn accept_edits_path_is_in_directory(path: &Path, dir: &Path) -> bool {
    let normalized_path = normalize_for_prefix_compare(path);
    let normalized_dir = normalize_for_prefix_compare(dir);
    normalized_path.starts_with(&normalized_dir)
}

fn normalize_for_prefix_compare(path: &Path) -> PathBuf {
    if let Ok(canonical) = path.canonicalize() {
        return canonical;
    }

    for ancestor in path.ancestors() {
        if ancestor.exists() {
            if let Ok(canonical_ancestor) = ancestor.canonicalize() {
                if let Ok(stripped) = path.strip_prefix(ancestor) {
                    return canonical_ancestor.join(stripped);
                }
            }
            break;
        }
    }

    path.to_path_buf()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;

    fn default_ctx() -> ToolPermissionContext {
        ToolPermissionContext {
            mode: PermissionMode::Default,
            additional_working_directories: HashMap::new(),
            always_allow_rules: HashMap::new(),
            always_deny_rules: HashMap::new(),
            always_ask_rules: HashMap::new(),
            session_allow_rules: HashMap::new(),
            auto_mode_stripped_always_allow_rules: Vec::new(),
            auto_mode_stripped_session_allow_rules: Vec::new(),
            is_bypass_permissions_mode_available: false,
            is_auto_mode_available: None,
            pre_plan_mode: None,
        }
    }

    #[test]
    fn test_exact_match() {
        assert!(rule_matches("Bash", &Value::Null, "Bash"));
        assert!(!rule_matches("BashTool", &Value::Null, "Bash"));
    }

    #[test]
    fn test_compatibility_alias_match() {
        assert!(rule_matches("view_image", &Value::Null, "ViewImage"));
        assert!(rule_matches("ViewImage", &Value::Null, "view_image"));
        assert!(rule_matches("apply_patch", &Value::Null, "ApplyPatch"));
        assert!(rule_matches("close_agent", &Value::Null, "CloseAgent"));
    }

    #[test]
    fn test_prefix_match() {
        assert!(rule_matches(
            "mcp__server__tool",
            &Value::Null,
            "mcp__server"
        ));
        assert!(!rule_matches("mcp__serverX", &Value::Null, "mcp__server"));
    }

    #[test]
    fn test_glob_match() {
        assert!(glob_match("mcp__anything", "mcp__*"));
        assert!(glob_match("Bash", "*"));
        assert!(!glob_match("FileRead", "Bash*"));
        assert!(glob_match("Bash(rm -rf)", "Bash(*)"));
    }

    #[test]
    fn test_literal_specifier_matches_bash_exactly() {
        let rule = format!("Bash({})", literal_specifier("cargo test *"));
        assert!(rule_matches(
            "Bash",
            &json!({ "command": "cargo test *" }),
            &rule
        ));
        assert!(!rule_matches(
            "Bash",
            &json!({ "command": "cargo test foo" }),
            &rule
        ));
    }

    #[test]
    fn test_literal_specifier_matches_powershell_exactly() {
        let rule = format!("PowerShell({})", literal_specifier("Get-ChildItem *"));
        assert!(rule_matches(
            "PowerShell",
            &json!({ "command": "Get-ChildItem *" }),
            &rule
        ));
        assert!(rule_matches(
            "pwsh",
            &json!({ "script": "Get-ChildItem *" }),
            &rule
        ));
        assert!(!rule_matches(
            "PowerShell",
            &json!({ "command": "Get-ChildItem src" }),
            &rule
        ));
    }

    #[test]
    fn test_literal_specifier_matches_file_path_exactly() {
        let rule = format!("Edit({})", literal_specifier("src/lib*.rs"));
        assert!(rule_matches(
            "Edit",
            &json!({ "path": "src/lib*.rs" }),
            &rule
        ));
        assert!(!rule_matches(
            "Edit",
            &json!({ "path": "src/lib.rs" }),
            &rule
        ));
    }

    #[test]
    fn test_deny_overrides_allow() {
        let mut ctx = default_ctx();
        ctx.always_allow_rules
            .insert("user".into(), vec!["Bash".into()]);
        ctx.always_deny_rules
            .insert("policy".into(), vec!["Bash".into()]);

        let result = check_tool_permission("Bash", &Value::Null, &ctx);
        assert!(matches!(result, PermissionCheckResult::Deny { .. }));
    }

    #[test]
    fn test_ask_overrides_allow() {
        // New precedence: ask wins over allow.
        let mut ctx = default_ctx();
        ctx.always_allow_rules
            .insert("user".into(), vec!["Bash".into()]);
        ctx.always_ask_rules
            .insert("project".into(), vec!["Bash".into()]);
        let result = check_tool_permission("Bash", &Value::Null, &ctx);
        assert!(
            matches!(result, PermissionCheckResult::Ask { .. }),
            "ask must override allow per spec"
        );
    }

    #[test]
    fn test_allow_rule() {
        let mut ctx = default_ctx();
        ctx.always_allow_rules
            .insert("user".into(), vec!["FileRead".into()]);

        let result = check_tool_permission("FileRead", &Value::Null, &ctx);
        assert!(matches!(result, PermissionCheckResult::Allow));
    }

    #[test]
    fn test_default_mode_asks() {
        let ctx = default_ctx();
        let result = check_tool_permission("SomeTool", &Value::Null, &ctx);
        assert!(matches!(result, PermissionCheckResult::Ask { .. }));
    }

    #[test]
    fn test_bypass_mode_allows() {
        let mut ctx = default_ctx();
        ctx.mode = PermissionMode::Bypass;
        let result = check_tool_permission("SomeTool", &Value::Null, &ctx);
        assert!(matches!(result, PermissionCheckResult::Allow));
    }

    #[test]
    fn test_auto_mode_allows() {
        let mut ctx = default_ctx();
        ctx.mode = PermissionMode::Auto;
        let result = check_tool_permission("SomeTool", &Value::Null, &ctx);
        assert!(matches!(result, PermissionCheckResult::Allow));
    }

    #[test]
    fn test_accept_edits_allows_edit_tools() {
        let mut ctx = default_ctx();
        ctx.mode = PermissionMode::AcceptEdits;
        assert!(matches!(
            check_tool_permission("Edit", &Value::Null, &ctx),
            PermissionCheckResult::Allow
        ));
        assert!(matches!(
            check_tool_permission("Write", &Value::Null, &ctx),
            PermissionCheckResult::Allow
        ));
        // Bash without an approved workspace command still asks.
        assert!(matches!(
            check_tool_permission("Bash", &Value::Null, &ctx),
            PermissionCheckResult::Ask { .. }
        ));
    }

    #[test]
    fn test_accept_edits_allows_workspace_filesystem_bash_commands() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = default_ctx();
        assert!(accept_edits_bash_command_is_safe(
            "mkdir src && touch src/lib.rs",
            dir.path(),
            &ctx
        ));
        assert!(accept_edits_bash_command_is_safe(
            "cp src/lib.rs src/lib.backup",
            dir.path(),
            &ctx
        ));
    }

    #[test]
    fn test_accept_edits_rejects_outside_workspace_paths() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = default_ctx();
        assert!(!accept_edits_bash_command_is_safe(
            "mkdir ../outside",
            dir.path(),
            &ctx
        ));
    }

    #[test]
    fn test_accept_edits_honors_additional_directories() {
        let dir = tempfile::tempdir().unwrap();
        let extra = tempfile::tempdir().unwrap();
        let mut ctx = default_ctx();
        ctx.additional_working_directories.insert(
            "extra".into(),
            allthecodes_types::permissions::AdditionalWorkingDirectory {
                path: extra.path().to_string_lossy().to_string(),
                read_only: false,
            },
        );
        let command = format!(
            "mkdir {}",
            extra
                .path()
                .join("generated")
                .to_string_lossy()
                .replace('\\', "/")
        );
        assert!(accept_edits_bash_command_is_safe(
            &command,
            dir.path(),
            &ctx
        ));
    }

    #[test]
    fn test_dont_ask_denies_silently() {
        let mut ctx = default_ctx();
        ctx.mode = PermissionMode::DontAsk;
        let r = check_tool_permission("AnyTool", &Value::Null, &ctx);
        assert!(matches!(r, PermissionCheckResult::Deny { .. }));
    }

    #[test]
    fn test_dont_ask_respects_allow_rules() {
        let mut ctx = default_ctx();
        ctx.mode = PermissionMode::DontAsk;
        ctx.always_allow_rules
            .insert("user".into(), vec!["Read".into()]);
        let r = check_tool_permission("Read", &Value::Null, &ctx);
        assert!(matches!(r, PermissionCheckResult::Allow));
    }

    // -- specifier tests --

    #[test]
    fn test_bash_specifier_compound_deny_via_token() {
        let input = json!({"command": "git status && rm -rf /tmp/x"});
        assert!(rule_matches("Bash", &input, "Bash(rm)"));
        assert!(rule_matches("Bash", &input, "Bash(prefix:rm)"));
    }

    #[test]
    fn test_bash_specifier_with_wrapper() {
        let input = json!({"command": "sudo cargo build"});
        assert!(rule_matches("Bash", &input, "Bash(prefix:cargo)"));
    }

    #[test]
    fn test_read_specifier_glob() {
        let input = json!({"file_path": "/tmp/foo.log"});
        assert!(rule_matches("Read", &input, "Read(/tmp/*)"));
        assert!(!rule_matches("Read", &input, "Read(/etc/*)"));
    }

    #[test]
    fn test_edit_specifier_matches_write_tool() {
        let input = json!({"file_path": "/tmp/foo.log"});
        assert!(rule_matches("Write", &input, "Edit(/tmp/*)"));
    }

    #[test]
    fn test_read_specifier_matches_glob_tool() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().to_string_lossy().to_string();
        let input = json!({"path": base, "pattern": "*.log"});
        let rule = format!(
            "Read({}{}*)",
            dir.path().display(),
            std::path::MAIN_SEPARATOR
        );
        assert!(rule_matches("Glob", &input, &rule));
    }

    #[test]
    fn test_read_specifier_matches_grep_tool() {
        let dir = tempfile::tempdir().unwrap();
        let input = json!({"path": dir.path().join("src").to_string_lossy().to_string()});
        let rule = format!(
            "Read({}{}*)",
            dir.path().display(),
            std::path::MAIN_SEPARATOR
        );
        assert!(rule_matches("Grep", &input, &rule));
    }

    #[test]
    fn test_webfetch_specifier_glob() {
        let input = json!({"url": "https://api.example.com/v1/foo"});
        assert!(rule_matches(
            "WebFetch",
            &input,
            "WebFetch(https://api.example.com/*)"
        ));
    }

    #[test]
    fn test_webfetch_domain_specifier_matches_subdomains() {
        let input = json!({"url": "https://api.example.com/v1/foo"});
        assert!(rule_matches(
            "WebFetch",
            &input,
            "WebFetch(domain:example.com)"
        ));
        assert!(!rule_matches(
            "WebFetch",
            &input,
            "WebFetch(domain:example.org)"
        ));
    }

    #[test]
    fn test_agent_specifier_exact() {
        let input = json!({"subagent_type": "Explore"});
        assert!(rule_matches("Agent", &input, "Agent(Explore)"));
        assert!(!rule_matches("Agent", &input, "Agent(Plan)"));
    }
}
