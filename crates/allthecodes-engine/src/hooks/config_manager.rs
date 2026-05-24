//! Hook configuration manager — metadata, grouping, matcher sorting.
//!
//! Port of TypeScript `hooksConfigManager.ts` and parts of `hooksSettings.ts`.

use std::collections::HashMap;

use allthecodes_types::hooks::{
    get_hook_event_metadata, HookEvent, HookEventMetadata, HookSource, IndividualHookConfig,
};

/// Type alias for the cc-types IndividualHookConfig used throughout this module.
pub type HookConfig = IndividualHookConfig;

/// Sorted priority for hook sources (lower = higher priority).
/// Mirrors the SOURCES order in TypeScript's settings/constants.ts.
fn source_priority(source: HookSource) -> u32 {
    match source {
        HookSource::LocalSettings => 0,
        HookSource::ProjectSettings => 1,
        HookSource::UserSettings => 2,
        HookSource::PolicySettings => 3,
        HookSource::SessionHook => 4,
        HookSource::PluginHook => 999,
        HookSource::BuiltinHook => 999,
    }
}

/// Sort matchers by priority (highest priority source first).
/// Same-later entries break ties alphabetically by matcher name.
pub fn sort_matchers_by_priority(
    matchers: &[String],
    hooks_by_matcher: &HashMap<String, Vec<HookConfig>>,
) -> Vec<String> {
    let mut result: Vec<String> = matchers.to_vec();
    result.sort_by(|a, b| {
        let a_hooks = hooks_by_matcher
            .get(a)
            .map(|v| v.as_slice())
            .unwrap_or_default();
        let b_hooks = hooks_by_matcher
            .get(b)
            .map(|v| v.as_slice())
            .unwrap_or_default();

        let a_priority = a_hooks
            .iter()
            .map(|h| source_priority(h.source))
            .min()
            .unwrap_or(999);
        let b_priority = b_hooks
            .iter()
            .map(|h| source_priority(h.source))
            .min()
            .unwrap_or(999);

        if a_priority != b_priority {
            a_priority.cmp(&b_priority)
        } else {
            a.cmp(b)
        }
    });
    result
}

/// Get sorted list of matcher keys for a specific event, sorted by priority.
pub fn get_sorted_matchers_for_event(
    hooks_by_event_and_matcher: &HashMap<HookEvent, HashMap<String, Vec<HookConfig>>>,
    event: &HookEvent,
) -> Vec<String> {
    let matchers = hooks_by_event_and_matcher
        .get(event)
        .map(|m| m.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();

    let for_event = hooks_by_event_and_matcher
        .get(event)
        .cloned()
        .unwrap_or_default();
    sort_matchers_by_priority(&matchers, &for_event)
}

/// Get hooks for a specific event and matcher key.
pub fn get_hooks_for_matcher(
    hooks_by_event_and_matcher: &HashMap<HookEvent, HashMap<String, Vec<HookConfig>>>,
    event: &HookEvent,
    matcher: Option<&str>,
) -> Vec<IndividualHookConfig> {
    let matcher_key = matcher.unwrap_or("");
    hooks_by_event_and_matcher
        .get(event)
        .and_then(|m| m.get(matcher_key))
        .cloned()
        .unwrap_or_default()
}

/// Group a flat list of hooks by event and matcher.
pub fn group_hooks_by_event_and_matcher(
    hooks: &[HookConfig],
) -> HashMap<HookEvent, HashMap<String, Vec<HookConfig>>> {
    let mut grouped: HashMap<HookEvent, HashMap<String, Vec<HookConfig>>> = HashMap::new();

    for hook in hooks {
        let event_group = grouped.entry(hook.event).or_default();
        let matcher_key = if hook.matcher.is_empty() {
            ""
        } else {
            &hook.matcher
        };
        event_group
            .entry(matcher_key.to_string())
            .or_default()
            .push(hook.clone());
    }

    grouped
}

/// Get the matcher metadata for an event.
pub fn get_matcher_metadata(
    event: &HookEvent,
) -> Option<allthecodes_types::hooks::MatcherMetadata> {
    get_hook_event_metadata()
        .get(event)
        .and_then(|m| m.matcher_metadata.clone())
}

/// Get metadata for an event.
pub fn get_hook_event_metadata_for(event: &HookEvent) -> Option<HookEventMetadata> {
    get_hook_event_metadata().get(event).cloned()
}

// ---------------------------------------------------------------------------
// Source display helpers
// ---------------------------------------------------------------------------

/// Display string for a hook source (long description).
pub fn hook_source_description(source: HookSource) -> &'static str {
    match source {
        HookSource::UserSettings => "User settings (~/.allthecodes/settings.json)",
        HookSource::ProjectSettings => "Project settings (.allthecodes/settings.json)",
        HookSource::LocalSettings => "Local settings (.allthecodes/settings.local.json)",
        HookSource::PolicySettings => "Policy settings (managed)",
        HookSource::PluginHook => "Plugin hooks (~/.allthecodes/plugins/*/hooks/hooks.json)",
        HookSource::SessionHook => "Session hooks (in-memory, temporary)",
        HookSource::BuiltinHook => "Built-in hooks (registered internally by allthecodes)",
    }
}

/// Short section header for a hook source.
pub fn hook_source_header(source: HookSource) -> &'static str {
    match source {
        HookSource::UserSettings => "User Settings",
        HookSource::ProjectSettings => "Project Settings",
        HookSource::LocalSettings => "Local Settings",
        HookSource::PolicySettings => "Policy Settings",
        HookSource::PluginHook => "Plugin Hooks",
        HookSource::SessionHook => "Session Hooks",
        HookSource::BuiltinHook => "Built-in Hooks",
    }
}

/// Inline label for a hook source.
pub fn hook_source_inline(source: HookSource) -> &'static str {
    match source {
        HookSource::UserSettings => "User",
        HookSource::ProjectSettings => "Project",
        HookSource::LocalSettings => "Local",
        HookSource::PolicySettings => "Policy",
        HookSource::PluginHook => "Plugin",
        HookSource::SessionHook => "Session",
        HookSource::BuiltinHook => "Built-in",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_types::hooks::{HookEntry, HookEvent, HookSource};

    fn make_config(event: HookEvent, matcher: &str, source: HookSource) -> HookConfig {
        IndividualHookConfig {
            event,
            config: HookEntry::Command {
                command: "echo test".into(),
                timeout: 10,
                shell: None,
                if_condition: None,
            },
            matcher: matcher.to_string(),
            source,
            plugin_name: None,
        }
    }

    #[test]
    fn test_sort_matchers_by_source_priority() {
        let mut map = HashMap::new();

        map.insert(
            "Bash".to_string(),
            vec![make_config(
                HookEvent::PreToolUse,
                "Bash",
                HookSource::UserSettings,
            )],
        );
        map.insert(
            "Read".to_string(),
            vec![make_config(
                HookEvent::PreToolUse,
                "Read",
                HookSource::LocalSettings,
            )],
        );

        let sorted = sort_matchers_by_priority(&["Bash".into(), "Read".into()], &map);
        // LocalSettings has higher priority (lower number) than UserSettings
        assert_eq!(sorted[0], "Read");
        assert_eq!(sorted[1], "Bash");
    }

    #[test]
    fn test_get_matcher_metadata_exists() {
        let meta = get_matcher_metadata(&HookEvent::PreToolUse);
        assert!(meta.is_some());
        assert_eq!(meta.unwrap().field_to_match, "tool_name");
    }

    #[test]
    fn test_get_matcher_metadata_none() {
        let meta = get_matcher_metadata(&HookEvent::Stop);
        assert!(meta.is_none());
    }
}
