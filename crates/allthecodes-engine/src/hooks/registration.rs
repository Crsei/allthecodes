//! Hook registration from frontmatter (agent/skill) and skill definitions.
//!
//! Port of TypeScript `registerFrontmatterHooks.ts` and `registerSkillHooks.ts`.

use tracing::debug;

use allthecodes_types::hooks::{HookEntry, HookEvent};

use super::session_hooks::SessionHookStore;

/// Register hooks from frontmatter (agent or skill) into session scope.
///
/// These hooks are active for the duration of the session/agent and are
/// cleaned up when the session/agent ends.
///
/// For agents, Stop hooks are converted to SubagentStop since subagents
/// trigger SubagentStop, not Stop.
pub fn register_frontmatter_hooks(
    store: &mut SessionHookStore,
    session_id: &str,
    hooks: &[(HookEvent, Vec<(String, Vec<HookEntry>)>)],
    source_name: &str,
    is_agent: bool,
) {
    if hooks.is_empty() {
        return;
    }

    let mut hook_count = 0;

    for (event, matchers) in hooks {
        if matchers.is_empty() {
            continue;
        }

        // For agents, convert Stop hooks to SubagentStop
        let target_event = if is_agent && *event == HookEvent::Stop {
            debug!(
                "Converting Stop hook to SubagentStop for {source_name} (subagents trigger SubagentStop)"
            );
            HookEvent::SubagentStop
        } else {
            *event
        };

        for (matcher, entries) in matchers {
            for hook in entries {
                store.add_hook(session_id, target_event, matcher, hook.clone(), None, None);
                hook_count += 1;
            }
        }
    }

    if hook_count > 0 {
        debug!("Registered {hook_count} frontmatter hook(s) from {source_name} for session {session_id}");
    }
}

/// Register hooks from a skill's frontmatter as session hooks.
///
/// If a hook has `once: true`, it should be auto-removed after first success.
/// This is currently handled by the caller by checking the on_hook_success callback.
pub fn register_skill_hooks(
    store: &mut SessionHookStore,
    session_id: &str,
    hooks: &[(HookEvent, Vec<(String, Vec<HookEntry>)>)],
    skill_name: &str,
    skill_root: Option<String>,
) {
    let mut registered_count = 0;

    for (event, matchers) in hooks {
        for (matcher, entries) in matchers {
            for hook in entries {
                store.add_hook(
                    session_id,
                    *event,
                    matcher,
                    hook.clone(),
                    None, // one-shot handling deferred to caller
                    skill_root.clone(),
                );
                registered_count += 1;
            }
        }
    }

    if registered_count > 0 {
        debug!("Registered {registered_count} hooks from skill '{skill_name}'");
    }
}

/// Parse a flat HooksSettings-style structure into the event-matcher-hooks triplet format.
///
/// Input format: HashMap<HookEvent, Vec<{ matcher: String, hooks: Vec<HookEntry> }>>
pub fn parse_hooks_settings(
    settings: &std::collections::HashMap<HookEvent, Vec<HookMatcherEntry>>,
) -> Vec<(HookEvent, Vec<(String, Vec<HookEntry>)>)> {
    settings
        .iter()
        .map(|(event, matchers)| {
            let entries: Vec<(String, Vec<HookEntry>)> = matchers
                .iter()
                .map(|m| (m.matcher.clone(), m.hooks.clone()))
                .collect();
            (*event, entries)
        })
        .collect()
}

/// A single matcher entry in hooks settings.
#[derive(Debug, Clone)]
pub struct HookMatcherEntry {
    pub matcher: String,
    pub hooks: Vec<HookEntry>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_types::hooks::HookEvent;

    fn make_hook(cmd: &str) -> HookEntry {
        HookEntry::Command {
            command: cmd.into(),
            timeout: 10,
            shell: None,
            if_condition: None,
        }
    }

    #[test]
    fn test_register_frontmatter_hooks_empty() {
        let mut store = SessionHookStore::new();
        register_frontmatter_hooks(&mut store, "session-1", &[], "test-source", false);
        assert!(store.session_ids().is_empty());
    }

    #[test]
    fn test_register_frontmatter_hooks() {
        let mut store = SessionHookStore::new();

        let hooks = vec![(
            HookEvent::Stop,
            vec![("".to_string(), vec![make_hook("echo stop")])],
        )];

        register_frontmatter_hooks(&mut store, "session-1", &hooks, "test-agent", true);

        // Stop should be converted to SubagentStop for agents
        let result = store.get_session_hooks("session-1", Some(&HookEvent::SubagentStop));
        assert!(result.contains_key(&HookEvent::SubagentStop));
    }

    #[test]
    fn test_register_skill_hooks() {
        let mut store = SessionHookStore::new();

        let hooks = vec![(
            HookEvent::PreToolUse,
            vec![("Bash".to_string(), vec![make_hook("echo pre-tool")])],
        )];

        register_skill_hooks(&mut store, "session-1", &hooks, "test-skill", None);

        let result = store.get_session_hooks("session-1", Some(&HookEvent::PreToolUse));
        assert!(result.contains_key(&HookEvent::PreToolUse));
    }
}
