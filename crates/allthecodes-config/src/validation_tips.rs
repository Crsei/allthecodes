//! User-facing diagnostic tips for common configuration problems.
//!
//! Generates actionable, human-readable tips from validation warnings and
//! permission validation diagnostics. Each tip carries a severity level,
//! a unique code for filtering, and an optional suggested fix.

use crate::permission_validation::PermissionValidationWarning;
use crate::validation::WarningSeverity;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A user-facing diagnostic tip.
#[derive(Debug, Clone)]
pub struct ValidationTip {
    /// Short machine-readable code (e.g. "missing-model", "shadowed-rule").
    pub code: String,
    /// Human-readable message describing the issue.
    pub message: String,
    /// How serious the issue is.
    pub severity: WarningSeverity,
    /// Optional suggested fix (e.g. a CLI command or config change).
    pub fix: Option<String>,
}

// ---------------------------------------------------------------------------
// Tip generation
// ---------------------------------------------------------------------------

/// Generate actionable diagnostic tips from validation warnings and permission
/// validation diagnostics.
///
/// This function translates raw validation data into user-friendly tips with
/// suggested fixes where applicable.
pub fn collect_validation_tips(
    _settings: &crate::settings::EffectiveSettings,
    diagnostics: &[PermissionValidationWarning],
) -> Vec<ValidationTip> {
    let mut tips = Vec::new();

    for diag in diagnostics {
        let tip = match diag.field.as_str() {
            "permissions.allow" if diag.message.contains("dangerous") => ValidationTip {
                code: "dangerous-allow-rule".to_string(),
                message: diag.message.clone(),
                severity: diag.severity.clone(),
                fix: Some(
                    "Consider using a more specific pattern or moving this to the deny list."
                        .to_string(),
                ),
            },
            "permissions.allow" | "permissions.deny"
                if diag.message.contains("appears in both") =>
            {
                ValidationTip {
                    code: "conflicting-permission-rules".to_string(),
                    message: diag.message.clone(),
                    severity: diag.severity.clone(),
                    fix: Some(
                        "Remove the rule from one of the lists. Deny takes precedence over allow."
                            .to_string(),
                    ),
                }
            }
            "permissions.autoMode" => ValidationTip {
                code: "auto-mode-no-rules".to_string(),
                message: diag.message.clone(),
                severity: diag.severity.clone(),
                fix: Some(
                    "Add auto mode rules in your settings file under permissions.autoMode."
                        .to_string(),
                ),
            },
            _ => ValidationTip {
                code: format!("permission-{}", diag.field.replace('.', "-")),
                message: diag.message.clone(),
                severity: diag.severity.clone(),
                fix: None,
            },
        };
        tips.push(tip);
    }

    tips
}

/// Generate tips for common configuration problems that may not be caught
/// by the validator (e.g. missing recommended settings).
pub fn collect_configuration_tips(
    settings: &crate::settings::EffectiveSettings,
) -> Vec<ValidationTip> {
    let mut tips = Vec::new();

    // Tip: missing model
    if settings.model.is_none() {
        tips.push(ValidationTip {
            code: "missing-model".to_string(),
            message: "No model is configured. The default model will be used.".to_string(),
            severity: WarningSeverity::Info,
            fix: Some("Set a model with `/config set model <model-id>` or via `CLAUDE_MODEL` environment variable.".to_string()),
        });
    }

    // Tip: empty available tools list (legacy allowed_tools)
    if settings.allowed_tools.is_empty() && settings.permissions.allow.is_empty() {
        tips.push(ValidationTip {
            code: "empty-tools-list".to_string(),
            message: "No tools are explicitly allowed. The default tool set will be used."
                .to_string(),
            severity: WarningSeverity::Info,
            fix: None,
        });
    }

    // Tip: sandbox not enabled
    if settings.sandbox.enabled != Some(true) {
        tips.push(ValidationTip {
            code: "sandbox-not-enabled".to_string(),
            message: "Sandbox mode is not enabled. Untrusted commands will run without isolation."
                .to_string(),
            severity: WarningSeverity::Info,
            fix: Some("Enable sandbox in settings: sandbox.enabled = true.".to_string()),
        });
    }

    // Tip: plugin install policy not configured
    // (This is informational since plugins may or may not be used.)

    // Tip: permission mode is bypass
    if settings.permission_mode.as_deref() == Some("bypass") {
        tips.push(ValidationTip {
            code: "bypass-mode-active".to_string(),
            message: "Permission mode is set to 'bypass'. All tools will execute without prompts."
                .to_string(),
            severity: WarningSeverity::Warning,
            fix: Some(
                "Change mode with `/permissions mode ask` or `/permissions mode auto`.".to_string(),
            ),
        });
    }

    tips
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::EffectiveSettings;

    #[test]
    fn test_collect_validation_tips_empty() {
        let settings = EffectiveSettings::default();
        let tips = collect_validation_tips(&settings, &[]);
        assert!(tips.is_empty());
    }

    #[test]
    fn test_collect_validation_tips_dangerous_allow() {
        let settings = EffectiveSettings::default();
        let warnings = vec![PermissionValidationWarning {
            severity: WarningSeverity::Warning,
            field: "permissions.allow".to_string(),
            message: "Allow rule 'Bash(rm -rf /)' contains dangerous pattern.".to_string(),
            source_info: None,
        }];
        let tips = collect_validation_tips(&settings, &warnings);
        assert!(!tips.is_empty());
        assert_eq!(tips[0].code, "dangerous-allow-rule");
        assert!(tips[0].fix.is_some());
    }

    #[test]
    fn test_collect_validation_tips_conflicting() {
        let settings = EffectiveSettings::default();
        let warnings = vec![PermissionValidationWarning {
            severity: WarningSeverity::Warning,
            field: "permissions.allow".to_string(),
            message: "Rule 'Bash' appears in both allow and deny lists.".to_string(),
            source_info: None,
        }];
        let tips = collect_validation_tips(&settings, &warnings);
        assert_eq!(tips[0].code, "conflicting-permission-rules");
    }

    #[test]
    fn test_collect_configuration_tips() {
        let settings = EffectiveSettings::default();
        let tips = collect_configuration_tips(&settings);
        assert!(!tips.is_empty());

        // Should include missing model tip.
        assert!(tips.iter().any(|t| t.code == "missing-model"));
        // Should include sandbox tip.
        assert!(tips.iter().any(|t| t.code == "sandbox-not-enabled"));
    }

    #[test]
    fn test_configuration_tips_with_model_set() {
        let settings = EffectiveSettings {
            model: Some("claude-opus-4-6".to_string()),
            sandbox: crate::settings::SandboxSettings {
                enabled: Some(true),
                ..Default::default()
            },
            ..Default::default()
        };
        let tips = collect_configuration_tips(&settings);
        // When model is set and sandbox enabled, those tips should not appear.
        assert!(!tips.iter().any(|t| t.code == "missing-model"));
        assert!(!tips.iter().any(|t| t.code == "sandbox-not-enabled"));
    }

    #[test]
    fn test_bypass_mode_tip() {
        let settings = EffectiveSettings {
            permission_mode: Some("bypass".to_string()),
            ..Default::default()
        };
        let tips = collect_configuration_tips(&settings);
        assert!(tips.iter().any(|t| t.code == "bypass-mode-active"));
    }

    #[test]
    fn test_tip_struct() {
        let tip = ValidationTip {
            code: "test-code".to_string(),
            message: "Test message".to_string(),
            severity: WarningSeverity::Info,
            fix: Some("Run `/config set ...`".to_string()),
        };
        assert_eq!(tip.code, "test-code");
        assert_eq!(tip.fix.as_deref(), Some("Run `/config set ...`"));
    }
}
