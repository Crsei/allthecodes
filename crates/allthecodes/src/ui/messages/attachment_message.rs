//! Rust-side helper for message attachment previews.
//!
//! Classifies raw `(label, detail)` pairs into structured attachment kinds
//! and renders formatted preview strings. Supports 25+ attachment types
//! matching the TS `AttachmentMessage.tsx` dispatch.

use crate::ui::theme::Theme;

/// Structured attachment kinds parsed from raw `(label, detail)` pairs.
///
/// Mirrors the TS switch in `AttachmentMessage.tsx:157-421`.
#[derive(Debug, Clone)]
pub enum AttachmentKind {
    // ── File & Reference types ──────────────────────────────────────────
    /// A regular file read (possibly with line count and truncation flag).
    File {
        display_path: String,
        lines: Option<usize>,
        truncated: bool,
    },
    /// An already-read file (unchanged).
    AlreadyReadFile {
        display_path: String,
        unchanged: bool,
    },
    /// A compact file reference (minimal path display).
    CompactFileReference { display_path: String },
    /// A PDF file reference with page count.
    PdfReference {
        display_path: String,
        page_count: u32,
    },
    /// A directory listing.
    Directory { display_path: String },
    /// Lines selected from an IDE.
    SelectedLinesInIde {
        display_path: String,
        line_start: u32,
        line_end: u32,
        ide_name: String,
    },
    /// A nested memory load.
    NestedMemory { display_path: String, verb: String },
    /// Relevant memories from a collapsed search group.
    RelevantMemories { count: usize, paths: Vec<String> },

    // ── Skills & Tools ──────────────────────────────────────────────────
    /// A dynamic skill loaded from a file.
    DynamicSkill { count: usize, display_path: String },
    /// Skill listing with availability count.
    SkillListing { count: usize, is_initial: bool },
    /// Agent type listing delta.
    AgentListingDelta { count: usize, is_initial: bool },
    /// Restored skills from a previous session.
    InvokedSkills { names: Vec<String> },
    /// Discovered skills with description.
    SkillDiscovery { count: usize },
    /// Discovered tools list.
    ToolDiscovery { count: usize },
    /// Diagnostics display placeholder.
    Diagnostics,

    // ── MCP & Commands ──────────────────────────────────────────────────
    /// A queued command prompt.
    QueuedCommand { prompt: String },
    /// A plan file reference.
    PlanFileReference { path: String },
    /// A read MCP resource.
    McpResource { name: String, server: String },
    /// Command permissions notification (typically hidden).
    CommandPermissions,

    // ── Hook types ──────────────────────────────────────────────────────
    /// An asynchronous hook completion.
    AsyncHookResponse { event: String, verbose: bool },
    /// A blocking error from a hook.
    HookBlockingError {
        name: String,
        hook_event: String,
        stderr: Option<String>,
    },
    /// A non-blocking error from a hook.
    HookNonBlockingError { name: String, hook_event: String },
    /// An error occurring during hook execution.
    HookErrorDuringExecution { name: String, hook_event: String },
    /// Successful hook completion (typically hidden).
    HookSuccess,
    /// A hook that stopped continuation.
    HookStoppedContinuation {
        name: String,
        hook_event: String,
        message: String,
    },
    /// A system message from a hook.
    HookSystemMessage { name: String, content: String },
    /// A permission decision made by a hook.
    HookPermissionDecision { name: String, decision: String },

    // ── Task & Teammate ─────────────────────────────────────────────────
    /// Generic task status update.
    TaskStatus { description: String, status: String },
    /// Batch shutdown notification.
    TeammateShutdownBatch { count: u32 },
    /// Teammate mailbox notification.
    TeammateMailbox { unread: usize },

    // ── Generic fallback ────────────────────────────────────────────────
    /// Generic/unknown attachment type shown as-is.
    Generic { label: String, detail: String },
}

/// Try to parse `detail` as a JSON object and extract a field.
fn json_field(detail: &str, field: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(detail).ok()?;
    value.get(field)?.as_str().map(ToOwned::to_owned)
}

/// Try to parse `detail` as a JSON object and extract a u32 field.
fn json_u32_field(detail: &str, field: &str) -> Option<u32> {
    let value: serde_json::Value = serde_json::from_str(detail).ok()?;
    value.get(field)?.as_u64().map(|v| v as u32)
}

/// Try to parse `detail` as a JSON object and extract a usize field.
fn json_usize_field(detail: &str, field: &str) -> Option<usize> {
    let value: serde_json::Value = serde_json::from_str(detail).ok()?;
    value.get(field)?.as_u64().map(|v| v as usize)
}

/// Try to parse `detail` as a JSON object and extract a bool field.
fn json_bool_field(detail: &str, field: &str) -> Option<bool> {
    let value: serde_json::Value = serde_json::from_str(detail).ok()?;
    value.get(field)?.as_bool()
}

/// Try to parse `detail` as a JSON array of strings.
fn json_str_array(detail: &str) -> Vec<String> {
    serde_json::from_str::<serde_json::Value>(detail)
        .ok()
        .and_then(|v| {
            v.as_array().map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(ToOwned::to_owned))
                    .collect()
            })
        })
        .unwrap_or_default()
}

/// Classify a raw `(label, detail)` pair into a structured attachment kind.
///
/// TS reference: `AttachmentMessage.tsx:157-421` (switch statement)
pub fn classify_attachment(label: &str, detail: &str) -> AttachmentKind {
    let trimmed_label = label.trim();
    let trimmed_detail = detail.trim();

    match trimmed_label {
        // ── File & Reference ────────────────────────────────────────────
        "file" => {
            let path = json_field(trimmed_detail, "display_path")
                .or_else(|| json_field(trimmed_detail, "file_path"))
                .or_else(|| json_field(trimmed_detail, "path"))
                .unwrap_or_else(|| trimmed_detail.to_string());
            AttachmentKind::File {
                display_path: path,
                lines: json_usize_field(trimmed_detail, "lines"),
                truncated: json_bool_field(trimmed_detail, "truncated").unwrap_or(false),
            }
        }
        "already_read_file" => {
            let path = json_field(trimmed_detail, "display_path")
                .or_else(|| json_field(trimmed_detail, "path"))
                .unwrap_or_else(|| trimmed_detail.to_string());
            AttachmentKind::AlreadyReadFile {
                display_path: path,
                unchanged: json_bool_field(trimmed_detail, "unchanged").unwrap_or(false),
            }
        }
        "compact_file_reference" => AttachmentKind::CompactFileReference {
            display_path: trimmed_detail.to_string(),
        },
        "pdf_reference" | "pdf" => {
            let path = json_field(trimmed_detail, "display_path")
                .or_else(|| json_field(trimmed_detail, "path"))
                .unwrap_or_else(|| trimmed_detail.to_string());
            AttachmentKind::PdfReference {
                display_path: path,
                page_count: json_u32_field(trimmed_detail, "page_count").unwrap_or(0),
            }
        }
        "directory" => AttachmentKind::Directory {
            display_path: trimmed_detail.to_string(),
        },
        "selected_lines_in_ide" => {
            let path = json_field(trimmed_detail, "display_path")
                .or_else(|| json_field(trimmed_detail, "path"))
                .unwrap_or_else(|| trimmed_detail.to_string());
            AttachmentKind::SelectedLinesInIde {
                display_path: path,
                line_start: json_u32_field(trimmed_detail, "line_start").unwrap_or(0),
                line_end: json_u32_field(trimmed_detail, "line_end").unwrap_or(0),
                ide_name: json_field(trimmed_detail, "ide_name").unwrap_or_default(),
            }
        }
        "nested_memory" => AttachmentKind::NestedMemory {
            display_path: trimmed_detail.to_string(),
            verb: "Loaded".to_string(),
        },
        "relevant_memories" | "collapsed_read_search_group" => {
            let count = json_usize_field(trimmed_detail, "count").unwrap_or(1);
            let paths = if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed_detail) {
                v.get("paths")
                    .and_then(|p| p.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(ToOwned::to_owned))
                            .collect()
                    })
                    .unwrap_or_default()
            } else {
                vec![trimmed_detail.to_string()]
            };
            AttachmentKind::RelevantMemories { count, paths }
        }

        // ── Skills & Tools ──────────────────────────────────────────────
        "dynamic_skill" => {
            let path = json_field(trimmed_detail, "display_path")
                .or_else(|| json_field(trimmed_detail, "path"))
                .unwrap_or_else(|| trimmed_detail.to_string());
            AttachmentKind::DynamicSkill {
                count: json_usize_field(trimmed_detail, "count").unwrap_or(1),
                display_path: path,
            }
        }
        "skill_listing" => AttachmentKind::SkillListing {
            count: json_usize_field(trimmed_detail, "count").unwrap_or(0),
            is_initial: json_bool_field(trimmed_detail, "is_initial").unwrap_or(false),
        },
        "agent_listing_delta" => AttachmentKind::AgentListingDelta {
            count: json_usize_field(trimmed_detail, "count").unwrap_or(0),
            is_initial: json_bool_field(trimmed_detail, "is_initial").unwrap_or(false),
        },
        "invoked_skills" => {
            let names: Vec<String> =
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed_detail) {
                    v.as_array()
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(ToOwned::to_owned))
                                .collect()
                        })
                        .unwrap_or_default()
                } else {
                    trimmed_detail
                        .split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(ToOwned::to_owned)
                        .collect()
                };
            AttachmentKind::InvokedSkills { names }
        }
        "skill_discovery" => AttachmentKind::SkillDiscovery {
            count: json_usize_field(trimmed_detail, "count")
                .unwrap_or(json_str_array(trimmed_detail).len()),
        },
        "tool_discovery" => AttachmentKind::ToolDiscovery {
            count: json_usize_field(trimmed_detail, "count")
                .unwrap_or(json_str_array(trimmed_detail).len()),
        },
        "diagnostics" => AttachmentKind::Diagnostics,

        // ── MCP & Commands ──────────────────────────────────────────────
        "queued_command" => AttachmentKind::QueuedCommand {
            prompt: json_field(trimmed_detail, "prompt")
                .unwrap_or_else(|| trimmed_detail.to_string()),
        },
        "plan_file_reference" => AttachmentKind::PlanFileReference {
            path: json_field(trimmed_detail, "path").unwrap_or_else(|| trimmed_detail.to_string()),
        },
        "mcp_resource" => AttachmentKind::McpResource {
            name: json_field(trimmed_detail, "name").unwrap_or_else(|| "resource".to_string()),
            server: json_field(trimmed_detail, "server").unwrap_or_else(|| "unknown".to_string()),
        },
        "command_permissions" => AttachmentKind::CommandPermissions,

        // ── Hook types ──────────────────────────────────────────────────
        "async_hook_response" => AttachmentKind::AsyncHookResponse {
            event: json_field(trimmed_detail, "event")
                .unwrap_or_else(|| trimmed_detail.to_string()),
            verbose: json_bool_field(trimmed_detail, "verbose").unwrap_or(false),
        },
        "hook_blocking_error" => {
            let name = json_field(trimmed_detail, "name").unwrap_or_else(|| "hook".to_string());
            let event = json_field(trimmed_detail, "hook_event").unwrap_or_default();
            AttachmentKind::HookBlockingError {
                name,
                hook_event: event,
                stderr: json_field(trimmed_detail, "stderr"),
            }
        }
        "hook_non_blocking_error" => {
            let name = json_field(trimmed_detail, "name").unwrap_or_else(|| "hook".to_string());
            AttachmentKind::HookNonBlockingError {
                name,
                hook_event: json_field(trimmed_detail, "hook_event").unwrap_or_default(),
            }
        }
        "hook_error_during_execution" => AttachmentKind::HookErrorDuringExecution {
            name: json_field(trimmed_detail, "name").unwrap_or_else(|| "hook".to_string()),
            hook_event: json_field(trimmed_detail, "hook_event").unwrap_or_default(),
        },
        "hook_success" => AttachmentKind::HookSuccess,
        "hook_stopped_continuation" => AttachmentKind::HookStoppedContinuation {
            name: json_field(trimmed_detail, "name").unwrap_or_else(|| "hook".to_string()),
            hook_event: json_field(trimmed_detail, "hook_event").unwrap_or_default(),
            message: json_field(trimmed_detail, "message")
                .unwrap_or_else(|| trimmed_detail.to_string()),
        },
        "hook_system_message" => AttachmentKind::HookSystemMessage {
            name: json_field(trimmed_detail, "name").unwrap_or_else(|| "hook".to_string()),
            content: json_field(trimmed_detail, "content")
                .unwrap_or_else(|| trimmed_detail.to_string()),
        },
        "hook_permission_decision" => AttachmentKind::HookPermissionDecision {
            name: json_field(trimmed_detail, "name").unwrap_or_else(|| "hook".to_string()),
            decision: json_field(trimmed_detail, "decision")
                .unwrap_or_else(|| "allowed".to_string()),
        },

        // ── Task & Teammate ─────────────────────────────────────────────
        "task_status" => AttachmentKind::TaskStatus {
            description: json_field(trimmed_detail, "description")
                .unwrap_or_else(|| trimmed_detail.to_string()),
            status: json_field(trimmed_detail, "status").unwrap_or_default(),
        },
        "teammate_shutdown_batch" => AttachmentKind::TeammateShutdownBatch {
            count: json_u32_field(trimmed_detail, "count").unwrap_or(0),
        },
        "teammate_mailbox" => AttachmentKind::TeammateMailbox {
            unread: json_usize_field(trimmed_detail, "unread").unwrap_or(0),
        },

        // ── Generic fallback ────────────────────────────────────────────
        _ => AttachmentKind::Generic {
            label: trimmed_label.to_string(),
            detail: trimmed_detail.to_string(),
        },
    }
}

/// Render a single line for a file attachment.
fn render_file(kind: &AttachmentKind) -> String {
    match kind {
        AttachmentKind::File {
            display_path,
            lines,
            truncated,
        } => {
            let mut parts = vec!["Read".to_string()];
            parts.push(display_path.to_string());
            if *truncated {
                parts.push("(truncated)".to_string());
            }
            if let Some(n) = lines {
                parts.push(format!("({n} lines)"));
            }
            parts.join(" ")
        }
        _ => String::new(),
    }
}

/// Render a directory listing attachment.
fn render_directory(kind: &AttachmentKind) -> String {
    match kind {
        AttachmentKind::Directory { display_path } => {
            format!("Listed directory {display_path}/")
        }
        _ => String::new(),
    }
}

/// Render a compact file reference.
fn render_compact_file_ref(kind: &AttachmentKind) -> String {
    match kind {
        AttachmentKind::CompactFileReference { display_path } => {
            format!("Referenced file {display_path}")
        }
        _ => String::new(),
    }
}

/// Render a PDF reference.
fn render_pdf_reference(kind: &AttachmentKind) -> String {
    match kind {
        AttachmentKind::PdfReference {
            display_path,
            page_count,
        } => {
            if *page_count > 0 {
                format!("Referenced PDF {display_path} ({page_count} pages)")
            } else {
                format!("Referenced PDF {display_path}")
            }
        }
        _ => String::new(),
    }
}

/// Render selected lines in IDE.
fn render_selected_lines(kind: &AttachmentKind) -> String {
    match kind {
        AttachmentKind::SelectedLinesInIde {
            display_path,
            line_start,
            line_end,
            ide_name,
        } => {
            if ide_name.is_empty() {
                format!(
                    "Selected {} lines from {display_path}",
                    line_end - line_start
                )
            } else {
                format!(
                    "Selected {} lines from {display_path} in {ide_name}",
                    line_end - line_start
                )
            }
        }
        _ => String::new(),
    }
}

/// Render nested memory.
fn render_nested_memory(kind: &AttachmentKind) -> String {
    match kind {
        AttachmentKind::NestedMemory { display_path, verb } => format!("{verb} {display_path}"),
        _ => String::new(),
    }
}

/// Render relevant memories.
fn render_relevant_memories(kind: &AttachmentKind) -> String {
    match kind {
        AttachmentKind::RelevantMemories { count, paths } => {
            let mut lines = vec![format!("● Recalled {count} memories")];
            for path in paths {
                let basename = path.rsplit('/').next().unwrap_or(path);
                lines.push(format!("  {basename}"));
            }
            lines.join("\n")
        }
        _ => String::new(),
    }
}

/// Render dynamic skill.
fn render_dynamic_skill(kind: &AttachmentKind) -> String {
    match kind {
        AttachmentKind::DynamicSkill {
            count,
            display_path,
        } => format!("Loaded {count} skills from {display_path}"),
        _ => String::new(),
    }
}

/// Render skill listing.
fn render_skill_listing(kind: &AttachmentKind) -> String {
    match kind {
        AttachmentKind::SkillListing { count, is_initial } => {
            if *is_initial {
                String::new()
            } else {
                format!("{count} skills available")
            }
        }
        _ => String::new(),
    }
}

/// Render agent listing delta.
fn render_agent_listing_delta(kind: &AttachmentKind) -> String {
    match kind {
        AttachmentKind::AgentListingDelta { count, is_initial } => {
            if *is_initial {
                String::new()
            } else {
                format!("{count} agent types available")
            }
        }
        _ => String::new(),
    }
}

/// Render invoked skills.
fn render_invoked_skills(kind: &AttachmentKind) -> String {
    match kind {
        AttachmentKind::InvokedSkills { names } => {
            format!("Skills restored ({})", names.join(", "))
        }
        _ => String::new(),
    }
}

/// Render skill discovery.
fn render_skill_discovery(kind: &AttachmentKind) -> String {
    match kind {
        AttachmentKind::SkillDiscovery { count } => {
            format!("{count} relevant skills found")
        }
        _ => String::new(),
    }
}

/// Render tool discovery.
fn render_tool_discovery(kind: &AttachmentKind) -> String {
    match kind {
        AttachmentKind::ToolDiscovery { count } => {
            format!("Discovered {count} tools")
        }
        _ => String::new(),
    }
}

/// Render a queued command.
fn render_queued_command(kind: &AttachmentKind) -> String {
    match kind {
        AttachmentKind::QueuedCommand { prompt } => {
            let preview = if prompt.len() > 80 {
                format!("{}...", &prompt[..77])
            } else {
                prompt.to_string()
            };
            format!("[queued: {preview}]")
        }
        _ => String::new(),
    }
}

/// Render plan file reference.
fn render_plan_file_reference(kind: &AttachmentKind) -> String {
    match kind {
        AttachmentKind::PlanFileReference { path } => {
            format!("Plan file referenced ({path})")
        }
        _ => String::new(),
    }
}

/// Render MCP resource read.
fn render_mcp_resource(kind: &AttachmentKind) -> String {
    match kind {
        AttachmentKind::McpResource { name, server } => {
            format!("Read MCP resource {name} from {server}")
        }
        _ => String::new(),
    }
}

/// Hook types that should not render for Stop/SubagentStop events.
fn is_stop_hook_event(event: &str) -> bool {
    matches!(event, "Stop" | "SubagentStop")
}

/// Render an async hook response.
fn render_async_hook_response(kind: &AttachmentKind) -> String {
    match kind {
        AttachmentKind::AsyncHookResponse { event, verbose } => {
            if is_stop_hook_event(event) && !verbose {
                return String::new();
            }
            format!("Async hook {event} completed")
        }
        _ => String::new(),
    }
}

/// Render a hook blocking error.
fn render_hook_blocking_error(kind: &AttachmentKind) -> String {
    match kind {
        AttachmentKind::HookBlockingError {
            name,
            hook_event,
            stderr,
        } => {
            if is_stop_hook_event(hook_event) {
                return String::new();
            }
            let mut msg = format!("{name} hook returned blocking error");
            if let Some(err) = stderr {
                if !err.is_empty() {
                    msg.push_str(&format!(": {err}"));
                }
            }
            msg
        }
        _ => String::new(),
    }
}

/// Render a hook non-blocking error.
fn render_hook_non_blocking_error(kind: &AttachmentKind) -> String {
    match kind {
        AttachmentKind::HookNonBlockingError { name, hook_event } => {
            if is_stop_hook_event(hook_event) {
                return String::new();
            }
            format!("{name} hook error")
        }
        _ => String::new(),
    }
}

/// Render a hook execution error.
fn render_hook_error_during_execution(kind: &AttachmentKind) -> String {
    match kind {
        AttachmentKind::HookErrorDuringExecution { name, hook_event } => {
            if is_stop_hook_event(hook_event) {
                return String::new();
            }
            format!("{name} hook warning")
        }
        _ => String::new(),
    }
}

/// Render a hook stopped continuation.
fn render_hook_stopped_continuation(kind: &AttachmentKind) -> String {
    match kind {
        AttachmentKind::HookStoppedContinuation {
            name,
            hook_event,
            message,
        } => {
            if is_stop_hook_event(hook_event) {
                return String::new();
            }
            format!("{name} hook stopped continuation: {message}")
        }
        _ => String::new(),
    }
}

/// Render a hook system message.
fn render_hook_system_message(kind: &AttachmentKind) -> String {
    match kind {
        AttachmentKind::HookSystemMessage { name, content } => {
            format!("{name} says: {content}")
        }
        _ => String::new(),
    }
}

/// Render a hook permission decision.
fn render_hook_permission_decision(kind: &AttachmentKind) -> String {
    match kind {
        AttachmentKind::HookPermissionDecision { name, decision } => {
            let action = match decision {
                d if d.eq_ignore_ascii_case("allowed") => "Allowed",
                d if d.eq_ignore_ascii_case("denied") => "Denied",
                _ => decision,
            };
            format!("{action} by {name} hook")
        }
        _ => String::new(),
    }
}

/// Render task status.
fn render_task_status(kind: &AttachmentKind) -> String {
    match kind {
        AttachmentKind::TaskStatus {
            description,
            status,
        } => {
            if status.is_empty() {
                format!("Task '{description}' updated")
            } else {
                format!("Task '{description}' {status}")
            }
        }
        _ => String::new(),
    }
}

/// Render teammate shutdown batch.
fn render_teammate_shutdown_batch(kind: &AttachmentKind) -> String {
    match kind {
        AttachmentKind::TeammateShutdownBatch { count } => {
            format!("{count} teammates shut down gracefully")
        }
        _ => String::new(),
    }
}

/// Render teammate mailbox.
fn render_teammate_mailbox(kind: &AttachmentKind) -> String {
    match kind {
        AttachmentKind::TeammateMailbox { unread } => {
            format!("{unread} unread teammate messages")
        }
        _ => String::new(),
    }
}

/// Render the classified attachment kind.
fn render_attachment_kind(kind: &AttachmentKind) -> String {
    match kind {
        // File & Reference
        AttachmentKind::File { .. } => render_file(kind),
        AttachmentKind::AlreadyReadFile {
            display_path,
            unchanged,
        } => {
            if *unchanged {
                format!("Read {display_path} (unchanged)")
            } else {
                format!("Read {display_path}")
            }
        }
        AttachmentKind::CompactFileReference { .. } => render_compact_file_ref(kind),
        AttachmentKind::PdfReference { .. } => render_pdf_reference(kind),
        AttachmentKind::Directory { .. } => render_directory(kind),
        AttachmentKind::SelectedLinesInIde { .. } => render_selected_lines(kind),
        AttachmentKind::NestedMemory { .. } => render_nested_memory(kind),
        AttachmentKind::RelevantMemories { .. } => render_relevant_memories(kind),

        // Skills & Tools
        AttachmentKind::DynamicSkill { .. } => render_dynamic_skill(kind),
        AttachmentKind::SkillListing { .. } => render_skill_listing(kind),
        AttachmentKind::AgentListingDelta { .. } => render_agent_listing_delta(kind),
        AttachmentKind::InvokedSkills { .. } => render_invoked_skills(kind),
        AttachmentKind::SkillDiscovery { .. } => render_skill_discovery(kind),
        AttachmentKind::ToolDiscovery { .. } => render_tool_discovery(kind),
        AttachmentKind::Diagnostics => String::new(),

        // MCP & Commands
        AttachmentKind::QueuedCommand { .. } => render_queued_command(kind),
        AttachmentKind::PlanFileReference { .. } => render_plan_file_reference(kind),
        AttachmentKind::McpResource { .. } => render_mcp_resource(kind),
        AttachmentKind::CommandPermissions => String::new(),

        // Hook types
        AttachmentKind::AsyncHookResponse { .. } => render_async_hook_response(kind),
        AttachmentKind::HookBlockingError { .. } => render_hook_blocking_error(kind),
        AttachmentKind::HookNonBlockingError { .. } => render_hook_non_blocking_error(kind),
        AttachmentKind::HookErrorDuringExecution { .. } => render_hook_error_during_execution(kind),
        AttachmentKind::HookSuccess => String::new(),
        AttachmentKind::HookStoppedContinuation { .. } => render_hook_stopped_continuation(kind),
        AttachmentKind::HookSystemMessage { .. } => render_hook_system_message(kind),
        AttachmentKind::HookPermissionDecision { .. } => render_hook_permission_decision(kind),

        // Task & Teammate
        AttachmentKind::TaskStatus { .. } => render_task_status(kind),
        AttachmentKind::TeammateShutdownBatch { .. } => render_teammate_shutdown_batch(kind),
        AttachmentKind::TeammateMailbox { .. } => render_teammate_mailbox(kind),

        // Generic fallback
        AttachmentKind::Generic { label, detail } => {
            let detail_line = if detail.is_empty() {
                "no details"
            } else {
                detail
            };
            format!("Attachment: {label} -> {detail_line}")
        }
    }
}

/// Render an attachment message from its label and detail strings.
///
/// Classifies the `(label, detail)` pair into a structured kind and renders it.
pub fn render_attachment_message(label: &str, detail: &str, _theme: &Theme) -> String {
    let kind = classify_attachment(label, detail);
    render_attachment_kind(&kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_with_lines_renders() {
        let result = render_attachment_message(
            "file",
            r#"{"display_path":"src/main.rs","lines":120}"#,
            &Theme::default(),
        );
        assert_eq!(result, "Read src/main.rs (120 lines)");
    }

    #[test]
    fn file_truncated_renders() {
        let result = render_attachment_message(
            "file",
            r#"{"display_path":"src/main.rs","lines":120,"truncated":true}"#,
            &Theme::default(),
        );
        assert!(result.contains("truncated"));
    }

    #[test]
    fn directory_renders() {
        let result = render_attachment_message("directory", "src/", &Theme::default());
        assert_eq!(result, "Listed directory src//");
    }

    #[test]
    fn compact_file_reference_renders() {
        let result =
            render_attachment_message("compact_file_reference", "src/lib.rs", &Theme::default());
        assert_eq!(result, "Referenced file src/lib.rs");
    }

    #[test]
    fn pdf_reference_with_pages() {
        let result = render_attachment_message(
            "pdf_reference",
            r#"{"display_path":"doc.pdf","page_count":42}"#,
            &Theme::default(),
        );
        assert_eq!(result, "Referenced PDF doc.pdf (42 pages)");
    }

    #[test]
    fn selected_lines_in_ide() {
        let result = render_attachment_message(
            "selected_lines_in_ide",
            r#"{"display_path":"src/app.rs","line_start":10,"line_end":25,"ide_name":"VS Code"}"#,
            &Theme::default(),
        );
        assert_eq!(result, "Selected 15 lines from src/app.rs in VS Code");
    }

    #[test]
    fn nested_memory_renders() {
        let result =
            render_attachment_message("nested_memory", "project/notes.md", &Theme::default());
        assert_eq!(result, "Loaded project/notes.md");
    }

    #[test]
    fn relevant_memories_renders() {
        let result = render_attachment_message(
            "relevant_memories",
            r#"{"count":3,"paths":["doc/a.md","doc/b.md"]}"#,
            &Theme::default(),
        );
        assert!(result.contains("Recalled 3 memories"));
    }

    #[test]
    fn dynamic_skill_renders() {
        let result = render_attachment_message(
            "dynamic_skill",
            r#"{"count":2,"display_path":".claude/skills/"}"#,
            &Theme::default(),
        );
        assert_eq!(result, "Loaded 2 skills from .claude/skills/");
    }

    #[test]
    fn skill_listing_not_initial() {
        let result = render_attachment_message(
            "skill_listing",
            r#"{"count":5,"is_initial":false}"#,
            &Theme::default(),
        );
        assert_eq!(result, "5 skills available");
    }

    #[test]
    fn skill_listing_initial_returns_empty() {
        let result = render_attachment_message(
            "skill_listing",
            r#"{"count":5,"is_initial":true}"#,
            &Theme::default(),
        );
        assert_eq!(result, "");
    }

    #[test]
    fn invoked_skills_renders() {
        let result = render_attachment_message(
            "invoked_skills",
            r#"["skill_a","skill_b"]"#,
            &Theme::default(),
        );
        assert_eq!(result, "Skills restored (skill_a, skill_b)");
    }

    #[test]
    fn queued_command_renders() {
        let result = render_attachment_message(
            "queued_command",
            r#"{"prompt":"run tests"}"#,
            &Theme::default(),
        );
        assert!(result.contains("queued"));
        assert!(result.contains("run tests"));
    }

    #[test]
    fn mcp_resource_renders() {
        let result = render_attachment_message(
            "mcp_resource",
            r#"{"name":"docs","server":"local"}"#,
            &Theme::default(),
        );
        assert_eq!(result, "Read MCP resource docs from local");
    }

    #[test]
    fn hook_blocking_error_renders() {
        let result = render_attachment_message(
            "hook_blocking_error",
            r#"{"name":"pre-commit","hook_event":"PreCommit","stderr":"lint failed"}"#,
            &Theme::default(),
        );
        assert!(result.contains("pre-commit hook returned blocking error"));
        assert!(result.contains("lint failed"));
    }

    #[test]
    fn hook_stop_event_suppressed() {
        let result = render_attachment_message(
            "hook_blocking_error",
            r#"{"name":"pre-commit","hook_event":"Stop","stderr":"err"}"#,
            &Theme::default(),
        );
        assert_eq!(result, "");
    }

    #[test]
    fn hook_non_blocking_error_suppressed_for_stop() {
        let result = render_attachment_message(
            "hook_non_blocking_error",
            r#"{"name":"hook","hook_event":"SubagentStop"}"#,
            &Theme::default(),
        );
        assert_eq!(result, "");
    }

    #[test]
    fn hook_stopped_continuation_renders() {
        let result = render_attachment_message(
            "hook_stopped_continuation",
            r#"{"name":"pre-commit","hook_event":"PreCommit","message":"lint errors"}"#,
            &Theme::default(),
        );
        assert!(result.contains("pre-commit hook stopped continuation: lint errors"));
    }

    #[test]
    fn hook_system_message_renders() {
        let result = render_attachment_message(
            "hook_system_message",
            r#"{"name":"deploy","content":"Deploying..."}"#,
            &Theme::default(),
        );
        assert_eq!(result, "deploy says: Deploying...");
    }

    #[test]
    fn hook_permission_decision_allowed() {
        let result = render_attachment_message(
            "hook_permission_decision",
            r#"{"name":"auth","hook_event":"PreCommit","decision":"allowed"}"#,
            &Theme::default(),
        );
        assert_eq!(result, "Allowed by auth hook");
    }

    #[test]
    fn hook_permission_decision_denied() {
        let result = render_attachment_message(
            "hook_permission_decision",
            r#"{"name":"audit","hook_event":"PreCommit","decision":"denied"}"#,
            &Theme::default(),
        );
        assert_eq!(result, "Denied by audit hook");
    }

    #[test]
    fn async_hook_response_renders() {
        let result = render_attachment_message(
            "async_hook_response",
            r#"{"event":"Deploy","verbose":false}"#,
            &Theme::default(),
        );
        assert_eq!(result, "Async hook Deploy completed");
    }

    #[test]
    fn task_status_renders() {
        let result = render_attachment_message(
            "task_status",
            r#"{"description":"build","status":"completed"}"#,
            &Theme::default(),
        );
        assert_eq!(result, "Task 'build' completed");
    }

    #[test]
    fn teammate_shutdown_batch_renders() {
        let result = render_attachment_message(
            "teammate_shutdown_batch",
            r#"{"count":3}"#,
            &Theme::default(),
        );
        assert_eq!(result, "3 teammates shut down gracefully");
    }

    #[test]
    fn generic_fallback() {
        let result = render_attachment_message("some_label", "some detail", &Theme::default());
        assert_eq!(result, "Attachment: some_label -> some detail");
    }

    #[test]
    fn empty_detail_for_generic() {
        let result = render_attachment_message("unknown_type", "", &Theme::default());
        assert_eq!(result, "Attachment: unknown_type -> no details");
    }
}
