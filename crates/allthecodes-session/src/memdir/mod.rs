//! Memory directory system — manages AGENTS.md/CLAUDE.md-based session memories.
//!
//! Provides reading, writing, and listing of memory entries stored alongside
//! session data. Memories are key-value pairs persisted as individual files
//! under `~/.allthecodes/memory/` (global) or `.allthecodes/memory/` (project-local).
//!
//! Corresponds to TypeScript: memdir/ (8 files)

mod crud;
mod index;
mod recall;
mod types;

pub use crud::{
    build_memory_context, build_memory_context_with, delete_memory, read_memory, search_memories,
    write_memory,
};
pub use index::{
    build_memory_index, list_memories, memory_dir, query_requests_memory_ignore, read_memory_index,
    refresh_memory_index,
};
pub use recall::{
    build_model_assisted_recall_prompt, build_relevant_memory_context_with,
    format_relevant_memory_context, parse_model_assisted_recall_selection,
    recall_relevant_memories, select_relevant_memories_by_identity,
};
pub use types::*;

#[cfg(test)]
use index::{build_memory_index_from_entries, key_to_filename, memory_entrypoint_path};
#[cfg(test)]
use std::collections::HashSet;
#[cfg(test)]
use std::path::{Path, PathBuf};

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a unique temporary directory for testing.
    fn make_temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cc-memdir-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Clean up a temp directory.
    fn cleanup(dir: &Path) {
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn test_key_to_filename() {
        assert_eq!(key_to_filename("simple"), "simple.json");
        assert_eq!(key_to_filename("my-key"), "my-key.json");
        assert_eq!(key_to_filename("has spaces"), "has_spaces.json");
        assert_eq!(key_to_filename("a/b\\c"), "a_b_c.json");
    }

    #[test]
    fn test_write_and_read_memory() {
        let cwd = make_temp_dir();

        let entry =
            write_memory("test-key", "test value", "test", MemoryScope::Project, &cwd).unwrap();
        assert_eq!(entry.key, "test-key");
        assert_eq!(entry.value, "test value");
        assert_eq!(entry.category, "test");

        let read = read_memory("test-key", MemoryScope::Project, &cwd).unwrap();
        assert_eq!(read.key, "test-key");
        assert_eq!(read.value, "test value");

        cleanup(&cwd);
    }

    #[test]
    fn test_update_preserves_created_at() {
        let cwd = make_temp_dir();

        let first = write_memory("key1", "value1", "cat", MemoryScope::Project, &cwd).unwrap();
        let created = first.created_at.clone();

        // Update the same key
        let second = write_memory("key1", "value2", "cat", MemoryScope::Project, &cwd).unwrap();
        assert_eq!(second.created_at, created);
        assert_eq!(second.value, "value2");

        cleanup(&cwd);
    }

    #[test]
    fn test_delete_memory() {
        let cwd = make_temp_dir();

        write_memory("to-delete", "val", "", MemoryScope::Project, &cwd).unwrap();
        assert!(delete_memory("to-delete", MemoryScope::Project, &cwd).unwrap());
        assert!(!delete_memory("to-delete", MemoryScope::Project, &cwd).unwrap());
        assert!(read_memory("to-delete", MemoryScope::Project, &cwd).is_err());

        cleanup(&cwd);
    }

    #[test]
    fn test_list_memories() {
        let cwd = make_temp_dir();

        write_memory("alpha", "val-a", "cat1", MemoryScope::Project, &cwd).unwrap();
        write_memory("beta", "val-b", "cat2", MemoryScope::Project, &cwd).unwrap();

        let all = list_memories(MemoryScope::Project, &cwd).unwrap();
        assert_eq!(all.len(), 2);

        cleanup(&cwd);
    }

    #[test]
    fn test_list_empty_dir() {
        let cwd = make_temp_dir();
        let all = list_memories(MemoryScope::Project, &cwd).unwrap();
        assert!(all.is_empty());
        cleanup(&cwd);
    }

    #[test]
    fn test_search_memories() {
        let cwd = make_temp_dir();

        write_memory(
            "rust-setup",
            "cargo build",
            "dev",
            MemoryScope::Project,
            &cwd,
        )
        .unwrap();
        write_memory(
            "python-env",
            "virtualenv",
            "dev",
            MemoryScope::Project,
            &cwd,
        )
        .unwrap();
        write_memory(
            "meeting-notes",
            "discussed rust",
            "notes",
            MemoryScope::Project,
            &cwd,
        )
        .unwrap();

        let results = search_memories("rust", MemoryScope::Project, &cwd).unwrap();
        assert_eq!(results.len(), 2); // rust-setup + meeting-notes

        cleanup(&cwd);
    }

    #[test]
    fn test_write_memory_refreshes_memory_md_index() {
        let cwd = make_temp_dir();

        write_memory(
            "rust-setup",
            "cargo build\nsecond line should not be in the hook",
            "dev",
            MemoryScope::Project,
            &cwd,
        )
        .unwrap();
        write_memory(
            "release-notes",
            "ship notes",
            "",
            MemoryScope::Project,
            &cwd,
        )
        .unwrap();

        let index_path = memory_entrypoint_path(MemoryScope::Project, &cwd).unwrap();
        let index = std::fs::read_to_string(index_path).unwrap();

        assert!(index.contains("- [rust-setup](rust-setup.json) - dev: cargo build"));
        assert!(index.contains("- [release-notes](release-notes.json) - ship notes"));
        assert!(!index.contains("second line should not be in the hook"));

        cleanup(&cwd);
    }

    #[test]
    fn test_delete_memory_removes_empty_memory_md_index() {
        let cwd = make_temp_dir();

        write_memory("temp", "temporary", "notes", MemoryScope::Project, &cwd).unwrap();
        let index_path = memory_entrypoint_path(MemoryScope::Project, &cwd).unwrap();
        assert!(index_path.exists());

        assert!(delete_memory("temp", MemoryScope::Project, &cwd).unwrap());
        assert!(!index_path.exists());

        cleanup(&cwd);
    }

    #[test]
    fn test_memory_md_index_obeys_entrypoint_limits() {
        let entries = (0..250)
            .map(|idx| MemoryEntry {
                key: format!("memory-{idx}"),
                value: "x".repeat(300),
                category: "project".to_string(),
                memory_type: Some(MemoryType::Project),
                description: None,
                search_terms: Vec::new(),
                created_at: "2026-05-06T00:00:00Z".to_string(),
                updated_at: "2026-05-06T00:00:00Z".to_string(),
            })
            .collect::<Vec<_>>();

        let index = build_memory_index_from_entries(&entries);

        assert!(index.lines().count() <= MEMORY_ENTRYPOINT_MAX_LINES);
        assert!(index.len() <= MEMORY_ENTRYPOINT_MAX_BYTES);
        assert!(index.contains("[truncated]"));
    }

    #[test]
    fn test_build_memory_context_empty() {
        let cwd = make_temp_dir();
        let ctx = build_memory_context(&cwd).unwrap();
        assert!(ctx.is_empty());
        cleanup(&cwd);
    }

    #[test]
    fn test_build_memory_context_with_entries() {
        let cwd = make_temp_dir();

        write_memory("pref", "dark mode", "ui", MemoryScope::Project, &cwd).unwrap();

        let ctx = build_memory_context(&cwd).unwrap();
        assert!(ctx.contains("<memory-context>"));
        assert!(ctx.contains("### MEMORY.md Index"));
        assert!(ctx.contains("[pref](pref.json) - ui: dark mode"));
        assert!(ctx.contains("pref"));
        assert!(ctx.contains("dark mode"));

        cleanup(&cwd);
    }

    #[test]
    fn test_recall_relevant_memories_scores_and_limits_results() {
        let cwd = make_temp_dir();
        for idx in 0..6 {
            write_memory(
                &format!("rust-build-{idx}"),
                "Use cargo test before cargo build when touching Rust context code.",
                "project",
                MemoryScope::Project,
                &cwd,
            )
            .unwrap();
        }
        write_memory(
            "unrelated-design",
            "Figma spacing notes for dashboard screens.",
            "reference",
            MemoryScope::Project,
            &cwd,
        )
        .unwrap();

        let results =
            recall_relevant_memories(&cwd, false, "rust build context", &[], &HashSet::new(), 5)
                .unwrap();

        assert_eq!(results.len(), 5);
        assert!(results
            .iter()
            .all(|memory| memory.entry.key.starts_with("rust-build-")));

        cleanup(&cwd);
    }

    #[test]
    fn test_recall_relevant_memories_filters_already_surfaced() {
        let cwd = make_temp_dir();
        write_memory(
            "rust-build",
            "Use cargo test before cargo build.",
            "project",
            MemoryScope::Project,
            &cwd,
        )
        .unwrap();

        let already = HashSet::from(["project:rust-build".to_string()]);
        let results =
            recall_relevant_memories(&cwd, false, "rust build", &[], &already, 5).unwrap();

        assert!(results.is_empty());
        cleanup(&cwd);
    }

    #[test]
    fn test_recall_relevant_memories_denoises_recent_tool_docs_but_keeps_warnings() {
        let cwd = make_temp_dir();
        write_memory(
            "bash-reference",
            "Bash tool usage guide and examples.",
            "reference",
            MemoryScope::Project,
            &cwd,
        )
        .unwrap();
        write_memory(
            "bash-warning",
            "Bash warning: avoid destructive git commands in dirty worktrees.",
            "reference",
            MemoryScope::Project,
            &cwd,
        )
        .unwrap();

        let recent_tools = vec!["Bash".to_string()];
        let results = recall_relevant_memories(
            &cwd,
            false,
            "bash git commands",
            &recent_tools,
            &HashSet::new(),
            5,
        )
        .unwrap();

        let keys = results
            .iter()
            .map(|memory| memory.entry.key.as_str())
            .collect::<Vec<_>>();
        assert!(!keys.contains(&"bash-reference"));
        assert!(keys.contains(&"bash-warning"));

        cleanup(&cwd);
    }

    #[test]
    fn test_build_relevant_memory_context_returns_surfaced_identities() {
        let cwd = make_temp_dir();
        write_memory(
            "migration-risk",
            "Rust migration risk: keep session export round trips covered.",
            "project",
            MemoryScope::Project,
            &cwd,
        )
        .unwrap();
        write_memory(
            "unrelated",
            "Weekly roadmap note.",
            "project",
            MemoryScope::Project,
            &cwd,
        )
        .unwrap();

        let (context, surfaced) = build_relevant_memory_context_with(
            &cwd,
            false,
            "migration risk",
            &[],
            &HashSet::new(),
            5,
        )
        .unwrap();

        assert!(context.contains("## Relevant Memories"));
        assert!(context.contains("migration-risk"));
        assert!(!context.contains("unrelated"));
        assert_eq!(surfaced, vec!["project:migration-risk".to_string()]);

        cleanup(&cwd);
    }

    #[test]
    fn test_model_assisted_recall_prompt_and_selection_helpers() {
        let cwd = make_temp_dir();
        write_memory(
            "migration-risk",
            "Rust migration risk: keep session export round trips covered.",
            "project",
            MemoryScope::Project,
            &cwd,
        )
        .unwrap();
        write_memory(
            "weekly-note",
            "Weekly roadmap note.",
            "project",
            MemoryScope::Project,
            &cwd,
        )
        .unwrap();

        let candidates = recall_relevant_memories(
            &cwd,
            false,
            "migration risk",
            &[],
            &HashSet::new(),
            MODEL_ASSISTED_RECALL_CANDIDATE_LIMIT,
        )
        .unwrap();
        let prompt = build_model_assisted_recall_prompt(
            "migration risk",
            &candidates,
            MODEL_ASSISTED_RECALL_MAX_RESULTS,
        );
        assert!(prompt.contains("Return only a JSON array"));
        assert!(prompt.contains("project:migration-risk"));

        let identities = parse_model_assisted_recall_selection(
            r#"{"selected":["project:migration-risk"]}"#,
            &candidates,
            MODEL_ASSISTED_RECALL_MAX_RESULTS,
        );
        let selected = select_relevant_memories_by_identity(
            &candidates,
            &identities,
            MODEL_ASSISTED_RECALL_MAX_RESULTS,
        );

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].identity, "project:migration-risk");
        let context = format_relevant_memory_context(&selected);
        assert!(context.contains("migration-risk"));
        assert!(!context.contains("weekly-note"));

        cleanup(&cwd);
    }

    #[test]
    fn test_recall_relevant_memories_respects_ignore_memory_request() {
        let cwd = make_temp_dir();
        write_memory(
            "rust-build",
            "Use cargo test before cargo build.",
            "project",
            MemoryScope::Project,
            &cwd,
        )
        .unwrap();

        let results = recall_relevant_memories(
            &cwd,
            false,
            "ignore memory and explain rust build",
            &[],
            &HashSet::new(),
            5,
        )
        .unwrap();

        assert!(results.is_empty());
        cleanup(&cwd);
    }

    #[test]
    fn test_legacy_memory_entries_without_recall_metadata_still_load() {
        let raw = r#"{
          "key": "legacy",
          "value": "legacy value",
          "category": "project",
          "created_at": "2026-05-06T00:00:00Z",
          "updated_at": "2026-05-06T00:00:00Z"
        }"#;

        let entry: MemoryEntry = serde_json::from_str(raw).unwrap();

        assert_eq!(entry.key, "legacy");
        assert_eq!(entry.description, None);
        assert!(entry.search_terms.is_empty());
        assert_eq!(entry.effective_memory_type(), Some(MemoryType::Project));
    }

    #[test]
    fn test_memory_type_closed_taxonomy_parse() {
        assert_eq!(MemoryType::ALL.len(), 4);
        assert_eq!(MemoryType::parse("user"), Some(MemoryType::User));
        assert_eq!(MemoryType::parse("feedback"), Some(MemoryType::Feedback));
        assert_eq!(MemoryType::parse("project"), Some(MemoryType::Project));
        assert_eq!(MemoryType::parse("reference"), Some(MemoryType::Reference));
        assert_eq!(MemoryType::parse("preference"), None);
    }

    #[test]
    fn test_write_memory_records_closed_memory_type() {
        let cwd = make_temp_dir();

        let entry = write_memory(
            "testing-feedback",
            "Prefer integration tests here.\n**Why:** catches migrations.",
            "feedback",
            MemoryScope::Project,
            &cwd,
        )
        .unwrap();

        assert_eq!(entry.memory_type, Some(MemoryType::Feedback));

        let read = read_memory("testing-feedback", MemoryScope::Project, &cwd).unwrap();
        assert_eq!(read.effective_memory_type(), Some(MemoryType::Feedback));

        let raw = std::fs::read_to_string(
            memory_dir(MemoryScope::Project, &cwd)
                .unwrap()
                .join(key_to_filename("testing-feedback")),
        )
        .unwrap();
        assert!(raw.contains(r#""type": "feedback""#));

        let ctx = build_memory_context(&cwd).unwrap();
        assert!(ctx.contains("[testing-feedback](testing-feedback.json) - feedback:"));
        assert!(ctx.contains("**testing-feedback** [feedback]:"));

        cleanup(&cwd);
    }

    /// Every `MemoryScope` variant resolves to a concrete path.
    /// Uses a `ALLTHECODES_HOME` override so tests don't touch real
    /// `~/.allthecodes/`.
    #[test]
    #[serial_test::serial]
    fn test_memory_dir_resolves_all_scopes() {
        let root = make_temp_dir();
        let previous = std::env::var("ALLTHECODES_HOME").ok();
        std::env::set_var("ALLTHECODES_HOME", &root);

        let cwd = root.join("my_project");
        std::fs::create_dir_all(&cwd).unwrap();

        let global = memory_dir(MemoryScope::Global, &cwd).unwrap();
        assert_eq!(global, root.join("memory"));

        let project = memory_dir(MemoryScope::Project, &cwd).unwrap();
        assert_eq!(project, cwd.join(".allthecodes").join("memory"));

        let team = memory_dir(MemoryScope::Team, &cwd).unwrap();
        let s = team.to_string_lossy().replace('\\', "/");
        assert!(
            s.ends_with("/memory/team"),
            "unexpected team path: {}",
            team.display()
        );

        let auto = memory_dir(MemoryScope::Auto, &cwd).unwrap();
        assert_eq!(auto, root.join("auto_memory"));

        match previous {
            Some(v) => std::env::set_var("ALLTHECODES_HOME", v),
            None => std::env::remove_var("ALLTHECODES_HOME"),
        }
        cleanup(&root);
    }

    /// Auto scope round-trip write/list/delete under a sandboxed
    /// `ALLTHECODES_HOME` so the real auto_memory/ is untouched.
    #[test]
    #[serial_test::serial]
    fn test_auto_scope_roundtrip() {
        let root = make_temp_dir();
        let previous = std::env::var("ALLTHECODES_HOME").ok();
        std::env::set_var("ALLTHECODES_HOME", &root);

        let cwd = root.join("scratch");
        std::fs::create_dir_all(&cwd).unwrap();

        write_memory("auto-key", "captured note", "auto", MemoryScope::Auto, &cwd).unwrap();
        let all = list_memories(MemoryScope::Auto, &cwd).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].key, "auto-key");
        assert!(delete_memory("auto-key", MemoryScope::Auto, &cwd).unwrap());

        match previous {
            Some(v) => std::env::set_var("ALLTHECODES_HOME", v),
            None => std::env::remove_var("ALLTHECODES_HOME"),
        }
        cleanup(&root);
    }

    #[test]
    fn test_scope_as_str_labels() {
        assert_eq!(MemoryScope::Global.as_str(), "global");
        assert_eq!(MemoryScope::Project.as_str(), "project");
        assert_eq!(MemoryScope::Team.as_str(), "team");
        assert_eq!(MemoryScope::Auto.as_str(), "auto");
    }
}
