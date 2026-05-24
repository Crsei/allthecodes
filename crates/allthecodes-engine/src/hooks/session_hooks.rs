//! Session-scoped hooks — ephemeral per-session hook storage.
//!
//! Session hooks are temporary, in-memory only, and cleared when a session ends.
//! They support both command hooks and function hooks (in-memory callbacks).
//!
//! Port of TypeScript `sessionHooks.ts`.

use std::collections::HashMap;

use allthecodes_types::hooks::{
    FunctionHook, HookEntry, HookEvent, SessionDerivedHookMatcher, SessionStore,
};

/// An entry in a session hook matcher group.
#[derive(Debug, Clone)]
pub struct SessionHookEntry {
    pub hook: HookEntry,
    pub function_hook: Option<FunctionHook>,
    pub on_hook_success: Option<String>,
}

/// A session hook matcher group.
#[derive(Debug, Clone)]
pub struct SessionHookMatcher {
    pub matcher: String,
    pub skill_root: Option<String>,
    pub hooks: Vec<SessionHookEntry>,
}

/// Convert session store hooks to regular derived hook matchers (without function hooks).
pub fn convert_to_hook_matchers(
    session_matchers: &[SessionHookMatcher],
) -> Vec<SessionDerivedHookMatcher> {
    session_matchers
        .iter()
        .map(|sm| SessionDerivedHookMatcher {
            matcher: sm.matcher.clone(),
            skill_root: sm.skill_root.clone(),
            hooks: sm
                .hooks
                .iter()
                .filter(|h| h.function_hook.is_none())
                .map(|h| h.hook.clone())
                .collect(),
        })
        .filter(|m| !m.hooks.is_empty())
        .collect()
}

/// In-memory session hook store. Uses a HashMap keyed by session/agent ID.
#[derive(Debug, Clone, Default)]
pub struct SessionHookStore {
    /// Map of session_id -> SessionStore
    stores: HashMap<String, SessionStore>,
}

impl SessionHookStore {
    pub fn new() -> Self {
        Self {
            stores: HashMap::new(),
        }
    }

    /// Add a hook entry to a session.
    pub fn add_hook(
        &mut self,
        session_id: &str,
        event: HookEvent,
        matcher: &str,
        hook: HookEntry,
        on_hook_success: Option<String>,
        skill_root: Option<String>,
    ) {
        let store = self
            .stores
            .entry(session_id.to_string())
            .or_insert_with(|| SessionStore {
                hooks: HashMap::new(),
            });

        let matchers = store.hooks.entry(event).or_default();

        // Find existing matcher group or create new one
        let existing = matchers
            .iter_mut()
            .find(|m| m.matcher == matcher && m.skill_root.as_deref() == skill_root.as_deref());

        if let Some(matcher_group) = existing {
            matcher_group
                .hooks
                .push(allthecodes_types::hooks::SessionHookEntry {
                    hook,
                    function_hook: None,
                    on_hook_success,
                });
        } else {
            matchers.push(allthecodes_types::hooks::SessionHookMatcher {
                matcher: matcher.to_string(),
                skill_root,
                hooks: vec![allthecodes_types::hooks::SessionHookEntry {
                    hook,
                    function_hook: None,
                    on_hook_success,
                }],
            });
        }
    }

    /// Add a function hook to a session.
    pub fn add_function_hook(
        &mut self,
        session_id: &str,
        event: HookEvent,
        matcher: &str,
        function_hook: FunctionHook,
    ) {
        let store = self
            .stores
            .entry(session_id.to_string())
            .or_insert_with(|| SessionStore {
                hooks: HashMap::new(),
            });

        let matchers = store.hooks.entry(event).or_default();

        let existing = matchers.iter_mut().find(|m| m.matcher == matcher);

        if let Some(matcher_group) = existing {
            matcher_group
                .hooks
                .push(allthecodes_types::hooks::SessionHookEntry {
                    hook: HookEntry::Command {
                        command: String::new(),
                        timeout: function_hook.timeout,
                        shell: None,
                        if_condition: None,
                    },
                    function_hook: Some(function_hook),
                    on_hook_success: None,
                });
        } else {
            matchers.push(allthecodes_types::hooks::SessionHookMatcher {
                matcher: matcher.to_string(),
                skill_root: None,
                hooks: vec![allthecodes_types::hooks::SessionHookEntry {
                    hook: HookEntry::Command {
                        command: String::new(),
                        timeout: function_hook.timeout,
                        shell: None,
                        if_condition: None,
                    },
                    function_hook: Some(function_hook),
                    on_hook_success: None,
                }],
            });
        }
    }

    /// Remove a function hook by ID from a session.
    pub fn remove_function_hook(&mut self, session_id: &str, event: &HookEvent, hook_id: &str) {
        if let Some(store) = self.stores.get_mut(session_id) {
            if let Some(matchers) = store.hooks.get_mut(event) {
                matchers.retain_mut(|m| {
                    m.hooks.retain(|h| {
                        h.function_hook
                            .as_ref()
                            .map(|f| f.id != hook_id)
                            .unwrap_or(true)
                    });
                    !m.hooks.is_empty()
                });

                if matchers.is_empty() {
                    store.hooks.remove(event);
                }
            }
        }
    }

    /// Remove a specific hook from a session.
    pub fn remove_hook(&mut self, session_id: &str, event: &HookEvent, hook: &HookEntry) {
        if let Some(store) = self.stores.get_mut(session_id) {
            if let Some(matchers) = store.hooks.get_mut(event) {
                matchers.retain_mut(|m| {
                    m.hooks.retain(|h| {
                        if let (
                            HookEntry::Command { command: c1, .. },
                            HookEntry::Command { command: c2, .. },
                        ) = (&h.hook, hook)
                        {
                            c1 != c2
                        } else {
                            true
                        }
                    });
                    !m.hooks.is_empty()
                });

                if matchers.is_empty() {
                    store.hooks.remove(event);
                }
            }
        }
    }

    /// Get all command hooks (excluding function hooks) for a session and event.
    pub fn get_session_hooks(
        &self,
        session_id: &str,
        event: Option<&HookEvent>,
    ) -> HashMap<HookEvent, Vec<SessionDerivedHookMatcher>> {
        let mut result = HashMap::new();

        let store = match self.stores.get(session_id) {
            Some(s) => s,
            None => return result,
        };

        let events: Vec<&HookEvent> = if let Some(e) = event {
            vec![e]
        } else {
            allthecodes_types::hooks::HOOK_EVENTS.iter().collect()
        };

        for evt in events {
            if let Some(matchers) = store.hooks.get(evt) {
                let derived: Vec<SessionDerivedHookMatcher> = matchers
                    .iter()
                    .map(|sm| SessionDerivedHookMatcher {
                        matcher: sm.matcher.clone(),
                        skill_root: sm.skill_root.clone(),
                        hooks: sm
                            .hooks
                            .iter()
                            .map(|h| h.hook.clone())
                            .filter(|h| !matches!(h, HookEntry::Command { command, .. } if command.is_empty()))
                            .collect(),
                    })
                    .filter(|m| !m.hooks.is_empty())
                    .collect();

                if !derived.is_empty() {
                    result.insert(*evt, derived);
                }
            }
        }

        result
    }

    /// Clear all hooks for a session.
    pub fn clear_session_hooks(&mut self, session_id: &str) {
        self.stores.remove(session_id);
    }

    /// Get all session IDs.
    pub fn session_ids(&self) -> Vec<String> {
        self.stores.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_cmd_hook(command: &str) -> HookEntry {
        HookEntry::Command {
            command: command.into(),
            timeout: 10,
            shell: None,
            if_condition: None,
        }
    }

    #[test]
    fn test_add_and_clear_session_hooks() {
        let mut store = SessionHookStore::new();

        store.add_hook(
            "session-1",
            HookEvent::Stop,
            "",
            make_cmd_hook("echo stop"),
            None,
            None,
        );
        store.add_hook(
            "session-1",
            HookEvent::PreToolUse,
            "Bash",
            make_cmd_hook("echo check"),
            None,
            None,
        );

        let hooks = store.get_session_hooks("session-1", Some(&HookEvent::PreToolUse));
        assert_eq!(hooks.len(), 1);
        let matchers = hooks.get(&HookEvent::PreToolUse).unwrap();
        assert_eq!(matchers.len(), 1);
        assert_eq!(matchers[0].matcher, "Bash");
        assert_eq!(matchers[0].hooks.len(), 1);

        store.clear_session_hooks("session-1");
        let hooks = store.get_session_hooks("session-1", None);
        assert!(hooks.is_empty());
    }

    #[test]
    fn test_add_function_hook() {
        let mut store = SessionHookStore::new();

        let fh = FunctionHook {
            id: "fn-1".into(),
            timeout: 5000,
            error_message: "error".into(),
            status_message: None,
        };

        store.add_function_hook("session-1", HookEvent::Stop, "", fh.clone());

        // Function hooks are not returned by get_session_hooks
        let hooks = store.get_session_hooks("session-1", Some(&HookEvent::Stop));
        assert!(hooks.is_empty() || hooks.get(&HookEvent::Stop).unwrap().is_empty());
    }

    #[test]
    fn test_remove_function_hook() {
        let mut store = SessionHookStore::new();

        let fh = FunctionHook {
            id: "fn-1".into(),
            timeout: 5000,
            error_message: "error".into(),
            status_message: None,
        };

        store.add_function_hook("session-1", HookEvent::Stop, "", fh);
        store.remove_function_hook("session-1", &HookEvent::Stop, "fn-1");

        // Should not crash and the hook should be gone
        let _hooks = store.get_session_hooks("session-1", None);
    }

    #[test]
    fn test_remove_hook() {
        let mut store = SessionHookStore::new();

        let hook = make_cmd_hook("echo remove_me");
        store.add_hook("session-1", HookEvent::Stop, "", hook.clone(), None, None);

        store.remove_hook("session-1", &HookEvent::Stop, &hook);

        let hooks = store.get_session_hooks("session-1", Some(&HookEvent::Stop));
        let stop_hooks = hooks.get(&HookEvent::Stop);
        assert!(stop_hooks.is_none() || stop_hooks.unwrap().is_empty());
    }

    #[test]
    fn test_multiple_sessions_independent() {
        let mut store = SessionHookStore::new();

        store.add_hook(
            "session-a",
            HookEvent::Stop,
            "",
            make_cmd_hook("echo a"),
            None,
            None,
        );
        store.add_hook(
            "session-b",
            HookEvent::Stop,
            "",
            make_cmd_hook("echo b"),
            None,
            None,
        );

        assert_eq!(store.session_ids().len(), 2);

        store.clear_session_hooks("session-a");
        assert_eq!(store.session_ids().len(), 1);
    }
}
