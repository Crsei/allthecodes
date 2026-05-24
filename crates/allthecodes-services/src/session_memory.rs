//! Session memory service — extracts conversation insights into persistent
//! memory entries on disk.
//!
//! Memory entries are stored as individual JSON files under `~/.allthecodes/memory/`.
//! This is complementary to the `session::memdir` module which manages
//! AGENTS.md/CLAUDE.md-based memories; this service focuses on structured, searchable
//! per-session insights.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for the session memory service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMemoryConfig {
    /// Whether the service is enabled.
    pub enabled: bool,
    /// Directory where memory entries are stored.
    pub memory_dir: PathBuf,
    /// Maximum number of memory entries per session.
    pub max_entries: usize,
    /// Minimum messages in a conversation before extraction is triggered.
    pub min_messages_before_extract: usize,
    /// Maximum age for entries injected back into context. Non-positive or
    /// `None` disables age filtering.
    #[serde(default = "default_max_context_age_seconds")]
    pub max_context_age_seconds: Option<i64>,
    /// Optional allowlist for tags injected back into context. When non-empty,
    /// entries must contain at least one matching tag.
    #[serde(default)]
    pub context_include_tags: Vec<String>,
    /// Optional denylist for tags injected back into context. Matching entries
    /// are never injected, even if they also match the include list.
    #[serde(default)]
    pub context_exclude_tags: Vec<String>,
}

impl Default for SessionMemoryConfig {
    fn default() -> Self {
        SessionMemoryConfig {
            enabled: true,
            memory_dir: allthecodes_config::paths::session_insights_dir(),
            max_entries: 50,
            min_messages_before_extract: 5,
            max_context_age_seconds: default_max_context_age_seconds(),
            context_include_tags: Vec::new(),
            context_exclude_tags: Vec::new(),
        }
    }
}

fn default_max_context_age_seconds() -> Option<i64> {
    Some(30 * 24 * 60 * 60)
}

// ---------------------------------------------------------------------------
// Memory entry
// ---------------------------------------------------------------------------

/// A single memory entry extracted from a conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    /// Unique identifier for this entry.
    pub id: String,
    /// Unix timestamp (seconds since epoch) when the entry was created.
    pub timestamp: i64,
    /// Session ID this entry was extracted from.
    pub session_id: String,
    /// Workspace path this entry was extracted from, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    /// The insight content extracted from the conversation.
    pub content: String,
    /// Categorization tags.
    pub tags: Vec<String>,
}

/// Deterministic insight extracted from a conversation turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedSessionInsight {
    pub content: String,
    pub tags: Vec<String>,
}

/// Build a durable session insight from the latest user request and assistant
/// answer. This is intentionally local and deterministic; it avoids persisting
/// short acknowledgements while keeping enough user intent to make the insight
/// useful when replayed in a later session.
pub fn extract_session_insight(
    user_prompt: Option<&str>,
    assistant_text: &str,
) -> Option<ExtractedSessionInsight> {
    let assistant = normalize_insight_text(assistant_text);
    if !is_meaningful_insight(&assistant) {
        return None;
    }

    let assistant_summary = first_sentences(&assistant, 2, 420);
    let user = user_prompt
        .map(normalize_insight_text)
        .filter(|text| text.len() >= 8)
        .map(|text| truncate_chars(&text, 160));

    let content = match user {
        Some(user) => format!("Request: {} | Insight: {}", user, assistant_summary),
        None => assistant_summary,
    };

    Some(ExtractedSessionInsight {
        tags: infer_insight_tags(&content),
        content,
    })
}

// ---------------------------------------------------------------------------
// Service
// ---------------------------------------------------------------------------

/// Background service for extracting and persisting conversation insights.
pub struct SessionMemoryService {
    config: SessionMemoryConfig,
    entries: Vec<MemoryEntry>,
}

impl SessionMemoryService {
    /// Create a new service with the given configuration.
    pub fn new(config: SessionMemoryConfig) -> Self {
        SessionMemoryService {
            config,
            entries: Vec::new(),
        }
    }

    /// Load all existing memory entries from disk.
    pub fn load_from_disk(&mut self) -> Result<()> {
        let dir = &self.config.memory_dir;
        if !dir.exists() {
            return Ok(());
        }

        let mut loaded: Vec<MemoryEntry> = Vec::new();

        let read_dir = std::fs::read_dir(dir)
            .with_context(|| format!("Failed to read memory directory: {}", dir.display()))?;

        for entry in read_dir {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                match std::fs::read_to_string(&path) {
                    Ok(content) => match serde_json::from_str::<MemoryEntry>(&content) {
                        Ok(mem_entry) => loaded.push(mem_entry),
                        Err(e) => {
                            tracing::warn!(
                                path = %path.display(),
                                error = %e,
                                "Skipping malformed memory entry"
                            );
                        }
                    },
                    Err(e) => {
                        tracing::warn!(
                            path = %path.display(),
                            error = %e,
                            "Failed to read memory entry"
                        );
                    }
                }
            }
        }

        // Sort by timestamp, newest first
        loaded.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        self.entries = loaded;
        Ok(())
    }

    /// Save a single entry to disk and add it to the in-memory collection.
    pub fn save_entry(&mut self, entry: MemoryEntry) -> Result<()> {
        // Ensure the memory directory exists
        if !self.config.memory_dir.exists() {
            std::fs::create_dir_all(&self.config.memory_dir).with_context(|| {
                format!(
                    "Failed to create memory directory: {}",
                    self.config.memory_dir.display()
                )
            })?;
        }

        let filename = format!("{}.json", entry.id);
        let path = self.config.memory_dir.join(&filename);
        let json =
            serde_json::to_string_pretty(&entry).context("Failed to serialize memory entry")?;
        std::fs::write(&path, json)
            .with_context(|| format!("Failed to write memory entry: {}", path.display()))?;

        self.entries.insert(0, entry); // newest first

        // Enforce max entries limit
        while self.entries.len() > self.config.max_entries {
            if let Some(removed) = self.entries.pop() {
                let remove_path = self.config.memory_dir.join(format!("{}.json", removed.id));
                let _ = std::fs::remove_file(&remove_path);
            }
        }

        Ok(())
    }

    /// Check whether the extraction threshold has been met.
    pub fn should_extract(&self, message_count: usize) -> bool {
        self.config.enabled && message_count >= self.config.min_messages_before_extract
    }

    /// Get the most recent memory entries for context injection.
    pub fn get_memory_context(&self, limit: usize) -> Vec<MemoryEntry> {
        self.entries.iter().take(limit).cloned().collect()
    }

    /// Format recent entries for injection into the system prompt.
    pub fn format_memory_context(&self, limit: usize) -> Option<String> {
        self.format_memory_context_for_workspace(limit, None)
    }

    /// Format recent entries scoped to a workspace for prompt injection.
    pub fn format_memory_context_for_workspace(
        &self,
        limit: usize,
        workspace: Option<&Path>,
    ) -> Option<String> {
        self.format_memory_context_for_workspace_excluding_session(limit, workspace, None)
    }

    /// Format recent entries scoped to a workspace while excluding insights
    /// extracted from the currently active session.
    pub fn format_memory_context_for_workspace_excluding_session(
        &self,
        limit: usize,
        workspace: Option<&Path>,
        excluded_session_id: Option<&str>,
    ) -> Option<String> {
        if !self.config.enabled {
            return None;
        }

        let workspace = workspace.map(|p| p.to_string_lossy().to_string());
        let excluded_session_id =
            excluded_session_id.and_then(|id| (!id.trim().is_empty()).then(|| id.trim()));
        let min_timestamp = self
            .config
            .max_context_age_seconds
            .and_then(|age| (age > 0).then(|| chrono::Utc::now().timestamp().saturating_sub(age)));
        let entries: Vec<MemoryEntry> = self
            .entries
            .iter()
            .filter(|entry| match workspace.as_deref() {
                Some(expected) => entry.workspace.as_deref() == Some(expected),
                None => true,
            })
            .filter(|entry| match excluded_session_id {
                Some(session_id) => entry.session_id != session_id,
                None => true,
            })
            .filter(|entry| match min_timestamp {
                Some(min) => entry.timestamp >= min,
                None => true,
            })
            .filter(|entry| entry_matches_context_tags(entry, &self.config))
            .take(limit)
            .cloned()
            .collect();
        if entries.is_empty() {
            return None;
        }

        let mut context = String::from("<session-insights>\n");
        for entry in entries {
            let content = entry.content.replace(['\r', '\n'], " ");
            let tags = if entry.tags.is_empty() {
                String::new()
            } else {
                format!(" tags={}", entry.tags.join(","))
            };
            context.push_str(&format!("- [{}{}] {}\n", entry.session_id, tags, content));
        }
        context.push_str("</session-insights>");
        Some(context)
    }

    /// Simple substring search across all entries' content and tags.
    pub fn search(&self, query: &str) -> Vec<&MemoryEntry> {
        let query_lower = query.to_lowercase();
        self.entries
            .iter()
            .filter(|e| {
                e.content.to_lowercase().contains(&query_lower)
                    || e.tags
                        .iter()
                        .any(|t| t.to_lowercase().contains(&query_lower))
            })
            .collect()
    }
}

fn entry_matches_context_tags(entry: &MemoryEntry, config: &SessionMemoryConfig) -> bool {
    if !config.context_exclude_tags.is_empty()
        && entry_has_any_tag(entry, &config.context_exclude_tags)
    {
        return false;
    }

    config.context_include_tags.is_empty() || entry_has_any_tag(entry, &config.context_include_tags)
}

fn entry_has_any_tag(entry: &MemoryEntry, expected: &[String]) -> bool {
    entry.tags.iter().any(|tag| {
        expected
            .iter()
            .any(|candidate| tag.eq_ignore_ascii_case(candidate))
    })
}

fn normalize_insight_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_meaningful_insight(text: &str) -> bool {
    if text.chars().count() < 24 {
        return false;
    }

    let lower = text.to_lowercase();
    !matches!(
        lower.trim_matches(['.', '!', '?', '。', '！', '？']),
        "ok" | "okay" | "done" | "finished" | "conversation cleared"
    )
}

fn first_sentences(text: &str, max_sentences: usize, max_chars: usize) -> String {
    let mut sentence_count = 0;
    for (idx, ch) in text.char_indices() {
        if matches!(ch, '.' | '!' | '?' | '。' | '！' | '？') {
            sentence_count += 1;
            if sentence_count >= max_sentences {
                return truncate_chars(&text[..idx + ch.len_utf8()], max_chars);
            }
        }
    }
    truncate_chars(text, max_chars)
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }

    let mut truncated = text.chars().take(max_chars).collect::<String>();
    truncated.push_str("...");
    truncated
}

fn infer_insight_tags(content: &str) -> Vec<String> {
    let lower = content.to_lowercase();
    let mut tags = vec!["auto-extract".to_string()];

    push_tag_if_any(
        &mut tags,
        &lower,
        "implementation",
        &["implemented", "added", "updated", "refactor", "wired"],
    );
    push_tag_if_any(
        &mut tags,
        &lower,
        "testing",
        &["test", "cargo test", "pytest", "verify", "verified"],
    );
    push_tag_if_any(
        &mut tags,
        &lower,
        "debugging",
        &["bug", "error", "failed", "panic", "fix"],
    );
    push_tag_if_any(
        &mut tags,
        &lower,
        "architecture",
        &[
            "architecture",
            "boundary",
            "protocol",
            "transport",
            "lifecycle",
        ],
    );
    push_tag_if_any(&mut tags, &lower, "mcp", &["mcp"]);
    push_tag_if_any(&mut tags, &lower, "memory", &["memory", "session-insights"]);

    tags
}

fn push_tag_if_any(tags: &mut Vec<String>, content: &str, tag: &str, needles: &[&str]) {
    if needles.iter().any(|needle| content.contains(needle)) && !tags.iter().any(|t| t == tag) {
        tags.push(tag.to_string());
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config(dir: &Path) -> SessionMemoryConfig {
        SessionMemoryConfig {
            enabled: true,
            memory_dir: dir.to_path_buf(),
            max_entries: 5,
            min_messages_before_extract: 3,
            max_context_age_seconds: default_max_context_age_seconds(),
            context_include_tags: Vec::new(),
            context_exclude_tags: Vec::new(),
        }
    }

    fn make_entry(id: &str, content: &str, tags: &[&str]) -> MemoryEntry {
        MemoryEntry {
            id: id.to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            session_id: "test-session".to_string(),
            workspace: None,
            content: content.to_string(),
            tags: tags.iter().map(|t| t.to_string()).collect(),
        }
    }

    #[test]
    fn should_extract_respects_threshold() {
        let cfg = SessionMemoryConfig {
            min_messages_before_extract: 5,
            ..Default::default()
        };
        let svc = SessionMemoryService::new(cfg);
        assert!(!svc.should_extract(2));
        assert!(!svc.should_extract(4));
        assert!(svc.should_extract(5));
        assert!(svc.should_extract(10));
    }

    #[test]
    fn should_extract_disabled_returns_false() {
        let cfg = SessionMemoryConfig {
            enabled: false,
            ..Default::default()
        };
        let svc = SessionMemoryService::new(cfg);
        assert!(!svc.should_extract(100));
    }

    #[test]
    fn search_matches_content_and_tags() {
        let tmp = std::env::temp_dir().join("cc_rust_test_session_mem_search");
        let mut svc = SessionMemoryService::new(test_config(&tmp));
        svc.entries.push(make_entry("1", "Rust is fast", &["lang"]));
        svc.entries.push(make_entry(
            "2",
            "Python is flexible",
            &["lang", "scripting"],
        ));
        svc.entries
            .push(make_entry("3", "Using cargo build", &["rust", "tooling"]));

        let results = svc.search("rust");
        assert_eq!(results.len(), 2); // entry 1 (content) + entry 3 (tag)

        let results = svc.search("scripting");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "2");
    }

    #[test]
    fn get_memory_context_respects_limit() {
        let tmp = std::env::temp_dir().join("cc_rust_test_session_mem_ctx");
        let mut svc = SessionMemoryService::new(test_config(&tmp));
        for i in 0..10 {
            svc.entries
                .push(make_entry(&format!("e{}", i), "content", &[]));
        }
        let ctx = svc.get_memory_context(3);
        assert_eq!(ctx.len(), 3);
    }

    #[test]
    fn format_memory_context_wraps_recent_entries() {
        let tmp = std::env::temp_dir().join("cc_rust_test_session_mem_format");
        let mut svc = SessionMemoryService::new(test_config(&tmp));
        svc.entries.push(make_entry(
            "1",
            "Use cargo test filters\nfor focused checks.",
            &["testing"],
        ));
        svc.entries
            .push(make_entry("2", "Prefer existing patterns", &[]));

        let ctx = svc.format_memory_context(1).unwrap();
        assert!(ctx.starts_with("<session-insights>"));
        assert!(ctx.contains("Use cargo test filters for focused checks."));
        assert!(ctx.contains("tags=testing"));
        assert!(!ctx.contains("Prefer existing patterns"));
        assert!(ctx.ends_with("</session-insights>"));
    }

    #[test]
    fn format_memory_context_for_workspace_filters_entries() {
        let tmp = std::env::temp_dir().join("cc_rust_test_session_mem_workspace");
        let mut svc = SessionMemoryService::new(test_config(&tmp));
        let workspace = tmp.join("workspace-a");
        let other = tmp.join("workspace-b");

        let mut matching = make_entry("1", "workspace insight", &[]);
        matching.workspace = Some(workspace.to_string_lossy().to_string());
        svc.entries.push(matching);

        let mut non_matching = make_entry("2", "other workspace insight", &[]);
        non_matching.workspace = Some(other.to_string_lossy().to_string());
        svc.entries.push(non_matching);

        let ctx = svc
            .format_memory_context_for_workspace(5, Some(&workspace))
            .unwrap();
        assert!(ctx.contains("workspace insight"));
        assert!(!ctx.contains("other workspace insight"));
    }

    #[test]
    fn format_memory_context_excludes_current_session_entries() {
        let tmp = std::env::temp_dir().join("cc_rust_test_session_mem_session_filter");
        let mut svc = SessionMemoryService::new(test_config(&tmp));
        let workspace = tmp.join("workspace-a");

        let mut current = make_entry("1", "current session insight", &[]);
        current.session_id = "session-current".to_string();
        current.workspace = Some(workspace.to_string_lossy().to_string());
        svc.entries.push(current);

        let mut previous = make_entry("2", "previous session insight", &[]);
        previous.session_id = "session-previous".to_string();
        previous.workspace = Some(workspace.to_string_lossy().to_string());
        svc.entries.push(previous);

        let ctx = svc
            .format_memory_context_for_workspace_excluding_session(
                5,
                Some(&workspace),
                Some("session-current"),
            )
            .unwrap();
        assert!(ctx.contains("previous session insight"));
        assert!(!ctx.contains("current session insight"));
    }

    #[test]
    fn format_memory_context_filters_old_entries_by_age() {
        let tmp = std::env::temp_dir().join("cc_rust_test_session_mem_age");
        let cfg = SessionMemoryConfig {
            max_context_age_seconds: Some(60),
            ..test_config(&tmp)
        };
        let mut svc = SessionMemoryService::new(cfg);
        let now = chrono::Utc::now().timestamp();

        let mut recent = make_entry("1", "recent insight", &[]);
        recent.timestamp = now - 30;
        svc.entries.push(recent);

        let mut old = make_entry("2", "old insight", &[]);
        old.timestamp = now - 120;
        svc.entries.push(old);

        let ctx = svc.format_memory_context(5).unwrap();
        assert!(ctx.contains("recent insight"));
        assert!(!ctx.contains("old insight"));
    }

    #[test]
    fn format_memory_context_applies_include_and_exclude_tags() {
        let tmp = std::env::temp_dir().join("cc_rust_test_session_mem_tags");
        let cfg = SessionMemoryConfig {
            context_include_tags: vec!["decision".to_string()],
            context_exclude_tags: vec!["noisy".to_string()],
            ..test_config(&tmp)
        };
        let mut svc = SessionMemoryService::new(cfg);
        svc.entries
            .push(make_entry("1", "keep this decision", &["Decision"]));
        svc.entries
            .push(make_entry("2", "drop because no include tag", &["testing"]));
        svc.entries.push(make_entry(
            "3",
            "drop because excluded wins",
            &["decision", "noisy"],
        ));

        let ctx = svc.format_memory_context(5).unwrap();
        assert!(ctx.contains("keep this decision"));
        assert!(!ctx.contains("drop because no include tag"));
        assert!(!ctx.contains("drop because excluded wins"));
    }

    #[test]
    fn extract_session_insight_keeps_user_intent_and_tags() {
        let insight = extract_session_insight(
            Some("Please add MCP reconnect tests"),
            "Implemented the manager reconnect path. cargo test -p cc-mcp manager passed.",
        )
        .unwrap();

        assert!(insight
            .content
            .contains("Request: Please add MCP reconnect tests"));
        assert!(insight
            .content
            .contains("Insight: Implemented the manager reconnect path."));
        assert!(insight.tags.contains(&"implementation".to_string()));
        assert!(insight.tags.contains(&"testing".to_string()));
        assert!(insight.tags.contains(&"mcp".to_string()));
    }

    #[test]
    fn extract_session_insight_skips_short_acknowledgements() {
        assert!(extract_session_insight(Some("ship it"), "Done.").is_none());
        assert!(extract_session_insight(None, "ok").is_none());
    }

    #[test]
    fn extract_session_insight_truncates_on_char_boundaries() {
        let assistant = "这是一个用于验证截断逻辑的中文 insight".repeat(80);
        let insight = extract_session_insight(None, &assistant).unwrap();
        assert!(insight.content.ends_with("..."));
        assert!(insight.content.is_char_boundary(insight.content.len()));
    }

    #[test]
    fn save_and_load_roundtrip() {
        let tmp = std::env::temp_dir().join("cc_rust_test_session_mem_roundtrip");
        let _ = std::fs::remove_dir_all(&tmp);

        let cfg = test_config(&tmp);
        let mut svc = SessionMemoryService::new(cfg.clone());

        let entry = make_entry("rt1", "roundtrip test", &["test"]);
        svc.save_entry(entry).unwrap();
        assert_eq!(svc.entries.len(), 1);

        // Load into a fresh service
        let mut svc2 = SessionMemoryService::new(cfg);
        svc2.load_from_disk().unwrap();
        assert_eq!(svc2.entries.len(), 1);
        assert_eq!(svc2.entries[0].id, "rt1");

        // Cleanup
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
