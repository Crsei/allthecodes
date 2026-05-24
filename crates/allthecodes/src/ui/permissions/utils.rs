//! Shared permission rendering models and formatting helpers.

use crate::ui::better_view_panel::{plain_row, selected_row, BetterViewPanel};

const REQUEST_SUMMARY_MAX_CHARS: usize = 140;
const REQUEST_DETAIL_MAX_CHARS: usize = 180;
const COMMAND_PREVIEW_MAX_CHARS: usize = 180;
const PATH_SUMMARY_MAX_CHARS: usize = 140;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionDecision {
    Allow,
    Deny,
    AlwaysAllow,
    Ask,
    Escalate,
}

impl PermissionDecision {
    pub fn label(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
            Self::AlwaysAllow => "always allow",
            Self::Ask => "ask",
            Self::Escalate => "escalate",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionScope {
    Request,
    Session,
    Project,
    Local,
    Policy,
}

impl PermissionScope {
    pub fn label(self) -> &'static str {
        match self {
            Self::Request => "this request",
            Self::Session => "session",
            Self::Project => "project",
            Self::Local => "local settings",
            Self::Policy => "managed policy",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionOption {
    pub label: String,
    pub description: String,
    pub decision: PermissionDecision,
    pub scope: PermissionScope,
}

impl PermissionOption {
    pub fn new(
        label: impl Into<String>,
        description: impl Into<String>,
        decision: PermissionDecision,
        scope: PermissionScope,
    ) -> Self {
        Self {
            label: label.into(),
            description: description.into(),
            decision,
            scope,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionRequestView {
    pub title: String,
    pub tool_name: String,
    pub summary: String,
    pub details: Vec<String>,
    pub risk: Option<String>,
    pub options: Vec<PermissionOption>,
    pub selected_index: usize,
    pub worker_name: Option<String>,
}

impl PermissionRequestView {
    pub fn new(
        title: impl Into<String>,
        tool_name: impl Into<String>,
        summary: impl Into<String>,
    ) -> Self {
        Self {
            title: title.into(),
            tool_name: tool_name.into(),
            summary: summary.into(),
            details: Vec::new(),
            risk: None,
            options: default_permission_options(),
            selected_index: 0,
            worker_name: None,
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        let detail = detail.into();
        if !detail.trim().is_empty() {
            self.details.push(detail);
        }
        self
    }

    pub fn with_details(mut self, details: impl IntoIterator<Item = impl Into<String>>) -> Self {
        for detail in details {
            let detail = detail.into();
            if !detail.trim().is_empty() {
                self.details.push(detail);
            }
        }
        self
    }

    pub fn with_risk(mut self, risk: impl Into<String>) -> Self {
        let risk = risk.into();
        if !risk.trim().is_empty() {
            self.risk = Some(risk);
        }
        self
    }

    pub fn with_options(mut self, options: Vec<PermissionOption>, selected_index: usize) -> Self {
        self.options = if options.is_empty() {
            default_permission_options()
        } else {
            options
        };
        self.selected_index = selected_index;
        self
    }

    pub fn for_worker(mut self, worker_name: impl Into<String>) -> Self {
        let worker_name = worker_name.into();
        if !worker_name.trim().is_empty() {
            self.worker_name = Some(worker_name);
        }
        self
    }
}

pub fn default_permission_options() -> Vec<PermissionOption> {
    vec![
        PermissionOption::new(
            "Allow",
            "run this tool once",
            PermissionDecision::Allow,
            PermissionScope::Request,
        ),
        PermissionOption::new(
            "Deny",
            "block this tool call",
            PermissionDecision::Deny,
            PermissionScope::Request,
        ),
        PermissionOption::new(
            "Always allow",
            "save a reusable allow rule",
            PermissionDecision::AlwaysAllow,
            PermissionScope::Project,
        ),
        PermissionOption::new(
            "Escalate",
            "ask for a higher-level approval path",
            PermissionDecision::Escalate,
            PermissionScope::Session,
        ),
    ]
}

pub fn render_permission_request(view: &PermissionRequestView) -> String {
    let mut lines = vec![plain_row(
        "Request",
        truncate_middle(&view.summary, REQUEST_SUMMARY_MAX_CHARS),
    )];
    if let Some(worker) = &view.worker_name {
        lines.push(plain_row("Context", format!("worker={worker}")));
    }
    if !view.details.is_empty() {
        lines.push(String::new());
        lines.push("Context".to_string());
        for detail in &view.details {
            for line in normalize_multiline(detail) {
                lines.push(plain_row(
                    "",
                    truncate_middle(&line, REQUEST_DETAIL_MAX_CHARS),
                ));
            }
        }
    }
    lines.push(String::new());
    lines.push("Decisions".to_string());
    lines.extend(render_permission_options(
        &view.options,
        view.selected_index,
    ));

    let risk = view.risk.as_deref().unwrap_or("tool request");
    BetterViewPanel::new(&view.title)
        .summary(format!("tool={} risk={risk}", view.tool_name))
        .sections_title("Request")
        .sections(
            vec![
                "Request".to_string(),
                "Context".to_string(),
                "Decisions".to_string(),
            ],
            0,
        )
        .detail_title("Decision")
        .detail_lines(lines)
        .footer("Up/Down decision | Enter confirm | Esc deny")
        .render()
}

pub fn render_permission_options(
    options: &[PermissionOption],
    selected_index: usize,
) -> Vec<String> {
    let options = if options.is_empty() {
        default_permission_options()
    } else {
        options.to_vec()
    };
    options
        .iter()
        .enumerate()
        .map(|(idx, option)| {
            let marker = if idx == selected_index.min(options.len().saturating_sub(1)) {
                ">"
            } else {
                " "
            };
            selected_row(
                &option.label,
                format!(
                    "{}  {}  scope={}",
                    option.decision.label(),
                    option.description,
                    option.scope.label()
                ),
                marker == ">",
            )
        })
        .collect()
}

pub fn render_key_values(title: &str, rows: &[(impl AsRef<str>, impl AsRef<str>)]) -> String {
    let mut lines = vec![title.to_string()];
    if rows.is_empty() {
        lines.push("  <none>".to_string());
    } else {
        lines.extend(
            rows.iter()
                .map(|(key, value)| format!("  {}: {}", key.as_ref(), value.as_ref())),
        );
    }
    lines.join("\n")
}

pub fn render_bullets(title: &str, rows: &[impl AsRef<str>]) -> String {
    let mut lines = vec![title.to_string()];
    if rows.is_empty() {
        lines.push("  <none>".to_string());
    } else {
        lines.extend(rows.iter().map(|row| format!("  - {}", row.as_ref())));
    }
    lines.join("\n")
}

pub fn truncate_middle(input: &str, max_chars: usize) -> String {
    let chars: Vec<char> = input.chars().collect();
    if chars.len() <= max_chars {
        return input.to_string();
    }
    if max_chars <= 3 {
        return chars.into_iter().take(max_chars).collect();
    }
    let keep = max_chars - 3;
    let left = keep / 2;
    let right = keep - left;
    let head: String = chars.iter().take(left).collect();
    let tail: String = chars
        .iter()
        .skip(chars.len().saturating_sub(right))
        .collect();
    format!("{head}...{tail}")
}

pub fn normalize_multiline(input: &str) -> Vec<String> {
    let rows = input
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    if rows.is_empty() {
        vec!["<empty>".to_string()]
    } else {
        rows
    }
}

pub fn command_preview(command: &str) -> String {
    normalize_multiline(command)
        .into_iter()
        .map(|line| truncate_middle(&line, COMMAND_PREVIEW_MAX_CHARS))
        .collect::<Vec<_>>()
        .join(" && ")
}

pub fn shell_risk_hint(command: &str) -> &'static str {
    let lower = command.to_ascii_lowercase();
    if lower.contains("rm ")
        || lower.contains("remove-item")
        || lower.contains("del ")
        || lower.contains("format ")
    {
        "destructive filesystem command"
    } else if lower.contains("curl ")
        || lower.contains("wget ")
        || lower.contains("invoke-webrequest")
    {
        "network command"
    } else if lower.contains("sudo") || lower.contains("runas") {
        "elevated command"
    } else {
        "shell command"
    }
}

pub fn path_action_summary(action: &str, path: &str) -> String {
    format!(
        "{} {}",
        action.trim(),
        truncate_middle(path.trim(), PATH_SUMMARY_MAX_CHARS)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escalate_decision_has_label_and_renders_as_option() {
        let rows = render_permission_options(
            &[PermissionOption::new(
                "Escalate",
                "ask the sandbox for elevated access",
                PermissionDecision::Escalate,
                PermissionScope::Session,
            )],
            0,
        );

        assert_eq!(PermissionDecision::Escalate.label(), "escalate");
        assert!(rows[0].contains("escalate"));
        assert!(rows[0].contains("scope=session"));
    }
}
