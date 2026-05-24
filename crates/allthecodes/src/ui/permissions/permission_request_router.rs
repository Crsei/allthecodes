//! Routes structured tool permission requests to tool-specific renderers.

use allthecodes_types::callbacks::PermissionRequestPayload;
use serde_json::Value;

use super::bash_permission_request::bash_permission_request::render_bash_permission_request;
use super::bash_permission_request::bash_tool_use_options::render_bash_tool_use_options;
use super::computer_use_approval::computer_use_approval::render_computer_use_approval;
use super::enter_plan_mode_permission_request::enter_plan_mode_permission_request::render_enter_plan_mode_permission_request;
use super::exit_plan_mode_permission_request::exit_plan_mode_permission_request::render_exit_plan_mode_permission_request;
use super::fallback_permission_request::render_fallback_permission_request;
use crate::ui::diff::file_edit_diff::unified_hunk_lines_from_edit;

use super::file_edit_permission_request::file_edit_permission_request::{
    render_file_edit_permission_request, render_file_edit_permission_request_with_diff,
};
use super::file_permission_dialog::file_permission_dialog::render_file_permission_dialog;
use super::file_permission_dialog::ide_diff_config::{render_ide_diff_config, IdeDiffConfig};
use super::file_permission_dialog::permission_options::file_permission_options;
use super::file_permission_dialog::use_file_permission_dialog::FilePermissionDialogState;
use super::file_permission_dialog::use_permission_handler::{
    describe_file_permission_decision, FilePermissionDecision,
};
use super::file_write_permission_request::file_write_permission_request::{
    render_file_write_permission_request, render_file_write_permission_request_with_diff,
};
use super::filesystem_permission_request::filesystem_permission_request::render_filesystem_permission_request;
use super::monitor_permission_request::monitor_permission_request::render_monitor_permission_request;
use super::notebook_edit_permission_request::notebook_edit_permission_request::render_notebook_edit_permission_request;
use super::power_shell_permission_request::power_shell_permission_request::render_power_shell_permission_request;
use super::power_shell_permission_request::powershell_tool_use_options::render_powershell_tool_use_options;
use super::review_artifact_permission_request::review_artifact_permission_request::render_review_artifact_permission_request;
use super::sandbox_permission_request::render_sandbox_permission_request;
use super::sed_edit_permission_request::sed_edit_permission_request::render_sed_edit_permission_request;
use super::shell_permission_helpers::shell_permission_options;
use super::skill_permission_request::skill_permission_request::render_skill_permission_request;
use super::utils::{default_permission_options, render_permission_request, PermissionRequestView};
use super::web_fetch_permission_request::web_fetch_permission_request::render_web_fetch_permission_request;

#[derive(Debug, Clone, PartialEq)]
pub struct PermissionDialogRequest {
    pub tool_use_id: String,
    pub tool_name: String,
    pub tool_input: Value,
    pub message: String,
    pub options: Vec<String>,
}

impl PermissionDialogRequest {
    pub fn from_payload(payload: PermissionRequestPayload) -> Self {
        Self {
            tool_use_id: payload.tool_use_id,
            tool_name: payload.tool_name,
            tool_input: payload.tool_input,
            message: payload.message,
            options: payload.options,
        }
    }

    pub fn legacy(tool_name: &str, input: &str, message: &str) -> Self {
        let tool_input =
            serde_json::from_str(input).unwrap_or_else(|_| Value::String(input.trim().to_string()));
        Self {
            tool_use_id: String::new(),
            tool_name: tool_name.to_string(),
            tool_input,
            message: message.to_string(),
            options: Vec::new(),
        }
    }

    pub fn input_summary(&self) -> String {
        input_summary(&self.tool_input, &self.message)
    }
}

impl From<PermissionRequestPayload> for PermissionDialogRequest {
    fn from(payload: PermissionRequestPayload) -> Self {
        Self::from_payload(payload)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionRouteKind {
    Bash,
    ComputerUse,
    EnterPlanMode,
    ExitPlanMode,
    Fallback,
    FileEdit,
    FileWrite,
    Filesystem,
    Monitor,
    NotebookEdit,
    PowerShell,
    ReviewArtifact,
    Sandbox,
    SedEdit,
    Skill,
    WebFetch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutedPermissionRequest {
    pub kind: PermissionRouteKind,
    pub rendered: String,
    pub options: Vec<String>,
}

pub struct PermissionRequestRouter;

impl PermissionRequestRouter {
    pub fn route(
        request: &PermissionDialogRequest,
        selected_index: usize,
    ) -> RoutedPermissionRequest {
        if let Some(kind) = exact_tool_kind(&request.tool_name) {
            return render_exact_kind(request, kind, selected_index)
                .unwrap_or_else(|| fallback(request, selected_index));
        }

        render_heuristic(request, selected_index)
            .unwrap_or_else(|| fallback(request, selected_index))
    }
}

fn render_exact_kind(
    request: &PermissionDialogRequest,
    kind: PermissionRouteKind,
    selected_index: usize,
) -> Option<RoutedPermissionRequest> {
    match kind {
        PermissionRouteKind::Bash => route_shell(request, kind, selected_index),
        PermissionRouteKind::PowerShell => route_shell(request, kind, selected_index),
        PermissionRouteKind::WebFetch => route_web_fetch(request, selected_index),
        PermissionRouteKind::FileWrite => route_file_write(request, selected_index),
        PermissionRouteKind::FileEdit => route_file_edit(request, selected_index),
        PermissionRouteKind::NotebookEdit => route_notebook_edit(request, selected_index),
        PermissionRouteKind::SedEdit => route_sed_edit(request, selected_index),
        PermissionRouteKind::Filesystem => route_filesystem(request, selected_index),
        PermissionRouteKind::Skill => route_skill(request, selected_index),
        PermissionRouteKind::Monitor => route_monitor(request, selected_index),
        PermissionRouteKind::ReviewArtifact => route_review_artifact(request, selected_index),
        PermissionRouteKind::ComputerUse => route_computer_use(request, selected_index),
        PermissionRouteKind::EnterPlanMode => Some(route_enter_plan_mode(request, selected_index)),
        PermissionRouteKind::ExitPlanMode => Some(route_exit_plan_mode(request, selected_index)),
        PermissionRouteKind::Sandbox => Some(route_sandbox(request, selected_index)),
        PermissionRouteKind::Fallback => None,
    }
}

fn render_heuristic(
    request: &PermissionDialogRequest,
    selected_index: usize,
) -> Option<RoutedPermissionRequest> {
    let normalized_tool = normalize_tool_name(&request.tool_name);
    let kind = if normalized_tool.contains("powershell") || normalized_tool.contains("pwsh") {
        PermissionRouteKind::PowerShell
    } else if normalized_tool.contains("bash")
        || normalized_tool.contains("shell")
        || normalized_tool.contains("exec_command")
    {
        PermissionRouteKind::Bash
    } else if normalized_tool.contains("webfetch")
        || normalized_tool.contains("web_fetch")
        || normalized_tool.contains("fetch")
        || normalized_tool.contains("url")
    {
        PermissionRouteKind::WebFetch
    } else if normalized_tool.contains("notebook") {
        PermissionRouteKind::NotebookEdit
    } else if normalized_tool.contains("sed") {
        PermissionRouteKind::SedEdit
    } else if normalized_tool.contains("write") || normalized_tool.contains("create_file") {
        PermissionRouteKind::FileWrite
    } else if normalized_tool.contains("edit")
        || normalized_tool.contains("patch")
        || normalized_tool.contains("multiedit")
    {
        PermissionRouteKind::FileEdit
    } else {
        return None;
    };
    render_exact_kind(request, kind, selected_index)
}

fn route_shell(
    request: &PermissionDialogRequest,
    kind: PermissionRouteKind,
    selected_index: usize,
) -> Option<RoutedPermissionRequest> {
    let command = string_or_legacy_field(&request.tool_input, &["command", "cmd", "script"])?;
    let options = option_labels(shell_permission_options(&command));
    let rendered = match kind {
        PermissionRouteKind::PowerShell => format!(
            "{}\n\nOptions\n{}",
            render_power_shell_permission_request(&command, selected_index),
            render_powershell_tool_use_options(&command, selected_index),
        ),
        _ => format!(
            "{}\n\nOptions\n{}",
            render_bash_permission_request(&command, selected_index),
            render_bash_tool_use_options(&command, selected_index),
        ),
    };
    Some(RoutedPermissionRequest {
        kind,
        rendered,
        options: merge_options(request, options),
    })
}

fn route_web_fetch(
    request: &PermissionDialogRequest,
    selected_index: usize,
) -> Option<RoutedPermissionRequest> {
    let url = string_field(&request.tool_input, &["url", "uri", "href"])?;
    let method =
        string_field(&request.tool_input, &["method"]).unwrap_or_else(|| "GET".to_string());
    Some(RoutedPermissionRequest {
        kind: PermissionRouteKind::WebFetch,
        rendered: render_web_fetch_permission_request(&url, &method, selected_index),
        options: merge_options(request, option_labels(default_permission_options())),
    })
}

fn route_file_write(
    request: &PermissionDialogRequest,
    selected_index: usize,
) -> Option<RoutedPermissionRequest> {
    let path = path_field(&request.tool_input)?;
    if let Some(hunk_lines) = diff_hunk_lines(&request.tool_input, &path, DiffInputKind::Write) {
        return Some(RoutedPermissionRequest {
            kind: PermissionRouteKind::FileWrite,
            rendered: render_file_write_permission_request_with_diff(
                &path,
                &hunk_lines,
                selected_index,
                80,
            ),
            options: merge_options(
                request,
                option_labels(file_permission_options(&path, false)),
            ),
        });
    }
    let new_lines = string_field(&request.tool_input, &["content"])
        .map(|content| content.lines().count())
        .or_else(|| usize_field(&request.tool_input, &["new_lines", "added_lines"]))
        .unwrap_or(0);
    let replaced_lines =
        usize_field(&request.tool_input, &["replaced_lines", "old_lines"]).unwrap_or(0);
    if new_lines == 0 && replaced_lines == 0 {
        return Some(RoutedPermissionRequest {
            kind: PermissionRouteKind::FileWrite,
            rendered: render_file_dialog_for_path(
                &path,
                "write",
                "no diff supplied",
                selected_index,
            ),
            options: merge_options(
                request,
                option_labels(file_permission_options(&path, false)),
            ),
        });
    }
    Some(RoutedPermissionRequest {
        kind: PermissionRouteKind::FileWrite,
        rendered: render_file_write_permission_request(
            &path,
            new_lines,
            replaced_lines,
            selected_index,
        ),
        options: merge_options(
            request,
            option_labels(file_permission_options(&path, false)),
        ),
    })
}

fn route_file_edit(
    request: &PermissionDialogRequest,
    selected_index: usize,
) -> Option<RoutedPermissionRequest> {
    let path = path_field(&request.tool_input)?;
    let operation = string_field(&request.tool_input, &["operation", "action"])
        .unwrap_or_else(|| "edit".to_string());
    if let Some(hunk_lines) = diff_hunk_lines(&request.tool_input, &path, DiffInputKind::Edit) {
        return Some(RoutedPermissionRequest {
            kind: PermissionRouteKind::FileEdit,
            rendered: render_file_edit_permission_request_with_diff(
                &path,
                &operation,
                &hunk_lines,
                selected_index,
                80,
            ),
            options: merge_options(
                request,
                option_labels(file_permission_options(&path, false)),
            ),
        });
    }
    Some(RoutedPermissionRequest {
        kind: PermissionRouteKind::FileEdit,
        rendered: if operation == "edit" {
            render_file_dialog_for_path(&path, &operation, "no diff supplied", selected_index)
        } else {
            render_file_edit_permission_request(&path, &operation, selected_index)
        },
        options: merge_options(
            request,
            option_labels(file_permission_options(&path, false)),
        ),
    })
}

fn route_notebook_edit(
    request: &PermissionDialogRequest,
    selected_index: usize,
) -> Option<RoutedPermissionRequest> {
    let path = path_field(&request.tool_input)?;
    let cell_index = usize_field(&request.tool_input, &["cell_index", "cell"]).unwrap_or(0);
    let language = string_field(&request.tool_input, &["language", "cell_type"])
        .unwrap_or_else(|| "unknown".to_string());
    Some(RoutedPermissionRequest {
        kind: PermissionRouteKind::NotebookEdit,
        rendered: render_notebook_edit_permission_request(
            &path,
            cell_index,
            &language,
            selected_index,
        ),
        options: merge_options(
            request,
            option_labels(file_permission_options(&path, false)),
        ),
    })
}

fn route_sed_edit(
    request: &PermissionDialogRequest,
    selected_index: usize,
) -> Option<RoutedPermissionRequest> {
    let path = path_field(&request.tool_input)?;
    let pattern = string_field(
        &request.tool_input,
        &["pattern", "old_string", "oldStr", "regex"],
    )?;
    let replacement = string_field(
        &request.tool_input,
        &["replacement", "new_string", "newStr"],
    )?;
    Some(RoutedPermissionRequest {
        kind: PermissionRouteKind::SedEdit,
        rendered: render_sed_edit_permission_request(&path, &pattern, &replacement, selected_index),
        options: merge_options(
            request,
            option_labels(file_permission_options(&path, false)),
        ),
    })
}

fn route_filesystem(
    request: &PermissionDialogRequest,
    selected_index: usize,
) -> Option<RoutedPermissionRequest> {
    let path = path_field(&request.tool_input)?;
    let action = string_field(&request.tool_input, &["action", "operation"])
        .unwrap_or_else(|| request.tool_name.clone());
    Some(RoutedPermissionRequest {
        kind: PermissionRouteKind::Filesystem,
        rendered: render_filesystem_permission_request(&action, &path, selected_index),
        options: merge_options(request, option_labels(default_permission_options())),
    })
}

fn route_skill(
    request: &PermissionDialogRequest,
    selected_index: usize,
) -> Option<RoutedPermissionRequest> {
    let skill_name = string_field(&request.tool_input, &["skill_name", "name", "skill"])?;
    let action = string_field(&request.tool_input, &["action", "operation"])
        .unwrap_or_else(|| "load".to_string());
    Some(RoutedPermissionRequest {
        kind: PermissionRouteKind::Skill,
        rendered: render_skill_permission_request(&skill_name, &action, selected_index),
        options: merge_options(request, option_labels(default_permission_options())),
    })
}

fn route_monitor(
    request: &PermissionDialogRequest,
    selected_index: usize,
) -> Option<RoutedPermissionRequest> {
    let process_name = string_field(&request.tool_input, &["process_name", "process", "command"])?;
    let duration_seconds =
        u64_field(&request.tool_input, &["duration_seconds", "seconds"]).unwrap_or(0);
    Some(RoutedPermissionRequest {
        kind: PermissionRouteKind::Monitor,
        rendered: render_monitor_permission_request(
            &process_name,
            duration_seconds,
            selected_index,
        ),
        options: merge_options(request, option_labels(default_permission_options())),
    })
}

fn route_review_artifact(
    request: &PermissionDialogRequest,
    selected_index: usize,
) -> Option<RoutedPermissionRequest> {
    let artifact_path = string_field(&request.tool_input, &["artifact_path", "path", "file_path"])?;
    let reviewer =
        string_field(&request.tool_input, &["reviewer"]).unwrap_or_else(|| "user".to_string());
    Some(
        RoutedPermissionRequest {
            kind: PermissionRouteKind::ReviewArtifact,
            rendered: render_review_artifact_permission_request(&artifact_path, &reviewer),
            options: merge_options(request, option_labels(default_permission_options())),
        }
        .with_selected(selected_index),
    )
}

fn route_computer_use(
    request: &PermissionDialogRequest,
    selected_index: usize,
) -> Option<RoutedPermissionRequest> {
    let action = string_field(&request.tool_input, &["action", "operation"])?;
    let target = string_field(&request.tool_input, &["target", "selector", "window"])
        .unwrap_or_else(|| "desktop".to_string());
    let screenshot_available =
        bool_field(&request.tool_input, &["screenshot_available", "screenshot"]).unwrap_or(false);
    Some(
        RoutedPermissionRequest {
            kind: PermissionRouteKind::ComputerUse,
            rendered: render_computer_use_approval(&action, &target, screenshot_available),
            options: merge_options(request, option_labels(default_permission_options())),
        }
        .with_selected(selected_index),
    )
}

fn route_enter_plan_mode(
    request: &PermissionDialogRequest,
    _selected_index: usize,
) -> RoutedPermissionRequest {
    let reason = string_field(&request.tool_input, &["reason"])
        .filter(|reason| !reason.trim().is_empty())
        .unwrap_or_else(|| fallback_message(request));
    RoutedPermissionRequest {
        kind: PermissionRouteKind::EnterPlanMode,
        rendered: render_enter_plan_mode_permission_request(&reason),
        options: merge_options(request, option_labels(default_permission_options())),
    }
}

fn route_exit_plan_mode(
    request: &PermissionDialogRequest,
    _selected_index: usize,
) -> RoutedPermissionRequest {
    let plan_summary = string_field(&request.tool_input, &["plan_summary", "plan", "summary"])
        .filter(|summary| !summary.trim().is_empty())
        .unwrap_or_else(|| fallback_message(request));
    let tests = string_array_field(&request.tool_input, &["tests", "verification"]);
    RoutedPermissionRequest {
        kind: PermissionRouteKind::ExitPlanMode,
        rendered: render_exit_plan_mode_permission_request(&plan_summary, &tests),
        options: merge_options(request, option_labels(default_permission_options())),
    }
}

fn route_sandbox(
    request: &PermissionDialogRequest,
    _selected_index: usize,
) -> RoutedPermissionRequest {
    let profile = string_field(&request.tool_input, &["profile", "sandbox"])
        .unwrap_or_else(|| "default".to_string());
    let operation = string_field(&request.tool_input, &["operation", "action"])
        .filter(|operation| !operation.trim().is_empty())
        .unwrap_or_else(|| fallback_message(request));
    let violations = string_array_field(&request.tool_input, &["violations", "denials"]);
    RoutedPermissionRequest {
        kind: PermissionRouteKind::Sandbox,
        rendered: render_sandbox_permission_request(&profile, &operation, &violations),
        options: merge_options(request, option_labels(default_permission_options())),
    }
}

fn fallback(request: &PermissionDialogRequest, _selected_index: usize) -> RoutedPermissionRequest {
    let summary = request.input_summary();
    let details = fallback_details(request, &summary);
    if let Some(worker_name) = worker_name_field(&request.tool_input) {
        let rendered = render_permission_request(
            &PermissionRequestView::new("Permission required", &request.tool_name, summary)
                .with_details(details.iter().cloned())
                .with_risk("unknown tool behavior")
                .for_worker(worker_name),
        );
        return RoutedPermissionRequest {
            kind: PermissionRouteKind::Fallback,
            rendered,
            options: merge_options(request, option_labels(default_permission_options())),
        };
    }
    RoutedPermissionRequest {
        kind: PermissionRouteKind::Fallback,
        rendered: render_fallback_permission_request(&request.tool_name, &summary, &details),
        options: merge_options(request, option_labels(default_permission_options())),
    }
}

trait RoutedSelection {
    fn with_selected(self, _selected_index: usize) -> Self;
}

impl RoutedSelection for RoutedPermissionRequest {
    fn with_selected(self, _selected_index: usize) -> Self {
        self
    }
}

fn exact_tool_kind(tool_name: &str) -> Option<PermissionRouteKind> {
    match tool_name {
        "Bash" | "bash" => Some(PermissionRouteKind::Bash),
        "PowerShell" | "powershell" | "Pwsh" | "pwsh" => Some(PermissionRouteKind::PowerShell),
        "WebFetch" | "web_fetch" | "Web_Fetch" => Some(PermissionRouteKind::WebFetch),
        "Write" | "write" => Some(PermissionRouteKind::FileWrite),
        "Edit" | "edit" | "MultiEdit" | "multiedit" => Some(PermissionRouteKind::FileEdit),
        "NotebookEdit" | "notebook_edit" => Some(PermissionRouteKind::NotebookEdit),
        "SedEdit" | "sed_edit" => Some(PermissionRouteKind::SedEdit),
        "Filesystem" | "filesystem" | "Read" | "read" | "Glob" | "glob" | "Grep" | "grep"
        | "LS" | "ls" => Some(PermissionRouteKind::Filesystem),
        "Skill" | "skill" | "SkillLoad" | "skill_load" => Some(PermissionRouteKind::Skill),
        "Monitor" | "monitor" => Some(PermissionRouteKind::Monitor),
        "ReviewArtifact" | "review_artifact" => Some(PermissionRouteKind::ReviewArtifact),
        "ComputerUse" | "computer_use" => Some(PermissionRouteKind::ComputerUse),
        "EnterPlanMode" | "enter_plan_mode" => Some(PermissionRouteKind::EnterPlanMode),
        "ExitPlanMode" | "exit_plan_mode" => Some(PermissionRouteKind::ExitPlanMode),
        "Sandbox" | "sandbox" => Some(PermissionRouteKind::Sandbox),
        _ => None,
    }
}

fn merge_options(request: &PermissionDialogRequest, rendered_options: Vec<String>) -> Vec<String> {
    if !request.options.is_empty() && !is_generic_options(&request.options) {
        request.options.clone()
    } else if rendered_options.is_empty() {
        option_labels(default_permission_options())
    } else {
        rendered_options
    }
}

fn is_generic_options(options: &[String]) -> bool {
    let normalized = options
        .iter()
        .map(|option| option.to_ascii_lowercase().replace('_', " "))
        .collect::<Vec<_>>();
    normalized == ["allow", "deny", "always allow"]
}

fn option_labels(options: Vec<super::utils::PermissionOption>) -> Vec<String> {
    options.into_iter().map(|option| option.label).collect()
}

fn input_summary(input: &Value, message: &str) -> String {
    match input {
        Value::Null => fallback_message_from_text(message),
        Value::String(value) if value.trim().is_empty() => fallback_message_from_text(message),
        Value::String(value) => value.trim().to_string(),
        Value::Object(map) if map.is_empty() => fallback_message_from_text(message),
        value => {
            serde_json::to_string(value).unwrap_or_else(|_| fallback_message_from_text(message))
        }
    }
}

fn fallback_message(request: &PermissionDialogRequest) -> String {
    fallback_message_from_text(&request.message)
}

fn fallback_message_from_text(message: &str) -> String {
    if message.trim().is_empty() {
        "(no details supplied)".to_string()
    } else {
        message.trim().to_string()
    }
}

fn fallback_details(request: &PermissionDialogRequest, summary: &str) -> Vec<String> {
    let mut details = Vec::new();
    if !request.message.trim().is_empty() && request.message.trim() != summary.trim() {
        details.push(request.message.trim().to_string());
    }
    if !matches!(request.tool_input, Value::Null) {
        let input = input_summary(&request.tool_input, "");
        if !input.trim().is_empty() && input.trim() != summary.trim() {
            details.push(format!("input: {input}"));
        }
    }
    details
}

fn normalize_tool_name(tool_name: &str) -> String {
    tool_name
        .chars()
        .filter(|ch| !ch.is_whitespace() && *ch != '-' && *ch != '.')
        .collect::<String>()
        .to_ascii_lowercase()
}

fn path_field(input: &Value) -> Option<String> {
    string_or_legacy_field(
        input,
        &["path", "file_path", "filepath", "file", "notebook_path"],
    )
}

fn render_file_dialog_for_path(
    path: &str,
    action: &str,
    diff_summary: &str,
    selected_index: usize,
) -> String {
    let mut state = FilePermissionDialogState::new(path);
    let options = file_permission_options(path, false);
    for _ in 0..selected_index.min(options.len().saturating_sub(1)) {
        state.select_next(options.len());
    }
    state.show_diff = !diff_summary.trim().is_empty();
    let mut rendered = render_file_permission_dialog(&state, action, diff_summary);
    rendered.push_str("\n\n");
    rendered.push_str(&render_ide_diff_config(&IdeDiffConfig {
        enabled: state.show_diff,
        editor: "default".to_string(),
        supports_inline_diff: true,
    }));
    if let Some(option) = options.get(state.selected_index.min(options.len().saturating_sub(1))) {
        rendered.push('\n');
        rendered.push_str(&describe_file_permission_decision(
            &FilePermissionDecision {
                decision: option.decision,
                path: path.to_string(),
                save_rule: matches!(option.scope, super::utils::PermissionScope::Project),
            },
        ));
    }
    rendered
}

fn worker_name_field(input: &Value) -> Option<String> {
    string_field(
        input,
        &[
            "worker",
            "worker_name",
            "workerName",
            "agent",
            "agent_name",
            "agentName",
        ],
    )
}

fn string_or_legacy_field(input: &Value, keys: &[&str]) -> Option<String> {
    match input {
        Value::String(value) => non_empty(value),
        _ => string_field(input, keys),
    }
}

fn string_field(input: &Value, keys: &[&str]) -> Option<String> {
    match input {
        Value::Object(map) => {
            for key in keys {
                if let Some(value) = map.get(*key) {
                    if let Some(value) = value_as_string(value) {
                        return Some(value);
                    }
                }
            }
            None
        }
        _ => None,
    }
}

fn string_array_field(input: &Value, keys: &[&str]) -> Vec<String> {
    let Value::Object(map) = input else {
        return Vec::new();
    };
    for key in keys {
        let Some(value) = map.get(*key) else {
            continue;
        };
        match value {
            Value::Array(values) => {
                return values.iter().filter_map(value_as_string).collect();
            }
            Value::String(value) => {
                return non_empty(value).into_iter().collect();
            }
            _ => {}
        }
    }
    Vec::new()
}

fn usize_field(input: &Value, keys: &[&str]) -> Option<usize> {
    u64_field(input, keys).and_then(|value| usize::try_from(value).ok())
}

fn u64_field(input: &Value, keys: &[&str]) -> Option<u64> {
    let Value::Object(map) = input else {
        return None;
    };
    for key in keys {
        if let Some(value) = map.get(*key) {
            if let Some(number) = value.as_u64() {
                return Some(number);
            }
            if let Some(text) = value.as_str().and_then(|text| text.parse::<u64>().ok()) {
                return Some(text);
            }
        }
    }
    None
}

fn bool_field(input: &Value, keys: &[&str]) -> Option<bool> {
    let Value::Object(map) = input else {
        return None;
    };
    for key in keys {
        if let Some(value) = map.get(*key) {
            if let Some(flag) = value.as_bool() {
                return Some(flag);
            }
            if let Some(text) = value.as_str() {
                match text.to_ascii_lowercase().as_str() {
                    "true" | "yes" | "1" => return Some(true),
                    "false" | "no" | "0" => return Some(false),
                    _ => {}
                }
            }
        }
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffInputKind {
    Edit,
    Write,
}

fn diff_hunk_lines(input: &Value, path: &str, kind: DiffInputKind) -> Option<Vec<String>> {
    let explicit = string_array_field(input, &["hunk_lines", "hunks"]);
    if !explicit.is_empty() {
        return Some(explicit);
    }

    for key in ["patch", "diff", "unified_diff"] {
        if let Some(diff) = string_field(input, &[key]) {
            let lines = diff.lines().map(str::to_string).collect::<Vec<_>>();
            if !lines.is_empty() {
                return Some(lines);
            }
        }
    }

    let (old, new) = match kind {
        DiffInputKind::Edit => (
            raw_string_field(input, &["old_content", "old_string"])?,
            raw_string_field(input, &["new_content", "new_string"])?,
        ),
        DiffInputKind::Write => (
            raw_string_field(input, &["old_content", "previous_content"]).unwrap_or_default(),
            raw_string_field(input, &["new_content", "content"])?,
        ),
    };
    let hunk_lines = unified_hunk_lines_from_edit(path, &old, &new);
    (!hunk_lines.is_empty()).then_some(hunk_lines)
}

fn raw_string_field(input: &Value, keys: &[&str]) -> Option<String> {
    let Value::Object(map) = input else {
        return None;
    };
    for key in keys {
        if let Some(value) = map.get(*key).and_then(Value::as_str) {
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn value_as_string(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => non_empty(value),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{PermissionDialogRequest, PermissionRequestRouter, PermissionRouteKind};
    use serde_json::json;

    fn request(tool_name: &str, input: serde_json::Value) -> PermissionDialogRequest {
        PermissionDialogRequest {
            tool_use_id: "tool-1".to_string(),
            tool_name: tool_name.to_string(),
            tool_input: input,
            message: "needs approval".to_string(),
            options: Vec::new(),
        }
    }

    #[test]
    fn routes_exact_bash_requests_to_bash_renderer() {
        let routed = PermissionRequestRouter::route(
            &request("Bash", json!({"command":"cargo test --workspace"})),
            0,
        );
        assert_eq!(routed.kind, PermissionRouteKind::Bash);
        assert!(routed.rendered.contains("Bash command permission"));
        assert!(routed.rendered.contains("shell: bash"));
        assert!(routed.rendered.contains("risk:"));
        assert!(routed
            .options
            .contains(&"Always allow exact command".to_string()));
    }

    #[test]
    fn routes_exact_tool_names_for_supported_renderers() {
        let cases = [
            (
                "PowerShell",
                json!({"command":"Get-ChildItem"}),
                PermissionRouteKind::PowerShell,
            ),
            (
                "WebFetch",
                json!({"url":"https://example.com/json","method":"POST"}),
                PermissionRouteKind::WebFetch,
            ),
            (
                "Write",
                json!({"file_path":"src/lib.rs"}),
                PermissionRouteKind::FileWrite,
            ),
            (
                "Edit",
                json!({"file_path":"src/lib.rs"}),
                PermissionRouteKind::FileEdit,
            ),
            (
                "NotebookEdit",
                json!({"notebook_path":"nb.ipynb","cell_index":2,"language":"python"}),
                PermissionRouteKind::NotebookEdit,
            ),
            (
                "SedEdit",
                json!({"file_path":"src/lib.rs","pattern":"old","replacement":"new"}),
                PermissionRouteKind::SedEdit,
            ),
            (
                "Filesystem",
                json!({"path":"src/lib.rs","action":"read"}),
                PermissionRouteKind::Filesystem,
            ),
            (
                "Skill",
                json!({"skill_name":"rust","action":"load"}),
                PermissionRouteKind::Skill,
            ),
            (
                "Monitor",
                json!({"process_name":"cargo test","duration_seconds":5}),
                PermissionRouteKind::Monitor,
            ),
            (
                "ReviewArtifact",
                json!({"artifact_path":"target/report.json","reviewer":"qa"}),
                PermissionRouteKind::ReviewArtifact,
            ),
            (
                "ComputerUse",
                json!({"action":"click","target":"button","screenshot_available":true}),
                PermissionRouteKind::ComputerUse,
            ),
            (
                "EnterPlanMode",
                json!({"reason":"inspect first"}),
                PermissionRouteKind::EnterPlanMode,
            ),
            (
                "ExitPlanMode",
                json!({"plan":"ship it","tests":["cargo test"]}),
                PermissionRouteKind::ExitPlanMode,
            ),
            (
                "Sandbox",
                json!({"profile":"read-only","operation":"write target","violations":["write denied"]}),
                PermissionRouteKind::Sandbox,
            ),
        ];

        for (tool, input, kind) in cases {
            let routed = PermissionRequestRouter::route(&request(tool, input), 0);
            assert_eq!(routed.kind, kind, "{tool} should route exactly");
            assert!(!routed.rendered.trim().is_empty());
        }
    }

    #[test]
    fn routes_file_edit_with_structured_diff_preview() {
        let routed = PermissionRequestRouter::route(
            &request(
                "Edit",
                json!({
                    "file_path": "src/lib.rs",
                    "old_string": "old_call();\n",
                    "new_string": "new_call();\n",
                }),
            ),
            0,
        );

        assert_eq!(routed.kind, PermissionRouteKind::FileEdit);
        assert!(routed.rendered.contains("summary: Added 1 line"));
        assert!(routed.rendered.contains("- old_call();"));
        assert!(routed.rendered.contains("+ new_call();"));
    }

    #[test]
    fn routes_file_write_content_as_diff_from_empty_file() {
        let routed = PermissionRequestRouter::route(
            &request(
                "Write",
                json!({
                    "file_path": "src/new.rs",
                    "content": "fn main() {}\n",
                }),
            ),
            0,
        );

        assert_eq!(routed.kind, PermissionRouteKind::FileWrite);
        assert!(routed.rendered.contains("summary: Added 1 line"));
        assert!(routed.rendered.contains("+ fn main() {}"));
    }

    #[test]
    fn falls_back_for_known_tool_with_incomplete_payload() {
        let routed = PermissionRequestRouter::route(&request("Bash", json!({"cwd":"/tmp"})), 0);
        assert_eq!(routed.kind, PermissionRouteKind::Fallback);
        assert!(routed.rendered.contains("Permission required"));
        assert!(routed.rendered.contains("unknown tool behavior"));
    }

    #[test]
    fn honors_non_generic_backend_options() {
        let mut req = request("Bash", json!({"command":"cargo test"}));
        req.options = vec!["Proceed".to_string(), "Reject".to_string()];

        let routed = PermissionRequestRouter::route(&req, 0);

        assert_eq!(
            routed.options,
            vec!["Proceed".to_string(), "Reject".to_string()]
        );
    }

    #[test]
    fn heuristic_fallback_keeps_legacy_string_inputs() {
        let routed = PermissionRequestRouter::route(
            &PermissionDialogRequest::legacy("custom_shell_tool", "cargo test", ""),
            0,
        );
        assert_eq!(routed.kind, PermissionRouteKind::Bash);
        assert!(routed.rendered.contains("cargo test"));
    }

    #[test]
    fn falls_back_for_unknown_tool_requests() {
        let routed =
            PermissionRequestRouter::route(&request("UnknownTool", json!({"opaque":true})), 0);
        assert_eq!(routed.kind, PermissionRouteKind::Fallback);
        assert!(routed.rendered.contains("Permission required"));
        assert!(routed.rendered.contains("unknown tool behavior"));
    }
}
