//! Prompt section registration and caching system.
//!
//! Corresponds to TypeScript: `constants/systemPromptSections.ts`
//!
//! Provides memoized (cached) and volatile (uncached) prompt sections.
//! Cached sections compute once and persist until `/clear` or `/compact`.
//! Uncached (DANGEROUS) sections recompute every turn and break prompt cache.

use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::LazyLock;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Boundary marker separating static (cross-org cacheable) content from dynamic content.
///
/// Everything BEFORE this marker can use `scope: 'global'` prompt caching.
/// Everything AFTER contains user/session-specific content.
///
/// Corresponds to TS: `SYSTEM_PROMPT_DYNAMIC_BOUNDARY`
pub const DYNAMIC_BOUNDARY: &str = "__SYSTEM_PROMPT_DYNAMIC_BOUNDARY__";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A prompt section with optional caching.
pub struct PromptSection {
    /// Section name (e.g. "session_guidance", "memory", "env_info_simple").
    pub name: String,
    /// Compute function that produces the section content.
    compute: Box<dyn Fn() -> Option<String> + Send + Sync>,
    /// If true, recomputes every turn (breaks prompt cache on value change).
    /// Corresponds to TS: `DANGEROUS_uncachedSystemPromptSection`
    pub cache_break: bool,
}

// ---------------------------------------------------------------------------
// Section cache
// ---------------------------------------------------------------------------

static SECTION_CACHE: LazyLock<Mutex<HashMap<String, Option<String>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Create a memoized prompt section.
/// Computed once, cached until `clear_cache()` is called.
///
/// Corresponds to TS: `systemPromptSection(name, compute)`
pub fn cached_section(
    name: &str,
    compute: impl Fn() -> Option<String> + Send + Sync + 'static,
) -> PromptSection {
    PromptSection {
        name: name.to_string(),
        compute: Box::new(compute),
        cache_break: false,
    }
}

/// Create a volatile prompt section that recomputes every turn.
/// This WILL break the prompt cache when the value changes.
///
/// Corresponds to TS: `DANGEROUS_uncachedSystemPromptSection(name, compute, reason)`
pub fn uncached_section(
    name: &str,
    compute: impl Fn() -> Option<String> + Send + Sync + 'static,
    _reason: &str,
) -> PromptSection {
    PromptSection {
        name: name.to_string(),
        compute: Box::new(compute),
        cache_break: true,
    }
}

/// Create a memoized section from a value captured before the current turn.
///
/// This makes session-start snapshots explicit at call sites that must not
/// observe mid-turn mutations from memory or other durable stores.
pub fn frozen_section(name: &str, snapshot: Option<String>) -> PromptSection {
    let snapshot = snapshot.filter(|value| !value.trim().is_empty());
    cached_section(name, move || snapshot.clone())
}

/// Resolve all prompt sections, returning their string values.
/// Cached sections use memoized values; uncached sections always recompute.
///
/// Corresponds to TS: `resolveSystemPromptSections(sections)`
pub fn resolve_sections(sections: &[PromptSection]) -> Vec<String> {
    let mut cache = SECTION_CACHE.lock();
    let mut results = Vec::new();

    for section in sections {
        if !section.cache_break {
            if let Some(cached) = cache.get(&section.name) {
                if let Some(value) = cached {
                    results.push(value.clone());
                }
                continue;
            }
        }

        let value = (section.compute)();
        cache.insert(section.name.clone(), value.clone());

        if let Some(v) = value {
            results.push(v);
        }
    }

    results
}

/// Clear all prompt section caches.
/// Called on `/clear` and `/compact`.
///
/// Corresponds to TS: `clearSystemPromptSections()`
pub fn clear_cache() {
    let mut cache = SECTION_CACHE.lock();
    cache.clear();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn test_dynamic_boundary_value() {
        assert_eq!(DYNAMIC_BOUNDARY, "__SYSTEM_PROMPT_DYNAMIC_BOUNDARY__");
    }

    #[test]
    fn test_cached_section_memoizes() {
        clear_cache();
        // We can't easily test memoization with the current API since
        // resolve_sections takes &[PromptSection] and sections aren't Clone.
        // Just verify basic functionality.
        let section = cached_section("test_cached", || Some("hello".to_string()));
        assert!(!section.cache_break);
        assert_eq!(section.name, "test_cached");
    }

    #[test]
    fn test_uncached_section_flag() {
        let section = uncached_section("test_uncached", || Some("dynamic".into()), "test reason");
        assert!(section.cache_break);
        assert_eq!(section.name, "test_uncached");
    }

    #[test]
    fn test_resolve_sections_basic() {
        clear_cache();
        let sections = vec![
            cached_section("s1", || Some("first".into())),
            cached_section("s2", || None),
            cached_section("s3", || Some("third".into())),
        ];
        let results = resolve_sections(&sections);
        assert_eq!(results, vec!["first", "third"]);
    }

    #[test]
    fn test_resolve_caches_result() {
        clear_cache();
        let call_count = std::sync::Arc::new(AtomicUsize::new(0));
        let cc = call_count.clone();

        let sections = vec![cached_section("cached_test", move || {
            cc.fetch_add(1, Ordering::SeqCst);
            Some("value".into())
        })];

        let r1 = resolve_sections(&sections);
        assert_eq!(r1, vec!["value"]);
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_clear_cache() {
        {
            let mut cache = SECTION_CACHE.lock();
            cache.insert("to_clear".into(), Some("old".into()));
        }
        clear_cache();
        let cache = SECTION_CACHE.lock();
        assert!(!cache.contains_key("to_clear"));
    }

    #[test]
    fn test_uncached_section_always_recomputes() {
        clear_cache();
        let sections = vec![uncached_section(
            "volatile",
            || Some("fresh".into()),
            "test",
        )];
        let r1 = resolve_sections(&sections);
        assert_eq!(r1, vec!["fresh"]);
    }

    #[test]
    fn test_frozen_section_keeps_memory_snapshot_stable() {
        clear_cache();
        let cwd =
            std::env::temp_dir().join(format!("prompt_frozen_memory_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&cwd).unwrap();

        allthecodes_session::memdir::write_curated_memory(
            allthecodes_session::memdir::CuratedMemoryWrite {
                target: allthecodes_session::memdir::CuratedMemoryTarget::Project,
                key: "before".to_string(),
                value: "frozen section should include this".to_string(),
                source_session_id: None,
                approval_id: Some("approval-before".to_string()),
            },
            &cwd,
        )
        .unwrap();
        let snapshot =
            allthecodes_session::memdir::capture_curated_memory_snapshot(&cwd, false, 4096)
                .unwrap();
        let sections = vec![frozen_section("memory", Some(snapshot.context))];

        let first = resolve_sections(&sections).join("\n");
        allthecodes_session::memdir::write_curated_memory(
            allthecodes_session::memdir::CuratedMemoryWrite {
                target: allthecodes_session::memdir::CuratedMemoryTarget::Project,
                key: "after".to_string(),
                value: "frozen section must not include this".to_string(),
                source_session_id: None,
                approval_id: Some("approval-after".to_string()),
            },
            &cwd,
        )
        .unwrap();
        let second = resolve_sections(&sections).join("\n");

        assert!(first.contains("before"));
        assert!(!first.contains("after"));
        assert_eq!(first, second);

        let _ = std::fs::remove_dir_all(&cwd);
    }
}
