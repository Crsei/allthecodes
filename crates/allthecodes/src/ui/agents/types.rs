//! Shared data types for Rust-side agent management UI surfaces.

use std::collections::BTreeMap;

pub const AGENT_FOLDER_NAME: &str = ".allthecodes";
pub const AGENTS_DIR: &str = "agents";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AgentSource {
    User,
    Project,
    Local,
    Policy,
    Flag,
    BuiltIn,
    Plugin,
}

impl AgentSource {
    pub fn display_name(self) -> &'static str {
        match self {
            AgentSource::User => "User",
            AgentSource::Project => "Project",
            AgentSource::Local => "Local",
            AgentSource::Policy => "Policy",
            AgentSource::Flag => "CLI argument",
            AgentSource::BuiltIn => "Built-in",
            AgentSource::Plugin => "Plugin",
        }
    }

    pub fn is_editable(self) -> bool {
        matches!(
            self,
            AgentSource::User | AgentSource::Project | AgentSource::Local | AgentSource::Policy
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentSourceFilter {
    All,
    BuiltIn,
    Plugin,
    Source(AgentSource),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentMemoryScope {
    User,
    Project,
    Local,
    None,
}

impl AgentMemoryScope {
    pub fn display_name(self) -> &'static str {
        match self {
            AgentMemoryScope::User => "user",
            AgentMemoryScope::Project => "project",
            AgentMemoryScope::Local => "local",
            AgentMemoryScope::None => "none",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentDefinition {
    pub agent_type: String,
    pub when_to_use: String,
    pub tools: Option<Vec<String>>,
    pub system_prompt: String,
    pub source: AgentSource,
    pub filename: Option<String>,
    pub base_dir: Option<String>,
    pub color: Option<String>,
    pub model: Option<String>,
    pub memory: Option<AgentMemoryScope>,
    pub effort: Option<String>,
    pub permission_mode: Option<String>,
    pub skills: Vec<String>,
    pub hooks: BTreeMap<String, Vec<String>>,
    pub plugin: Option<String>,
    pub overridden_by: Option<AgentSource>,
}

impl AgentDefinition {
    pub fn new(
        agent_type: impl Into<String>,
        when_to_use: impl Into<String>,
        system_prompt: impl Into<String>,
        source: AgentSource,
    ) -> Self {
        Self {
            agent_type: agent_type.into(),
            when_to_use: when_to_use.into(),
            tools: None,
            system_prompt: system_prompt.into(),
            source,
            filename: None,
            base_dir: None,
            color: None,
            model: None,
            memory: None,
            effort: None,
            permission_mode: None,
            skills: Vec::new(),
            hooks: BTreeMap::new(),
            plugin: None,
            overridden_by: None,
        }
    }

    pub fn with_tools(mut self, tools: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.tools = Some(tools.into_iter().map(Into::into).collect());
        self
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    pub fn with_memory(mut self, memory: AgentMemoryScope) -> Self {
        self.memory = Some(memory);
        self
    }

    pub fn with_color(mut self, color: impl Into<String>) -> Self {
        self.color = Some(color.into());
        self
    }

    pub fn with_base_dir(mut self, base_dir: impl Into<String>) -> Self {
        self.base_dir = Some(base_dir.into());
        self
    }

    pub fn with_filename(mut self, filename: impl Into<String>) -> Self {
        self.filename = Some(filename.into());
        self
    }

    pub fn is_built_in(&self) -> bool {
        self.source == AgentSource::BuiltIn
    }

    pub fn is_plugin(&self) -> bool {
        self.source == AgentSource::Plugin
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentValidationResult {
    pub is_valid: bool,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

impl AgentValidationResult {
    pub fn ok() -> Self {
        Self {
            is_valid: true,
            warnings: Vec::new(),
            errors: Vec::new(),
        }
    }

    pub fn with_error(mut self, error: impl Into<String>) -> Self {
        self.is_valid = false;
        self.errors.push(error.into());
        self
    }

    pub fn with_warning(mut self, warning: impl Into<String>) -> Self {
        self.warnings.push(warning.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentModeState {
    MainMenu,
    ListAgents {
        source: AgentSourceFilter,
    },
    AgentMenu {
        agent_type: String,
        previous: Box<AgentModeState>,
    },
    ViewAgent {
        agent_type: String,
        previous: Box<AgentModeState>,
    },
    CreateAgent,
    EditAgent {
        agent_type: String,
        previous: Box<AgentModeState>,
    },
    DeleteConfirm {
        agent_type: String,
        previous: Box<AgentModeState>,
    },
}
