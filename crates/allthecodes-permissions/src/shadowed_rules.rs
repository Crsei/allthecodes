//! Shadowed (unreachable) permission rule detection.
//!
//! An allow rule is "shadowed" when a higher-priority deny or ask rule
//! for the same tool makes it unreachable:
//!
//! - **Deny shadowing**: a tool-wide deny rule (e.g. `"Bash"`) completely
//!   blocks all invocations, making specific allow rules like `"Bash(ls:*)"`
//!   unreachable.
//! - **Ask shadowing**: a tool-wide ask rule (e.g. `"Bash"`) forces a prompt
//!   before any allow check can run, rendering specific allow rules moot.
//!
//! Both are detected by [`detect_unreachable_rules`], which also provides
//! a user-facing fix suggestion for each shadowed rule.

use allthecodes_types::permissions::ToolPermissionRulesBySource;

/// Whether an allow rule is shadowed by a deny or ask rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShadowType {
    /// Shadowed by a deny rule (completely blocked).
    Deny,
    /// Shadowed by an ask rule (will always prompt).
    Ask,
}

/// An unreachable (shadowed) permission rule with explanation.
#[derive(Debug, Clone)]
pub struct UnreachableRule {
    /// The allow rule that is unreachable (string form, e.g. `"Bash(ls:*)"`).
    pub rule: String,
    /// Source of the allow rule.
    pub rule_source: String,
    /// Human-readable explanation of why it is unreachable.
    pub reason: String,
    /// The deny or ask rule that shadows this allow rule.
    pub shadowed_by: String,
    /// Source of the shadowing rule.
    pub shadowed_by_source: String,
    /// Whether the rule is shadowed by a deny or ask rule.
    pub shadow_type: ShadowType,
    /// Suggested fix.
    pub fix: String,
}

/// Options for shadowed rule detection.
#[derive(Debug, Clone, Default)]
pub struct DetectUnreachableRulesOptions {
    /// When true, tool-wide ask rules from *personal* (non-shared) settings
    /// do NOT shadow specific `Bash` allow rules, because sandbox mode will
    /// auto-allow the command regardless of the ask rule.
    pub sandbox_auto_allow_enabled: bool,
}

/// Sources that are considered "shared" (visible to other team members).
/// Shared setting rules trigger warnings unconditionally because other
/// users may not have sandbox or the same personal overrides.
fn is_shared_setting_source(source: &str) -> bool {
    matches!(source, "managed" | "policy" | "project" | "cli" | "command")
}

/// Extract the tool-name portion from a rule string.
///
/// `"Bash"` → `"Bash"`
/// `"Bash(ls:*)"` → `"Bash"`
/// `"mcp__server__tool"` → `"mcp__server__tool"`
fn tool_name_from_rule(rule: &str) -> &str {
    if let Some(open) = rule.find('(') {
        let name = &rule[..open];
        // Handle edge case: "Bash()" is still a specific rule with empty pattern
        if name.is_empty() {
            return rule;
        }
        return name;
    }
    rule
}

/// Returns `true` when `rule` is a tool-wide rule (no specifier, e.g. `"Bash"`)
/// as opposed to a specific pattern rule (e.g. `"Bash(ls:*)"`).
fn is_tool_wide_rule(rule: &str) -> bool {
    // A rule is tool-wide if it has no parenthesised specifier.
    !rule.contains('(')
}

/// Returns `true` when `rule` has a tool-name + specifier pattern.
fn is_specific_rule(rule: &str) -> bool {
    rule.contains('(') && rule.ends_with(')')
}

/// Format a source name for display in warning messages.
fn format_source(source: &str) -> &str {
    source
}

/// Generate a suggested fix for a shadowed rule.
fn generate_fix(
    shadow_type: ShadowType,
    shadowing_rule: &str,
    shadowing_source: &str,
    shadowed_rule: &str,
    shadowed_source: &str,
) -> String {
    let tool = tool_name_from_rule(shadowing_rule);
    match shadow_type {
        ShadowType::Deny => {
            format!(
                "Remove the \"{tool}\" deny rule from {} or remove \
                 the specific allow rule \"{shadowed_rule}\" from {}",
                format_source(shadowing_source),
                format_source(shadowed_source)
            )
        }
        ShadowType::Ask => {
            format!(
                "Remove the \"{tool}\" ask rule from {} or remove \
                 the specific allow rule \"{shadowed_rule}\" from {}",
                format_source(shadowing_source),
                format_source(shadowed_source)
            )
        }
    }
}

/// Check if a specific allow rule is shadowed by a tool-wide ask rule.
///
/// An allow rule is unreachable when a tool-wide ask rule for the same tool
/// forces a prompt before the allow check runs.
///
/// Exception: For `Bash` with `sandbox_auto_allow_enabled`, personal-setting
/// ask rules do NOT shadow — sandbox auto-allows the command regardless.
#[allow(unused_variables)]
fn is_allow_rule_shadowed_by_ask_rule<'a>(
    allow_rule_source: &str,
    allow_rule: &str,
    ask_rules: impl Iterator<Item = (&'a String, &'a String)>,
    options: &DetectUnreachableRulesOptions,
) -> Option<(&'a String, &'a String)> {
    let allow_tool = tool_name_from_rule(allow_rule);

    // Only specific allow rules can be shadowed — tool-wide allow rules
    // can't be unreachable from a higher-priority rule of a different type.
    if !is_specific_rule(allow_rule) {
        return None;
    }

    for (ask_source, ask_rule) in ask_rules {
        if !is_tool_wide_rule(ask_rule) {
            continue;
        }
        if tool_name_from_rule(ask_rule) != allow_tool {
            continue;
        }

        // Sandbox exception: for Bash, personal ask rules don't shadow
        // when sandbox auto-allow is enabled.
        if allow_tool == "Bash"
            && options.sandbox_auto_allow_enabled
            && !is_shared_setting_source(ask_source)
        {
            continue;
        }

        return Some((ask_source, ask_rule));
    }

    None
}

/// Check if a specific allow rule is shadowed by a tool-wide deny rule.
///
/// A deny rule completely blocks the tool — the allow rule will never fire.
fn is_allow_rule_shadowed_by_deny_rule<'a>(
    allow_rule: &str,
    deny_rules: impl Iterator<Item = (&'a String, &'a String)>,
) -> Option<(&'a String, &'a String)> {
    let allow_tool = tool_name_from_rule(allow_rule);

    if !is_specific_rule(allow_rule) {
        return None;
    }

    for (deny_source, deny_rule) in deny_rules {
        if !is_tool_wide_rule(deny_rule) {
            continue;
        }
        if tool_name_from_rule(deny_rule) == allow_tool {
            return Some((deny_source, deny_rule));
        }
    }

    None
}

/// Detect all unreachable (shadowed) allow rules in the given context.
///
/// Iterates all allow rules and checks for:
/// - Deny shadowing: tool-wide deny rule blocks a specific allow rule
/// - Ask shadowing: tool-wide ask rule makes a specific allow rule moot
///
/// Returns a list of [`UnreachableRule`] with explanations and fix suggestions.
///
/// # Example
///
/// ```ignore
/// let options = DetectUnreachableRulesOptions::default();
/// let result = detect_unreachable_rules(
///     &ctx.always_allow_rules,
///     &ctx.always_ask_rules,
///     &ctx.always_deny_rules,
///     &options,
/// );
/// for r in &result {
///     eprintln!("⚠️  {} — {}", r.rule, r.reason);
/// }
/// ```
pub fn detect_unreachable_rules(
    allow_rules: &ToolPermissionRulesBySource,
    ask_rules: &ToolPermissionRulesBySource,
    deny_rules: &ToolPermissionRulesBySource,
    options: &DetectUnreachableRulesOptions,
) -> Vec<UnreachableRule> {
    let mut unreachable: Vec<UnreachableRule> = Vec::new();

    // Flatten ask/deny rules into (source, rule) iterator for efficient lookup.
    let ask_flat: Vec<(&String, &String)> = ask_rules
        .iter()
        .flat_map(|(source, rules)| rules.iter().map(move |r| (source, r)))
        .collect();
    let deny_flat: Vec<(&String, &String)> = deny_rules
        .iter()
        .flat_map(|(source, rules)| rules.iter().map(move |r| (source, r)))
        .collect();

    for (allow_source, allow_rule) in allow_rules
        .iter()
        .flat_map(|(source, rules)| rules.iter().map(move |r| (source, r)))
    {
        // Check deny shadowing first — more severe
        if let Some((deny_source, deny_rule)) =
            is_allow_rule_shadowed_by_deny_rule(allow_rule, deny_flat.iter().copied())
        {
            unreachable.push(UnreachableRule {
                rule: allow_rule.clone(),
                rule_source: allow_source.clone(),
                reason: format!(
                    "Blocked by \"{}\" deny rule from {}",
                    tool_name_from_rule(deny_rule),
                    format_source(deny_source)
                ),
                shadowed_by: (*deny_rule).clone(),
                shadowed_by_source: deny_source.clone(),
                shadow_type: ShadowType::Deny,
                fix: generate_fix(
                    ShadowType::Deny,
                    deny_rule,
                    deny_source,
                    allow_rule,
                    allow_source,
                ),
            });
            continue;
        }

        // Check ask shadowing
        if let Some((ask_source, ask_rule)) = is_allow_rule_shadowed_by_ask_rule(
            allow_source,
            allow_rule,
            ask_flat.iter().copied(),
            options,
        ) {
            unreachable.push(UnreachableRule {
                rule: allow_rule.clone(),
                rule_source: allow_source.clone(),
                reason: format!(
                    "Shadowed by \"{}\" ask rule from {} — user will always be prompted",
                    tool_name_from_rule(ask_rule),
                    format_source(ask_source)
                ),
                shadowed_by: (*ask_rule).clone(),
                shadowed_by_source: ask_source.clone(),
                shadow_type: ShadowType::Ask,
                fix: generate_fix(
                    ShadowType::Ask,
                    ask_rule,
                    ask_source,
                    allow_rule,
                    allow_source,
                ),
            });
        }
    }

    unreachable
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Helper: build a ToolPermissionRulesBySource from rules.
    fn source(source: &str, rules: Vec<&str>) -> ToolPermissionRulesBySource {
        let mut map = HashMap::new();
        map.insert(
            source.to_string(),
            rules.into_iter().map(String::from).collect(),
        );
        map
    }

    // -- Deny shadowing tests --

    #[test]
    fn deny_shadows_specific_allow_rule() {
        let allow = source("user", vec!["Bash(ls:*)"]);
        let deny = source("policy", vec!["Bash"]);
        let ask = HashMap::new();

        let result = detect_unreachable_rules(
            &allow,
            &ask,
            &deny,
            &DetectUnreachableRulesOptions::default(),
        );

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].shadow_type, ShadowType::Deny);
        assert_eq!(result[0].rule, "Bash(ls:*)");
    }

    #[test]
    fn tool_wide_allow_not_shadowed_by_deny() {
        let allow = source("user", vec!["Bash"]);
        let deny = source("policy", vec!["Bash"]);

        let result = detect_unreachable_rules(
            &allow,
            &HashMap::new(),
            &deny,
            &DetectUnreachableRulesOptions::default(),
        );

        // Tool-wide allow vs tool-wide deny is a conflict, but not "shadowed"
        // in the specific-allow-by-tool-wide-deny sense.
        assert!(result.is_empty());
    }

    #[test]
    fn deny_different_tool_no_shadow() {
        let allow = source("user", vec!["Bash(ls:*)"]);
        let deny = source("policy", vec!["Read"]);

        let result = detect_unreachable_rules(
            &allow,
            &HashMap::new(),
            &deny,
            &DetectUnreachableRulesOptions::default(),
        );

        assert!(result.is_empty());
    }

    // -- Ask shadowing tests --

    #[test]
    fn ask_shadows_specific_allow_rule() {
        let allow = source("user", vec!["Bash(ls:*)"]);
        let ask = source("project", vec!["Bash"]);

        let result = detect_unreachable_rules(
            &allow,
            &ask,
            &HashMap::new(),
            &DetectUnreachableRulesOptions::default(),
        );

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].shadow_type, ShadowType::Ask);
        assert_eq!(result[0].rule, "Bash(ls:*)");
    }

    #[test]
    fn ask_different_tool_no_shadow() {
        let allow = source("user", vec!["Read(/tmp/*)"]);
        let ask = source("project", vec!["Bash"]);

        let result = detect_unreachable_rules(
            &allow,
            &ask,
            &HashMap::new(),
            &DetectUnreachableRulesOptions::default(),
        );

        assert!(result.is_empty());
    }

    // -- Sandbox auto-allow exception --

    #[test]
    fn sandbox_exception_personal_ask_does_not_shadow_bash() {
        let allow = source("user", vec!["Bash(ls:*)"]);
        let ask = source("user", vec!["Bash"]);

        let result = detect_unreachable_rules(
            &allow,
            &ask,
            &HashMap::new(),
            &DetectUnreachableRulesOptions {
                sandbox_auto_allow_enabled: true,
            },
        );

        // Personal "user" source ask rule should NOT shadow with sandbox enabled
        assert!(result.is_empty());
    }

    #[test]
    fn sandbox_exception_shared_ask_still_shadows() {
        let allow = source("user", vec!["Bash(ls:*)"]);
        let ask = source("project", vec!["Bash"]);

        let result = detect_unreachable_rules(
            &allow,
            &ask,
            &HashMap::new(),
            &DetectUnreachableRulesOptions {
                sandbox_auto_allow_enabled: true,
            },
        );

        // "project" is a shared source — still shadows
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].shadow_type, ShadowType::Ask);
    }

    #[test]
    fn sandbox_exception_non_bash_tool_not_exempted() {
        let allow = source("user", vec!["Read(/tmp/*)"]);
        let ask = source("user", vec!["Read"]);

        let result = detect_unreachable_rules(
            &allow,
            &ask,
            &HashMap::new(),
            &DetectUnreachableRulesOptions {
                sandbox_auto_allow_enabled: true,
            },
        );

        // Read tool — sandbox exception does not apply
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].shadow_type, ShadowType::Ask);
    }

    // -- Mixed / edge cases --

    #[test]
    fn deny_shadowing_reported_before_ask() {
        let allow = source("user", vec!["Bash(ls:*)"]);
        let deny = source("policy", vec!["Bash"]);
        let ask = source("project", vec!["Bash"]);

        let result = detect_unreachable_rules(
            &allow,
            &ask,
            &deny,
            &DetectUnreachableRulesOptions::default(),
        );

        // Only deny shadow reported (deny is more severe, so we don't also
        // report ask shadow for the same rule)
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].shadow_type, ShadowType::Deny);
    }

    #[test]
    fn multiple_allow_rules_all_shadowed() {
        let mut allow = HashMap::new();
        allow.insert(
            "user".to_string(),
            vec!["Bash(ls:*)".to_string(), "Bash(git *)".to_string()],
        );
        let deny = source("policy", vec!["Bash"]);

        let result = detect_unreachable_rules(
            &allow,
            &HashMap::new(),
            &deny,
            &DetectUnreachableRulesOptions::default(),
        );

        assert_eq!(result.len(), 2);
        assert!(result.iter().all(|r| r.shadow_type == ShadowType::Deny));
    }

    #[test]
    fn specific_deny_does_not_shadow_specific_allow() {
        let allow = source("user", vec!["Bash(ls:*)"]);
        let deny = source("policy", vec!["Bash(rm *)"]);

        let result = detect_unreachable_rules(
            &allow,
            &HashMap::new(),
            &deny,
            &DetectUnreachableRulesOptions::default(),
        );

        // Specific deny only blocks matching pattern — does not shadow
        // a different specific allow rule.
        assert!(result.is_empty());
    }

    #[test]
    fn fix_suggestion_contains_removal_options() {
        let allow = source("user", vec!["Bash(ls:*)"]);
        let deny = source("project", vec!["Bash"]);

        let result = detect_unreachable_rules(
            &allow,
            &HashMap::new(),
            &deny,
            &DetectUnreachableRulesOptions::default(),
        );

        assert_eq!(result.len(), 1);
        assert!(result[0].fix.contains("project"));
        assert!(result[0].fix.contains("Bash"));
        assert!(result[0].fix.contains("user"));
    }

    // -- Shared source detection --

    #[test]
    fn managed_source_is_shared() {
        assert!(is_shared_setting_source("managed"));
    }

    #[test]
    fn user_source_is_not_shared() {
        assert!(!is_shared_setting_source("user"));
    }

    #[test]
    fn local_source_is_not_shared() {
        assert!(!is_shared_setting_source("local"));
    }

    // -- Tool name extraction --

    #[test]
    fn extracts_tool_name_from_plain_rule() {
        assert_eq!(tool_name_from_rule("Bash"), "Bash");
    }

    #[test]
    fn extracts_tool_name_from_specifier() {
        assert_eq!(tool_name_from_rule("Bash(ls:*)"), "Bash");
    }

    #[test]
    fn extracts_tool_name_from_mcp_prefix() {
        assert_eq!(
            tool_name_from_rule("mcp__server__tool"),
            "mcp__server__tool"
        );
    }
}
