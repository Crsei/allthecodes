//! Tool selection model for agent creation and editing.

use std::collections::{BTreeMap, BTreeSet};

use super::utils::selection_marker;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ToolBucketKind {
    ReadOnly,
    Edit,
    Execution,
    Mcp,
    Other,
}

impl ToolBucketKind {
    pub fn title(self) -> &'static str {
        match self {
            ToolBucketKind::ReadOnly => "Read-only tools",
            ToolBucketKind::Edit => "Edit tools",
            ToolBucketKind::Execution => "Execution tools",
            ToolBucketKind::Mcp => "MCP tools",
            ToolBucketKind::Other => "Other tools",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOption {
    pub name: String,
    pub description: String,
    pub bucket: ToolBucketKind,
}

impl ToolOption {
    pub fn new(name: impl Into<String>, bucket: ToolBucketKind) -> Self {
        let name = name.into();
        Self {
            description: default_description(&name, bucket),
            name,
            bucket,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSelectorState {
    pub tools: Vec<ToolOption>,
    pub selected_tools: BTreeSet<String>,
    pub focus_index: usize,
    pub show_individual_tools: bool,
}

impl ToolSelectorState {
    pub fn new(tools: Vec<ToolOption>, initial_tools: Option<Vec<String>>) -> Self {
        let selected_tools = match initial_tools {
            None => tools.iter().map(|tool| tool.name.clone()).collect(),
            Some(values) if values.iter().any(|value| value == "*") => {
                tools.iter().map(|tool| tool.name.clone()).collect()
            }
            Some(values) => values.into_iter().collect(),
        };
        Self {
            tools,
            selected_tools,
            focus_index: 0,
            show_individual_tools: false,
        }
    }
    pub fn selected_output(&self) -> Option<Vec<String>> {
        if self.selected_tools.len() == self.tools.len() && !self.tools.is_empty() {
            None
        } else {
            Some(self.selected_tools.iter().cloned().collect())
        }
    }
    pub fn toggle_tool(&mut self, tool_name: &str) {
        if !self.selected_tools.remove(tool_name) {
            self.selected_tools.insert(tool_name.to_string());
        }
    }

    pub fn move_next(&mut self) {
        if !self.tools.is_empty() {
            self.focus_index = (self.focus_index + 1) % self.tools.len();
        }
    }

    pub fn move_prev(&mut self) {
        if self.tools.is_empty() {
            return;
        }
        self.focus_index = if self.focus_index == 0 {
            self.tools.len() - 1
        } else {
            self.focus_index - 1
        };
    }

    pub fn toggle_focused(&mut self) {
        if let Some(tool) = self.tools.get(self.focus_index) {
            let name = tool.name.clone();
            self.toggle_tool(&name);
        }
    }

    pub fn render(&self) -> String {
        let mut lines = vec![format!(
            "{} of {} tools selected",
            self.selected_tools.len(),
            self.tools.len()
        )];

        let mut buckets: BTreeMap<ToolBucketKind, Vec<&ToolOption>> = BTreeMap::new();
        for tool in &self.tools {
            buckets.entry(tool.bucket).or_default().push(tool);
        }

        for (bucket, tools) in buckets {
            let selected = tools
                .iter()
                .filter(|tool| self.selected_tools.contains(&tool.name))
                .count();
            lines.push(format!("{} ({}/{})", bucket.title(), selected, tools.len()));
            if self.show_individual_tools {
                for tool in tools {
                    let mark = if self.selected_tools.contains(&tool.name) {
                        "[x]"
                    } else {
                        "[ ]"
                    };
                    let selected = self
                        .tools
                        .get(self.focus_index)
                        .is_some_and(|focused| focused.name == tool.name);
                    lines.push(format!(
                        "{} {mark} {:<18} {}",
                        selection_marker(selected),
                        tool.name,
                        tool.description
                    ));
                }
            }
        }

        lines.join("\n")
    }
}

pub fn default_agent_tools() -> Vec<ToolOption> {
    vec![
        ToolOption::new("Read", ToolBucketKind::ReadOnly),
        ToolOption::new("Grep", ToolBucketKind::ReadOnly),
        ToolOption::new("Glob", ToolBucketKind::ReadOnly),
        ToolOption::new("Edit", ToolBucketKind::Edit),
        ToolOption::new("Write", ToolBucketKind::Edit),
        ToolOption::new("Bash", ToolBucketKind::Execution),
        ToolOption::new("WebFetch", ToolBucketKind::ReadOnly),
        ToolOption::new("mcp__server__tool", ToolBucketKind::Mcp),
    ]
}

fn default_description(name: &str, bucket: ToolBucketKind) -> String {
    match bucket {
        ToolBucketKind::ReadOnly => format!("{name} can inspect context"),
        ToolBucketKind::Edit => format!("{name} can modify files"),
        ToolBucketKind::Execution => format!("{name} can run commands"),
        ToolBucketKind::Mcp => format!("{name} is provided by MCP"),
        ToolBucketKind::Other => format!("{name} is uncategorized"),
    }
}
