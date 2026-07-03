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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct McpServerHealthDetails {
    pub last_error: Option<String>,
    pub last_error_kind: Option<String>,
    pub failure_count: Option<u32>,
    pub connect_attempt_count: Option<u64>,
    pub retry_scheduled_count: Option<u64>,
    pub retry_exhausted_count: Option<u64>,
    pub recovered_count: Option<u64>,
    pub next_retry_at: Option<i64>,
    pub last_attempt_at: Option<i64>,
    pub last_success_at: Option<i64>,
    pub stderr_tail: Vec<String>,
    pub stderr_tail_dropped_line_count: Option<u64>,
}

impl McpServerHealthDetails {
    pub fn is_empty(&self) -> bool {
        self.last_error.is_none()
            && self.last_error_kind.is_none()
            && self.failure_count.is_none()
            && self.connect_attempt_count.is_none()
            && self.retry_scheduled_count.is_none()
            && self.retry_exhausted_count.is_none()
            && self.recovered_count.is_none()
            && self.next_retry_at.is_none()
            && self.last_attempt_at.is_none()
            && self.last_success_at.is_none()
            && self.stderr_tail.is_empty()
            && self.stderr_tail_dropped_line_count.is_none()
    }

    pub fn summary_fields(&self) -> Vec<String> {
        let mut fields = Vec::new();
        if let Some(kind) = self.last_error_kind.as_deref() {
            fields.push(format!("error_kind={kind}"));
        }
        if let Some(count) = self.failure_count {
            fields.push(format!("failures={count}"));
        }
        if let Some(count) = self.connect_attempt_count {
            fields.push(format!("attempts={count}"));
        }
        if let Some(count) = self.retry_scheduled_count {
            fields.push(format!("retries_scheduled={count}"));
        }
        if let Some(count) = self.retry_exhausted_count {
            fields.push(format!("retries_exhausted={count}"));
        }
        if let Some(count) = self.recovered_count {
            fields.push(format!("recovered={count}"));
        }
        if let Some(next_retry_at) = self.next_retry_at {
            fields.push(format!("next_retry_at={next_retry_at}"));
        }
        if let Some(count) = self.stderr_tail_dropped_line_count {
            fields.push(format!("stderr_dropped={count}"));
        }
        fields
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServer {
    pub name: String,
    pub kind: McpServerKind,
    pub status: McpServerStatus,
    pub command_or_url: String,
    pub config_source: Option<String>,
    pub binding_scope: Option<String>,
    pub binding_permissions: Vec<String>,
    pub tools: Vec<McpTool>,
    pub capabilities: Vec<McpCapability>,
    pub warnings: Vec<String>,
    pub health: McpServerHealthDetails,
}

impl McpServer {
    pub fn new(name: impl Into<String>, kind: McpServerKind) -> Self {
        Self {
            name: name.into(),
            kind,
            status: McpServerStatus::Connected,
            command_or_url: String::new(),
            config_source: None,
            binding_scope: None,
            binding_permissions: Vec::new(),
            tools: Vec::new(),
            capabilities: Vec::new(),
            warnings: Vec::new(),
            health: McpServerHealthDetails::default(),
        }
    }
}
