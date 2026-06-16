//! Permission update management.
//!
//! Corresponds to Bun's `PermissionUpdate.ts` and `PermissionUpdateSchema.ts`.
//!
//! Provides types for describing permission changes (`PermissionUpdate`) and
//! functions to apply them to a [`ToolPermissionContext`] in memory.

use allthecodes_types::permissions::{
    AdditionalWorkingDirectory, PermissionMode, ToolPermissionContext,
};

/// Where to apply a permission update.
///
/// Mirrors the editable setting sources in Bun's PermissionUpdateSchema.ts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionUpdateDestination {
    /// Global user settings (`~/.allthecodes/settings.json`).
    UserSettings,
    /// Project settings (`.allthecodes/settings.json`).
    ProjectSettings,
    /// Local per-machine settings (`.allthecodes/settings.local.json`).
    LocalSettings,
    /// In-memory session-level grant (not persisted).
    Session,
    /// CLI argument (runtime only).
    CliArg,
}

impl PermissionUpdateDestination {
    /// String key used in the `ToolPermissionRulesBySource` hashmap.
    pub fn as_source_key(&self) -> &'static str {
        match self {
            PermissionUpdateDestination::UserSettings => "user",
            PermissionUpdateDestination::ProjectSettings => "project",
            PermissionUpdateDestination::LocalSettings => "local",
            PermissionUpdateDestination::Session => "session",
            PermissionUpdateDestination::CliArg => "cli",
        }
    }

    /// Whether this destination supports persistence to disk.
    pub fn supports_persistence(&self) -> bool {
        matches!(
            self,
            PermissionUpdateDestination::UserSettings
                | PermissionUpdateDestination::ProjectSettings
                | PermissionUpdateDestination::LocalSettings
        )
    }
}

/// Desired behavior for rules in an update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleBehavior {
    Allow,
    Deny,
    Ask,
}

/// A single permission update describing a mutation to the permission context.
#[derive(Debug, Clone)]
pub enum PermissionUpdate {
    /// Add rules with the given behavior to the specified destination.
    AddRules {
        rules: Vec<String>,
        behavior: RuleBehavior,
        destination: PermissionUpdateDestination,
    },
    /// Replace ALL rules of the given behavior for the specified destination.
    ReplaceRules {
        rules: Vec<String>,
        behavior: RuleBehavior,
        destination: PermissionUpdateDestination,
    },
    /// Remove specific rules from the specified destination.
    RemoveRules {
        rules: Vec<String>,
        behavior: RuleBehavior,
        destination: PermissionUpdateDestination,
    },
    /// Switch the permission mode.
    SetMode {
        mode: PermissionMode,
        destination: PermissionUpdateDestination,
    },
    /// Add additional working directories.
    AddDirectories {
        directories: Vec<String>,
        destination: PermissionUpdateDestination,
    },
    /// Remove additional working directories.
    RemoveDirectories {
        directories: Vec<String>,
        destination: PermissionUpdateDestination,
    },
}

/// Helper: select the correct rules field based on behavior.
fn rules_field_mut(
    ctx: &mut ToolPermissionContext,
    behavior: RuleBehavior,
) -> &mut std::collections::HashMap<String, Vec<String>> {
    match behavior {
        RuleBehavior::Allow => &mut ctx.always_allow_rules,
        RuleBehavior::Deny => &mut ctx.always_deny_rules,
        RuleBehavior::Ask => &mut ctx.always_ask_rules,
    }
}

/// Apply a single permission update to the context in-place.
///
/// Returns `true` if the context was modified, `false` otherwise.
pub fn apply_permission_update(ctx: &mut ToolPermissionContext, update: &PermissionUpdate) -> bool {
    match update {
        PermissionUpdate::SetMode { mode, .. } => {
            if ctx.mode == *mode {
                return false;
            }
            ctx.mode = mode.clone();
            true
        }

        PermissionUpdate::AddRules {
            rules,
            behavior,
            destination,
        } => {
            let dest_key = destination.as_source_key().to_string();
            let field = rules_field_mut(ctx, *behavior);
            let entries = field.entry(dest_key).or_default();
            let mut changed = false;
            for rule in rules {
                if !entries.contains(rule) {
                    entries.push(rule.clone());
                    changed = true;
                }
            }
            changed
        }

        PermissionUpdate::ReplaceRules {
            rules,
            behavior,
            destination,
        } => {
            let dest_key = destination.as_source_key().to_string();
            let field = rules_field_mut(ctx, *behavior);
            let existing = field.entry(dest_key.clone()).or_default();
            if *existing == *rules {
                return false;
            }
            field.insert(dest_key, rules.clone());
            true
        }

        PermissionUpdate::RemoveRules {
            rules,
            behavior,
            destination,
        } => {
            let dest_key = destination.as_source_key();
            let field = rules_field_mut(ctx, *behavior);
            let Some(existing) = field.get_mut(dest_key) else {
                return false;
            };
            let before = existing.len();
            existing.retain(|r| !rules.contains(r));
            existing.len() != before
        }

        PermissionUpdate::AddDirectories { directories, .. } => {
            let mut changed = false;
            for dir in directories {
                if !ctx.additional_working_directories.contains_key(dir) {
                    ctx.additional_working_directories.insert(
                        dir.clone(),
                        AdditionalWorkingDirectory {
                            path: dir.clone(),
                            read_only: false,
                        },
                    );
                    changed = true;
                }
            }
            changed
        }

        PermissionUpdate::RemoveDirectories { directories, .. } => {
            let mut changed = false;
            for dir in directories {
                if ctx.additional_working_directories.remove(dir).is_some() {
                    changed = true;
                }
            }
            changed
        }
    }
}

/// Apply multiple permission updates in sequence.
///
/// Returns `true` if any update modified the context.
pub fn apply_permission_updates(
    ctx: &mut ToolPermissionContext,
    updates: &[PermissionUpdate],
) -> bool {
    let mut any_changed = false;
    for update in updates {
        any_changed |= apply_permission_update(ctx, update);
    }
    any_changed
}

/// Extract just the "add rules" updates as rule strings (for batch processing).
pub fn extract_add_rules(updates: &[PermissionUpdate]) -> Vec<(RuleBehavior, Vec<String>)> {
    let mut result = Vec::new();
    for update in updates {
        if let PermissionUpdate::AddRules {
            rules, behavior, ..
        } = update
        {
            result.push((*behavior, rules.clone()));
        }
    }
    result
}

/// Check whether any of the updates contain "add rules".
pub fn has_add_rules(updates: &[PermissionUpdate]) -> bool {
    updates
        .iter()
        .any(|u| matches!(u, PermissionUpdate::AddRules { .. }))
}

/// Generate a `Read` rule suggestion for a directory path.
///
/// Returns `None` for the root directory `/`.
pub fn create_read_rule_suggestion(
    dir_path: &str,
    destination: PermissionUpdateDestination,
) -> Option<PermissionUpdate> {
    // Root directory is too broad.
    if dir_path.trim() == "/" {
        return None;
    }

    let rule = format!("{}/**", dir_path);

    Some(PermissionUpdate::AddRules {
        rules: vec![format!("Read({})", rule)],
        behavior: RuleBehavior::Allow,
        destination,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn default_ctx() -> ToolPermissionContext {
        ToolPermissionContext {
            mode: PermissionMode::Default,
            additional_working_directories: HashMap::new(),
            always_allow_rules: HashMap::new(),
            always_deny_rules: HashMap::new(),
            always_ask_rules: HashMap::new(),
            session_allow_rules: HashMap::new(),
            auto_mode_stripped_always_allow_rules: Vec::new(),
            auto_mode_stripped_session_allow_rules: Vec::new(),
            is_bypass_permissions_mode_available: false,
            is_auto_mode_available: None,
            pre_plan_mode: None,
        }
    }

    #[test]
    fn set_mode_updates_mode() {
        let mut ctx = default_ctx();
        assert_eq!(ctx.mode, PermissionMode::Default);

        let update = PermissionUpdate::SetMode {
            mode: PermissionMode::Auto,
            destination: PermissionUpdateDestination::Session,
        };
        assert!(apply_permission_update(&mut ctx, &update));
        assert_eq!(ctx.mode, PermissionMode::Auto);
    }

    #[test]
    fn set_mode_same_mode_no_op() {
        let mut ctx = default_ctx();
        let update = PermissionUpdate::SetMode {
            mode: PermissionMode::Default,
            destination: PermissionUpdateDestination::Session,
        };
        assert!(!apply_permission_update(&mut ctx, &update));
    }

    #[test]
    fn add_rules_appends() {
        let mut ctx = default_ctx();
        let update = PermissionUpdate::AddRules {
            rules: vec!["Bash(ls:*)".to_string()],
            behavior: RuleBehavior::Allow,
            destination: PermissionUpdateDestination::UserSettings,
        };
        assert!(apply_permission_update(&mut ctx, &update));
        assert_eq!(
            ctx.always_allow_rules.get("user").map(|v| v.as_slice()),
            Some(&["Bash(ls:*)".to_string()][..])
        );
    }

    #[test]
    fn add_rules_dedup() {
        let mut ctx = default_ctx();
        let update = PermissionUpdate::AddRules {
            rules: vec!["Bash(ls:*)".to_string()],
            behavior: RuleBehavior::Allow,
            destination: PermissionUpdateDestination::UserSettings,
        };
        apply_permission_update(&mut ctx, &update);

        // Second add of same rule should be a no-op.
        assert!(!apply_permission_update(&mut ctx, &update));
        assert_eq!(ctx.always_allow_rules.get("user").map(|v| v.len()), Some(1));
    }

    #[test]
    fn remove_rules_filters() {
        let mut ctx = default_ctx();
        ctx.always_allow_rules.insert(
            "user".to_string(),
            vec!["Bash(ls:*)".to_string(), "Read".to_string()],
        );

        let update = PermissionUpdate::RemoveRules {
            rules: vec!["Bash(ls:*)".to_string()],
            behavior: RuleBehavior::Allow,
            destination: PermissionUpdateDestination::UserSettings,
        };
        assert!(apply_permission_update(&mut ctx, &update));
        assert_eq!(
            ctx.always_allow_rules.get("user").map(|v| v.as_slice()),
            Some(&["Read".to_string()][..])
        );
    }

    #[test]
    fn replace_rules_overwrites() {
        let mut ctx = default_ctx();
        ctx.always_deny_rules.insert(
            "project".to_string(),
            vec!["Bash(rm *)".to_string(), "Bash(sudo *)".to_string()],
        );

        let update = PermissionUpdate::ReplaceRules {
            rules: vec!["Bash".to_string()],
            behavior: RuleBehavior::Deny,
            destination: PermissionUpdateDestination::ProjectSettings,
        };
        assert!(apply_permission_update(&mut ctx, &update));
        assert_eq!(
            ctx.always_deny_rules.get("project").map(|v| v.as_slice()),
            Some(&["Bash".to_string()][..])
        );
    }

    #[test]
    fn add_directories_creates_new_entries() {
        let mut ctx = default_ctx();
        let update = PermissionUpdate::AddDirectories {
            directories: vec!["/tmp/work".to_string()],
            destination: PermissionUpdateDestination::Session,
        };
        assert!(apply_permission_update(&mut ctx, &update));
        assert!(ctx.additional_working_directories.contains_key("/tmp/work"));
    }

    #[test]
    fn add_directories_dedup() {
        let mut ctx = default_ctx();
        let update = PermissionUpdate::AddDirectories {
            directories: vec!["/tmp/work".to_string()],
            destination: PermissionUpdateDestination::Session,
        };
        apply_permission_update(&mut ctx, &update);
        assert!(!apply_permission_update(&mut ctx, &update));
    }

    #[test]
    fn remove_directories_deletes_entries() {
        let mut ctx = default_ctx();
        ctx.additional_working_directories.insert(
            "/tmp/work".to_string(),
            AdditionalWorkingDirectory {
                path: "/tmp/work".to_string(),
                read_only: false,
            },
        );
        ctx.additional_working_directories.insert(
            "/tmp/other".to_string(),
            AdditionalWorkingDirectory {
                path: "/tmp/other".to_string(),
                read_only: false,
            },
        );

        let update = PermissionUpdate::RemoveDirectories {
            directories: vec!["/tmp/work".to_string()],
            destination: PermissionUpdateDestination::Session,
        };
        assert!(apply_permission_update(&mut ctx, &update));
        assert!(!ctx.additional_working_directories.contains_key("/tmp/work"));
        assert!(ctx
            .additional_working_directories
            .contains_key("/tmp/other"));
    }

    #[test]
    fn batch_updates_apply_in_order() {
        let mut ctx = default_ctx();
        let updates = vec![
            PermissionUpdate::SetMode {
                mode: PermissionMode::Auto,
                destination: PermissionUpdateDestination::Session,
            },
            PermissionUpdate::AddRules {
                rules: vec!["Bash(ls:*)".to_string()],
                behavior: RuleBehavior::Allow,
                destination: PermissionUpdateDestination::Session,
            },
        ];
        assert!(apply_permission_updates(&mut ctx, &updates));
        assert_eq!(ctx.mode, PermissionMode::Auto);
        assert!(ctx
            .always_allow_rules
            .get("session")
            .unwrap()
            .contains(&"Bash(ls:*)".to_string()));
    }

    #[test]
    fn destination_source_keys() {
        assert_eq!(
            PermissionUpdateDestination::UserSettings.as_source_key(),
            "user"
        );
        assert_eq!(
            PermissionUpdateDestination::ProjectSettings.as_source_key(),
            "project"
        );
        assert_eq!(
            PermissionUpdateDestination::LocalSettings.as_source_key(),
            "local"
        );
        assert_eq!(
            PermissionUpdateDestination::Session.as_source_key(),
            "session"
        );
        assert_eq!(PermissionUpdateDestination::CliArg.as_source_key(), "cli");
    }

    #[test]
    fn supports_persistence() {
        assert!(PermissionUpdateDestination::UserSettings.supports_persistence());
        assert!(PermissionUpdateDestination::ProjectSettings.supports_persistence());
        assert!(PermissionUpdateDestination::LocalSettings.supports_persistence());
        assert!(!PermissionUpdateDestination::Session.supports_persistence());
        assert!(!PermissionUpdateDestination::CliArg.supports_persistence());
    }

    #[test]
    fn create_read_rule_suggestion_basic() {
        let result =
            create_read_rule_suggestion("/home/user/project", PermissionUpdateDestination::Session);
        assert!(result.is_some());
        if let Some(PermissionUpdate::AddRules {
            rules, behavior, ..
        }) = result
        {
            assert_eq!(behavior, RuleBehavior::Allow);
            assert_eq!(rules, vec!["Read(/home/user/project/**)"]);
        } else {
            panic!("expected AddRules variant");
        }
    }

    #[test]
    fn create_read_rule_suggestion_root_returns_none() {
        let result = create_read_rule_suggestion("/", PermissionUpdateDestination::Session);
        assert!(result.is_none());
    }

    #[test]
    fn extract_add_rules_works() {
        let updates = vec![
            PermissionUpdate::AddRules {
                rules: vec!["Bash(ls:*)".to_string()],
                behavior: RuleBehavior::Allow,
                destination: PermissionUpdateDestination::Session,
            },
            PermissionUpdate::SetMode {
                mode: PermissionMode::Auto,
                destination: PermissionUpdateDestination::Session,
            },
        ];
        let extracted = extract_add_rules(&updates);
        assert_eq!(extracted.len(), 1);
        assert_eq!(extracted[0].1, vec!["Bash(ls:*)"]);
    }
}
