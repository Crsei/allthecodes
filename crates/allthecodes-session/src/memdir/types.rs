use serde::{Deserialize, Serialize};

pub const MEMORY_ENTRYPOINT_NAME: &str = "MEMORY.md";
pub const MEMORY_ENTRYPOINT_MAX_LINES: usize = 200;
pub const MEMORY_ENTRYPOINT_MAX_BYTES: usize = 25_000;
pub const CURATED_MEMORY_PROFILE_MAX_BYTES: usize = MEMORY_ENTRYPOINT_MAX_BYTES;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A single memory entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryEntry {
    /// Unique key for this memory.
    pub key: String,
    /// The memory value / content.
    pub value: String,
    /// Category tag (e.g. "project", "preference", "context").
    #[serde(default)]
    pub category: String,
    /// Closed memory taxonomy aligned with Bun's user / feedback / project /
    /// reference types. Older entries may not have this field; in that case we
    /// infer it from `category` when possible.
    #[serde(
        default,
        rename = "type",
        alias = "memory_type",
        skip_serializing_if = "Option::is_none"
    )]
    pub memory_type: Option<MemoryType>,
    /// Optional short description used by relevant-memory recall.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Optional search terms used by relevant-memory recall.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub search_terms: Vec<String>,
    /// Session that produced or justified this curated memory, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_session_id: Option<String>,
    /// Approval record that authorized this curated memory write, when it came
    /// from a review or self-improvement proposal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_id: Option<String>,
    /// When this entry was created (ISO 8601).
    pub created_at: String,
    /// When this entry was last updated (ISO 8601).
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelevantMemory {
    pub identity: String,
    pub scope: MemoryScope,
    pub entry: MemoryEntry,
    pub score: u32,
    pub matched_terms: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CuratedMemoryTarget {
    User,
    Project,
    Reference,
    Feedback,
}

impl CuratedMemoryTarget {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Project => "project",
            Self::Reference => "reference",
            Self::Feedback => "feedback",
        }
    }

    pub fn memory_type(self) -> MemoryType {
        match self {
            Self::User => MemoryType::User,
            Self::Project => MemoryType::Project,
            Self::Reference => MemoryType::Reference,
            Self::Feedback => MemoryType::Feedback,
        }
    }

    pub fn default_scope(self) -> MemoryScope {
        match self {
            Self::User => MemoryScope::Global,
            Self::Project | Self::Reference | Self::Feedback => MemoryScope::Project,
        }
    }

    pub fn profile_name(self) -> &'static str {
        match self {
            Self::User => "USER.md",
            Self::Project => "PROJECT.md",
            Self::Reference => "REFERENCE.md",
            Self::Feedback => "FEEDBACK.md",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CuratedMemoryWrite {
    pub target: CuratedMemoryTarget,
    pub key: String,
    pub value: String,
    pub source_session_id: Option<String>,
    pub approval_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CuratedMemorySnapshot {
    pub context: String,
    pub captured_at: String,
}

pub const MODEL_ASSISTED_RECALL_CANDIDATE_LIMIT: usize = 20;
pub const MODEL_ASSISTED_RECALL_MAX_RESULTS: usize = 5;

impl MemoryEntry {
    /// Effective closed taxonomy type, including legacy `category` fallback.
    pub fn effective_memory_type(&self) -> Option<MemoryType> {
        self.memory_type
            .or_else(|| MemoryType::parse(&self.category))
    }

    pub(super) fn display_label(&self) -> Option<&str> {
        self.effective_memory_type()
            .map(MemoryType::as_str)
            .or_else(|| {
                let category = self.category.trim();
                (!category.is_empty()).then_some(category)
            })
    }

    pub(super) fn bracketed_label(&self) -> String {
        self.display_label()
            .map(|label| format!(" [{label}]"))
            .unwrap_or_default()
    }
}

/// Bun-compatible closed memory taxonomy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryType {
    User,
    Feedback,
    Project,
    Reference,
}

impl MemoryType {
    pub const ALL: [MemoryType; 4] = [
        MemoryType::User,
        MemoryType::Feedback,
        MemoryType::Project,
        MemoryType::Reference,
    ];

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "user" => Some(MemoryType::User),
            "feedback" => Some(MemoryType::Feedback),
            "project" => Some(MemoryType::Project),
            "reference" => Some(MemoryType::Reference),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            MemoryType::User => "user",
            MemoryType::Feedback => "feedback",
            MemoryType::Project => "project",
            MemoryType::Reference => "reference",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            MemoryType::User => "User role, preferences, goals, responsibilities, or background.",
            MemoryType::Feedback => {
                "User guidance about behavior to avoid or repeat, including corrections and validated approaches."
            }
            MemoryType::Project => {
                "Project context, goals, deadlines, incidents, or motivations that cannot be inferred from code."
            }
            MemoryType::Reference => {
                "Pointers to external systems or resources where current information can be found."
            }
        }
    }
}

/// Scope of memory storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryScope {
    /// Global memories: `{data_root}/memory/`
    Global,
    /// Project-local memories: `.allthecodes/memory/` relative to cwd
    Project,
    /// Team-shared memories: `{data_root}/projects/{sanitized_cwd}/memory/team/`.
    /// Gated by `FEATURE_TEAMMEM`; the directory itself is readable/writable
    /// even when the feature is off so legacy data is never stranded.
    Team,
    /// Auto-captured memories: `{data_root}/auto_memory/`.
    /// Gated at the context-injection layer by the `auto_memory_enabled`
    /// toggle; the directory is always readable so prior captures can be
    /// inspected and purged.
    Auto,
}

impl MemoryScope {
    /// Short label used in selector output and JSON representations.
    pub fn as_str(self) -> &'static str {
        match self {
            MemoryScope::Global => "global",
            MemoryScope::Project => "project",
            MemoryScope::Team => "team",
            MemoryScope::Auto => "auto",
        }
    }

    pub(super) fn context_title(self) -> &'static str {
        match self {
            MemoryScope::Global => "Global Memories",
            MemoryScope::Project => "Project Memories",
            MemoryScope::Team => "Team Memories",
            MemoryScope::Auto => "Auto Memories",
        }
    }
}
