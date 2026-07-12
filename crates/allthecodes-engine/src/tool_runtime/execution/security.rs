//! Security validation for tool execution.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use allthecodes_types::security::TaintDecisionKind;
use serde_json::Value;

use crate::permissions::dangerous;
use crate::permissions::path_validation;
use crate::types::tool::ToolUseContext;
use crate::types::tool::{PermissionMode, Tool, Tools};

use super::{make_error_result, ToolExecutionResult};

/// Centralized security checks run before hooks and permission evaluation.
///
/// Returns `Some(ToolExecutionResult)` if the tool call should be **rejected**,
/// or `None` if validation passed and the pipeline should continue.
///
/// Checks performed (in order):
///   3c.1  Plan-mode gate — non-read-only tools are blocked in Plan mode
///         except writes to the dedicated plan file
///   3c.2  Dangerous command detection — Bash/PowerShell commands screened
///   3c.3  Path boundary enforcement — Write/Edit paths must be within allowed dirs
///
/// All checks are skipped when the permission mode is `Bypass`.
pub(crate) fn security_validate(
    tool_use_id: &str,
    tool_name: &str,
    input: &Value,
    tool: &dyn Tool,
    ctx: &ToolUseContext,
    started: Instant,
) -> Option<ToolExecutionResult> {
    let app_state = (ctx.get_app_state)();
    let mode = &app_state.tool_permission_context.mode;

    // Taint enforcement is evaluated before bypass/auto permission modes. A
    // bypass setting is user policy for ordinary permissions, not authority
    // granted to remote content.
    let sink = crate::permissions::taint_policy::classify_tool_sink(tool_name, input);
    let command_risk = shell_command_risk(tool_name, input);
    let taint_decision = crate::permissions::taint_policy::decide_tainted_sink(
        &ctx.taint_context,
        sink,
        command_risk.as_ref(),
    );
    match taint_decision.decision {
        TaintDecisionKind::Allow => {}
        TaintDecisionKind::Ask | TaintDecisionKind::Deny => {
            let disposition = if taint_decision.decision == TaintDecisionKind::Ask {
                "Exact approval required"
            } else {
                "Blocked"
            };
            return Some(make_error_result(
                tool_use_id,
                tool_name,
                &format!(
                    "{disposition} by workflow injection defense: {} (rule: {}, sink: {:?}, untrusted sources: {})",
                    taint_decision.reason,
                    taint_decision.rule_id,
                    taint_decision.sink,
                    taint_decision.source_digests.len(),
                ),
                started,
            ));
        }
    }

    // Bypass mode skips all security validation
    if *mode == PermissionMode::Bypass {
        return None;
    }

    let plan_file_write_allowed =
        *mode == PermissionMode::Plan && is_plan_mode_plan_file_write(tool_name, input);

    // ── 3c.1: Plan mode gate ───────────────────────────────────────
    if *mode == PermissionMode::Plan && !tool.is_read_only(input) && !plan_file_write_allowed {
        return Some(make_error_result(
            tool_use_id,
            tool_name,
            &format!(
                "Tool '{}' is not available in Plan mode (read-only exploration only)",
                tool_name
            ),
            started,
        ));
    }

    // ── 3c.2: Dangerous command check (Bash / PowerShell) ──────────
    if tool_name == "Bash" || tool_name == "PowerShell" {
        if let Some(command) = input.get("command").and_then(|v| v.as_str()) {
            let danger = if tool_name == "PowerShell" {
                dangerous::is_dangerous_powershell_command(command)
            } else {
                dangerous::is_dangerous_command(command)
            };

            if let Some(reason) = danger {
                return Some(make_error_result(
                    tool_use_id,
                    tool_name,
                    &format!("Dangerous command blocked: {}", reason),
                    started,
                ));
            }
        }
    }

    // ── 3c.3: Path boundary check (Write / Edit) ──────────────────
    const FILE_TOOL_NAMES: &[&str] = &["Write", "Edit", "FileWrite", "FileEdit"];
    if FILE_TOOL_NAMES.contains(&tool_name) && !plan_file_write_allowed {
        if let Some(file_path_str) = input.get("file_path").and_then(|v| v.as_str()) {
            // Step 1: validate path structure (traversal attacks, null bytes, etc.)
            let canonical = match path_validation::validate_file_path(file_path_str) {
                Ok(p) => p,
                Err(e) => {
                    return Some(make_error_result(
                        tool_use_id,
                        tool_name,
                        &format!("Invalid file path: {}", e),
                        started,
                    ));
                }
            };

            // Step 2: check the path is within allowed directories
            let cwd = crate::bootstrap::PROCESS_STATE.read().original_cwd.clone();
            let perm_ctx = &app_state.tool_permission_context;

            if !path_validation::is_path_within_allowed_directories(&canonical, &cwd, perm_ctx) {
                return Some(make_error_result(
                    tool_use_id,
                    tool_name,
                    &format!(
                        "Path '{}' is outside the allowed working directories. \
                         Allowed: {}{}",
                        file_path_str,
                        cwd.display(),
                        if perm_ctx.additional_working_directories.is_empty() {
                            String::new()
                        } else {
                            format!(
                                " (and {} additional directories)",
                                perm_ctx.additional_working_directories.len()
                            )
                        }
                    ),
                    started,
                ));
            }
        }
    }

    None // all checks passed
}

fn shell_command_risk(
    tool_name: &str,
    input: &Value,
) -> Option<crate::permissions::command_risk::CommandRisk> {
    let shell = match tool_name {
        "Bash" | "bash" => crate::permissions::command_risk::ShellKind::Bash,
        "PowerShell" | "powershell" | "pwsh" | "Pwsh" => {
            crate::permissions::command_risk::ShellKind::PowerShell
        }
        _ => return None,
    };
    let command = input
        .get("command")
        .or_else(|| input.get("cmd"))
        .or_else(|| input.get("script"))
        .and_then(Value::as_str)?;
    Some(crate::permissions::command_risk::classify_command_risk(
        command, shell,
    ))
}

/// True when a write/edit tool targets the dedicated plan file that plan mode
/// is allowed to maintain.
pub(crate) fn is_plan_mode_plan_file_write(tool_name: &str, input: &Value) -> bool {
    const PLAN_FILE_WRITE_TOOLS: &[&str] = &["Write", "Edit", "FileWrite", "FileEdit"];
    if !PLAN_FILE_WRITE_TOOLS.contains(&tool_name) {
        return false;
    }

    let Some(file_path) = input.get("file_path").and_then(|value| value.as_str()) else {
        return false;
    };
    let Ok(candidate) = path_validation::validate_file_path(file_path) else {
        return false;
    };

    let cwd = crate::bootstrap::state::original_cwd();
    let plan_path = crate::config::paths::current_plan_file_path(&cwd);
    paths_equivalent_for_plan_file(&candidate, &plan_path)
}

/// True when sandbox `allowedCommands` should pre-approve this shell call.
///
/// This is intentionally narrower than permission allow rules: it applies
/// only to sandboxed workspace-mode shell commands and only after the normal
/// deny/ask checks have already had a chance to win.
pub(crate) fn sandbox_allowed_command_applies(
    tool_name: &str,
    input: &Value,
    app_state: &allthecodes_tools::tool::ToolAppState,
) -> bool {
    if !matches!(tool_name, "Bash" | "PowerShell") {
        return false;
    }
    if input
        .get("dangerouslyDisableSandbox")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return false;
    }
    let Some(command) = input.get("command").and_then(|v| v.as_str()) else {
        return false;
    };
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let policy = crate::sandbox::policy_from_app_state(
        &app_state.tool_permission_context,
        &app_state.settings.sandbox,
        cwd,
        false,
    );
    policy.enabled
        && policy.mode == crate::sandbox::SandboxMode::Workspace
        && policy.availability.is_available()
        && policy.is_allowed_command(command)
}

fn paths_equivalent_for_plan_file(candidate: &Path, plan_path: &Path) -> bool {
    normalize_for_compare(candidate) == normalize_for_compare(plan_path)
}

fn normalize_for_compare(path: &Path) -> String {
    let normalized = path
        .canonicalize()
        .unwrap_or_else(|_| normalize_nonexistent_path(path));
    let mut rendered = normalized.to_string_lossy().replace('\\', "/");
    while rendered.len() > 1 && rendered.ends_with('/') && !rendered.ends_with(":/") {
        rendered.pop();
    }
    if cfg!(windows) {
        rendered.make_ascii_lowercase();
    }
    rendered
}

fn normalize_nonexistent_path(path: &Path) -> PathBuf {
    if let Some(parent) = path.parent() {
        if let Ok(parent) = parent.canonicalize() {
            return path
                .file_name()
                .map(|name| parent.join(name))
                .unwrap_or(parent);
        }
    }
    path.to_path_buf()
}

/// Find a tool by name, with alias fallback.
pub(crate) fn find_tool(name: &str, tools: &Tools) -> Option<Arc<dyn Tool>> {
    // Primary: exact name match
    if let Some(tool) = tools.iter().find(|t| t.name() == name) {
        return Some(Arc::clone(tool));
    }

    // Fallback: check user_facing_name
    if let Some(tool) = tools.iter().find(|t| t.user_facing_name(None) == name) {
        return Some(Arc::clone(tool));
    }

    None
}
