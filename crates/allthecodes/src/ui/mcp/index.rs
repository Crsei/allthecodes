//! Shared MCP UI data model.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpServerKind {
    Stdio,
    Remote,
    Agent,
}

impl McpServerKind {
    pub fn label(self) -> &'static str {
        match self {
            McpServerKind::Stdio => "stdio",
            McpServerKind::Remote => "remote",
            McpServerKind::Agent => "agent",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpServerStatus {
    Connected,
    Connecting,
    Failed,
    Disabled,
}

impl McpServerStatus {
    pub fn label(self) -> &'static str {
        match self {
            McpServerStatus::Connected => "connected",
            McpServerStatus::Connecting => "connecting",
            McpServerStatus::Failed => "failed",
            McpServerStatus::Disabled => "disabled",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpCapability {
    pub name: String,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    pub input_schema: Vec<(String, String)>,
}

impl McpTool {
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            input_schema: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServer {
    pub name: String,
    pub kind: McpServerKind,
    pub status: McpServerStatus,
    pub command_or_url: String,
    pub tools: Vec<McpTool>,
    pub capabilities: Vec<McpCapability>,
    pub warnings: Vec<String>,
}

impl McpServer {
    pub fn new(name: impl Into<String>, kind: McpServerKind) -> Self {
        Self {
            name: name.into(),
            kind,
            status: McpServerStatus::Connected,
            command_or_url: String::new(),
            tools: Vec::new(),
            capabilities: Vec::new(),
            warnings: Vec::new(),
        }
    }
}
