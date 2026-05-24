//! Settings permission validation layer.
//!
//! Validates permission rules for consistency, detects shadowed rules
//! (e.g. a user-level allow-rule that is overridden by a managed deny-rule),
//! and validates auto mode classifier settings.

use crate::settings::{AutoModeSettings, PermissionsSettings, SettingsSource};

use crate::validation::WarningSeverity;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A permission rule that is shadowed (overridden) by a higher-priority or
/// managed-policy rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShadowedRule {
    /// The rule pattern that is being shadowed (e.g. "Bash(rm)").
    pub rule: String,
    /// Description of what shadowed it (e.g. "managed deny-rule").
    pub shadowed_by: String,
    /// Which source originally provided this rule.
    pub source: SettingsSource,
}

/// A validation warning specific to permission settings.
#[derive(Debug, Clone)]
pub struct PermissionValidationWarning {
    /// How serious the issue is.
    pub severity: WarningSeverity,
    /// The permission field that triggered the warning (e.g. "permissions.allow").
    pub field: String,
    /// Human-readable description of the issue.
    pub message: String,
    /// Optional source context (e.g. "User" or "Managed").
    pub source_info: Option<String>,
}

// ---------------------------------------------------------------------------
// Validation helpers
// ---------------------------------------------------------------------------

/// Validate permission settings for consistency across sources.
///
/// Checks for:
/// - Overly broad allow rules that could be security risks.
/// - Conflicting allow/deny rules within the same source.
/// - Invalid permission modes.
/// - Rules with known dangerous patterns.
pub fn validate_permission_settings(
    settings: &PermissionsSettings,
    _sources: &crate::settings::SourceMap,
) -> Vec<PermissionValidationWarning> {
    let mut warnings = Vec::new();

    // Check for dangerous patterns in allow rules.
    for rule in &settings.allow {
        if let Some(w) = check_dangerous_allow(rule) {
            warnings.push(w);
        }
    }

    // Check for conflicting rules (same rule in both allow and deny).
    for allow_rule in &settings.allow {
        if settings.deny.contains(allow_rule) {
            warnings.push(PermissionValidationWarning {
                severity: WarningSeverity::Warning,
                field: "permissions.allow".to_string(),
                message: format!(
                    "Rule '{}' appears in both allow and deny lists. Deny takes precedence.",
                    allow_rule
                ),
                source_info: None,
            });
        }
    }

    // Check auto mode settings if present.
    if let Some(ref auto) = settings.auto_mode {
        warnings.extend(check_auto_mode_settings(auto));
    }

    // Check for empty allow/deny lists when mode is restrictive.
    if settings.default_mode.as_deref() == Some("ask") && settings.allow.is_empty() {
        warnings.push(PermissionValidationWarning {
            severity: WarningSeverity::Info,
            field: "permissions.defaultMode".to_string(),
            message: "Permission mode is 'ask' with no allow rules. All tools will prompt.".into(),
            source_info: None,
        });
    }

    warnings
}

/// Check a single allow rule for dangerous patterns.
fn check_dangerous_allow(rule: &str) -> Option<PermissionValidationWarning> {
    let dangerous_patterns = [
        ("Bash(rm -rf", "Dangerous recursive deletion pattern"),
        ("Bash(dd if=", "Dangerous disk write operation"),
        ("Bash(> /dev/", "Dangerous device write"),
        ("Bash(*rm *)", "Very broad delete pattern"),
        ("Bash(*dd *)", "Very broad disk write pattern"),
        ("Bash(chmod 777", "Overly permissive file mode"),
        ("Bash(chown", "Ownership change operation"),
    ];

    for (pattern, msg) in &dangerous_patterns {
        if rule.contains(pattern) {
            return Some(PermissionValidationWarning {
                severity: WarningSeverity::Warning,
                field: "permissions.allow".to_string(),
                message: format!(
                    "Allow rule '{}' contains dangerous pattern '{}': {}.",
                    rule, pattern, msg
                ),
                source_info: None,
            });
        }
    }
    None
}

/// Validate auto mode classifier rules.
pub fn check_auto_mode_settings(settings: &AutoModeSettings) -> Vec<PermissionValidationWarning> {
    let mut warnings = Vec::new();

    if settings.allow.is_empty() && settings.soft_deny.is_empty() && settings.environment.is_empty()
    {
        warnings.push(PermissionValidationWarning {
            severity: WarningSeverity::Info,
            field: "permissions.autoMode".to_string(),
            message: "Auto mode is enabled with no rules. The classifier will use defaults.".into(),
            source_info: None,
        });
    }

    // Warn about vague prose rules that may not be effective.
    for rule in &settings.allow {
        if rule.len() < 10 {
            warnings.push(PermissionValidationWarning {
                severity: WarningSeverity::Info,
                field: "permissions.autoMode.allow".to_string(),
                message: format!(
                    "Auto mode allow rule '{}' is very short and may be too vague.",
                    rule
                ),
                source_info: None,
            });
        }
    }

    warnings
}

/// Find permission rules that are shadowed by managed policies.
///
/// Managed policy fields (especially `permissions.deny`, `permissions.allow`,
/// `permissions.ask`) always win over user/project/local rules. This function
/// identifies rules from lower-priority sources that would be overridden.
///
/// Returns a list of `ShadowedRule` entries, one per shadowed rule.
pub fn find_shadowed_rules(
    permissions: &PermissionsSettings,
    managed_permissions: Option<&PermissionsSettings>,
) -> Vec<ShadowedRule> {
    let mut shadowed = Vec::new();

    let Some(mp) = managed_permissions else {
        return shadowed;
    };

    // User/project/local allow rules shadowed by managed deny rules.
    for user_rule in &permissions.allow {
        if mp.deny.contains(user_rule) {
            // Check if this rule also came from a managed source.
            shadowed.push(ShadowedRule {
                rule: user_rule.clone(),
                shadowed_by: format!("managed deny rule"),
                source: SettingsSource::User,
            });
        }
    }

    // User/project/local deny rules shadowed by managed allow rules.
    for user_rule in &permissions.deny {
        if mp.allow.contains(user_rule) {
            shadowed.push(ShadowedRule {
                rule: user_rule.clone(),
                shadowed_by: format!("managed allow rule"),
                source: SettingsSource::User,
            });
        }
    }

    shadowed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{AutoModeSettings, PermissionsSettings, SourceMap};

    #[test]
    fn test_validate_permission_settings_empty() {
        let settings = PermissionsSettings::default();
        let warnings = validate_permission_settings(&settings, &SourceMap::new());
        assert!(
            warnings.is_empty(),
            "empty permissions should have no warnings"
        );
    }

    #[test]
    fn test_validate_permission_settings_conflicting_rules() {
        let settings = PermissionsSettings {
            allow: vec!["Bash".to_string()],
            deny: vec!["Bash".to_string()],
            ..Default::default()
        };
        let warnings = validate_permission_settings(&settings, &SourceMap::new());
        assert!(warnings
            .iter()
            .any(|w| w.field == "permissions.allow" && w.message.contains("appears in both")));
    }

    #[test]
    fn test_validate_permission_settings_dangerous_allow() {
        let settings = PermissionsSettings {
            allow: vec!["Bash(rm -rf /)".to_string()],
            ..Default::default()
        };
        let warnings = validate_permission_settings(&settings, &SourceMap::new());
        assert!(warnings.iter().any(|w| w.message.contains("dangerous")));
    }

    #[test]
    fn test_check_auto_mode_settings_empty() {
        let auto = AutoModeSettings::default();
        let warnings = check_auto_mode_settings(&auto);
        assert!(!warnings.is_empty());
        assert!(warnings[0].message.contains("no rules"));
    }

    #[test]
    fn test_check_auto_mode_settings_vague() {
        let auto = AutoModeSettings {
            allow: vec!["OK".to_string()],
            ..Default::default()
        };
        let warnings = check_auto_mode_settings(&auto);
        assert!(warnings.iter().any(|w| w.message.contains("very short")));
    }

    #[test]
    fn test_find_shadowed_rules_no_managed() {
        let perms = PermissionsSettings::default();
        let shadowed = find_shadowed_rules(&perms, None);
        assert!(shadowed.is_empty());
    }

    #[test]
    fn test_find_shadowed_rules_allow_deny_conflict() {
        let user_perms = PermissionsSettings {
            allow: vec!["Bash".to_string()],
            ..Default::default()
        };
        let managed_perms = PermissionsSettings {
            deny: vec!["Bash".to_string()],
            ..Default::default()
        };

        let shadowed = find_shadowed_rules(&user_perms, Some(&managed_perms));
        assert_eq!(shadowed.len(), 1);
        assert_eq!(shadowed[0].rule, "Bash");
        assert!(shadowed[0].shadowed_by.contains("deny"));
    }

    #[test]
    fn test_find_shadowed_rules_deny_allow_conflict() {
        let user_perms = PermissionsSettings {
            deny: vec!["Read".to_string()],
            ..Default::default()
        };
        let managed_perms = PermissionsSettings {
            allow: vec!["Read".to_string()],
            ..Default::default()
        };

        let shadowed = find_shadowed_rules(&user_perms, Some(&managed_perms));
        assert_eq!(shadowed.len(), 1);
        assert_eq!(shadowed[0].rule, "Read");
        assert!(shadowed[0].shadowed_by.contains("allow"));
    }

    #[test]
    fn test_shadowed_rule_struct() {
        let rule = ShadowedRule {
            rule: "Bash".to_string(),
            shadowed_by: "managed deny".to_string(),
            source: SettingsSource::User,
        };
        assert_eq!(rule.rule, "Bash");
        assert_eq!(rule.source, SettingsSource::User);
    }

    #[test]
    fn test_validate_with_ask_mode_no_rules() {
        let settings = PermissionsSettings {
            default_mode: Some("ask".to_string()),
            ..Default::default()
        };
        let warnings = validate_permission_settings(&settings, &SourceMap::new());
        assert!(warnings
            .iter()
            .any(|w| w.field == "permissions.defaultMode" && w.severity == WarningSeverity::Info));
    }

    #[test]
    fn test_no_warnings_for_valid_permissions() {
        let settings = PermissionsSettings {
            default_mode: Some("auto".to_string()),
            allow: vec!["Read".to_string(), "Grep".to_string()],
            deny: vec!["Bash(rm)".to_string()],
            ask: vec!["Bash".to_string()],
            auto_mode: Some(AutoModeSettings {
                environment: vec!["Trusted repo".to_string()],
                allow: vec!["Run cargo test".to_string()],
                soft_deny: vec!["Network operations".to_string()],
                ..Default::default()
            }),
            ..Default::default()
        };
        let warnings = validate_permission_settings(&settings, &SourceMap::new());
        let danger_warnings: Vec<_> = warnings
            .iter()
            .filter(|w| w.severity == WarningSeverity::Warning)
            .collect();
        assert!(
            danger_warnings.is_empty(),
            "expected no warnings for reasonable permissions, got: {:?}",
            danger_warnings
        );
    }
}
