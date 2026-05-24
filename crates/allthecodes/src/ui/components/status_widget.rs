//! Built-in rich status widget.

use crate::ui::status_icon::{LegacyStatus, StatusIcon};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusSeverity {
    Ok,
    Warning,
    Error,
}

impl StatusSeverity {
    fn label(self) -> Option<&'static str> {
        match self {
            StatusSeverity::Ok => None,
            StatusSeverity::Warning => Some(StatusIcon::Warning.label()),
            StatusSeverity::Error => Some(StatusIcon::Error.label()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusIndicator {
    pub label: String,
    pub value: String,
    pub severity: StatusSeverity,
}

impl StatusIndicator {
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            severity: StatusSeverity::Ok,
        }
    }

    pub fn warning(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            severity: StatusSeverity::Warning,
        }
    }

    pub fn error(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            severity: StatusSeverity::Error,
        }
    }

    pub fn remote(value: impl Into<String>, severity: StatusSeverity) -> Self {
        Self {
            label: "remote".to_string(),
            value: value.into(),
            severity,
        }
    }

    fn render_inline(&self) -> String {
        match self.severity.label() {
            Some(severity) => format!("{}={}/{}", self.label, self.value, severity),
            None => format!("{}={}", self.label, self.value),
        }
    }

    fn render_detail(&self) -> String {
        match self.severity.label() {
            Some(severity) => format!("{}: {} ({})", self.label, self.value, severity),
            None => format!("{}: {}", self.label, self.value),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct StatusSnapshot {
    pub model: String,
    pub cwd: String,
    pub permission_mode: String,
    pub sandbox: String,
    pub cost_usd: f64,
    pub running_tools: usize,
    pub active_agents: usize,
    pub subsystems_ok: bool,
    pub indicators: Vec<StatusIndicator>,
}

impl StatusSnapshot {
    pub fn render_line(&self) -> String {
        let mut parts = vec![
            self.model.clone(),
            self.cwd.clone(),
            format!("perm={}", self.permission_mode),
            format!("sandbox={}", self.sandbox),
            format!("tools={}", self.running_tools),
            format!("agents={}", self.active_agents),
            format!("${:.4}", self.cost_usd),
            format!("subsystems={}", self.subsystems_label()),
        ];
        parts.extend(self.indicators.iter().map(StatusIndicator::render_inline));
        parts.join(" | ")
    }

    pub fn render_details(&self) -> String {
        let mut lines = vec![
            format!("model: {}", self.model),
            format!("cwd: {}", self.cwd),
            format!("permission: {}", self.permission_mode),
            format!("sandbox: {}", self.sandbox),
            format!("running tools: {}", self.running_tools),
            format!("active agents: {}", self.active_agents),
            format!("subsystems: {}", self.subsystems_label()),
        ];

        if self.indicators.is_empty() {
            lines.push("indicators: none".to_string());
        } else {
            lines.push("indicators:".to_string());
            lines.extend(
                self.indicators
                    .iter()
                    .map(|indicator| format!("- {}", indicator.render_detail())),
            );
        }

        lines.join("\n")
    }

    fn subsystems_label(&self) -> &'static str {
        if self.subsystems_ok {
            LegacyStatus::Ok.label()
        } else {
            LegacyStatus::Attention.label()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{StatusIndicator, StatusSnapshot};

    #[test]
    fn status_widget_renders_present_and_absent_optional_indicators() {
        let all_present = StatusSnapshot {
            model: "claude-sonnet".into(),
            cwd: "F:/repo".into(),
            permission_mode: "acceptEdits".into(),
            sandbox: "workspace,no-net".into(),
            cost_usd: 0.42,
            running_tools: 2,
            active_agents: 1,
            subsystems_ok: false,
            indicators: vec![
                StatusIndicator::new("effort", "medium"),
                StatusIndicator::new("ide", "3 lines selected"),
                StatusIndicator::warning("memory", "1.4 GiB"),
                StatusIndicator::new("pr", "#42 approved"),
                StatusIndicator::error("mcp", "github"),
                StatusIndicator::remote("attention", super::StatusSeverity::Warning),
            ],
        };

        let absent = StatusSnapshot {
            indicators: Vec::new(),
            subsystems_ok: true,
            ..all_present.clone()
        };

        insta::assert_snapshot!(
            "status_widget_optional_indicators",
            format!(
                "## all-present\n{}\n{}\n\n## absent\n{}\n{}",
                all_present.render_line(),
                all_present.render_details(),
                absent.render_line(),
                absent.render_details()
            )
        );
    }
}
